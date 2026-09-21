; Goraw-asm: функция суммы по Win64 ABI (аргументы в rcx, rdx; результат в rax)
section .text
global goraw_sum

goraw_sum:
    mov rax, rcx     ; a
    add rax, rdx     ; + b
    ret
