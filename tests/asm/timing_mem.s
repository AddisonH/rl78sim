; Reading code flash as data uses the "note 2" clocks (manual 7.2.1), also
; through the mirror; a register written by the previous instruction and used
; for addressing costs one wait (7.2.3).
; mov a,[hl] (flash) 4, mov a,[de] (RAM) 1, mov a,!3000 (mirror) 4,
; mov a,!a000 1, mov es 1, mov a,es:!3000 5, movw hl 1, mov a,[hl] 1+1,
; movw hl 1, nop 1, mov a,[hl] 1, mov l 1, mov a,[hl+0] 1+1, movw hl 1,
; nop 1, bt [hl].0 (flash, taken: C3H has bit 0 set) 7.
; CLOCKS: 34
; INSTRS: 16
	.section .vec,"a"
	.short _start
	.section .rodata,"a"
	.byte 0xc3
	.text
_start:
	movw sp, #0xfe00
	movw hl, #0x3000
	movw de, #0xa000
	nop
t0:
	mov a, [hl]
	mov a, [de]
	mov a, !(0x3000)
	mov a, !(0xa000)
	mov es, #0
	mov a, es:!(0x3000)
	movw hl, #0xa000
	mov a, [hl]
	movw hl, #0xa002
	nop
	mov a, [hl]
	mov l, #2
	mov a, [hl+0]
	movw hl, #0x3000
	nop
	bt [hl].0, $5f
	nop
5:
t1:
	clrb a
	halt
