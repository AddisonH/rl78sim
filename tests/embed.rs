//! The API for embedding the simulator (a debugger's target model, a device
//! model): `Io` hooks, stores from outside, and per-device memory maps.
//!
//! The programs are machine code; each line's assembly is beside it, and
//! llvm-mc encodes it the same way.

use rl78sim::cpu::{Cpu, Stop};
use rl78sim::isa::{Core, reg};
use rl78sim::mem::{Fault, Io, MemMap, Memory};

/// A CPU reset into `code` at 00100H.
fn cpu_with(map: MemMap, code: &[u8]) -> Cpu {
    let mut mem = Memory::new(map);
    mem.poke(0, &[0x00, 0x01]);
    mem.poke(0x100, code);
    let mut cpu = Cpu::new(mem, Core::S2);
    cpu.reset();
    cpu
}

/// The G15's map: 8 KB flash, mirror F0800H-F1FFFH of 00800H-01FFFH, RAM
/// from FFB00H, 2nd SFRs F0000H-F07FFH.
fn g15() -> MemMap {
    MemMap {
        rom_end: 0x2000,
        mirror_start: 0xF0800,
        mirror_len: 0x1800,
        mirror_src: 0x0800,
        ram_start: 0xFFB00,
        ..MemMap::default_strict()
    }
}

#[derive(Default)]
struct Regs {
    writes: Vec<(u32, u8)>,
}

impl Io for Regs {
    fn read(&mut self, _mem: &mut Memory, a: u32) -> Option<u8> {
        match a {
            0xFFF30 => Some(0x5A),
            0xFFF32 => Some(0x11),
            0xFFF33 => Some(0x22),
            0xF0010 => Some(0x33),
            _ => None,
        }
    }

    fn write(&mut self, mem: &mut Memory, a: u32, v: u8) -> bool {
        self.writes.push((a, v));
        match a {
            0xFFF35 => true,
            0xF0020 => {
                mem.stop_for_io();
                false
            }
            _ => false,
        }
    }
}

#[rustfmt::skip]
const IO_PROGRAM: &[u8] = &[
    0x8E, 0x30,       // mov a, 0xfff30
    0x9F, 0x00, 0xFB, // mov !0xffb00, a
    0xAE, 0x32,       // movw ax, 0xfff32
    0xBF, 0x02, 0xFB, // movw !0xffb02, ax
    0x8F, 0x10, 0x00, // mov a, !0xf0010
    0x9F, 0x04, 0xFB, // mov !0xffb04, a
    0x51, 0x77,       // mov a, #0x77
    0x9E, 0x35,       // mov 0xfff35, a
    0x71, 0x32, 0x06, // set1 0xfff06.3
    0x9F, 0x20, 0x00, // mov !0xf0020, a
    0x00,             // nop
    0x61, 0xED,       // halt
];

#[test]
fn io_hook_sees_sfr_and_2nd_sfr_accesses() {
    let mut cpu = cpu_with(MemMap::default_strict(), IO_PROGRAM);
    cpu.mem.b[0xFFF06] = 0x01;
    cpu.mem.b[0xFFF35] = 0xEE;
    cpu.mem.set_io(Box::new(Regs::default()));

    // The store to F0020H asks to stop: the run ends after it, before the NOP.
    assert_eq!(cpu.run(10_000, None), Stop::Io);
    assert_eq!(cpu.pc, 0x11A);

    let b = &cpu.mem.b;
    assert_eq!(b[0xFFB00], 0x5A, "byte read");
    assert_eq!(b[0xFFB02..0xFFB04], [0x11, 0x22], "word read");
    assert_eq!(b[0xFFB04], 0x33, "2nd SFR read");
    assert_eq!(b[0xFFF35], 0xEE, "a store the hook took is not made");
    assert_eq!(b[0xFFF06], 0x09, "bit operation: stored read, then a store");
    assert_eq!(b[0xF0020], 0x77, "a store the hook left is made");
    let regs = cpu.mem.io_mut::<Regs>().unwrap();
    assert_eq!(
        regs.writes,
        [(0xFFF35, 0x77), (0xFFF06, 0x09), (0xF0020, 0x77)]
    );

    // Without the hook the registers are plain storage again (33H after
    // power-on, as in rl78-elf-gdb).
    assert!(cpu.mem.take_io().is_some());
    assert_eq!(cpu.mem.read8(0xFFF30), 0x33);
    assert_eq!(cpu.mem.read16(0xFFF32), 0x3333);
}

#[test]
fn io_hook_does_not_see_cpu_registers() {
    #[derive(Default)]
    struct Seen(Vec<u32>);
    impl Io for Seen {
        fn read(&mut self, _mem: &mut Memory, a: u32) -> Option<u8> {
            self.0.push(a);
            None
        }
        fn write(&mut self, _mem: &mut Memory, a: u32, _v: u8) -> bool {
            self.0.push(a);
            false
        }
    }
    #[rustfmt::skip]
    let code = [
        0xCB, 0xF8, 0x00, 0xFE, // movw sp, #0xfe00
        0x51, 0x06,             // mov a, #6
        0x9E, 0xFA,             // mov psw, a
        0x61, 0xED,             // halt
    ];
    let mut cpu = cpu_with(MemMap::default_strict(), &code);
    cpu.mem.set_io(Box::new(Seen::default()));
    assert_eq!(cpu.run(10_000, None), Stop::Exit(6));
    assert_eq!(cpu.sp(), 0xFE00);
    assert!(cpu.mem.io_mut::<Seen>().unwrap().0.is_empty());
}

#[test]
fn stores_from_outside_replace_decoded_code() {
    #[rustfmt::skip]
    let code = [
        0x51, 0x11, // mov a, #0x11
        0x61, 0xED, // halt
    ];
    let mut cpu = cpu_with(MemMap::default_strict(), &code);
    assert_eq!(cpu.step(), None);
    assert_eq!(cpu.r8(reg::A), 0x11);

    // poke: a debugger writing memory, or flash being reprogrammed.
    cpu.pc = 0x100;
    cpu.mem.poke(0x101, &[0x22]);
    assert_eq!(cpu.step(), None);
    assert_eq!(cpu.r8(reg::A), 0x22);

    // A store straight into the backing array, announced with mark_written.
    cpu.pc = 0x100;
    cpu.mem.b[0x101] = 0x33;
    cpu.mem.mark_written(0x101, 1);
    assert_eq!(cpu.step(), None);
    assert_eq!(cpu.r8(reg::A), 0x33);

    // The run loop catches up too.
    cpu.pc = 0x100;
    cpu.mem.poke(0x101, &[0x44]);
    assert_eq!(cpu.run(10_000, None), Stop::Exit(0x44));
}

#[test]
fn io_hook_can_reprogram_running_code() {
    // A flash sequencer finishing a write: the hook pokes code the CPU has
    // already decoded, and the next pass through it runs the new bytes.
    struct Sequencer;
    impl Io for Sequencer {
        fn write(&mut self, mem: &mut Memory, a: u32, _v: u8) -> bool {
            if a == 0xFFF40 {
                mem.poke(0x101, &[0x55]);
            }
            false
        }
    }
    #[rustfmt::skip]
    let code = [
        0x51, 0x11, // mov a, #0x11
        0x9E, 0x40, // mov 0xfff40, a
        0x61, 0xED, // halt
    ];
    let mut cpu = cpu_with(MemMap::default_strict(), &code);
    cpu.mem.set_io(Box::new(Sequencer));
    assert_eq!(cpu.run(10_000, None), Stop::Exit(0x11));
    cpu.pc = 0x100;
    assert_eq!(cpu.run(10_000, None), Stop::Exit(0x55));
}

#[rustfmt::skip]
const READ_F0800: &[u8] = &[
    0x8F, 0x00, 0x08, // mov a, !0xf0800
    0x61, 0xED,       // halt
];

#[test]
fn g15_mirror_starts_where_the_2nd_sfrs_end() {
    let mut cpu = cpu_with(g15(), READ_F0800);
    cpu.mem.poke(0x800, &[0xAB]);
    assert_eq!(cpu.run(10_000, None), Stop::Exit(0xAB));

    // A mirror overlapping a longer 2nd SFR area still wins.
    let map = MemMap {
        sfr2_end: 0xF1000,
        ..g15()
    };
    let mut cpu = cpu_with(map, READ_F0800);
    cpu.mem.poke(0x800, &[0xAB]);
    assert_eq!(cpu.run(10_000, None), Stop::Exit(0xAB));
}

#[test]
fn f0800_is_reserved_without_a_mirror_there() {
    // The manual's map: 2nd SFRs end at F07FFH.
    let mut cpu = cpu_with(MemMap::default_strict(), READ_F0800);
    assert_eq!(
        cpu.run(10_000, None),
        Stop::Fault(Fault::Unmapped {
            addr: 0xF0800,
            write: false
        })
    );

    // rl78-elf-gdb's: 2nd SFRs to F0FFFH, all reading 11H.
    let mut cpu = cpu_with(MemMap::vendor(), READ_F0800);
    assert_eq!(cpu.run(10_000, None), Stop::Exit(0x11));
}
