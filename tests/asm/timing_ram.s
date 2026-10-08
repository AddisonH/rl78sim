; Instructions fetched from RAM: the manual gives a maximum of 2n+3 clocks
; (S2/S3, 7.2.2), which is what is charged. call !! (flash) 3, nop 5, nop 5,
; ret 15.
; CLOCKS: 28
; INSTRS: 4
	.section .vec,"a"
	.short _start
	.text
_start:
	movw sp, #0xfe00
	mov !(0x9000), #0x00	; nop
	mov !(0x9001), #0x00	; nop
	mov !(0x9002), #0xd7	; ret
	nop
t0:
	call !!0xf9000
t1:
	clrb a
	halt
