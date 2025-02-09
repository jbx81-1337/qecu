use rhai::{Engine, Scope, AST};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::{fs, thread::sleep, time};
use std::fmt;
use rand::RngCore;


use super::emulator::Emulator;

#[derive(Clone, Deserialize, Serialize)]
pub struct CodeHook {
    id: u64,
    begin: u64,
    end: u64,
    code_type: u8,
    content: String
}


#[derive(Clone, Deserialize, Serialize)]
pub struct EventCallback {
    event_type: String,
    code_type: u8,
    content: String
}

#[derive(Clone)]
pub struct Interceptor <'a>{
    emulator: Option<Emulator<'static>>,
    ast: AST,
    scope: Scope<'a>,
    code_hooks: Vec<CodeHook>,
    hook_datas: HashMap<u64, rhai::Map>,
    on_events: Vec<EventCallback>
}

unsafe impl Send for Interceptor<'static>{}

impl <'a> Interceptor <'static> {
    pub fn new(init_script: String) -> Interceptor <'a>{
        let ast = {
            let  eng = make_engine();
            let script_code = fs::read_to_string(init_script).expect("[interceptor::new] Cannot open init_script");
            let ret = eng.compile(script_code).expect("[rhai::engine::compile]Cannot compile");
            ret
        };

        return Interceptor {
            emulator: None,
            ast: ast,
            scope: Scope::new(),
            code_hooks: Vec::new(),
            hook_datas: HashMap::new(),
            on_events: Vec::new()
        };
    }

    pub fn set_emulator(&mut self, emulator: Emulator<'static>) {
        self.emulator = Some(emulator);
    }
    
    pub fn init(&mut self) {
        let _engine = make_engine();
        let mut scope = self.scope.clone();
        scope.push("Interceptor", self.clone());
        _engine.run_ast_with_scope(&mut scope, &self.ast).expect("[engine::run_with_scope] Error running init script.\n");
        let intercept = scope.get_value::<Interceptor>("Interceptor").unwrap();
        self.code_hooks = intercept.code_hooks;
        self.on_events = intercept.on_events;
        self.hook_datas = intercept.hook_datas;
        self.scope = scope.clone();
    }

    pub fn disas(&mut self, address: i64, size: i64) -> String {
        let code = self.read_memory(address, size);
        let address: u64 = address.try_into().unwrap();
        let size: u32 = size.try_into().unwrap();
        let mut disas = self.emulator.as_ref().unwrap().disas(code, address, size);
        let mut out = String::new();
        for ins in disas {
            let line = format!("{} {}", ins.mnemonic, ins.body);
            out.push_str(line.as_str());
        }
        return out;
    }
    
    pub fn read_register(&mut self, reg_name: String) -> i64{
        self.emulator.as_ref().unwrap().read_register(reg_name).try_into().unwrap()
    }

    pub fn write_register(&mut self, reg_name: String, value: i64) -> i64 {
        self.emulator.as_ref().unwrap().write_register(reg_name, value.try_into().unwrap()).try_into().unwrap()
    }

    pub fn read_memory(&mut self, address: i64, size: i64) -> Vec<u8>{
        self.emulator.as_ref().unwrap().read_memory(address.try_into().unwrap(), size.try_into().unwrap())
    }

    pub fn write_memory(&mut self, address: i64, data: Vec<u8>) -> i64 {
        self.emulator.as_ref().unwrap().write_memory(address.try_into().unwrap(), data).try_into().unwrap()
    }

    pub fn add_cb_hook(&mut self, hook_type: String, address: i64, size: i64, callback: rhai::FnPtr) {
        let fn_name = callback.fn_name().to_string();
        match hook_type.as_str() {
            "CODE" => {
                let begin: u64 = address.try_into().unwrap();
                let end: u64 = (address + size).try_into().unwrap();
                let code_hook = CodeHook {
                    id: rand::thread_rng().next_u64(),
                    begin: begin, 
                    end: end, 
                    code_type: 1, 
                    content: fn_name.clone()
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
                    id: rand::thread_rng().next_u64(),
                    begin: begin, 
                    end: end, 
                    code_type: 0, 
                    content: function_name.clone()
                };
                self.code_hooks.push(code_hook);
            }
            _ => {
                panic!("[interceptor::add_hook] Unknown hook type {} ", hook_type);
            }
        }

    }

    pub fn add_hook_with_data(&mut self, hook_type: String, address: i64, size: i64, function_name: String, data: rhai::Map) {
        match hook_type.as_str() {
            "CODE" => {
                let begin: u64 = address.try_into().unwrap();
                let end: u64 = (address + size).try_into().unwrap();
                let id = rand::thread_rng().next_u64();
                let code_hook = CodeHook {
                    id : id,
                    begin: begin, 
                    end: end, 
                    code_type: 0, 
                    content: function_name.clone()
                };
                self.code_hooks.push(code_hook);
                self.hook_datas.insert(id, data);
            }
            _ => {
                panic!("[interceptor::add_hook] Unknown hook type {} ", hook_type);
            }
        }
    }

    pub fn add_cb_hook_with_data(&mut self, hook_type: String, address: i64, size: i64, callback: rhai::FnPtr, data: rhai::Map) {
        let fn_name = callback.fn_name().to_string();
        match hook_type.as_str() {
            "CODE" => {
                let begin: u64 = address.try_into().unwrap();
                let end: u64 = (address + size).try_into().unwrap();
                let id = rand::thread_rng().next_u64();
                let code_hook = CodeHook {
                    id: id,
                    begin: begin, 
                    end: end, 
                    code_type: 1, 
                    content: fn_name.clone()
                };
                self.code_hooks.push(code_hook);
                self.hook_datas.insert(id, data);
            }
            _ => {
                panic!("[interceptor::add_hook] Unknown hook type {} ", hook_type);
            }
        }

    }
    pub fn on_cb_event(&mut self, event_type: String, callback: rhai::FnPtr) {
        let fn_name = callback.fn_name().to_string();
        let evt = EventCallback {
            event_type: event_type,
            code_type: 1,
            content: fn_name
        };
        self.on_events.push(evt);

    }

    pub fn on_event(&mut self, event_type: String, function_name: String) {
        let evt = EventCallback {
            event_type: event_type,
            code_type: 1,
            content: function_name
        };
        self.on_events.push(evt);
    }

    pub fn sleep(&mut self, millis: i64) {
        let millis = time::Duration::from_millis(millis.try_into().unwrap());
        sleep(millis);
    }

    pub fn set_pc(&mut self, addr: i64) {
        self.emulator.as_mut().unwrap().set_pc(addr.try_into().unwrap());
    }

    pub fn on_code_hook(&mut self, addr: u64, size: u32) {
        let addr_rhai: i64 = addr.try_into().unwrap();
        let size_rhai: i64 = size.try_into().unwrap();
        let code_hooks = &self.code_hooks;
        let size: u64 = size.try_into().unwrap();
        for code_hook in code_hooks {
            if addr >= code_hook.begin && (addr + size) <= code_hook.end {
                let mut _engine = make_engine();
                let mut _scope = self.scope.clone();
                let ast = self.ast.clone_functions_only();
                let data = self.hook_datas.get(&code_hook.id);
                match code_hook.code_type {
                    0 => {
                        match data {
                            None => { _engine.call_fn::<i64>(&mut _scope, &ast, &code_hook.content, (addr_rhai, size_rhai))
                                             .expect(format!("[interceptor::on_code_hook] Cannot call function {} ", code_hook.content).as_str()); },
                            Some(data) =>  {
                                _engine.call_fn::<i64>(&mut _scope, &ast, &code_hook.content, (addr_rhai, size_rhai, data.clone()))
                                        .expect(format!("[interceptor::on_code_hook] Cannot call function {} ", code_hook.content).as_str()); 
                            }
                        }
                        
                    }
                    1 => {
                        match data {
                            None => { _engine.call_fn::<i64>(&mut _scope, &ast, &code_hook.content, (self.clone(), addr_rhai, size_rhai))
                                             .expect(format!("[interceptor::on_code_hook] Cannot call function {} ", code_hook.content).as_str()); },
                            Some(data) => { 
                                _engine.call_fn::<i64>(&mut _scope, &ast, &code_hook.content, (self.clone(), addr_rhai, size_rhai, data.clone()))
                                        .expect(format!("[interceptor::on_code_hook] Cannot call function {} ", code_hook.content).as_str()); }
                        } 
                        
                    }
                    _ => {}
                }
            }
        }
    }

    pub fn emit(&self, event_type: String, msg: String) {
        for event in &self.on_events {
            if event.event_type == event_type {
                let mut _engine = make_engine();
                let mut _scope = self.scope.clone();
                let ast = self.ast.clone_functions_only();
                let msg = msg.clone();
                match event.code_type {
                    0 => {
                        _engine.call_fn::<i64>(&mut _scope, &ast, &event.content, (event_type.clone(), msg))
                            .expect(format!("[interceptor::on_code_hook] Cannot call function {} ", event.content).as_str());
                    }
                    1 => { _engine.call_fn::<i64>(&mut _scope, &ast, &event.content, (self.clone(), event_type.clone(), msg))
                            .expect(format!("[interceptor::on_code_hook] Cannot call function {} ", event.content).as_str());
                    }
                    _ => {}
                }
            }
        }
    }

    pub fn get_code_hooks(&self) -> Vec<CodeHook> {
        self.code_hooks.clone()
    }

    pub fn get_event_hooks(&self) -> Vec<EventCallback> {
        self.on_events.clone()
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
        register_fn("add_hook_with_data", Interceptor::add_hook_with_data).
        register_fn("add_hook_with_data", Interceptor::add_cb_hook_with_data).
        register_fn("on_event", Interceptor::on_event).
        register_fn("on_event", Interceptor::on_cb_event).
        register_fn("disas", Interceptor::disas).
        register_fn("sleep", Interceptor::sleep).
        register_fn("set_pc", Interceptor::set_pc);
    return engine;
}

impl <'a> fmt::Debug for Interceptor<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "UcWrapper")
    }
}