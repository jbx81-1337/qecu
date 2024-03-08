use rust_sleigh::SleighDecompiler;
use unicorn_engine::{Unicorn, RegisterTRICORE};
use unicorn_engine::unicorn_const::{Arch, Mode, Permission};
use crate::utils::{self, workflow::Workflow};
use std::os::raw::c_void;
use std::sync::{Arc, Mutex};
use std::fmt;
use super::interceptor::Interceptor;

struct UcWrapper <'a>{
    uc: Unicorn<'a, ()>
}
unsafe impl Send for UcWrapper<'static>{}
impl <'a> fmt::Debug for UcWrapper<'static> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "UcWrapper")
    }
}

struct SleighDecompilerWrapper {
    disas: SleighDecompiler
}
unsafe impl Send for SleighDecompilerWrapper{}
impl fmt::Debug for SleighDecompilerWrapper {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "SleighDecompilerWrapper")
    }
}
#[derive(Debug, Clone)]
pub struct Emulator <'a>{
    pub wf: Arc<Workflow>,
    uc: Arc<Mutex<UcWrapper<'static>>>,
    disas: Arc<Mutex<SleighDecompilerWrapper>>,
    pub interceptor: Arc<Mutex<Option<Interceptor<'a>>>>
}

impl<'a> Emulator <'static>{

    pub fn new(workflow: Workflow) -> Emulator<'static> {
        let mut unicorn: Unicorn<'_, ()> = Unicorn::new(Arch::TRICORE, Mode::LITTLE_ENDIAN).expect("failed to initialize Unicorn instance");
        {
            let workflow = workflow.clone();
            let input = workflow.input;
            let registers = &workflow.registers;
            let code_sections = utils::loader::Loader::new(&input.format, &input.path);
            let uc = &mut unicorn;
            for mem_map in workflow.mem_map {
                let mut perms = Permission::NONE;
                for chr in mem_map.flags.to_uppercase().chars() {
                    let new_permission_feature = match chr {
                        'R' => Permission::READ,
                        'W' => Permission::WRITE,
                        'X' => Permission::EXEC,
                        '*' => Permission::ALL,
                        _ => Permission::NONE
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
        let sleigh_path = {
            workflow.sleigh_path.clone()
        };
        let init_script = {
            workflow.init_script.clone()
        };

        Emulator {
            wf: Arc::new(workflow), 
            uc: Arc::new(Mutex::new(UcWrapper { uc: unicorn })),
            disas: 
                Arc::new(
                    Mutex::new(SleighDecompilerWrapper { 
                        disas: SleighDecompiler::new(sleigh_path,
                                                        String::from("/tricore/data/languages/tricore.sla"),
                                                        String::from("/tricore/data/languages/tricore.pspec"))
                                                    }
                                                )
                                            ),
            interceptor: Arc::new(Mutex::new(Some(Interceptor::new(init_script))))
        }
    }

    pub fn mut_uc(&self) -> Unicorn<'_, ()>{
        return Unicorn::try_from(self.get_uc_handle()).unwrap();
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

    pub fn get_uc_handle(&self) -> *mut c_void {
        self.uc.lock().unwrap().uc.get_handle()
    }

    pub fn emit(&self, event_type: String, msg: String) {
        let mut lock = self.interceptor.lock();
        let intercept = lock.as_mut().unwrap().as_mut().unwrap();
        intercept.emit(event_type, msg);
    }

    pub fn run(&self) {
        {
            let mut lock = self.interceptor.lock();
            let intercept = lock.as_mut().unwrap().as_mut().unwrap();
            intercept.set_emulator(self.clone());
            intercept.init();
            drop(lock);
        }

        let mut uc = self.mut_uc();   
        let callback = move |_uc: &mut Unicorn<'_, ()>, addr: u64, size: u32| {
            self.on_code_hook(addr, size);
        };
        uc.add_code_hook(0, 0xFFFFFFFF, callback).expect("[emulator::run] Cannot install default code_hook");
        uc.emu_start(0x80003d10, 0xFFFFFFFF, 0x00, 0x00).unwrap();
    }

    pub fn disas(&self, code: Vec<u8>, addr: u64, size: u32) -> String{
        let disas = {
            self.disas.lock().unwrap().disas.clone()
        };
        return disas.disas(code, addr, size);
    }

    pub fn get_register(reg_name: &String) -> RegisterTRICORE {
        match reg_name.to_uppercase().as_str() {
            "A0" => RegisterTRICORE::A0,
            "A1" => RegisterTRICORE::A1,
            "A2" => RegisterTRICORE::A2,
            "A3" => RegisterTRICORE::A3,
            "A4" => RegisterTRICORE::A4,
            "A5" => RegisterTRICORE::A5,
            "A6" => RegisterTRICORE::A6,
            "A7" => RegisterTRICORE::A7,
            "A8" => RegisterTRICORE::A8,
            "A9" => RegisterTRICORE::A9,
            "A10" => RegisterTRICORE::A10,
            "A11" => RegisterTRICORE::A11,
            "A12" => RegisterTRICORE::A12,
            "A13" => RegisterTRICORE::A13,
            "A14" => RegisterTRICORE::A14,
            "A15" => RegisterTRICORE::A15,

            "D0" => RegisterTRICORE::D0,
            "D1" => RegisterTRICORE::D1,
            "D2" => RegisterTRICORE::D2,
            "D3" => RegisterTRICORE::D3,
            "D4" => RegisterTRICORE::D4,
            "D5" => RegisterTRICORE::D5,
            "D6" => RegisterTRICORE::D6,
            "D7" => RegisterTRICORE::D7,
            "D8" => RegisterTRICORE::D8,
            "D9" => RegisterTRICORE::D9,
            "D10" => RegisterTRICORE::D10,
            "D11" => RegisterTRICORE::D11,
            "D12" => RegisterTRICORE::D12,
            "D13" => RegisterTRICORE::D13,
            "D14" => RegisterTRICORE::D14,
            "D15" => RegisterTRICORE::D15,
            
            "BIV" => RegisterTRICORE::BIV,
            "FCX" => RegisterTRICORE::FCX,
            _ => RegisterTRICORE::A0
        }
    }
}