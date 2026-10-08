; Stores to the flash mirror stop the simulation (hardware ignores them).
; STOP: fault
	.section .vec,"a"
	.short _start
	.text
_start:
	movw sp, #0xfe00
	mov !(0x5000), #1
	clrb a
	halt
