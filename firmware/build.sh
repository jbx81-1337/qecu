#!/bin/sh
# Rebuilds firmware.elf (needs clang + ld.lld).
set -e
cd "$(dirname "$0")"
clang --target=aarch64-none-elf -c firmware.S -o firmware.o
ld.lld -T firmware.ld -z norelro --no-rosegment firmware.o -o firmware.elf
rm -f firmware.o
