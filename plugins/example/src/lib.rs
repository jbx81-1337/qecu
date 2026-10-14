//! Example qecu plugin. Build with:
//!   rustup target add wasm32-wasip1
//!   cargo build --release --target wasm32-wasip1   (inside plugins/example)

mod host {
    #[link(wasm_import_module = "qecu")]
    extern "C" {
        pub fn add_code_hook(begin: i64, size: i64) -> i64;
        pub fn on_event(ptr: *const u8, len: usize);
        pub fn read_register(ptr: *const u8, len: usize) -> i64;
        pub fn write_register(ptr: *const u8, len: usize, value: i64);
        pub fn set_pc(addr: i64);
        pub fn sleep(ms: i64);
        pub fn stop();
    }
}

// Addresses of the bare-metal AArch64 firmware under test (adapt to your ELF).
// Addresses in firmware/firmware.elf.
const START: i64 = 0x40000000;
const SKIP_ME: i64 = 0x40000018;
const AFTER_SKIP: i64 = 0x40000020;

const END: i64 = 0x40000024;

static mut HOOKS: [i64; 4] = [0; 4];

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
        HOOKS[0] = host::add_code_hook(SKIP_ME, 4);
        HOOKS[1] = host::add_code_hook(AFTER_SKIP, 4);
        HOOKS[2] = host::add_code_hook(START, 4);
        HOOKS[3] = host::add_code_hook(END, 4);
        let ev = "can-recv";
        host::on_event(ev.as_ptr(), ev.len());
    }
}

#[no_mangle]
pub extern "C" fn qecu_on_code(id: i64, _addr: i64, _size: i32) {
    unsafe {
        if id == HOOKS[0] {
            println!("> skip_me hit, skipping `mov x4, #0xdead` and `mov x5, #0xbeef`");
            host::write_register("X5".as_ptr(), 2, 0x10);
            host::set_pc(AFTER_SKIP);
        } else if id == HOOKS[1] {
            println!("> after_skip reached, X3 = {}", host::read_register("X3".as_ptr(), 2));
        } else if id == HOOKS[2] {
            println!("> firmware start");
            host::sleep(10);
        } else if id == HOOKS[3] {
            println!("> firmware_end reached, stopping emulation");
            host::stop();
        }
    }
}

#[no_mangle]
pub extern "C" fn qecu_on_event(_tp: *const u8, _tl: usize, mp: *const u8, ml: usize) {
    let msg = unsafe { std::slice::from_raw_parts(mp, ml) };
    println!("{}", String::from_utf8_lossy(msg));
}
