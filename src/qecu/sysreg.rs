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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use unicorn_engine::{RegisterARM64, RegisterARM64CP, Unicorn};

use crate::debug_log;

use super::mte;

/// State shared by the system-register hook. All fields are `Arc` so the struct
/// is cheaply cloneable into each hook closure.
#[derive(Clone)]
pub struct SysregState {
    pub counter: Arc<AtomicU64>,
    pub tpidr: Arc<Mutex<HashMap<(u32, u32, u32, u32, u32), u64>>>,
    pub mte: Arc<Mutex<mte::MteState>>,
    pub logged: Arc<Mutex<HashSet<(u64, u32, u32, u32, u32, u32)>>>,
}

impl SysregState {
    pub fn new() -> Self {
        SysregState {
            counter: Arc::new(AtomicU64::new(0)),
            tpidr: Arc::new(Mutex::new(HashMap::new())),
            mte: Arc::new(Mutex::new(mte::MteState::default())),
            logged: Arc::new(Mutex::new(HashSet::new())),
        }
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
                0
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
fn cp_reg_write(uc: &mut Unicorn<'_, ()>, crn: u32, crm: u32, op0: u32, op1: u32, op2: u32, value: u64) {
    let mut cp = [0u8; 32];
    for (i, field) in [crn, crm, op0, op1, op2].iter().enumerate() {
        cp[i * 4..i * 4 + 4].copy_from_slice(&field.to_ne_bytes());
    }
    cp[24..32].copy_from_slice(&value.to_ne_bytes());
    let _ = uc.reg_write_long(RegisterARM64::CP_REG, &cp);
}
