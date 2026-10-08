; CMPS X, [HL+byte]: Z if X == mem; CY if the result is non-zero OR A == 0 OR
; X == 0; AC = borrow into bit 3. MOVS [HL+byte], X: Z if X == 0; CY if
; A == 0 OR X == 0 (manual 6.2, 6.3). rl78-elf-gdb gets both CY rules wrong.
; cmps: (A=05 X=10 m=10) (A=00 X=10 m=10) (A=05 X=10 m=11) (A=05 X=00 m=00)
; EXPECT-MEM: 0xfa020 46 47 17 47
; movs (A=05 X=07) (A=00 X=07) (A=05 X=00): stored value and PSW & 41H
; EXPECT-MEM: 0xfa030 07 00 07 01 00 41
	.section .vec,"a"
	.short _start
	.text
_start:
	movw sp, #0xfe00
	movw hl, #0xa000
	mov !(0xa000), #0x10
	mov !(0xa001), #0x11
	mov !(0xa002), #0x00
	mov x, #0x10
	mov a, #0x05
	cmps x, [hl+0]
	mov a, psw
	mov !(0xa020), a
	mov x, #0x10
	mov a, #0x00
	cmps x, [hl+0]
	mov a, psw
	mov !(0xa021), a
	mov x, #0x10
	mov a, #0x05
	cmps x, [hl+1]
	mov a, psw
	mov !(0xa022), a
	mov x, #0x00
	mov a, #0x05
	cmps x, [hl+2]
	mov a, psw
	mov !(0xa023), a

	mov x, #0x07
	mov a, #0x05
	movs [hl+0x30], x
	mov a, psw
	and a, #0x41
	mov !(0xa031), a
	mov x, #0x07
	mov a, #0x00
	movs [hl+0x32], x
	mov a, psw
	and a, #0x41
	mov !(0xa033), a
	mov x, #0x00
	mov a, #0x05
	movs [hl+0x34], x
	mov a, psw
	and a, #0x41
	mov !(0xa035), a
	clrb a
	halt
