use unicorn_engine::Unicorn;

use crate::emulator::Emulator;
use std::sync::{Arc, RwLock};

#[derive(Clone)]
pub struct Interceptor <'a>{
    emulator: Arc<RwLock<Unicorn<'a, Emulator<'a>>>>
}

impl Interceptor <'_>{
    pub fn new<'a>(emulator: Arc<RwLock<Unicorn<'a, Emulator<'a>>>>) -> Interceptor <'a>{
        return Interceptor {emulator: emulator};
    }

    pub fn read_register(&mut self, reg_name: String) -> i64{
        let emulator = self.emulator.try_read().expect("[interceptor::read_register] Cannot Lock emulator obj\n");
        let ret = emulator.reg_read(Emulator::get_register(&reg_name)).expect("[interceptor::write_register] Cannot write register\n");
        return ret.try_into().unwrap();
    }

    pub fn write_register(&mut self, reg_name: String, value: i64) -> i64 {
        let mut emulator = self.emulator.try_write().expect("[interceptor::write_register] Cannot Lock emulator obj\n");
        emulator.reg_write(Emulator::get_register(&reg_name), value.try_into().unwrap()).expect("[interceptor::write_register] Cannot write register\n");
        return 0;
    }

    pub fn read_memory(&mut self, address: i64, size: i64) -> Vec<u8>{
        let emulator = self.emulator.try_read().expect("[interceptor::write_register] Cannot Lock emulator obj\n");
        let ret = emulator.mem_read_as_vec(address.try_into().unwrap(), size.try_into().unwrap()).expect("[interceptor::write_register] Cannot write register\n");
        return ret;
    }

    pub fn write_memory(&mut self, address: i64, data: Vec<u8>) -> i64{
        let data = &data; // b: &Vec<u8>
        let data: &[u8] = &data; // c: &[u8]
        let mut emulator = self.emulator.try_write().expect("[interceptor::write_register] Cannot Lock emulator obj\n");
        emulator.mem_write(address.try_into().unwrap(), data).expect("[interceptor::write_register] Cannot write register\n");
        return 0;
    }
    // pub fn add_hook()
}