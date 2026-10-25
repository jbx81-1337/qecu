use capstone::prelude::*;
use unicorn_engine::{Arm64Insn, RegisterARM64, Unicorn};
use unicorn_engine::unicorn_const::{Arch, HookType, Mode, Prot};
use crate::utils::{self, workflow::Workflow};
use crate::debug_log;
use std::sync::{Arc, Mutex};
use std::fmt;
use super::wasm_engine::WasmEngine;
use super::mte;
use super::sysreg;
use super::channel::{ChannelBroker, ChannelFrame};

struct UcWrapper <'a>{
    uc: Unicorn<'a, ()>
}
unsafe impl Send for UcWrapper<'static>{}
impl <'a> fmt::Debug for UcWrapper<'static> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "UcWrapper")
    }
}

struct CapstoneWrapper {
    cs: Capstone
}
unsafe impl Send for CapstoneWrapper{}
impl fmt::Debug for CapstoneWrapper {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "CapstoneWrapper")
    }
}

pub struct DisasLine {
    pub mnemonic: String,
    pub body: String
}

#[derive(Debug, Clone)]
pub struct Emulator <'a>{
    pub wf: Arc<Workflow>,
    uc: Arc<Mutex<UcWrapper<'static>>>,
    disas: Arc<Mutex<CapstoneWrapper>>,
    entry: u64,
    until: u64,
    _marker: std::marker::PhantomData<&'a ()>,
    pub interceptor: Arc<Mutex<Option<WasmEngine>>>,
    pub broker: Arc<Mutex<ChannelBroker>>
}

impl<'a> Emulator <'static>{

    pub fn new(workflow: Workflow, broker: Arc<Mutex<ChannelBroker>>) -> Emulator<'static> {
        let mut unicorn: Unicorn<'_, ()> = Unicorn::new(Arch::ARM64, Mode::LITTLE_ENDIAN).expect("failed to initialize Unicorn instance");
        // The default CPU lacks newer system registers (e.g. RAS ERRSELR_EL1).
        unicorn.ctl_set_cpu_model(unicorn_engine::Arm64CpuModel::MAX as i32)
            .expect("failed to select Unicorn CPU model");
        let content_entry = workflow
            .mem_map
            .iter()
            .find(|mem_map| mem_map.content.is_some())
            .map(|mem_map| mem_map.content_address.unwrap_or(mem_map.from));
        let entry = workflow
            .entry
            .or_else(|| workflow.boot.as_ref().and_then(|boot| boot.begin))
            .unwrap_or_else(|| match &workflow.input {
                Some(input) if input.format != "raw" => {
                    let path = input
                        .path
                        .as_ref()
                        .expect("[qecu::loader] Input path is required for this format.");
                    utils::loader::Loader::load_entry(&input.format, path)
                }
                _ => content_entry.unwrap_or(0),
            });
        {
            let workflow = workflow.clone();
            let registers = &workflow.registers;
            let mut code_sections = Vec::new();
            if let Some(input) = &workflow.input {
                if let Some(path) = &input.path {
                    code_sections.extend(utils::loader::Loader::load_code_sections(&input.format, path));
                } else if input.format != "raw" {
                    panic!("[qecu::loader] Input path is required for this format.");
                }
            }
            code_sections.extend(workflow.mem_map.iter().filter_map(|mem_map| {
                mem_map.content.as_deref().map(|path| {
                    utils::loader::Loader::load_raw_code_section(path, mem_map.content_address.unwrap_or(mem_map.from))
                })
            }));
            let uc = &mut unicorn;
            for mem_map in workflow.mem_map {
                let mut perms = Prot::NONE;
                for chr in mem_map.flags.to_uppercase().chars() {
                    let new_permission_feature = match chr {
                        'R' => Prot::READ,
                        'W' => Prot::WRITE,
                        'X' => Prot::EXEC,
                        '*' => Prot::ALL,
                        _ => Prot::NONE
                    };
                    perms = perms | new_permission_feature;
                }
                uc.mem_map(mem_map.from, mem_map.size, perms)
                    .expect(format!("[unicorn::mem_map] Failed to write data at {:#01x} of size {}\n", mem_map.from, mem_map.size).as_str());
                debug_log!("[unicorn::mem_map] address: {:#01x} size: {}", mem_map.from, mem_map.size);
            }
        
            for code_section in code_sections {
                uc.mem_write(code_section.address, &code_section.data)
                    .expect(format!("[unicorn::mem_wirte] Failed to write data at {:#01x} of size {}\n", code_section.address, code_section.size).as_str());
                debug_log!("[unicorn::mem_write] address: {:#01x} size: {}", code_section.address, code_section.size);
            }

            for register in registers {
                uc.reg_write(Self::get_register(&register.name), register.value)
                    .expect(format!("[unicorn::reg_write] Failed to write register {} with data {:#01x}\n", register.name, register.value).as_str());
                debug_log!("[unicorn::reg_write] register: {} value: {:#01x}", register.name, register.value);
                if register.name.eq_ignore_ascii_case("PSTATE") {
                    Self::rebuild_cpu_flags(uc);
                }
            }
        }
        let until = workflow
            .until
            .or_else(|| workflow.boot.as_ref().and_then(|boot| boot.until))
            .unwrap_or(u64::MAX);
        let init_script = {
            workflow.init_script.clone()
        };

        Emulator {
            wf: Arc::new(workflow), 
            uc: Arc::new(Mutex::new(UcWrapper { uc: unicorn })),
            disas: Arc::new(Mutex::new(CapstoneWrapper {
                cs: Capstone::new()
                    .arm64()
                    .mode(arch::arm64::ArchMode::Arm)
                    .build()
                    .expect("[capstone] Cannot initialize disassembler")
            })),
            entry,
            until: until,
            _marker: std::marker::PhantomData,
            interceptor: Arc::new(Mutex::new(Some(WasmEngine::new(init_script)))),
            broker
        }
    }

    pub fn mut_uc(&self) -> Unicorn<'_, ()> {
        unsafe {
            let uc_handle = self.get_uc_handle();
            return Unicorn::from_handle(uc_handle).unwrap();
        }
    }
    
    pub fn read_register(&self, reg_name: String) -> u64 {
        let ret = self.mut_uc().reg_read(Emulator::get_register(&reg_name))
            .expect("[emulator::write_register] Cannot read register\n");
        return ret;
    }

    pub fn write_register(&self, reg_name: String, value: u64) -> u64 {
        self.mut_uc().reg_write(Emulator::get_register(&reg_name), value.try_into().unwrap())
            .expect("[emulator::write_register] Cannot write register\n");
        return 0;
    }

    pub fn read_memory(&self, address: u64, size: usize) -> Vec<u8>{
        match self.mut_uc().mem_read_as_vec(address, size) {
            Ok(v) => v,
            Err(e) => {
                debug_log!(
                    "[emulator::read_memory] READ_UNMAPPED address={:#x} size={} ({:?})",
                    address, size, e
                );
                vec![0u8; size]
            }
        }
    }

    pub fn write_memory(&self, address: u64, data: Vec<u8>) -> u64{
        let data: &[u8] = &data;
        match self.mut_uc().mem_write(address, data) {
            Ok(()) => 0,
            Err(e) => {
                debug_log!(
                    "[emulator::write_memory] WRITE_UNMAPPED address={:#x} size={} ({:?})",
                    address, data.len(), e
                );
                1
            }
        }
    }

    pub fn set_pc(&self, addr: u64) {
        self.mut_uc().set_pc(addr).expect("[emulator::set_pc] Cannot set pc");
    }

    pub fn stop(&self) {
        self.mut_uc().emu_stop().expect("[emulator::stop] Cannot stop emulation");
    }

    pub fn on_code_hook(&self, _addr: u64, _size: u32) {
        let mut lock = self.interceptor.lock();
        let intercept = lock.as_mut().unwrap().as_mut().unwrap();
        intercept.on_code_hook(_addr, _size);
    }

    pub fn get_uc_handle(&self) -> *mut unicorn_engine::uc_engine {
        self.uc.lock().unwrap().uc.get_handle()
    }

    pub fn emit(&self, event_type: String, msg: String) {
        // let mut lock = self.interceptor.lock();
        // let intercept = lock.as_mut().unwrap().as_mut().unwrap();
        self.interceptor.lock().unwrap().as_mut().unwrap().emit(event_type, msg);
    }

    pub fn run(&self) {
        {
            let mut mutex_guard = self.interceptor.lock().unwrap();
            let intercept = mutex_guard.as_mut().unwrap();
            intercept.set_emulator(self.clone());
            intercept.set_broker(self.broker.clone());
            intercept.init();
        }

        let mut uc = self.mut_uc();   
        let callback = move |uc: &mut Unicorn<'_, ()>, addr: u64, size: u32| {
            // Emulate Memory Tagging Extension instructions (Unicorn does not
            // model them and raises an exception on the first one). The logic
            // lives in `mte::emulate_insn`.
            if let Some(insn) = uc
                .mem_read_as_vec(addr, 4)
                .ok()
                .and_then(|b| <[u8; 4]>::try_from(b.as_slice()).ok())
                .map(|b| u32::from_le_bytes(b))
            {
                if mte::emulate_insn(uc, addr, insn) {
                    return;
                }
            }
            self.on_code_hook(addr, size);
        };
        uc.add_code_hook(0, u64::MAX, callback).expect("[emulator::run] Cannot install default code_hook");
        uc.add_mem_hook(HookType::MEM_UNMAPPED, 0, u64::MAX, |uc, mem_type, address, size, value| {
            debug_log!(
                "[emulator::mem] {:?} address={:#x} size={} value={:#x} pc={:#x}",
                mem_type, address, size, value, uc.pc_read().unwrap_or(0)
            );
            false
        }).expect("[emulator::run] Cannot install unmapped memory hook");
        // System-register (MRS/MSR) handling lives in `sysreg::handle`. Unicorn
        // only natively models a subset of registers, and its MRS/MSR emulation
        // does not advance the PC when an instruction is skipped, so we intercept
        // every one: forward the MMU registers to Unicorn, round-trip MTE/TPIDR,
        // and emulate the handful of values the boot firmware depends on.
        let sysreg_state = sysreg::SysregState::new();
        for insn in [Arm64Insn::UC_ARM64_INS_MSR, Arm64Insn::UC_ARM64_INS_MRS] {
            let is_read = insn == Arm64Insn::UC_ARM64_INS_MRS;
            let state = sysreg_state.clone();
            uc.add_insn_sys_hook_arm64(insn, 1, 0, move |uc, reg, cp| {
                sysreg::handle(uc, reg, cp, is_read, &state)
            }).expect("[emulator::run] Cannot install system register hook");
        }
        // Paused/pump lifecycle: the emulator thread is the sole owner of
        // Unicorn execution and the plugin callbacks.  `emu_start` returns when
        // the guest executes WFI (Unicorn halts on WFI, PC advanced past it) or
        // when the plugin calls `stop()`.  Between slices we drain queued
        // inbound channel frames into the plugin and let the plugin's
        // `channel_send` queue outbound frames into the broker (which the
        // transport pump then routes back to the originating client).
        let mut first_slice = true;
        loop {
            // Resume from the current PC (past WFI on subsequent slices).
            let start_pc = if first_slice {
                self.entry
            } else {
                uc.pc_read().unwrap_or(self.entry)
            };
            first_slice = false;
            if let Err(error) = uc.emu_start(start_pc, self.until, 0x00, 0x00) {
                let pc = uc.pc_read().unwrap_or(0);
                let insn = uc.mem_read_as_vec(pc, 4).unwrap_or_default();
                let disas = self.disas(insn.clone(), pc, 4).into_iter()
                    .map(|line| format!("{} {}", line.mnemonic, line.body))
                    .collect::<Vec<_>>().join("; ");
                let lr = uc.reg_read(RegisterARM64::X30).unwrap_or(0);
                panic!("[emulator::run] Emulation failed: {:?} at pc={:#x} lr={:#x} bytes={:02x?} insn=[{}]", error, pc, lr, insn, disas);
            }

            // Drain validated inbound frames for the plugin (host -> guest).
            let mut frames: Vec<ChannelFrame> = Vec::new();
            {
                let mut broker = self.broker.lock().unwrap();
                while let Some(frame) = broker.recv_for_plugin() {
                    frames.push(frame);
                }
            }
            let mut delivered = false;
            if !frames.is_empty() {
                let mut mutex_guard = self.interceptor.lock().unwrap();
                let intercept = mutex_guard.as_mut().unwrap();
                for frame in &frames {
                    delivered |= intercept.on_channel(frame);
                }
            }

            // Advance the free-running counter and service the ARM generic
            // timer: LK's scheduler programs CNTP_CVAL/CNTP_CTL and expects a
            // timer IRQ (PPI 30) to wake sleeping threads. The emulator does
            // not model the GIC, so we take the IRQ exception by hand once the
            // deadline passes.
            sysreg_state.tick(0x100_000);
            if sysreg_state.timer_expired() {
                sysreg_state.timer_pending.store(true, std::sync::atomic::Ordering::Relaxed);
                sysreg_state.timer_enabled.store(false, std::sync::atomic::Ordering::Relaxed);
                let pc = uc.pc_read().unwrap_or(self.entry);
                let pstate = uc.reg_read(RegisterARM64::PSTATE).unwrap_or(0);
                debug_log!("[emulator::timer] IRQ: save pc={pc:#x} pstate={pstate:#x} -> vector={:#x}", sysreg::IRQ_VECTOR);
                // Save the interrupted context (ELR/SPSR) and enter the IRQ
                // vector at EL1h with IRQs masked. SPSR_EL1 is written via the
                // CP_REG interface (Unicorn's eret reads it from there).
                let _ = uc.reg_write(RegisterARM64::ELR_EL1, pc);
                sysreg_state.spsr_el1.store(pstate, std::sync::atomic::Ordering::Relaxed);
                sysreg::cp_reg_write(&mut uc, 4, 0, 3, 0, 0, pstate);
                let _ = uc.set_pc(sysreg::IRQ_VECTOR);
                let new_pstate = (pstate & 0xF000_0000) | 0x385; // keep NZCV, EL1h + DAIF masked
                let _ = uc.reg_write(RegisterARM64::PSTATE, new_pstate);
                continue;
            }

            // The plugin may have queued outbound frames via `channel_send`
            // while delivering; the transport pump drains them.  If we
            // delivered inbound work, resume immediately; otherwise poll.
            if !delivered {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        }
    }

    pub fn disas(&self, code: Vec<u8>, addr: u64, _size: u32) -> Vec<DisasLine> {
        let cs = self.disas.lock().unwrap();
        let result = match cs.cs.disasm_all(&code, addr) {
            Ok(insns) => insns.iter().map(|i| DisasLine {
                mnemonic: i.mnemonic().unwrap_or("").to_string(),
                body: i.op_str().unwrap_or("").to_string()
            }).collect(),
            Err(_) => Vec::new()
        };
        result
    }

    // Unicorn only refreshes its cached exception-level flags on a CP_REG write,
    // so write VBAR_EL3 (0 at reset) after changing PSTATE.
    fn rebuild_cpu_flags(uc: &mut Unicorn<'_, ()>) {
        let mut cp_reg = [0u8; 32];
        for (index, field) in [12u32, 0, 3, 6, 0].iter().enumerate() {
            cp_reg[index * 4..index * 4 + 4].copy_from_slice(&field.to_ne_bytes());
        }
        uc.reg_write_long(RegisterARM64::CP_REG, &cp_reg)
            .expect("[unicorn::reg_write] Failed to refresh CPU flags after PSTATE write");
    }

    pub fn get_register(reg_name: &String) -> RegisterARM64 {
        match reg_name.to_uppercase().as_str() {
            "X0" => RegisterARM64::X0,
            "X1" => RegisterARM64::X1,
            "X2" => RegisterARM64::X2,
            "X3" => RegisterARM64::X3,
            "X4" => RegisterARM64::X4,
            "X5" => RegisterARM64::X5,
            "X6" => RegisterARM64::X6,
            "X7" => RegisterARM64::X7,
            "X8" => RegisterARM64::X8,
            "X9" => RegisterARM64::X9,
            "X10" => RegisterARM64::X10,
            "X11" => RegisterARM64::X11,
            "X12" => RegisterARM64::X12,
            "X13" => RegisterARM64::X13,
            "X14" => RegisterARM64::X14,
            "X15" => RegisterARM64::X15,
            "X16" => RegisterARM64::X16,
            "X17" => RegisterARM64::X17,
            "X18" => RegisterARM64::X18,
            "X19" => RegisterARM64::X19,
            "X20" => RegisterARM64::X20,
            "X21" => RegisterARM64::X21,
            "X22" => RegisterARM64::X22,
            "X23" => RegisterARM64::X23,
            "X24" => RegisterARM64::X24,
            "X25" => RegisterARM64::X25,
            "X26" => RegisterARM64::X26,
            "X27" => RegisterARM64::X27,
            "X28" => RegisterARM64::X28,
            "X29" => RegisterARM64::X29,
            "X30" => RegisterARM64::X30,
            "FP" => RegisterARM64::X29,
            "LR" => RegisterARM64::X30,
            "SP" => RegisterARM64::SP,
            "PC" => RegisterARM64::PC,
            "NZCV" => RegisterARM64::NZCV,
            "PSTATE" => RegisterARM64::PSTATE,
            other => panic!("[emulator::get_register] Unknown register {}", other)
        }
    }
}
