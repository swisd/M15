; --- Memory Setup ---
ARRAY_START = $0200    ; Start of memory array in RAM

; --- Program Start ---
    LDX #$05           ; Set loop counter to 5
    LDA #$00           ; Clear accumulator to 0

LOOP:
    CLC                ; Clear carry before addition
    ADC #$02           ; Add 2 to accumulator

    ; --- Stack Operations ---
    PHA                ; Push current sum onto the stack

    ; --- Memory Operations ---
    DEX                ; Decrement X (acts as loop counter AND array index)
    STA $0200,X  ; Store sum in array (Addresses: $0204, $0203, $0202...)

    ; --- Loop Check ---
    TXA                ; Copy X to A to check if counter hit 0 (DEX already ran)
    BNE LOOP           ; Repeat if X is not 0

    ; --- Retrieving from Stack ---
    PLA                ; Pull the last value off the stack (A now equals 10)

    BRK                ; Stop execution


