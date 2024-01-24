use rhai::{Engine, Scope};
use unicorn_engine::{Unicorn, RegisterTRICORE};
use unicorn_engine::unicorn_const::{Arch, Mode, Permission};
use crate::utils::{self, workflow::Workflow};

pub struct Emulator <'a>{
    pub engine: Engine,
    pub scope: Scope<'a>
}

impl<'a> Emulator <'a>{

    pub fn new(workflow: Workflow) -> Unicorn<'static, Emulator<'a>>{
        let emu = Emulator { engine: Engine::new(), scope: Scope::new()};
        let input = workflow.input;
        let registers = &workflow.registers;
        let code_sections = utils::loader::Loader::new(&input.format, &input.path);
        let mut unicorn: Unicorn<'_, Emulator> = Unicorn::new_with_data(Arch::TRICORE, Mode::LITTLE_ENDIAN, emu).expect("failed to initialize Unicorn instance");
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
        return unicorn;
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
