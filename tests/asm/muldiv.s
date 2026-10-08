; Multiply, divide, multiply-accumulate (manual 6.5).
; MULU: AX = A*X. MULHU/MULH: BCAX = AX*BC. DIVHU: AX/DE -> AX, rem DE; by 0:
; DE = AX, AX = FFFFH. DIVWU: BCAX/HLDE -> BCAX, rem HLDE; by 0: HLDE = BCAX,
; BCAX = FFFFFFFFH. MACHU: CY = carry out of 32 bits, AC = 0. MACH: CY = signed
; overflow (not merely a sign change), AC = sign of the result.
; mulu 200*100 = 4E20
; EXPECT-MEM: 0xfa000 20 4e
; mulhu FFFF*FFFF = FFFE0001 (AX, BC)
; EXPECT-MEM: 0xfa002 01 00 fe ff
; mulh -1*2 = -2
; EXPECT-MEM: 0xfa006 fe ff ff ff
; divhu 1000/7 = 142 r 6
; EXPECT-MEM: 0xfa00a 8e 00 06 00
; divhu 1234/0: AX = FFFF, DE = 1234
; EXPECT-MEM: 0xfa00e ff ff 34 12
; divwu 100000H/3 = 55555 r 1 (AX BC DE HL)
; EXPECT-MEM: 0xfa012 55 55 05 00 01 00 00 00
; divwu 12345678/0: BCAX = FFFFFFFF, HLDE = 12345678
; EXPECT-MEM: 0xfa01a ff ff ff ff 78 56 34 12
; machu FFFFFFF0 + 4*5 = 1_00000004: MACR 4, CY=1 AC=0 -> PSW 07
; EXPECT-MEM: 0xfa022 04 00 00 00 07
; mach 5 + (-10)*1 = -5: sign change but no overflow: CY=0, AC=1 -> PSW 16
; EXPECT-MEM: 0xfa028 fb ff ff ff 16
; mach 7FFFFFFF + 1*1: overflow: MACR 80000000, CY=1, AC=1 -> 17
; EXPECT-MEM: 0xfa02e 00 00 00 80 17
; mach 80000000 + (-1)*1: overflow: MACR 7FFFFFFF, CY=1, AC=0 -> 07
; EXPECT-MEM: 0xfa034 ff ff ff 7f 07
	.section .vec,"a"
	.short _start
	.text
_start:
	movw sp, #0xfe00
	mov a, #200
	mov x, #100
	mulu x
	movw !(0xa000), ax

	movw ax, #0xffff
	movw bc, #0xffff
	mulhu
	movw !(0xa002), ax
	movw ax, bc
	movw !(0xa004), ax

	movw ax, #0xffff
	movw bc, #0x0002
	mulh
	movw !(0xa006), ax
	movw ax, bc
	movw !(0xa008), ax

	movw ax, #1000
	movw de, #7
	divhu
	nop
	movw !(0xa00a), ax
	movw ax, de
	movw !(0xa00c), ax

	movw ax, #0x1234
	movw de, #0
	divhu
	nop
	movw !(0xa00e), ax
	movw ax, de
	movw !(0xa010), ax

	movw bc, #0x0010
	movw ax, #0x0000
	movw hl, #0
	movw de, #3
	divwu
	nop
	movw !(0xa012), ax
	movw ax, bc
	movw !(0xa014), ax
	movw ax, de
	movw !(0xa016), ax
	movw ax, hl
	movw !(0xa018), ax

	movw bc, #0x1234
	movw ax, #0x5678
	movw hl, #0
	movw de, #0
	divwu
	nop
	movw !(0xa01a), ax
	movw ax, bc
	movw !(0xa01c), ax
	movw ax, de
	movw !(0xa01e), ax
	movw ax, hl
	movw !(0xa020), ax

	movw ax, #0xfff0	; MACR = FFFFFFF0
	movw 0xffff0, ax
	movw ax, #0xffff
	movw 0xffff2, ax
	movw ax, #4
	movw bc, #5
	machu
	movw ax, 0xffff0
	movw !(0xa022), ax
	movw ax, 0xffff2
	movw !(0xa024), ax
	mov a, psw
	mov !(0xa026), a

	movw ax, #5		; MACR = 5
	movw 0xffff0, ax
	clrw ax
	movw 0xffff2, ax
	movw ax, #0xfff6	; -10
	movw bc, #1
	mach
	movw ax, 0xffff0
	movw !(0xa028), ax
	movw ax, 0xffff2
	movw !(0xa02a), ax
	mov a, psw
	mov !(0xa02c), a

	movw ax, #0xffff	; MACR = 7FFFFFFF
	movw 0xffff0, ax
	movw ax, #0x7fff
	movw 0xffff2, ax
	movw ax, #1
	movw bc, #1
	mach
	movw ax, 0xffff0
	movw !(0xa02e), ax
	movw ax, 0xffff2
	movw !(0xa030), ax
	mov a, psw
	mov !(0xa032), a

	clrw ax			; MACR = 80000000
	movw 0xffff0, ax
	movw ax, #0x8000
	movw 0xffff2, ax
	movw ax, #0xffff
	movw bc, #1
	mach
	movw ax, 0xffff0
	movw !(0xa034), ax
	movw ax, 0xffff2
	movw !(0xa036), ax
	mov a, psw
	mov !(0xa038), a
	clrb a
	halt
