use rhai::{Engine, Scope, AST};
use unicorn_engine::Unicorn;
use crate::{emulator::Emulator, qecu::interceptor};
use std::{collections::HashMap, os::raw::c_void, sync::{Arc, RwLock}};


#[derive(Clone)]
pub struct Interceptor <'a>{
    emulator: Arc<RwLock<Emulator<'a>>>,
    uc_handle: *mut c_void,
    ast: AST
}

impl <'a> Interceptor <'static> {
    pub fn new(emulator: Arc<RwLock<Emulator<'a>>>, uc_handle: *mut c_void, ast: AST) -> Interceptor <'a>{
        return Interceptor {emulator: emulator, uc_handle: uc_handle, ast: ast};
    }

    
    pub fn read_register(&mut self, reg_name: String) -> i64{
        let emulator = Unicorn::try_from(self.uc_handle).unwrap();
        let ret = emulator.reg_read(Emulator::get_register(&reg_name))
            .expect("[interceptor::write_register] Cannot read register\n");
        return ret.try_into().unwrap();
    }

    pub fn write_register(&mut self, reg_name: String, value: i64) -> i64 {
        let mut emulator = Unicorn::try_from(self.uc_handle).unwrap();
        emulator.reg_write(Emulator::get_register(&reg_name), value.try_into().unwrap())
            .expect("[interceptor::write_register] Cannot write register\n");
        return 0;
    }

    pub fn read_memory(&mut self, address: i64, size: i64) -> Vec<u8>{
        let emulator = Unicorn::try_from(self.uc_handle).unwrap();
        let ret = emulator.mem_read_as_vec(address.try_into().unwrap(), size.try_into().unwrap())
            .expect("[interceptor::write_register] Cannot write register\n");
        return ret;
    }

    pub fn write_memory(&mut self, address: i64, data: Vec<u8>) -> i64{
        let data = &data; // b: &Vec<u8>
        let data: &[u8] = &data; // c: &[u8]
        let mut emulator = Unicorn::try_from(self.uc_handle).unwrap();
        emulator.mem_write(address.try_into().unwrap(), data)
            .expect("[interceptor::write_register] Cannot write register\n");
        return 0;
    }

    pub fn add_code_hook(&mut self, function_name: String) {
        let mut uc = Unicorn::try_from(self.uc_handle).unwrap();
        {
            let data = self.emulator.write().unwrap();
            let mut hookmap = data.hookmap.write().unwrap();
            match hookmap.get("CODE") {
                Some(code_hookmap) => {
                    let mut code_hookmap = code_hookmap.try_write()
                        .expect("[interceptor::add_code_hook] Cannot lock code hookmap obj.\n");
                    code_hookmap.insert(0x0, function_name.clone());
                }
                None => {
                    hookmap.insert(String::from("CODE"), RwLock::new(HashMap::new()));
                    let mut code_hookmap = hookmap.get("CODE").unwrap().try_write()
                        .expect("[interceptor::add_code_hook] Cannot lock code hookmap obj.\n");
                    code_hookmap.insert(0x0, function_name.clone());
                }
            }
        }
        let ast = self.ast.clone_functions_only();
        let intercept = self.clone();
        let callback = move |_uc: &mut Unicorn<'_, ()>, addr: u64, size: u32| {

            let fn_name = function_name.clone();
            let eng = interceptor::make_engine();
            let mut scope = Scope::new();
            scope.set_value("Interceptor", intercept.clone());
            let ast = ast.clone();
            eng.call_fn::<i64>(&mut scope, &ast, fn_name, (addr, size)).unwrap();
            ()
        };
        uc.add_code_hook(0x0, 0xFFFFFFFF, callback).unwrap();


    }
}

pub fn make_engine() -> Engine{
    let mut engine = Engine::new();
    engine.register_type::<Interceptor>().
        register_fn("read_register", Interceptor::read_register).
        register_fn("write_register", Interceptor::write_register).
        register_fn("read_memory", Interceptor::read_memory).
        register_fn("write_memory", Interceptor::write_memory).
        register_fn("add_code_hook", Interceptor::add_code_hook);
    return engine;
}