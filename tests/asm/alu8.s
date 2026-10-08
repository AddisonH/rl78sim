; 8-bit arithmetic flags (manual 6.3): Z; AC = carry out of bit 3 / borrow
; into bit 3; CY. Each result byte and the PSW after it are stored in pairs.
; Reset PSW is 06H (ISP1, ISP0). Z=40H AC=10H CY=01H.
; EXPECT-MEM: 0xfa000 10 16 00 47 80 16 0f 16 ff 17 ff 17 30 16 00 56 10 16 0f 17 00 47 00 46 f0 06
	.section .vec,"a"
	.short _start
	.text
_start:
	movw sp, #0xfe00
	mov a, #0x0f		; 0F+01 = 10: AC
	add a, #0x01
	mov !(0xa000), a
	mov a, psw
	mov !(0xa001), a
	mov a, #0xf0		; F0+10 = 100: Z, CY
	add a, #0x10
	mov !(0xa002), a
	mov a, psw
	mov !(0xa003), a
	set1 cy			; 7E+01+1 = 80: AC
	mov a, #0x7e
	addc a, #0x01
	mov !(0xa004), a
	mov a, psw
	mov !(0xa005), a
	mov a, #0x10		; 10-01 = 0F: AC (borrow into bit 3)
	sub a, #0x01
	mov !(0xa006), a
	mov a, psw
	mov !(0xa007), a
	mov a, #0x00		; 00-01 = FF: AC, CY
	sub a, #0x01
	mov !(0xa008), a
	mov a, psw
	mov !(0xa009), a
	set1 cy			; 05-05-1 = FF: AC, CY
	mov a, #0x05
	subc a, #0x05
	mov !(0xa00a), a
	mov a, psw
	mov !(0xa00b), a
	mov a, #0x30		; CMP 30, 0A: AC (low nibble 0 < A), no CY
	cmp a, #0x0a
	mov !(0xa00c), a
	mov a, psw
	mov !(0xa00d), a
	mov a, #0x55		; AND: Z only; AC/CY kept (AC=1, CY=0)
	and a, #0xaa
	mov !(0xa00e), a
	mov a, psw
	mov !(0xa00f), a
	mov a, #0x0f		; INC 0F: AC; CY kept
	inc a
	mov !(0xa010), a
	mov a, psw
	mov !(0xa011), a
	set1 cy			; DEC 10: AC; CY kept (1)
	mov a, #0x10
	dec a
	mov !(0xa012), a
	mov a, psw
	mov !(0xa013), a
	mov a, #0x01		; DEC 01: Z, no AC; CY kept (1)
	dec a
	mov !(0xa014), a
	mov a, psw
	mov !(0xa015), a
	mov a, #0x00		; CMP0 0: Z; AC and CY cleared
	set1 cy
	cmp0 a
	mov !(0xa016), a
	mov a, psw
	mov !(0xa017), a
	mov a, #0xff		; XOR: F0, Z clear; AC/CY kept (0)
	xor a, #0x0f
	mov !(0xa018), a
	mov a, psw
	mov !(0xa019), a
	clrb a
	halt
