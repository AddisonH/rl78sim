; call !! 3, ret 6, callt 5, ret 6, push 1, pop 1, movw sp 1, push 1 + 1
; (SP written by the previous instruction, manual 7.2.3).
; CLOCKS: 25
; INSTRS: 8
	.section .vec,"a"
	.short _start
	.section .callt0,"a"
	.short ct
	.text
_start:
	movw sp, #0xfe00
	nop
t0:
	call !!f
	callt [0x0080]
	push ax
	pop ax
	movw sp, #0xfe00
	push ax
t1:
	clrb a
	halt
f:
	ret
ct:
	ret
