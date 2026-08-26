.include "m328pdef.inc"

.equ F_CPU = 16000000
.equ BAUD = 9600
.equ UBRR_VAL = (F_CPU/(16*BAUD))-1

.org 0x0000
    rjmp reset

reset:
    ; Initialize Stack Pointer
    ldi r16, high(RAMEND)
    out SPH, r16
    ldi r16, low(RAMEND)
    out SPL, r16

    ; Set Baud Rate
    ldi r16, high(UBRR_VAL)
    out UBRR0H, r16
    ldi r16, low(UBRR_VAL)
    out UBRR0L, r16

    ; Enable Transmitter
    ldi r16, (1<<TXEN0)
    out UCSR0B, r16

    ; Set Frame Format: 8 data bits, 1 stop bit
    ldi r16, (1<<UCSZ01) | (1<<UCSZ00)
    out UCSR0C, r16

send_string:
    ; Load pointer to Hello World string (Z register)
    ldi ZH, high(msg * 2)
    ldi ZL, low(msg * 2)

transmit_loop:
    lpm r16, Z+          ; Load byte from program memory into r16
    cpi r16, 0           ; Check for null terminator
    breq hang            ; If zero, finish/hang

    ; Wait for empty transmit buffer
wait_buffer:
    in r17, UCSR0A
    sbrs r17, UDRE0
    rjmp wait_buffer

    ; Send character
    out UDR0, r16
    rjmp transmit_loop

hang:
    rjmp hang

msg:
    .db "Hello World!", 13, 10, 0
