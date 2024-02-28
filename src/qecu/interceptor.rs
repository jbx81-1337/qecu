use rhai::{Engine, Scope, AST};
use std::{fs, thread::sleep, time};

use super::emulator::Emulator;

#[derive(Clone)]
pub struct CodeHook {
    begin: u64,
    end: u64,
    code_type: u8,
    content: String
}

#[derive(Clone)]
pub struct Interceptor <'a>{
    emulator: Emulator<'a>,
    ast: AST,
    scope: Scope<'a>,
    code_hooks: Vec<CodeHook>,
}

impl <'a> Interceptor <'static> {
    pub fn new(emulator: Emulator<'a>, init_script: String) -> Interceptor <'a>{
        let ast = {
            let  eng = make_engine();
            let script_code = fs::read_to_string(init_script).expect("[interceptor::new] Cannot open init_script");
            let ret = eng.compile(script_code).expect("[rhai::engine::compile]Cannot compile");
            ret
        };

        return Interceptor {
            emulator: emulator, 
            ast: ast,
            scope: Scope::new(),
            code_hooks: Vec::new(),
        };
    }

    pub fn init(&mut self) {
        let _engine = make_engine();
        let mut scope = self.scope.clone();
        scope.push("Interceptor", self.clone());
        _engine.run_ast_with_scope(&mut scope, &self.ast).expect("[engine::run_with_scope] Error running init script.\n");
        let intercept = scope.get_value::<Interceptor>("Interceptor").unwrap();
        self.code_hooks = intercept.code_hooks;
        self.scope = scope.clone();
    }

    pub fn disas(&mut self, address: i64, size: i64) -> String {
        let code = self.read_memory(address, size);
        let address: u64 = address.try_into().unwrap();
        let size: u32 = size.try_into().unwrap();
        let mut disas = self.emulator.disas.disas(code, address, size);
        disas.pop();
        return disas;
    }
    
    pub fn read_register(&mut self, reg_name: String) -> i64{
        self.emulator.read_register(reg_name).try_into().unwrap()
    }

    pub fn write_register(&mut self, reg_name: String, value: i64) -> i64 {
        self.emulator.write_register(reg_name, value.try_into().unwrap()).try_into().unwrap()
    }

    pub fn read_memory(&mut self, address: i64, size: i64) -> Vec<u8>{
        self.emulator.read_memory(address.try_into().unwrap(), size.try_into().unwrap())
    }

    pub fn write_memory(&mut self, address: i64, data: Vec<u8>) -> i64 {
        self.emulator.write_memory(address.try_into().unwrap(), data).try_into().unwrap()
    }

    pub fn add_cb_hook(&mut self, hook_type: String, address: i64, size: i64, callback: rhai::FnPtr) {
        let fn_name = callback.fn_name().to_string();
        match hook_type.as_str() {
            "CODE" => {
                let begin: u64 = address.try_into().unwrap();
                let end: u64 = (address + size).try_into().unwrap();
                let code_hook = CodeHook {
                    begin: begin, end: end, code_type: 1, content: fn_name.clone()
                };
                self.code_hooks.push(code_hook);
            }
            _ => {
                panic!("[interceptor::add_hook] Unknown hook type {} ", hook_type);
            }
        }

    }
    pub fn add_hook(&mut self, hook_type: String, address: i64, size: i64, function_name: String) {
        match hook_type.as_str() {
            "CODE" => {
                let begin: u64 = address.try_into().unwrap();
                let end: u64 = (address + size).try_into().unwrap();
                let code_hook = CodeHook {
                    begin: begin, end: end, code_type: 0, content: function_name.clone()
                };
                self.code_hooks.push(code_hook);
            }
            _ => {
                panic!("[interceptor::add_hook] Unknown hook type {} ", hook_type);
            }
        }

    }

    pub fn sleep(&mut self, millis: i64) {
        let millis = time::Duration::from_millis(millis.try_into().unwrap());
        sleep(millis);
    }

    pub fn set_pc(&mut self, addr: i64) {
        self.emulator.mut_uc().set_pc(addr.try_into().unwrap()).expect("[interceptor::set_pc] Cannot set pc");
    }

    pub fn on_code_hook(&mut self, addr: u64, size: u32) {
        let addr_rhai: i64 = addr.try_into().unwrap();
        let size_rhai: i64 = size.try_into().unwrap();
        let code_hooks = self.code_hooks.clone();
        let size: u64 = size.try_into().unwrap();
        for code_hook in code_hooks {
            if addr >= code_hook.begin && (addr + size) <= code_hook.end {
                let mut _engine = make_engine();
                let mut _scope = self.scope.clone();
                let ast = self.ast.clone_functions_only();
                match code_hook.code_type {
                    0 => {
                        _engine.call_fn::<i64>(&mut _scope, &ast, &code_hook.content, (addr_rhai, size_rhai))
                            .expect(format!("[interceptor::on_code_hook] Cannot call function {} ", code_hook.content).as_str());
                    }
                    1 => { _engine.call_fn::<i64>(&mut _scope, &ast, &code_hook.content, (self.clone(), addr_rhai, size_rhai))
                    .       expect(format!("[interceptor::on_code_hook] Cannot call function {} ", code_hook.content).as_str());
                    }
                    _ => {}
                }
            }
        }
    }
}

pub fn make_engine() -> Engine{
    let mut engine = Engine::new();
    engine.set_allow_anonymous_fn(true);
    engine.register_type::<Interceptor>().
        register_fn("read_register", Interceptor::read_register).
        register_fn("write_register", Interceptor::write_register).
        register_fn("read_memory", Interceptor::read_memory).
        register_fn("write_memory", Interceptor::write_memory).
        register_fn("add_hook", Interceptor::add_hook).
        register_fn("add_hook", Interceptor::add_cb_hook).
        register_fn("disas", Interceptor::disas).
        register_fn("sleep", Interceptor::sleep).
        register_fn("set_pc", Interceptor::set_pc);
    return engine;
}