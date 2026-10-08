; BRK with no handler (vector 0000H or erased FFFFH) ends the run.
; STOP: brk
	.section .vec,"a"
	.short _start
	.text
_start:
	movw sp, #0xfe00
	brk
