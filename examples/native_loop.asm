; Goraw-asm: вычисление суммы чисел от 1 до N в цикле
; Win64 ABI: n в rcx, результат в rax
section .text
global goraw_sum_to_n

goraw_sum_to_n:
    mov rax, 0
.loop:
    cmp rcx, 0
    jle .done
    add rax, rcx
    dec rcx
    jmp .loop
.done:
    ret
