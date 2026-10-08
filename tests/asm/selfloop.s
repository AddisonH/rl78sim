; newlib's nosys _exit is "br $": treated as exit with the code in X.
; STOP: exit 7
	.section .vec,"a"
	.short _start
	.text
_start:
	movw sp, #0xfe00
	movw ax, #7
	br $.
