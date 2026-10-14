/* Dummy bare-metal AArch64 firmware used to exercise qecu. */
    .section .text
    .global _start
_start:
    mov  x0, #1
    mov  x1, #2
    add  x2, x0, x1          /* x2 = 3 */
    ldr  x9, =0x50000000     /* SRAM */
    str  x2, [x9]            /* store result */
    ldr  x3, [x9]
    .global skip_me
skip_me:
    mov  x4, #0xdead         /* plugin hook skips this (sets pc past it) */
    mov  x5, #0xbeef
    .global after_skip
after_skip:
    add  x6, x3, x5
    .global firmware_end
firmware_end:
    b    firmware_end        /* qecu stops here (plugin calls qecu::stop) */
