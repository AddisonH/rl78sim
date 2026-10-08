; S3 clocks from the operation list (Table 5-7), code in flash:
; nop 1, mov 1, movw 1, addw 1, mulhu 2, mach 3, movw 1, divhu 9, nop 1,
; divwu 17, nop 1.
; CLOCKS: 38
; INSTRS: 11
	.section .vec,"a"
	.short _start
	.text
_start:
	movw sp, #0xfe00
t0:
	nop
	mov a, #1
	movw ax, #0x1234
	addw ax, #1
	mulhu
	mach
	movw de, #1
	divhu
	nop
	divwu
	nop
t1:
	clrb a
	halt
