.model small
.stack 100h
.data
    num1 dw 10
    num2 dw 20
    largest dw ?
.code
main proc
    mov ax, @data        ; Initialize data segment
    mov ds, ax
mov ax, num1         ; Load num1 into AX
    mov bx, num2         ; Load num2 into BX
    cmp ax, bx           ; Compare AX and BX
    jg ax_is_larger      ; Jump if AX > BX
    mov ax, bx           ; Otherwise, BX is larger
ax_is_larger:
    mov largest, ax      ; Store the largest number
    mov ax, 4c00h        ; Exit program
    int 21h
main endp
end main