; Stack frames (manual 6.10, 6.11).
; CALL: (SP-2) <- PC[19:16], (SP-3) <- PC[15:8], (SP-4) <- PC[7:0], SP -= 4.
;   _start = 200H: "movw sp" is 4 bytes, "call !!" 4 bytes -> return to 208H.
; EXPECT-MEM: 0xfa000 08 02 00
; PUSH PSW: (SP-1) <- PSW, (SP-2) <- 00H; POP AX then gives A = PSW, X = 0.
; PUSH AX(1234H); POP PSW: PSW <- (SP+1) = 12H.
; EXPECT-MEM: 0xfa004 00 47 12
; CALLT [0080H] reached its target
; EXPECT-MEM: 0xfa010 5a
; BRK: handler sees IE = 0 and the saved PSW (86H) at [SP+3]; RETB restores it.
; EXPECT-MEM: 0xfa018 06 86 86
; RETI pops PC (3 bytes) and PSW from a hand-made interrupt frame.
; EXPECT-MEM: 0xfa020 46 a5
	.section .vec,"a"
	.short _start
	.section .callt0,"a"
	.short ct
	.section .brkvec,"a"
	.short bh
	.text
_start:
	movw sp, #0xfe00
	call !!func
	mov psw, #0x47
	push psw
	pop ax
	movw !(0xa004), ax
	movw ax, #0x1234
	push ax
	pop psw
	mov a, psw
	mov !(0xa006), a
	mov psw, #0x06

	callt [0x0080]

	mov psw, #0x86		; IE + ISP
	brk
	mov a, psw
	mov !(0xa01a), a
	mov psw, #0x06

	movw ax, #0x4600	; frame: PSW 46H, PC[19:16] 0
	push ax
	movw ax, #rt		; PC[15:0]
	push ax
	reti
	mov !(0xa021), #0xee	; not reached
rt:
	mov a, psw
	mov !(0xa020), a
	mov !(0xa021), #0xa5
	mov psw, #0x06
	clrb a
	halt

func:
	mov a, [sp+0]
	mov !(0xa000), a
	mov a, [sp+1]
	mov !(0xa001), a
	mov a, [sp+2]
	mov !(0xa002), a
	ret

ct:
	mov !(0xa010), #0x5a
	ret

bh:
	mov a, psw
	mov !(0xa018), a
	mov a, [sp+3]
	mov !(0xa019), a
	retb
