//! Example qecu plugin. Build with:
//!   rustup target add wasm32-wasip1
//!   cargo build --release --target wasm32-wasip1   (inside plugins/example)

mod host {
    #[link(wasm_import_module = "qecu")]
    extern "C" {
        pub fn add_code_hook(begin: i64, size: i64) -> i64;
        pub fn on_event(ptr: *const u8, len: usize);
        pub fn write_register(ptr: *const u8, len: usize, value: i64);
        pub fn set_pc(addr: i64);
        pub fn sleep(ms: i64);
    }
}

const WAIT_EVENT: i64 = 0x800019de;
const WAIT_TIME: i64 = 0x80000200;
const STARTUP_DONE: i64 = 0x80003f4c;

static mut HOOKS: [i64; 3] = [0; 3];

#[no_mangle]
pub extern "C" fn qecu_alloc(len: usize) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(len);
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

#[no_mangle]
pub extern "C" fn qecu_init() {
    println!("> Interceptor initialization.");
    unsafe {
        HOOKS[0] = host::add_code_hook(WAIT_EVENT, 4);
        HOOKS[1] = host::add_code_hook(WAIT_TIME, 4);
        HOOKS[2] = host::add_code_hook(STARTUP_DONE, 4);
        let ev = "can-recv";
        host::on_event(ev.as_ptr(), ev.len());
    }
}

#[no_mangle]
pub extern "C" fn qecu_on_code(id: i64, _addr: i64, _size: i32) {
    unsafe {
        if id == HOOKS[0] {
            println!("> IfxCpu_waitEvent. Skipping");
            host::write_register("D2".as_ptr(), 2, 0);
            host::set_pc(0x80001a58);
        } else if id == HOOKS[1] {
            host::sleep(10);
            host::set_pc(0x800002e2);
        } else if id == HOOKS[2] {
            println!("> SoftwareStartup finished, core0_start...");
        }
    }
}

#[no_mangle]
pub extern "C" fn qecu_on_event(_tp: *const u8, _tl: usize, mp: *const u8, ml: usize) {
    let msg = unsafe { std::slice::from_raw_parts(mp, ml) };
    println!("{}", String::from_utf8_lossy(msg));
}
