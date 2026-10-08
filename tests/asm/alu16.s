; 16-bit arithmetic: Z and CY. AC is undefined after ADDW/SUBW/CMPW, so it
; is masked out (PSW & EFH). INCW/DECW and ADDW/SUBW SP leave flags alone.
; EXPECT-MEM: 0xfa000 00 00 47
; EXPECT-MEM: 0xfa004 ff ff 07
; EXPECT-MEM: 0xfa008 34 12 06
; EXPECT-MEM: 0xfa00c 34 12 46
; EXPECT-MEM: 0xfa010 00 00 07 06 02 fe
	.section .vec,"a"
	.short _start
	.text
_start:
	movw sp, #0xfe00
	movw ax, #0xffff	; FFFF+1 = 0: Z, CY
	addw ax, #0x0001
	movw !(0xa000), ax
	mov a, psw
	and a, #0xef
	mov !(0xa002), a
	movw ax, #0x0000	; 0-1 = FFFF: CY
	subw ax, #0x0001
	movw !(0xa004), ax
	mov a, psw
	and a, #0xef
	mov !(0xa006), a
	movw ax, #0x1234	; CMPW 1234 > 0001: no Z, no CY
	cmpw ax, #0x0001
	movw !(0xa008), ax
	mov a, psw
	and a, #0xef
	mov !(0xa00a), a
	movw ax, #0x1234	; CMPW equal: Z
	cmpw ax, #0x1234
	movw !(0xa00c), ax
	mov a, psw
	and a, #0xef
	mov !(0xa00e), a
	set1 cy			; INCW FFFF: no flags (Z=0 from the AND above, CY=1 kept)
	movw ax, #0xffff
	incw ax
	movw !(0xa010), ax
	mov a, psw
	and a, #0xef
	mov !(0xa012), a
	clr1 cy			; ADDW SP, #2: flags unchanged
	clr1 psw.6
	addw sp, #2
	mov a, psw
	and a, #0xef
	mov !(0xa013), a
	movw ax, sp
	movw !(0xa014), ax
	clrb a
	halt
