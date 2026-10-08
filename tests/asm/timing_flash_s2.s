; RL78-S2 clocks when reading code flash as data (Table 5-6,
; "Note 2"; here through the mirror at F3000H): mov a,!addr16 1/4,
; movw ax,!addr16 1/4; a RAM read is 1.
; ARGS: core=s2
; CLOCKS: 9
; INSTRS: 3
	.section .vec,"a"
	.short _start
	.text
_start:
	movw sp, #0xfe00
	nop
t0:
	mov a, !0xfe00
	mov a, !0x3000
	movw ax, !0x3000
t1:
	clrb a
	halt
	.section .rodata,"a"
	.short 0x1234
