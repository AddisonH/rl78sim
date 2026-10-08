; RL78/G13 multiplier and divider/multiply-accumulator (R01UH0146 ch. 14),
; using the manual's own examples (Figures 14-6 to 14-10).
; MDAL FFFF0H, MDAH FFFF2H, MDBH FFFF4H, MDBL FFFF6H, MDCL F00E0H,
; MDCH F00E2H, MDUC F00E8H.
; ARGS: core=s2 mdu
; unsigned 2*3 = 6; FFFF*FFFF = FFFE0001 (MDBL, MDBH)
; EXPECT-MEM: 0xfa000 06 00 00 00 01 00 fe ff
; signed -2*32767 = -65534 (FFFF0002); FFFF*FFFF signed = 1
; EXPECT-MEM: 0xfa008 02 00 ff ff 01 00 00 00
; MAC unsigned: 2*3+3 = 9 (MDCL, MDCH, MDUC 40H); then 32767*2 + FFFF0002H =
; 0 with overflow (MDUC 44H)
; EXPECT-MEM: 0xfa010 09 00 00 00 40
; EXPECT-MEM: 0xfa016 00 00 00 00 44
; MAC signed: 2*3 + (-4) = 2 (MDUC 48H); then 32767*(-1) + 80000001H
; overflows to 7FFF8002H (MDUC 4CH: MACOF set, MACSF clear)
; EXPECT-MEM: 0xfa020 02 00 00 00 48
; EXPECT-MEM: 0xfa026 02 80 ff 7f 4c
; division 35/6 = 5 r 5, DIVST cleared after polling
; EXPECT-MEM: 0xfa030 05 00 00 00 05 00 00 00 80
	.section .vec,"a"
	.short _start
	.text
_start:
	movw sp, #0xfe00
	mov !(0x00e8), #0x00	; unsigned multiply
	movw ax, #2
	movw 0xffff0, ax
	movw ax, #3
	movw 0xffff2, ax
	nop
	movw ax, 0xffff6
	movw !(0xa000), ax
	movw ax, 0xffff4
	movw !(0xa002), ax
	movw ax, #0xffff
	movw 0xffff0, ax
	movw 0xffff2, ax
	nop
	movw ax, 0xffff6
	movw !(0xa004), ax
	movw ax, 0xffff4
	movw !(0xa006), ax

	mov !(0x00e8), #0x08	; signed multiply
	movw ax, #0xfffe
	movw 0xffff0, ax
	movw ax, #0x7fff
	movw 0xffff2, ax
	nop
	movw ax, 0xffff6
	movw !(0xa008), ax
	movw ax, 0xffff4
	movw !(0xa00a), ax
	movw ax, #0xffff
	movw 0xffff0, ax
	movw 0xffff2, ax
	nop
	movw ax, 0xffff6
	movw !(0xa00c), ax
	movw ax, 0xffff4
	movw !(0xa00e), ax

	mov !(0x00e8), #0x40	; unsigned multiply-accumulate
	movw ax, #3
	movw !(0x00e0), ax	; MDCL
	clrw ax
	movw !(0x00e2), ax	; MDCH
	movw ax, #2
	movw 0xffff0, ax
	movw ax, #3
	movw 0xffff2, ax
	nop
	nop
	movw ax, !(0x00e0)
	movw !(0xa010), ax
	movw ax, !(0x00e2)
	movw !(0xa012), ax
	mov a, !(0x00e8)
	mov !(0xa014), a
	movw ax, #0x0002
	movw !(0x00e0), ax
	movw ax, #0xffff
	movw !(0x00e2), ax
	movw ax, #0x7fff
	movw 0xffff0, ax
	movw ax, #2
	movw 0xffff2, ax
	nop
	nop
	movw ax, !(0x00e0)
	movw !(0xa016), ax
	movw ax, !(0x00e2)
	movw !(0xa018), ax
	mov a, !(0x00e8)
	mov !(0xa01a), a

	mov !(0x00e8), #0x48	; signed multiply-accumulate
	movw ax, #0xfffc
	movw !(0x00e0), ax
	movw ax, #0xffff
	movw !(0x00e2), ax
	movw ax, #2
	movw 0xffff0, ax
	movw ax, #3
	movw 0xffff2, ax
	nop
	nop
	movw ax, !(0x00e0)
	movw !(0xa020), ax
	movw ax, !(0x00e2)
	movw !(0xa022), ax
	mov a, !(0x00e8)
	mov !(0xa024), a
	movw ax, #0x0001
	movw !(0x00e0), ax
	movw ax, #0x8000
	movw !(0x00e2), ax
	movw ax, #0x7fff
	movw 0xffff0, ax
	movw ax, #0xffff
	movw 0xffff2, ax
	nop
	nop
	movw ax, !(0x00e0)
	movw !(0xa026), ax
	movw ax, !(0x00e2)
	movw !(0xa028), ax
	mov a, !(0x00e8)
	mov !(0xa02a), a

	mov !(0x00e8), #0x80	; division
	movw ax, #35
	movw 0xffff0, ax
	clrw ax
	movw 0xffff2, ax
	movw 0xffff4, ax
	movw ax, #6
	movw 0xffff6, ax
	mov !(0x00e8), #0x81
1:	mov a, !(0x00e8)
	bt a.0, $1b
	mov !(0xa038), a
	movw ax, 0xffff0
	movw !(0xa030), ax
	movw ax, 0xffff2
	movw !(0xa032), ax
	movw ax, !(0x00e0)
	movw !(0xa034), ax
	movw ax, !(0x00e2)
	movw !(0xa036), ax
	clrb a
	halt
