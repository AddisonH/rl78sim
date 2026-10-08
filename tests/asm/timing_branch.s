; Branch clocks are "not taken/taken"; a skipped instruction costs 1 clock,
; 2 when ES:-prefixed (manual 6.14).
; clr1 cy 1, bc (not taken) 2, bnc (taken) 4, mov 1, bt a.0 (taken) 5,
; bf a.0 (not taken) 3, set1 cy 1, skc 1 + skipped 1, sknc 1, mov 1,
; skc 1 + skipped ES: 2.
; CLOCKS: 24
; INSTRS: 11
	.section .vec,"a"
	.short _start
	.text
_start:
	movw sp, #0xfe00
t0:
	clr1 cy
	bc $1f
1:
	bnc $2f
	nop
2:
	mov a, #1
	bt a.0, $3f
	nop
3:
	bf a.0, $4f
4:
	set1 cy
	skc
	mov a, #2
	sknc
	mov a, #3
	skc
	mov a, es:!(0x1234)
t1:
	clrb a
	halt
