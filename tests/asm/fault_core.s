; MULHU does not exist on the S2 core.
; ARGS: core=s2
; STOP: illegal
	.section .vec,"a"
	.short _start
	.text
_start:
	movw sp, #0xfe00
	mulhu
	clrb a
	halt
