//! Memory Tagging Extension (MTE) emulation.
//!
//! Unicorn does not model MTE: its instructions raise an exception, and its
//! system registers (`GCR_EL1`, `RGSR_EL1`, `TFSR_ELx`) do not round-trip.
//! This module emulates both, treating the tag half of the architecture as
//! absent (a flat, tag-less address space):
//!
//! * [`emulate_insn`] decodes and emulates the MTE instructions the firmware
//!   executes. Tag-producing operations keep the address half; tag memory
//!   accesses are no-ops.
//! * [`MteState`] models the MTE system registers so an `MSR GCR_EL1` /
//!   `MRS GCR_EL1` enable sequence round-trips instead of reading back zero.

use unicorn_engine::{RegisterARM64, Unicorn};

/// Maps an AArch64 X-register index (0..31, where 31 is SP) to Unicorn's
/// `RegisterARM64`. X29/X30/SP are not contiguous with X0..X28 in Unicorn's
/// enum, hence the explicit arms.
fn x_reg(index: u32) -> RegisterARM64 {
    match index {
        29 => RegisterARM64::X29,
        30 => RegisterARM64::X30,
        31 => RegisterARM64::SP,
        n => unsafe { std::mem::transmute::<u32, RegisterARM64>(199 + n) },
    }
}

/// Emulates the MTE instruction word `insn` at `addr`. Returns `true` (and
/// advances the PC past the instruction) if it was an MTE instruction;
/// otherwise returns `false`, leaving the PC untouched.
pub fn emulate_insn(uc: &mut Unicorn<'_, ()>, addr: u64, insn: u32) -> bool {
    if insn & 0xffe0_fc00 == 0x9ac0_1000 {
        // irg Xd, Xn{, Xm}: Xd = Xn (tag ignored).
        let rd = insn & 0x1f;
        let rn = (insn >> 5) & 0x1f;
        let v = uc.reg_read(x_reg(rn)).unwrap_or(0);
        let _ = uc.reg_write(x_reg(rd), v);
    } else if insn & 0xffc0_0000 == 0x9180_0000 {
        // addg Xd, Xn, #uimm6, #uimm4: Xd = Xn + (uimm6 << 4).
        let rd = insn & 0x1f;
        let rn = (insn >> 5) & 0x1f;
        let add = ((insn >> 16) & 0x3f) << 4;
        let v = uc.reg_read(x_reg(rn)).unwrap_or(0).wrapping_add(add as u64);
        let _ = uc.reg_write(x_reg(rd), v);
    } else if insn & 0xffc0_0000 == 0xd180_0000 {
        // subg Xd, Xn, #uimm6, #uimm4: Xd = Xn - (uimm6 << 4).
        let rd = insn & 0x1f;
        let rn = (insn >> 5) & 0x1f;
        let sub = ((insn >> 16) & 0x3f) << 4;
        let v = uc.reg_read(x_reg(rn)).unwrap_or(0).wrapping_sub(sub as u64);
        let _ = uc.reg_write(x_reg(rd), v);
    } else if insn & 0xffe0_fc00 == 0x9ac0_0000 {
        // subp Xd, Xn, Xm: Xd = Xn - Xm (tag-adjusted pointer subtract).
        let rd = insn & 0x1f;
        let rn = (insn >> 5) & 0x1f;
        let rm = (insn >> 16) & 0x1f;
        let v = uc.reg_read(x_reg(rn)).unwrap_or(0)
            .wrapping_sub(uc.reg_read(x_reg(rm)).unwrap_or(0));
        let _ = uc.reg_write(x_reg(rd), v);
    } else if insn & 0xffe0_fc00 == 0xbac0_0000 {
        // subps Xd, Xn, Xm: Xd = Xn - Xm, flags set. Flags are not modeled;
        // treat as subp.
        let rd = insn & 0x1f;
        let rn = (insn >> 5) & 0x1f;
        let rm = (insn >> 16) & 0x1f;
        let v = uc.reg_read(x_reg(rn)).unwrap_or(0)
            .wrapping_sub(uc.reg_read(x_reg(rm)).unwrap_or(0));
        let _ = uc.reg_write(x_reg(rd), v);
    } else if insn & 0xff00_0000 == 0xd900_0000 {
        // stg / st2g / stzg / stz2g / ldg / stgm / ldgm: the whole 0xd9xxxxxx
        // opcode space is MTE tag-memory access. No-op in a tag-less emulator.
    } else if insn & 0xfe40_0000 == 0x6800_0000 {
        // stgp Xt1, Xt2, [Xn|SP{, #simm}]: store pair + allocation tags.
        // Emulate the data store (two 64-bit registers); tags ignored. Bit 25
        // is 0 for STGP and 1 for the adds/subs (shifted-register) family that
        // shares bits[31:26], so the mask keeps it.
        let rt = insn & 0x1f;
        let rn = (insn >> 5) & 0x1f;
        let rt2 = (insn >> 10) & 0x1f;
        let imm7 = ((insn >> 15) & 0x7f) as i64;
        let imm = (if imm7 >= 0x40 { imm7 - 0x80 } else { imm7 }) << 4;
        let base = uc.reg_read(x_reg(rn)).unwrap_or(0);
        let mode = (insn >> 23) & 0x7;
        let ea = match mode {
            0b001 => base,                          // post-index: base, then bump
            0b011 => base.wrapping_add(imm as u64), // pre-index
            _ => base.wrapping_add(imm as u64),     // offset
        };
        let v1 = if rt == 31 { 0 } else { uc.reg_read(x_reg(rt)).unwrap_or(0) };
        let v2 = if rt2 == 31 { 0 } else { uc.reg_read(x_reg(rt2)).unwrap_or(0) };
        let mut bytes = [0u8; 16];
        bytes[0..8].copy_from_slice(&v1.to_le_bytes());
        bytes[8..16].copy_from_slice(&v2.to_le_bytes());
        let _ = uc.mem_write(ea, &bytes);
        if mode != 0b010 {
            // pre/post-index: write back Rn = base + imm.
            let _ = uc.reg_write(x_reg(rn), base.wrapping_add(imm as u64));
        }
    } else {
        return false;
    }
    let _ = uc.set_pc(addr + 4);
    true
}

/// MTE system registers, modeled so their enable/configure sequence
/// round-trips. Each field holds the last written value; reads report it.
#[derive(Default)]
pub struct MteState {
    pub gcr_el1: u64,
    pub rgsr_el1: u64,
    pub tfsr_el1: u64,
    pub tfsre0_el1: u64,
    pub tfsr_el2: u64,
    pub tfsr_el3: u64,
}

impl MteState {
    /// Value to report for an `MRS` of an MTE system register, or `None` if
    /// `(op0, op1, crn, crm, op2)` is not an MTE register.
    pub fn read(&self, op0: u32, op1: u32, crn: u32, crm: u32, op2: u32) -> Option<u64> {
        if op0 != 3 {
            return None;
        }
        match (op1, crn, crm, op2) {
            (0, 1, 0, 6) => Some(self.gcr_el1),    // GCR_EL1
            (0, 1, 0, 5) => Some(self.rgsr_el1),   // RGSR_EL1
            (0, 5, 6, 0) => Some(self.tfsr_el1),   // TFSR_EL1
            (0, 5, 6, 1) => Some(self.tfsre0_el1), // TFSRE0_EL1
            (4, 5, 6, 0) => Some(self.tfsr_el2),   // TFSR_EL2
            (6, 5, 6, 0) => Some(self.tfsr_el3),   // TFSR_EL3
            _ => None,
        }
    }

    /// Records an `MSR` to an MTE system register. Returns `true` if the
    /// register was MTE and the value has been captured.
    pub fn write(&mut self, op0: u32, op1: u32, crn: u32, crm: u32, op2: u32, value: u64) -> bool {
        if op0 != 3 {
            return false;
        }
        match (op1, crn, crm, op2) {
            (0, 1, 0, 6) => self.gcr_el1 = value,
            (0, 1, 0, 5) => self.rgsr_el1 = value,
            (0, 5, 6, 0) => self.tfsr_el1 = value,
            (0, 5, 6, 1) => self.tfsre0_el1 = value,
            (4, 5, 6, 0) => self.tfsr_el2 = value,
            (6, 5, 6, 0) => self.tfsr_el3 = value,
            _ => return false,
        }
        true
    }
}
