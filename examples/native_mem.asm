section .text
global gsum3
global gstore
global gelem

; rcx = *i64; вернуть a[0]+a[1]+a[2]
gsum3:
    mov rax, [rcx]
    add rax, [rcx + 8]
    mov r8, [rcx + 16]
    add rax, r8
    ret

; rcx = *i64, rdx = val; a[0] = val
gstore:
    mov [rcx], rdx
    ret

; rcx = *i64, rdx = i; вернуть адрес &a[i]  (rcx + i*8)
gelem:
    lea rax, [rcx + rdx * 8]
    ret
