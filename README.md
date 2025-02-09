# QECU
QECU is an attempt to create an ECU emulator framework for the following architectures:
- Tricore
- Renesas
- MIPS
- ARM
- V850

Currently only Tricore is supported.

## INSTALLATION
```bash
sudo apt update
sudo apt install build-essential cmake pkg-config zlib1g-dev
git clone https://github.com/jbx81-1337/qecu
cd qecu
cargo build
```

## Features
Qecu should be able to support mainly three formats: Elf, Intel-Hex and raw binary. Currently only the ELF format is implemented.

The configuration of qecu start with a `YAML` file to be used as config. inside the `YAML` file is specified:
- Firmware location and format
- Memory Map
- Register values on Boot time
- Start and Stop address for the emulation

One testing example is present with the name `config.yml`

Under the hood QECU is using Unicorn (QEMU) and around that instance we can communicate with the Emulator in different ways.

The core feature QECU has is to be developed in Rust but being scriptable using Rhai, to do so we can use the aid of the `Interceptor`.

The Interceptor is Rhai living object that expose a way to interact with the Unicorn Emulator. With the aid of the Interceptor is possible to hook the execution of the Emulator at a certein address range and perform operation on registers and memory.

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
../rust-sleigh/vendor/share/sleigh/specfiles/Ghidra/Processors
> Loading tc375 Memory Map
> Loading Tricore tc375 CPU
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
