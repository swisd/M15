.model small
.stack 100h

.data
    BUFFER DB 256 DUP(0FH)
    num1 dw 20       ; First number
    num2 dw 5       ; Second number
    result dw ?     ; Result variable
    msg db 'OK', '$'


.code
main proc
    mov bx, @data        ; Initialize data segment
    mov ds, bx
mov bx, num1         ; Load num1 into AX
    add bx, num2         ; Add num2 to AX
    mov dx, result       ; Store the result
    MOV AH, 02H     ; Select DOS function 2: Display character
    INT 21H         ; Call the DOS interrupt to execute
    MOV AH, 09H         ; DOS function to print a string
    MOV DX, OFFSET msg  ; Load address of string (must end with '$')
    lea cx, msg
    INT 21H             ; Execute
    mov ax, 4c00h        ; Exit program
    int 21h
main endp
end main


