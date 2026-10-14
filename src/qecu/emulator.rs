use capstone::prelude::*;
use unicorn_engine::{RegisterARM64, Unicorn};
use unicorn_engine::unicorn_const::{Arch, Mode, Prot};
use crate::utils::{self, workflow::Workflow};
use std::sync::{Arc, Mutex};
use std::fmt;
use super::wasm_engine::WasmEngine;

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
    _marker: std::marker::PhantomData<&'a ()>,
    pub interceptor: Arc<Mutex<Option<WasmEngine>>>
}

impl<'a> Emulator <'static>{

    pub fn new(workflow: Workflow) -> Emulator<'static> {
        let mut unicorn: Unicorn<'_, ()> = Unicorn::new(Arch::ARM64, Mode::LITTLE_ENDIAN).expect("failed to initialize Unicorn instance");
        {
            let workflow = workflow.clone();
            let input = workflow.input;
            let registers = &workflow.registers;
            let code_sections = utils::loader::Loader::load_code_sections(&input.format, &input.path);
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
                println!("[unicorn::mem_map] address: {:#01x} size: {}", mem_map.from, mem_map.size);
            }
        
            for code_section in code_sections {
                uc.mem_write(code_section.address, &code_section.data)
                    .expect(format!("[unicorn::mem_wirte] Failed to write data at {:#01x} of size {}\n", code_section.address, code_section.size).as_str());
                println!("[unicorn::mem_write] address: {:#01x} size: {}", code_section.address, code_section.size);
            }

            for register in registers {
                uc.reg_write(Self::get_register(&register.name), register.value)
                    .expect(format!("[unicorn::reg_write] Failed to write register {} with data {:#01x}\n", register.name, register.value).as_str());
                println!("[unicorn::reg_write] register: {} value: {:#01x}", register.name, register.value);
            }
        }
        let entry = workflow.entry.unwrap_or_else(|| {
            utils::loader::Loader::load_entry(&workflow.input.format, &workflow.input.path)
        });
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
            _marker: std::marker::PhantomData,
            interceptor: Arc::new(Mutex::new(Some(WasmEngine::new(init_script))))
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
        let ret = self.mut_uc().mem_read_as_vec(address, size)
            .expect("[emulator::read_memory] Cannot write register\n");
        return ret;
    }

    pub fn write_memory(&self, address: u64, data: Vec<u8>) -> u64{
        let data = &data; // b: &Vec<u8>
        let data: &[u8] = &data; // c: &[u8]
        self.mut_uc().mem_write(address, data)
            .expect("[emulator::write_memory] Cannot write register\n");
        return 0;
    }

    pub fn set_pc(&self, addr: u64) {
        self.mut_uc().set_pc(addr).expect("[emulator::set_pc] Cannot set pc");
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
            intercept.init();
        }

        let mut uc = self.mut_uc();   
        let callback = move |_uc: &mut Unicorn<'_, ()>, addr: u64, size: u32| {
            self.on_code_hook(addr, size);
        };
        uc.add_code_hook(0, u64::MAX, callback).expect("[emulator::run] Cannot install default code_hook");
        uc.emu_start(self.entry, u64::MAX, 0x00, 0x00).unwrap();
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
            other => panic!("[emulator::get_register] Unknown register {}", other)
        }
    }
}
