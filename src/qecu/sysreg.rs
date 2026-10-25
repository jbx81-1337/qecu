//! AArch64 system-register (MRS/MSR) emulation.
//!
//! Unicorn's ARM64 core only natively models a subset of system registers, and
//! its MRS/MSR emulation does not advance the PC when an instruction is skipped.
//! This module intercepts every `MRS`/`MSR` and:
//!
//! * forwards the MMU registers (SCTLR/TCR/TTBR0/TTBR1/MAIR) to Unicorn so its
//!   built-in MMU performs VA->PA translation (4/16/64KB granules);
//! * round-trips the MTE and TPIDR thread-ID registers;
//! * writes ELR/SPSR directly (Unicorn's `eret` reads those from its register
//!   file but its own MSR/MRS emulation never populates them);
//! * returns zero for everything else, logging each unique access once.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use unicorn_engine::{RegisterARM64, RegisterARM64CP, Unicorn};

use crate::debug_log;

use super::mte;

/// ARM generic timer PPI (private peripheral interrupt) id.
pub const TIMER_PPI: u64 = 30;
/// Spurious interrupt id returned by ICC_IAR1_EL1 when nothing is pending.
const ICC_SPURIOUS: u64 = 0x3ff;
/// The AArch64 IRQ exception vector (VBAR_EL1 + 0x80 for EL1h entries, IRQ is
/// the second entry at offset 0x80). LK sets VBAR_EL1 = 0xffff0000f8801000.
pub const IRQ_VECTOR: u64 = 0xffff_0000_f880_1280u64;

/// State shared by the system-register hook. All fields are `Arc` so the struct
/// is cheaply cloneable into each hook closure.
#[derive(Clone)]
pub struct SysregState {
    pub counter: Arc<AtomicU64>,
    pub tpidr: Arc<Mutex<HashMap<(u32, u32, u32, u32, u32), u64>>>,
    pub mte: Arc<Mutex<mte::MteState>>,
    pub logged: Arc<Mutex<HashSet<(u64, u32, u32, u32, u32, u32)>>>,
    // ARM generic timer emulation. `timer_deadline` is the CNTPCT value at
    // which the physical timer fires; `timer_enabled` mirrors CNTP_CTL_EL0.
    pub timer_deadline: Arc<AtomicU64>,
    pub timer_enabled: Arc<AtomicBool>,
    pub timer_pending: Arc<AtomicBool>,
    /// Last SPSR_EL1 value written via `msr spsr_el1`. Unicorn's `eret` reads
    /// SPSR from its CP_REG space, but `mrs spsr_el1` cannot read it back, so
    /// we round-trip it ourselves.
    pub spsr_el1: Arc<AtomicU64>,
}

impl SysregState {
    pub fn new() -> Self {
        SysregState {
            counter: Arc::new(AtomicU64::new(0)),
            tpidr: Arc::new(Mutex::new(HashMap::new())),
            mte: Arc::new(Mutex::new(mte::MteState::default())),
            logged: Arc::new(Mutex::new(HashSet::new())),
            timer_deadline: Arc::new(AtomicU64::new(u64::MAX)),
            timer_enabled: Arc::new(AtomicBool::new(false)),
            timer_pending: Arc::new(AtomicBool::new(false)),
            spsr_el1: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Advances the free-running counter and returns the new value.
    pub fn tick(&self, delta: u64) -> u64 {
        self.counter.fetch_add(delta, Ordering::Relaxed) + delta
    }

    pub fn counter_now(&self) -> u64 {
        self.counter.load(Ordering::Relaxed)
    }

    /// Returns true when the timer is armed and its deadline has passed. The
    /// caller is responsible for actually taking the IRQ exception.
    pub fn timer_expired(&self) -> bool {
        self.timer_enabled.load(Ordering::Relaxed)
            && self.counter_now() >= self.timer_deadline.load(Ordering::Relaxed)
    }
}

/// Handles one intercepted `MRS`/`MSR`. Returns `true` if the instruction was
/// emulated (and the PC has been advanced past it); `false` lets Unicorn run it
/// natively.
pub fn handle(
    uc: &mut Unicorn<'_, ()>,
    reg: RegisterARM64,
    cp: &RegisterARM64CP,
    is_read: bool,
    state: &SysregState,
) -> bool {
    // ELR_ELx (op2 = 1) and SPSR_ELx (op2 = 0). Unicorn's `eret` reads these
    // from its register file, but its MSR/MRS emulation does not populate them,
    // so move the value directly.
    if cp.op0 == 3 && cp.crn == 4 && cp.crm == 0 && (cp.op2 == 0 || cp.op2 == 1) {
        let el_index = match cp.op1 {
            0 => 1, // EL1
            4 => 2, // EL2
            6 => 3, // EL3
            _ => 0,
        };
        if is_read {
            let value = if cp.op2 == 1 {
                match el_index {
                    1 => uc.reg_read(RegisterARM64::ELR_EL1),
                    2 => uc.reg_read(RegisterARM64::ELR_EL2),
                    3 => uc.reg_read(RegisterARM64::ELR_EL3),
                    _ => Ok(0),
                }
                .unwrap_or(0)
            } else {
                // SPSR reads must round-trip the value we wrote via CP_REG so
                // the exception return path restores the saved PSTATE. Unicorn
                // cannot read SPSR back, so use our tracked copy.
                state.spsr_el1.load(Ordering::Relaxed)
            };
            let _ = uc.reg_write(reg, value);
        } else if let Ok(value) = uc.reg_read(reg) {
            if cp.op2 == 1 {
                let target = match el_index {
                    1 => RegisterARM64::ELR_EL1,
                    2 => RegisterARM64::ELR_EL2,
                    3 => RegisterARM64::ELR_EL3,
                    _ => RegisterARM64::ELR_EL0,
                };
                let _ = uc.reg_write(target, value);
            } else {
                if el_index == 1 {
                    state.spsr_el1.store(value, Ordering::Relaxed);
                }
                cp_reg_write(uc, 4, 0, 3, cp.op1, 0, value);
            }
        }
        let pc = uc.pc_read().unwrap_or(0);
        let _ = uc.set_pc(pc + 4);
        return true;
    }

    // scr_el3: pass through (Unicorn models it; eret from EL2 does not consult it).
    if (cp.op0, cp.op1, cp.crn, cp.crm, cp.op2) == (3, 6, 1, 1, 0) {
        return false;
    }

    // MMU system registers: pass through to Unicorn so its built-in MMU performs
    // VA->PA translation. Unicorn supports 4/16/64KB granules natively.
    if is_mmu_reg(cp.op0, cp.op1, cp.crn, cp.crm, cp.op2) {
        return false;
    }

    // MTE system registers: round-trip so the firmware's MTE enable/configure
    // sequence completes instead of reading back zero.
    if is_read {
        if let Some(value) = state
            .mte
            .lock()
            .unwrap()
            .read(cp.op0, cp.op1, cp.crn, cp.crm, cp.op2)
        {
            let _ = uc.reg_write(reg, value);
            let pc = uc.pc_read().unwrap_or(0);
            let _ = uc.set_pc(pc + 4);
            return true;
        }
    } else if let Ok(value) = uc.reg_read(reg) {
        if state
            .mte
            .lock()
            .unwrap()
            .write(cp.op0, cp.op1, cp.crn, cp.crm, cp.op2, value)
        {
            let pc = uc.pc_read().unwrap_or(0);
            let _ = uc.set_pc(pc + 4);
            return true;
        }
    }

    // ARM generic timer + GICv3 CPU interface (LK's scheduler depends on both).
    let key = (cp.op0, cp.op1, cp.crn, cp.crm, cp.op2);
    match key {
        // CNTP_CTL_EL0: arm/disarm the physical timer.
        (3, 3, 14, 2, 1) => {
            let pc = uc.pc_read().unwrap_or(0);
            if is_read {
                let enabled = state.timer_enabled.load(Ordering::Relaxed) as u64;
                let _ = uc.reg_write(reg, enabled);
            } else if let Ok(value) = uc.reg_read(reg) {
                let enabled = value & 1 != 0;
                state.timer_enabled.store(enabled, Ordering::Relaxed);
                if enabled && state.timer_expired() {
                    state.timer_pending.store(true, Ordering::Relaxed);
                }
            }
            let _ = uc.set_pc(pc + 4);
            return true;
        }
        // CNTP_CVAL_EL0: program the timer deadline.
        (3, 3, 14, 2, 2) => {
            let pc = uc.pc_read().unwrap_or(0);
            if is_read {
                let _ = uc.reg_write(reg, state.timer_deadline.load(Ordering::Relaxed));
            } else if let Ok(value) = uc.reg_read(reg) {
                state.timer_deadline.store(value, Ordering::Relaxed);
                state.timer_pending.store(false, Ordering::Relaxed);
                if state.timer_expired() {
                    state.timer_pending.store(true, Ordering::Relaxed);
                }
            }
            let _ = uc.set_pc(pc + 4);
            return true;
        }
        // CNTP_TVAL_EL0: program a relative deadline.
        (3, 3, 14, 2, 0) => {
            if is_read {
                let _ = uc.reg_write(reg, 0);
            } else if let Ok(value) = uc.reg_read(reg) {
                let deadline = state.counter_now().wrapping_add(value as u64);
                state.timer_deadline.store(deadline, Ordering::Relaxed);
                state.timer_pending.store(false, Ordering::Relaxed);
                if state.timer_expired() {
                    state.timer_pending.store(true, Ordering::Relaxed);
                }
            }
            let pc = uc.pc_read().unwrap_or(0);
            let _ = uc.set_pc(pc + 4);
            return true;
        }
        // ICC_IAR1_EL1: acknowledge the highest-priority pending interrupt.
        (3, 0, 12, 12, 0) => {
            let irq = if state.timer_pending.swap(false, Ordering::Relaxed) {
                TIMER_PPI
            } else {
                ICC_SPURIOUS
            };
            let _ = uc.reg_write(reg, irq);
            let pc = uc.pc_read().unwrap_or(0);
            let _ = uc.set_pc(pc + 4);
            return true;
        }
        // ICC_EOIR1_EL1: end-of-interrupt; nothing further to track.
        (3, 0, 12, 12, 1) => {
            let pc = uc.pc_read().unwrap_or(0);
            let _ = uc.set_pc(pc + 4);
            return true;
        }
        _ => {}
    }

    // Everything else: log each unique access site once, return an emulated
    // value for reads (zero for unknown registers), and skip the instruction.
    let pc = uc.pc_read().unwrap_or(0);
    let fresh = {
        let mut set = state.logged.lock().unwrap();
        set.insert((pc, cp.op0, cp.op1, cp.crn, cp.crm, cp.op2))
    };
    if fresh {
        debug_log!(
            "[emulator::sysreg] {} s{}_{}_c{}_c{}_{} at pc={:#x} {}",
            if is_read { "MRS" } else { "MSR" },
            cp.op0,
            cp.op1,
            cp.crn,
            cp.crm,
            cp.op2,
            pc,
            if is_read { "-> emulated" } else { "ignored" }
        );
    }
    if is_read {
        let key = (cp.op0, cp.op1, cp.crn, cp.crm, cp.op2);
        let value = if is_tpidr(key) {
            *state.tpidr.lock().unwrap().entry(key).or_insert(0)
        } else {
            match key {
                (3, 0, 0, 0, 0) => 0x410f_d821, // midr_el1 (Cortex-X4)
                // id_aa64pfr0_el1: report GICv3 CPU interface (bits 27:24 = 1)
                // so LK's GIC init proceeds instead of failing its version
                // check and leaving max_irq = 0.
                (3, 0, 0, 4, 0) => 0x0100_0000,
                (3, 0, 0, 0, 5) => 0x8000_0000, // mpidr_el1 (primary CPU)
                // currentel: derive from the live PSTATE (EL bits).
                (3, 0, 4, 2, 2) => uc.reg_read(RegisterARM64::PSTATE).unwrap_or(0) & 0xc,
                (3, 3, 14, 0, 0) => 0x124f_800, // cntfrq_el0 (19.2 MHz)
                (3, 3, 14, 0, 1) | (3, 3, 14, 0, 2) => {
                    // cntpct_el0 / cntvct_el0: monotonic counter.
                    state.counter.fetch_add(0x1000, Ordering::Relaxed) + 0x1000
                }
                _ => 0,
            }
        };
        let _ = uc.reg_write(reg, value);
    } else if is_tpidr((cp.op0, cp.op1, cp.crn, cp.crm, cp.op2)) {
        // Track the TPIDR thread-ID registers so a later MRS round-trips the
        // written value (LK stores its current thread pointer in TPIDR_EL1).
        if let Ok(value) = uc.reg_read(reg) {
            state
                .tpidr
                .lock()
                .unwrap()
                .insert((cp.op0, cp.op1, cp.crn, cp.crm, cp.op2), value);
        }
    }

    // Unicorn does not advance the PC when an instruction is skipped.
    let pc = uc.pc_read().unwrap_or(0);
    let _ = uc.set_pc(pc + 4);
    true
}

/// Returns true for the MMU system registers (SCTLR/TCR/TTBR0/TTBR1/MAIR at
/// EL1/EL2/EL3), which must be passed through to Unicorn's MMU.
fn is_mmu_reg(op0: u32, op1: u32, crn: u32, crm: u32, op2: u32) -> bool {
    op0 == 3
        && (op1 == 0 || op1 == 4 || op1 == 6)
        && matches!(
            (crn, crm, op2),
            (1, 0, 0) | // SCTLR_ELx
            (2, 0, 0) | // TTBR0_ELx
            (2, 0, 1) | // TTBR1_ELx
            (2, 0, 2) | // TCR_ELx
            (10, 2, 0)  // MAIR_ELx
        )
}

/// Returns true if the system register is one of the TPIDR thread-ID registers
/// (TPIDR_EL0/EL1/EL2/EL3, TPIDRRO_EL0): op0=3, crn=13, crm=0, op2 in 2..=4.
fn is_tpidr(key: (u32, u32, u32, u32, u32)) -> bool {
    key.0 == 3 && key.2 == 13 && key.3 == 0 && (2..=4).contains(&key.4)
}

/// Writes an AArch64 system register via Unicorn's CP_REG interface. The CP_REG
/// buffer is 32 bytes: {crn, crm, op0, op1, op2} as u32s (20 bytes), 4 bytes of
/// padding, then the 64-bit value.
pub fn cp_reg_write(uc: &mut Unicorn<'_, ()>, crn: u32, crm: u32, op0: u32, op1: u32, op2: u32, value: u64) {
    let mut cp = [0u8; 32];
    for (i, field) in [crn, crm, op0, op1, op2].iter().enumerate() {
        cp[i * 4..i * 4 + 4].copy_from_slice(&field.to_ne_bytes());
    }
    cp[24..32].copy_from_slice(&value.to_ne_bytes());
    let _ = uc.reg_write_long(RegisterARM64::CP_REG, &cp);
}

/// Reads an AArch64 system register via Unicorn's CP_REG interface. Unicorn
/// requires the descriptor to be written to CP_REG first, then read back.
pub fn cp_reg_read(uc: &mut Unicorn<'_, ()>, crn: u32, crm: u32, op0: u32, op1: u32, op2: u32) -> Option<u64> {
    let mut cp = [0u8; 32];
    for (i, field) in [crn, crm, op0, op1, op2].iter().enumerate() {
        cp[i * 4..i * 4 + 4].copy_from_slice(&field.to_ne_bytes());
    }
    uc.reg_write_long(RegisterARM64::CP_REG, &cp).ok()?;
    let buf = uc.reg_read_long(RegisterARM64::CP_REG).ok()?;
    Some(u64::from_le_bytes(buf[24..32].try_into().unwrap()))
}
