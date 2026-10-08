; Register banks (manual 3.2): bank n lives 8n bytes below FFEF8H; RBS0 is
; PSW bit 3, RBS1 bit 5.
; EXPECT: AX=0x1234 BC=0x5678
; EXPECT-MEM: 0xffef0 34 12
; EXPECT-MEM: 0xffee2 78 56
; PSW & 28H in bank 3 and bank 2, full PSW back in bank 0:
; EXPECT-MEM: 0xfa000 28 20 06
; STOP: exit 0x12
	.section .vec,"a"
	.short _start
	.text
_start:
	movw sp, #0xfe00
	sel rb1
	movw ax, #0x1234	; bank 1 AX = FFEF0H
	sel rb3
	movw bc, #0x5678	; bank 3 BC = FFEE2H
	mov a, psw		; (bank 3's A)
	and a, #0x28
	mov !(0xa000), a
	sel rb2
	mov a, psw
	and a, #0x28
	mov !(0xa001), a
	sel rb0
	mov a, psw
	mov !(0xa002), a
	movw ax, 0xffef0
	movw bc, 0xffee2
	halt
