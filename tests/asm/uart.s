; libgloss writes stdout to UART0 SDR00 (FFF10H); the first zero byte (baud
; setup) is dropped.
; UART: Hi\n
	.section .vec,"a"
	.short _start
	.text
_start:
	movw sp, #0xfe00
	mov 0xfff10, #0
	mov 0xfff10, #0x48
	mov 0xfff10, #0x69
	mov 0xfff10, #0x0a
	mov a, #3
	halt
; STOP: exit 3
