mod qecu;
mod utils;
use std::{fs, sync::{Arc, RwLock}};
use crate::qecu::{emulator::{self}, interceptor::{self, Interceptor}};
use rhai::Engine;
use rhai::Scope;
use unicorn_engine::Unicorn;

fn main() {
    let mut _engine = Engine::new();
    let mut _scope = Scope::new();
    let config: String = fs::read_to_string("./config.yml").unwrap();
    let wf = utils::workflow::Workflow::new(config);
    print!("{}\n", wf.project);
    let file_path = wf.init_script.clone();
    let sleigh_path = wf.sleigh_path.clone();
    let emulator = emulator::Emulator::new(wf);
    let init_script: String = fs::read_to_string(file_path).expect("Cannot open file");
    let uc_handle = {
        emulator.uc.get_handle()
    };
    let me = Arc::new(RwLock::new(emulator));
    {
        let engine = interceptor::make_engine();
        let mut scope: Scope;
        {
            let mut locked = me.try_write().expect("[main] Cannot lock emulator\n");      
            let ast_script = engine.compile(init_script.clone()).expect("[engine::compile] Cannot compile script\n");
            scope = Scope::new();
            locked.ast = ast_script.clone();
            scope.push("Interceptor", Interceptor::new(me.clone(), uc_handle, ast_script.clone(), sleigh_path));
        }
        engine.run_with_scope(&mut scope, &init_script).expect("[engine::run_with_scope] Error running init script.\n");
        {
            me.try_write().unwrap().scope = scope.clone();
        }
    }
    let mut uc = Unicorn::try_from(uc_handle).unwrap();
    uc.emu_start(0x80003d10, 0xFFFFFFFF, 0x00, 0x00).unwrap();

}