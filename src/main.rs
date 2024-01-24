mod qecu;
mod utils;
use std::{fs, sync::{Arc, RwLock}, borrow::{BorrowMut, Borrow}};
use crate::qecu::{emulator::{self}, interceptor::Interceptor};
use elf::file;
use rhai::Engine;
use rhai::Scope;

fn main() {
    let mut _engine = Engine::new();
    let mut _scope = Scope::new();
    let config: String = fs::read_to_string("./config.yml").unwrap();
    let wf = utils::workflow::Workflow::new(config);
    print!("{}\n", wf.project);
    let file_path = wf.init_script.clone();
    let mut emulator = emulator::Emulator::new(wf);
    let init_script: String = fs::read_to_string(file_path).expect("Cannot open file");
    let mut me = Arc::new(RwLock::new(emulator));
    {       
            let mut scope;
            {
                let mut locked = me.write().unwrap();
                let mut emulator_data = locked.get_data_mut();
                scope = emulator_data.scope.clone();
                scope.push("emulator", me.clone());
            }
            let mut engine = Engine::new();
            engine.register_type::<Interceptor>().
                register_fn("new_interceptor", Interceptor::new).
                register_fn("read_register", Interceptor::read_register).
                register_fn("write_register", Interceptor::write_register).
                register_fn("read_memory", Interceptor::read_memory).
                register_fn("write_memory", Interceptor::write_memory);
        let r = engine.run_with_scope(&mut scope, &init_script);
    }
    // {
    //     me.write().unwrap().emu_start(0x00, 0x00, 0x00, 0x00).unwrap();
    //     print!("here");
    // }

}