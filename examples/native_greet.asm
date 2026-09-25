default rel

section .rdata
greeting:
    db "Greeting from Goraw-asm called by Goraw language!", 10, 0

section .text
extern printf
global goraw_greet

goraw_greet:
    sub rsp, 40
    lea rcx, [rip + greeting]
    call printf
    add rsp, 40
    ret
