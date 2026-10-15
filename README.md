# QECU
QECU is an attempt to create an ECU emulator framework for the following architectures:
- AArch64 (bare-metal firmware ELF, not Linux)
- Renesas
- MIPS
- ARM
- V850

Currently only AArch64 is supported (disassembly via Capstone).

## INSTALLATION
```bash
sudo apt update
sudo apt install build-essential cmake pkg-config zlib1g-dev
git clone https://github.com/jbx81-1337/qecu
cd qecu
cargo build
```

## Features
Qecu should be able to support mainly three formats: ELF, Intel-Hex and raw binary. ELF and raw binary formats are implemented.

The configuration of qecu start with a `YAML` file to be used as config. inside the `YAML` file is specified:
- Firmware location and format
- Memory Map
- Register values on Boot time
- Start and Stop address for the emulation

Raw binary files have no embedded load address. A raw file can be attached to a memory map entry with `content`; its bytes are loaded at that entry's `from` address, or at `content_address` if set (it must fall inside the region):

```yaml
mem_map:
  - from: 0x02000000
    size: 0x00040000
    flags: RWX
    content: ./firmware.bin

boot:
  begin: 0x02027000
```

`input` is optional and can be omitted (or left empty) when all firmware comes from `mem_map.content`. If `input` is set, it is loaded first and `content` regions are written afterwards, so they take precedence. A raw `input.path` is loaded at address `0`. The execution start is `entry`, then `boot.begin`; otherwise the ELF entry, or the first `content` region's address (or `0`) for raw/no input.

One testing example is present with the name `config.yml`

Under the hood QECU is using Unicorn (QEMU) and around that instance we can communicate with the Emulator in different ways.

The core feature QECU has is to be developed in Rust but being extensible through WebAssembly plugins (Rust compiled to `wasm32-wasip1`, JIT-compiled and sandboxed by Wasmtime).

The plugin (the `init_script` path in the config) imports the host module `qecu` (`add_code_hook`, `on_event`, `read_register`, `write_register`, `read_memory`, `write_memory`, `set_pc`, `stop`, `sleep`, `disas`, `get_cpu_symbol`) and exports `qecu_alloc`, `qecu_init`, `qecu_on_code`, `qecu_on_event`. See `plugins/example` and `src/qecu/wasm_engine.rs`. With it is possible to hook the execution of the Emulator at a certein address range and perform operation on registers and memory.

Also an API server is in development to being able to expose information and accept external event.

By supporting external event and hooking them through the interceptor class we are able to receive information from the outside. This can be useful for example to implement fuzzers or scanner.

The last piece of QECU to have a minimum MVP sould be a way to send information. The idea is to implement a minimal HTTP client able to be invoked like this:
```js
Interceptor.emit_event('http://.../uds/response', method="POST", data=data);
```

```
$ cargo run
Emulator Config
[qecu::loader] Loading segments.
[qecu::loader] Loading sections.
[unicorn::mem_map] address: 0x0 size: 268435456
[unicorn::mem_map] address: 0x50000000 size: 268435456
[unicorn::mem_map] address: 0x60000000 size: 268435456
[unicorn::mem_map] address: 0x70000000 size: 268435456
[unicorn::mem_map] address: 0x80000000 size: 268435456
[unicorn::mem_map] address: 0xa0000000 size: 268435456
[unicorn::mem_map] address: 0xd0000000 size: 268435456
[unicorn::mem_map] address: 0xf0000000 size: 268435456
[unicorn::mem_write] address: 0x70000000 size: 0
[unicorn::mem_write] address: 0x70000008 size: 0
...
[unicorn::mem_write] address: 0x80000100 size: 256
[unicorn::mem_write] address: 0x80300000 size: 256
[unicorn::mem_write] address: 0x80300100 size: 256
[unicorn::mem_write] address: 0xa0300200 size: 32
[unicorn::mem_write] address: 0x0 size: 92
[unicorn::mem_write] address: 0x0 size: 1319440
[unicorn::reg_write] register: A0 value: 0x1337
> Loading tc375 Memory Map
> Loading AArch64 CPU
> Mempry Map loaded
> Interceptor initialization.
Address of ASCLIN1_ACCEN0 is f00007fc
Address of ASCLIN1_ACCEN0 is f00007fc
Address of ASCLIN1_ACCEN0 is f00007fc
...
```
### Conclusion
This project is a toy and mainly it was to learn more about Automotive, since I've left that industry I am not longer activily developing it but on my free-time I generally try to allocate some time mainly to learn Rust.
Every contribution is well accepted.

### Try it with the dummy AArch64 firmware
```bash
firmware/build.sh                       # optional: rebuilds firmware/firmware.elf (clang + ld.lld)
(cd plugins/example && cargo build --release --target wasm32-wasip1)
cargo run                               # uses config.yml
curl -X POST localhost:3000/emit/can-recv -H 'content-type: application/json' -d '{"msg":"hello"}'
```
