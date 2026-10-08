; Addressing (manual chapter 4).
; saddr operand byte 00H-1FH maps to FFF00H-FFF1FH, 20H-FFH to FFE20H-FFEFFH.
; [HL+B], word[BC], word[C], [DE+byte], [SP+byte] are near (F0000H + 16 bits).
; ES:[HL+byte] adds in 20 bits: ES=0EH HL=FFF0H +20H reaches F0010H.
; !addr16 reads the mirror (F3000H -> 03000H); ES:!addr16 with ES=0 reads
; flash directly. Results in order: saddr lo, saddr hi, [HL+B], word[BC],
; word[C], [DE+byte], [SP+byte], ES carry, mirror, far.
; EXPECT-MEM: 0xfa040 11 22 33 44 55 66 77 88 c3 c3
	.section .vec,"a"
	.short _start
	.section .rodata,"a"
	.byte 0xc3
	.text
_start:
	movw sp, #0xfe00
	mov 0xfff1c, #0x11	; saddr byte 1CH
	mov a, !(0xff1c)
	mov !(0xa040), a
	mov 0xffe30, #0x22	; saddr byte 30H
	mov a, !(0xfe30)
	mov !(0xa041), a

	mov !(0xa005), #0x33
	movw hl, #0xa000
	mov b, #5
	mov a, [hl+b]
	mov !(0xa042), a
	mov !(0xa013), #0x44
	movw bc, #0x0013
	mov a, 0xa000[bc]
	mov !(0xa043), a
	mov !(0xa027), #0x55
	mov c, #7
	mov a, 0xa020[c]
	mov !(0xa044), a
	mov !(0xa031), #0x66
	movw de, #0xa030
	mov a, [de+1]
	mov !(0xa045), a
	mov [sp+6], #0x77
	mov a, !(0xfe06)
	mov !(0xa046), a

	mov es, #0x0e
	movw hl, #0xfff0
	mov es:[hl+0x20], #0x88
	mov a, !(0x0010)	; F0010H
	mov !(0xa047), a

	mov a, !(0x3000)	; mirror F3000H -> flash 03000H
	mov !(0xa048), a
	mov es, #0
	mov a, es:!(0x3000)	; 03000H directly
	mov !(0xa049), a
	clrb a
	halt
