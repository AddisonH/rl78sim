; [HL+byte] must not pass FFFFH (manual 4.2 caution); --strict stops on it.
; ARGS: strict
; STOP: undefined
	.section .vec,"a"
	.short _start
	.text
_start:
	movw sp, #0xfe00
	movw hl, #0xfff0
	mov a, [hl+0x20]
	clrb a
	halt
