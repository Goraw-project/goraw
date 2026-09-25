; native_hello.asm — чистый Goraw-asm с extern, секцией данных и CRT-вызовом
default rel

section .rdata
msg:
    db "Hello from native Goraw-asm with external relocations and .rdata!", 10, 0

section .text
extern printf
global main

main:
    sub rsp, 40
    lea rcx, [rip + msg]
    call printf
    add rsp, 40
    xor eax, eax
    ret
