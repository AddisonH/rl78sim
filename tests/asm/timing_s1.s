; RL78-S1 clocks (Table 5-5): movw ax,#word 2, push 2, pop 2, call !! 4, ret 7.
; ARGS: core=s1
; CLOCKS: 17
; INSTRS: 5
	.section .vec,"a"
	.short _start
	.text
_start:
	movw sp, #0xfe00
	nop
t0:
	movw ax, #1
	push ax
	pop ax
	call !!f
t1:
	clrb a
	halt
f:
	ret
