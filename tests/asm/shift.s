; Shifts and rotates (manual 6.7, 6.8): CY = the last bit shifted out; no
; other flag changes. Each result is followed by CY (read with ROLC into 0).
; shl 81<<1 = 02 CY1; shr 81>>1 = 40 CY1; sar 81>>2 = E0 CY0 (bit 1)
; EXPECT-MEM: 0xfa000 02 01 40 01 e0 00
; shlw 8001<<1 = 0002 CY1; shrw 8003>>2 = 2000 CY1; sarw 8000>>15 = FFFF CY0
; EXPECT-MEM: 0xfa006 02 00 01
; EXPECT-MEM: 0xfa00a 00 20 01
; EXPECT-MEM: 0xfa00e ff ff 00
; ror 01 = 80 CY1; rol 80 = 01 CY1; rorc 01 (CY0) = 00 CY1; rolc 80 (CY1) = 01 CY1
; EXPECT-MEM: 0xfa011 80 01 01 01 00 01 01 01
; rolwc 8000 (CY0) = 0000 CY1
; EXPECT-MEM: 0xfa01a 00 00 01
	.section .vec,"a"
	.short _start
	.text
_start:
	movw sp, #0xfe00
	mov a, #0x81
	shl a, 1
	mov !(0xa000), a
	mov a, #0
	rolc a, 1
	mov !(0xa001), a
	mov a, #0x81
	shr a, 1
	mov !(0xa002), a
	mov a, #0
	rolc a, 1
	mov !(0xa003), a
	mov a, #0x81
	sar a, 2
	mov !(0xa004), a
	mov a, #0
	rolc a, 1
	mov !(0xa005), a

	movw ax, #0x8001
	shlw ax, 1
	movw !(0xa006), ax
	mov a, #0
	rolc a, 1
	mov !(0xa008), a
	movw ax, #0x8003
	shrw ax, 2
	movw !(0xa00a), ax
	mov a, #0
	rolc a, 1
	mov !(0xa00c), a
	movw ax, #0x8000
	sarw ax, 15
	movw !(0xa00e), ax
	mov a, #0
	rolc a, 1
	mov !(0xa010), a

	mov a, #0x01
	ror a, 1
	mov !(0xa011), a
	mov a, #0
	rolc a, 1
	mov !(0xa012), a
	mov a, #0x80
	rol a, 1
	mov !(0xa013), a
	mov a, #0
	rolc a, 1
	mov !(0xa014), a
	clr1 cy
	mov a, #0x01
	rorc a, 1
	mov !(0xa015), a
	mov a, #0
	rolc a, 1
	mov !(0xa016), a
	set1 cy
	mov a, #0x80
	rolc a, 1
	mov !(0xa017), a
	mov a, #0
	rolc a, 1
	mov !(0xa018), a

	clr1 cy
	movw ax, #0x8000
	rolwc ax, 1
	movw !(0xa01a), ax
	mov a, #0
	rolc a, 1
	mov !(0xa01c), a
	clrb a
	halt
