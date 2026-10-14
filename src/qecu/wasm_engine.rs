//! WebAssembly plugin engine (Wasmtime, Cranelift JIT).
//!
//! Plugins are Rust (or anything else) compiled to `wasm32-wasip1` and loaded from
//! the `init_script` path of the workflow. See `plugins/example` for a plugin.
//!
//! Guest exports:
//!   * `memory`                                  linear memory
//!   * `qecu_alloc(len: i32) -> i32`             allocator used by the host to pass strings
//!   * `qecu_init()`                             optional, registers hooks
//!   * `qecu_on_code(id: i64, addr: i64, size: i32)`   optional, code hook callback
//!   * `qecu_on_event(tp: i32, tl: i32, mp: i32, ml: i32)` optional, event callback
//!
//! Host imports (module `qecu`): see `link_host_api`.
use serde::{Deserialize, Serialize};
use wasmtime::{Caller, Engine, Instance, Linker, Memory, Module, Store, TypedFunc};
use wasmtime_wasi::p1::{self, WasiP1Ctx};
use wasmtime_wasi::WasiCtxBuilder;
use std::{thread::sleep, time};

use super::emulator::Emulator;

#[derive(Clone, Deserialize, Serialize)]
pub struct CodeHook {
    id: u64,
    begin: u64,
    end: u64,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct EventCallback {
    event_type: String,
}

struct HostState {
    emulator: Option<Emulator<'static>>,
    wasi: WasiP1Ctx,
    code_hooks: Vec<CodeHook>,
    on_events: Vec<EventCallback>,
    next_id: u64,
}

struct Guest {
    store: Store<HostState>,
    alloc: TypedFunc<i32, i32>,
    memory: Memory,
    on_code: Option<TypedFunc<(i64, i64, i32), ()>>,
    on_event: Option<TypedFunc<(i32, i32, i32, i32), ()>>,
}

pub struct WasmEngine {
    engine: Engine,
    module: Module,
    emulator: Option<Emulator<'static>>,
    guest: Option<Guest>,
    code_hooks: Vec<CodeHook>,
    on_events: Vec<EventCallback>,
}

// Emulator is shared across threads behind mutexes, same contract as before.
unsafe impl Send for WasmEngine {}

fn guest_memory(caller: &mut Caller<'_, HostState>) -> wasmtime::Result<Memory> {
    caller
        .get_export("memory")
        .and_then(|e| e.into_memory())
        .ok_or_else(|| wasmtime::format_err!("guest does not export `memory`"))
}

fn read_guest(caller: &mut Caller<'_, HostState>, ptr: i32, len: i32) -> wasmtime::Result<Vec<u8>> {
    let mem = guest_memory(caller)?;
    let mut buf = vec![0u8; len as u32 as usize];
    mem.read(&caller, ptr as u32 as usize, &mut buf)?;
    Ok(buf)
}

fn read_guest_str(caller: &mut Caller<'_, HostState>, ptr: i32, len: i32) -> wasmtime::Result<String> {
    Ok(String::from_utf8_lossy(&read_guest(caller, ptr, len)?).into_owned())
}

fn emu(caller: &Caller<'_, HostState>) -> wasmtime::Result<Emulator<'static>> {
    caller
        .data()
        .emulator
        .clone()
        .ok_or_else(|| wasmtime::format_err!("emulator not attached"))
}

fn link_host_api(linker: &mut Linker<HostState>) -> wasmtime::Result<()> {
    // add_code_hook(begin, size) -> hook id
    linker.func_wrap("qecu", "add_code_hook", |mut c: Caller<'_, HostState>, begin: i64, size: i64| -> i64 {
        let st = c.data_mut();
        st.next_id += 1;
        let id = st.next_id;
        st.code_hooks.push(CodeHook { id, begin: begin as u64, end: (begin as u64).wrapping_add(size as u64) });
        id as i64
    })?;
    linker.func_wrap("qecu", "on_event", |mut c: Caller<'_, HostState>, p: i32, l: i32| -> wasmtime::Result<()> {
        let event_type = read_guest_str(&mut c, p, l)?;
        c.data_mut().on_events.push(EventCallback { event_type });
        Ok(())
    })?;
    linker.func_wrap("qecu", "read_register", |mut c: Caller<'_, HostState>, p: i32, l: i32| -> wasmtime::Result<i64> {
        let name = read_guest_str(&mut c, p, l)?;
        Ok(emu(&c)?.read_register(name) as i64)
    })?;
    linker.func_wrap("qecu", "write_register", |mut c: Caller<'_, HostState>, p: i32, l: i32, v: i64| -> wasmtime::Result<()> {
        let name = read_guest_str(&mut c, p, l)?;
        emu(&c)?.write_register(name, v as u64);
        Ok(())
    })?;
    // read_memory(addr, dst_ptr, len)
    linker.func_wrap("qecu", "read_memory", |mut c: Caller<'_, HostState>, addr: i64, dst: i32, len: i32| -> wasmtime::Result<()> {
        let data = emu(&c)?.read_memory(addr as u64, len as u32 as usize);
        let mem = guest_memory(&mut c)?;
        mem.write(&mut c, dst as u32 as usize, &data)?;
        Ok(())
    })?;
    // write_memory(addr, src_ptr, len)
    linker.func_wrap("qecu", "write_memory", |mut c: Caller<'_, HostState>, addr: i64, src: i32, len: i32| -> wasmtime::Result<()> {
        let data = read_guest(&mut c, src, len)?;
        emu(&c)?.write_memory(addr as u64, data);
        Ok(())
    })?;
    linker.func_wrap("qecu", "set_pc", |c: Caller<'_, HostState>, addr: i64| -> wasmtime::Result<()> {
        emu(&c)?.set_pc(addr as u64);
        Ok(())
    })?;
    linker.func_wrap("qecu", "stop", |c: Caller<'_, HostState>| -> wasmtime::Result<()> {
        emu(&c)?.stop();
        Ok(())
    })?;
    linker.func_wrap("qecu", "sleep", |_c: Caller<'_, HostState>, ms: i64| {
        sleep(time::Duration::from_millis(ms.max(0) as u64));
    })?;
    linker.func_wrap("qecu", "get_cpu_symbol", |mut c: Caller<'_, HostState>, cp: i32, cl: i32, sp: i32, sl: i32| -> wasmtime::Result<i32> {
        let cpu = read_guest_str(&mut c, cp, cl)?;
        let symbol = read_guest_str(&mut c, sp, sl)?;
        Ok(get_cpu_symbol(cpu, symbol) as i32)
    })?;
    // disas(addr, size, dst_ptr, dst_cap) -> bytes written
    linker.func_wrap("qecu", "disas", |mut c: Caller<'_, HostState>, addr: i64, size: i32, dst: i32, cap: i32| -> wasmtime::Result<i32> {
        let e = emu(&c)?;
        let code = e.read_memory(addr as u64, size as u32 as usize);
        let mut out = String::new();
        for ins in e.disas(code, addr as u64, size as u32) {
            out.push_str(&format!("{} {}\n", ins.mnemonic, ins.body));
        }
        let bytes = out.as_bytes();
        let n = bytes.len().min(cap as u32 as usize);
        let mem = guest_memory(&mut c)?;
        mem.write(&mut c, dst as u32 as usize, &bytes[..n])?;
        Ok(n as i32)
    })?;
    Ok(())
}

impl WasmEngine {
    /// Compile the plugin (`.wasm` or `.wat`) with Cranelift.
    pub fn new(plugin_path: String) -> WasmEngine {
        let engine = Engine::default();
        let module = Module::from_file(&engine, &plugin_path)
            .unwrap_or_else(|e| panic!("[wasm_engine::new] Cannot load plugin {}: {}", plugin_path, e));
        WasmEngine { engine, module, emulator: None, guest: None, code_hooks: Vec::new(), on_events: Vec::new() }
    }

    pub fn set_emulator(&mut self, emulator: Emulator<'static>) {
        self.emulator = Some(emulator);
    }

    /// Instantiate the plugin and run `qecu_init`.
    pub fn init(&mut self) {
        let mut linker: Linker<HostState> = Linker::new(&self.engine);
        p1::add_to_linker_sync(&mut linker, |s: &mut HostState| &mut s.wasi)
            .expect("[wasm_engine::init] Cannot link WASI");
        link_host_api(&mut linker).expect("[wasm_engine::init] Cannot link host API");

        let wasi = WasiCtxBuilder::new().inherit_stdout().inherit_stderr().build_p1();
        let mut store = Store::new(&self.engine, HostState {
            emulator: self.emulator.clone(),
            wasi,
            code_hooks: Vec::new(),
            on_events: Vec::new(),
            next_id: 0,
        });
        let instance: Instance = linker
            .instantiate(&mut store, &self.module)
            .expect("[wasm_engine::init] Cannot instantiate plugin");
        // WASI reactor modules export `_initialize`; commands have none.
        if let Ok(f) = instance.get_typed_func::<(), ()>(&mut store, "_initialize") {
            f.call(&mut store, ()).expect("[wasm_engine::init] _initialize failed");
        }
        let memory = instance.get_memory(&mut store, "memory").expect("[wasm_engine::init] plugin must export `memory`");
        let alloc = instance
            .get_typed_func::<i32, i32>(&mut store, "qecu_alloc")
            .expect("[wasm_engine::init] plugin must export `qecu_alloc`");
        if let Ok(f) = instance.get_typed_func::<(), ()>(&mut store, "qecu_init") {
            f.call(&mut store, ()).expect("[wasm_engine::init] qecu_init failed");
        }
        let on_code = instance.get_typed_func(&mut store, "qecu_on_code").ok();
        let on_event = instance.get_typed_func(&mut store, "qecu_on_event").ok();
        self.code_hooks = store.data().code_hooks.clone();
        self.on_events = store.data().on_events.clone();
        self.guest = Some(Guest { store, alloc, memory, on_code, on_event });
    }

    pub fn on_code_hook(&mut self, addr: u64, size: u32) {
        let guest = match self.guest.as_mut() { Some(g) => g, None => return };
        let on_code = match guest.on_code.clone() { Some(f) => f, None => return };
        let end = addr + size as u64;
        for hook in self.code_hooks.iter().filter(|h| addr >= h.begin && end <= h.end) {
            on_code
                .call(&mut guest.store, (hook.id as i64, addr as i64, size as i32))
                .unwrap_or_else(|e| panic!("[wasm_engine::on_code_hook] hook {} failed: {}", hook.id, e));
        }
    }

    pub fn emit(&mut self, event_type: String, msg: String) {
        if !self.on_events.iter().any(|e| e.event_type == event_type) {
            return;
        }
        let guest = match self.guest.as_mut() { Some(g) => g, None => return };
        let on_event = match guest.on_event.clone() { Some(f) => f, None => return };
        let mut put = |s: &str| -> (i32, i32) {
            let ptr = guest.alloc.call(&mut guest.store, s.len() as i32).expect("[wasm_engine::emit] qecu_alloc failed");
            guest.memory.write(&mut guest.store, ptr as u32 as usize, s.as_bytes()).expect("[wasm_engine::emit] write failed");
            (ptr, s.len() as i32)
        };
        let (tp, tl) = put(&event_type);
        let (mp, ml) = put(&msg);
        on_event
            .call(&mut guest.store, (tp, tl, mp, ml))
            .unwrap_or_else(|e| panic!("[wasm_engine::emit] handler for {} failed: {}", event_type, e));
    }

    pub fn get_code_hooks(&self) -> Vec<CodeHook> {
        self.code_hooks.clone()
    }

    pub fn get_event_hooks(&self) -> Vec<EventCallback> {
        self.on_events.clone()
    }
}

pub fn get_cpu_symbol(cpu: String, symbol: String) -> u32 {
    crate::qecu::arch::tricore::get_cpu_symbol(cpu, symbol)
}

impl std::fmt::Debug for WasmEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "WasmEngine")
    }
}
