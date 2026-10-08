//! The RL78 1 MB address space: code flash, mirror, RAM (with the register
//! banks at FFEE0H-FFEFFH), SFRs (FFF00H-FFFFFH) and 2nd SFRs (F0000H-F07FFH).
//!
//! The CPU registers that live in the SFR area (SP, PSW, CS, ES, PMC, MACR)
//! and the general registers are kept in memory, so every access path (named
//! register, saddr, !addr16, ...) sees the same storage, as on hardware.

use std::fmt;

pub const SIZE: usize = 1 << 20;
pub const MASK: u32 = 0xFFFFF;

pub const SFR_MACRL: u32 = 0xFFFF0;
pub const SFR_MACRH: u32 = 0xFFFF2;
pub const SFR_SP: u32 = 0xFFFF8;
pub const SFR_PSW: u32 = 0xFFFFA;
pub const SFR_CS: u32 = 0xFFFFC;
pub const SFR_ES: u32 = 0xFFFFD;
pub const SFR_PMC: u32 = 0xFFFFE;
/// UART0 transmit data register (SDR00): the libgloss `_write` used with the
/// simulator sends stdout here.
pub const SFR_SDR00: u32 = 0xFFF10;
/// Register bank 0 (X..H at FFEF8H..FFEFFH); bank n is 8n bytes lower.
pub const GPR_BANK0: u32 = 0xFFEF8;
pub const GPR_LO: u32 = 0xFFEE0;

/// What a store to flash (directly or through the mirror) does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum RomWrites {
    /// Stop with an error (hardware ignores the store; a program doing it is
    /// almost certainly mislinked).
    Trap,
    /// Ignore it, as hardware does.
    Ignore,
    /// Store into flash, like the rl78-elf-gdb simulator.
    Through,
}

#[derive(Clone, Debug)]
pub struct MemMap {
    /// Code flash is [0, rom_end).
    pub rom_end: u32,
    /// Mirror area [mirror_start, mirror_start + mirror_len) reads flash at
    /// mirror_src (+ 10000H when PMC.MAA = 1).
    pub mirror_start: u32,
    pub mirror_len: u32,
    pub mirror_src: u32,
    /// RAM is [ram_start, FFF00H); the register banks are its top 32 bytes.
    pub ram_start: u32,
    pub rom_writes: RomWrites,
    /// Trap on reads/writes outside every region (else they hit plain storage).
    pub strict: bool,
    /// rl78-elf-gdb quirk: every 2nd-SFR read (F0000H-F0FFFH) returns 11H.
    pub sfr2_reads_11: bool,
}

impl MemMap {
    /// The rl78-elf-gdb simulator's defaults (sim/rl78/mem.c): all of
    /// 00000H-EFFFFH is ROM, mirror F1000H-F7FFFH reads 01000H-07FFFH, RAM
    /// from F8000H, stores go through the mirror into flash.
    pub fn vendor() -> MemMap {
        MemMap {
            rom_end: 0xF0000,
            mirror_start: 0xF1000,
            mirror_len: 0x7000,
            mirror_src: 0x01000,
            ram_start: 0xF8000,
            rom_writes: RomWrites::Through,
            strict: false,
            sfr2_reads_11: true,
        }
    }

    /// The same layout with hardware semantics: flash and mirror are
    /// read-only and stores to them stop the simulation.
    pub fn default_strict() -> MemMap {
        MemMap {
            rom_writes: RomWrites::Trap,
            strict: true,
            sfr2_reads_11: false,
            ..MemMap::vendor()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    Rom,
    Mirror,
    Sfr2,
    Ram,
    Sfr,
    Reserved,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault {
    RomWrite { addr: u32, val: u8 },
    Unmapped { addr: u32, write: bool },
    OddSp { val: u16 },
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fault::RomWrite { addr, val } => write!(
                f,
                "store of {val:#04x} to {addr:#07x}, which is code flash or the flash mirror \
                 (read-only on hardware; --rom-writes through emulates rl78-elf-gdb)"
            ),
            Fault::Unmapped { addr, write } => write!(
                f,
                "{} unmapped address {addr:#07x}",
                if *write { "store to" } else { "load from" }
            ),
            Fault::OddSp { val } => write!(f, "odd stack pointer {val:#06x}"),
        }
    }
}

/// RL78/G13 multiplier and divider/multiply-accumulator (R01UH0146 chapter
/// 14). Its data registers MDAL/MDAH/MDBH/MDBL sit at FFFF0H-FFFF7H (where
/// the S3 core has MACR), MDCL/MDCH at F00E0H-F00E3H, MDUC at F00E8H.
/// Results appear immediately; DIVST stays set for the 16 clocks a division
/// takes.
#[derive(Clone, Debug, Default)]
pub struct Mdu {
    div_done_at: u64,
}

pub const MDUC: u32 = 0xF00E8;
const MDAL: u32 = 0xFFFF0;
const MDAH: u32 = 0xFFFF2;
const MDBH: u32 = 0xFFFF4;
const MDBL: u32 = 0xFFFF6;
const MDCL: u32 = 0xF00E0;
const MDCH: u32 = 0xF00E2;

pub struct Memory {
    pub b: Box<[u8; SIZE]>,
    pub map: MemMap,
    pub fault: Option<Fault>,
    /// Set by a data read from flash or the mirror (selects the manual's
    /// "note 2" clocks).
    pub flash_read: bool,
    /// Bytes written to SDR00 (UART0 transmit).
    pub uart: Vec<u8>,
    uart_seen: bool,
    /// A store of 42H to PMC (the libgloss simulator syscall trap).
    pub syscall: bool,
    /// Register-file bytes (bit n = FFEE0H + n) written since the CPU last
    /// cleared this (for the register-write hazard model).
    pub wrote_gpr: u32,
    /// Bytes covered by the CPU's decode cache; a store to one sets
    /// `wrote_code` so the cache can be flushed.
    pub code_mark: Box<[u8; SIZE]>,
    pub wrote_code: bool,
    /// The G13 multiplier/divider, when the device has one.
    pub mdu: Option<Mdu>,
    /// Current clock count (kept up to date by the CPU; used by the MDU).
    pub now: u64,
    /// Report stores that land on these (physical) addresses.
    pub watch: Vec<u32>,
    pub watch_hits: Vec<(u32, u8)>,
    /// Set with any of `fault`, `syscall`, `wrote_code`, `watch_hits` (and by
    /// the CPU for a pending stop): one flag for the CPU to test per
    /// instruction.
    pub event: bool,
}

/// Types for which all-zero bytes are a valid value.
///
/// # Safety
/// Implement only for plain integer data.
pub unsafe trait Zeroable {}
unsafe impl Zeroable for u8 {}
unsafe impl Zeroable for [u64; 2] {}
unsafe impl Zeroable for crate::isa::Insn {}

/// A zeroed fixed-size array on the heap, from zeroed (lazily mapped) pages;
/// indexing it with an address masked to 20 bits needs no bounds check.
pub fn zeroed_box<T: Zeroable>() -> Box<[T; SIZE]> {
    // SAFETY: T is valid when zeroed (Zeroable).
    unsafe { Box::<[T; SIZE]>::new_zeroed().assume_init() }
}

impl Memory {
    pub fn new(map: MemMap) -> Memory {
        // Power-on contents as rl78-elf-gdb sets them (sim/rl78 init_mem):
        // FFH everywhere (erased flash), 33H in F0000H-FFFFFH, 11H in the
        // register banks. On hardware RAM and registers are undefined after
        // reset; a fixed pattern keeps runs reproducible and comparable.
        let mut b = zeroed_box();
        b.fill(0xFF);
        b[0xF0000..].fill(0x33);
        b[GPR_LO as usize..0xFFF00].fill(0x11);
        if !map.sfr2_reads_11 {
            b[0xF0000..0xF1000].fill(0);
        }
        Memory {
            b,
            map,
            fault: None,
            flash_read: false,
            uart: Vec::new(),
            uart_seen: false,
            syscall: false,
            wrote_gpr: 0,
            code_mark: zeroed_box(),
            wrote_code: false,
            mdu: None,
            now: 0,
            watch: Vec::new(),
            watch_hits: Vec::new(),
            event: false,
        }
    }

    pub fn region(&self, a: u32) -> Region {
        let m = &self.map;
        if a < m.rom_end {
            Region::Rom
        } else if a >= 0xFFF00 {
            Region::Sfr
        } else if a >= m.mirror_start && a < m.mirror_start + m.mirror_len {
            Region::Mirror
        } else if a >= m.ram_start {
            Region::Ram
        } else if (0xF0000..0xF1000).contains(&a) {
            Region::Sfr2
        } else {
            Region::Reserved
        }
    }

    #[inline]
    fn mirror(&self, a: u32) -> u32 {
        let maa = (self.b[SFR_PMC as usize] & 1) as u32;
        a - self.map.mirror_start + self.map.mirror_src + (maa << 16)
    }

    /// Data read (sets `flash_read` for flash/mirror addresses).
    #[inline]
    pub fn read8(&mut self, a: u32) -> u8 {
        let a = a & MASK;
        if a >= self.map.ram_start {
            return self.b[a as usize];
        }
        if (0xF0000..0xF1000).contains(&a) {
            if a == MDUC {
                if let Some(m) = &self.mdu {
                    let v = self.b[a as usize];
                    return if self.now >= m.div_done_at { v & !1 } else { v };
                }
            }
            return match a {
                // SSR00 (UART0 status): never busy, as in rl78-elf-gdb.
                0xF0100 | 0xF0101 => 0,
                // rl78-elf-gdb reads 11H from every other 2nd SFR except PER0,
                // the TAU0 channel 0 counter and the G13 multiplier/divider.
                0xF00F0 if self.map.sfr2_reads_11 => 0,
                0xF0180 | 0xF0181 | 0xF00E0..=0xF00E8 => self.b[a as usize],
                _ if self.map.sfr2_reads_11 => 0x11,
                _ => self.b[a as usize],
            };
        }
        match self.region(a) {
            Region::Rom => {
                self.flash_read = true;
                self.b[a as usize]
            }
            Region::Mirror => {
                self.flash_read = true;
                self.b[self.mirror(a) as usize]
            }
            Region::Reserved if self.map.strict => {
                self.event = true;
                self.fault.get_or_insert(Fault::Unmapped {
                    addr: a,
                    write: false,
                });
                0
            }
            _ => self.b[a as usize],
        }
    }

    /// The (little-endian) word at an even address, without side effects.
    #[inline(always)]
    pub fn get16(&self, a: u32) -> u16 {
        let i = (a & MASK & !1) as usize;
        u16::from_le_bytes(self.b[i..i + 2].try_into().unwrap())
    }

    #[inline(always)]
    pub fn put16_at(&mut self, a: u32, v: u16) {
        let i = (a & MASK & !1) as usize;
        self.b[i..i + 2].copy_from_slice(&v.to_le_bytes());
    }

    #[inline(always)]
    pub fn read16(&mut self, a: u32) -> u16 {
        // Word accesses ignore address bit 0 on hardware ("bit 0 = 0 when an
        // odd address is specified").
        let a = a & !1;
        if (a & MASK) >= self.map.ram_start {
            // RAM and SFRs: plain storage for reads (as read8).
            return self.get16(a);
        }
        self.read16_slow(a)
    }

    #[inline(never)]
    fn read16_slow(&mut self, a: u32) -> u16 {
        self.read8(a) as u16 | (self.read8(a + 1) as u16) << 8
    }

    /// Instruction fetch (no side effects; mirror reads flash).
    #[inline]
    pub fn fetch(&self, a: u32) -> u8 {
        let a = a & MASK;
        if a >= self.map.mirror_start && a < self.map.mirror_start + self.map.mirror_len {
            self.b[self.mirror(a) as usize]
        } else {
            self.b[a as usize]
        }
    }

    #[inline]
    fn store(&mut self, a: u32, v: u8) {
        if !self.watch.is_empty() && self.watch.contains(&a) {
            self.watch_hits.push((a, v));
            self.event = true;
        }
        if a >= GPR_LO && a < 0xFFF00 {
            self.wrote_gpr |= 1 << (a - GPR_LO);
        }
        let i = (a & MASK) as usize;
        if self.code_mark[i] != 0 {
            self.wrote_code = true;
            self.event = true;
        }
        self.b[i] = v;
    }

    fn w16(&self, a: u32) -> u16 {
        self.b[a as usize] as u16 | (self.b[a as usize + 1] as u16) << 8
    }

    fn w32(&self, lo: u32, hi: u32) -> u32 {
        self.w16(lo) as u32 | (self.w16(hi) as u32) << 16
    }

    fn put16(&mut self, a: u32, v: u16) {
        self.b[a as usize] = v as u8;
        self.b[a as usize + 1] = (v >> 8) as u8;
    }

    /// MDU side effects of a byte store at `a` (already stored).
    fn mdu_store(&mut self, a: u32) {
        let mduc = self.b[MDUC as usize];
        let (div, mac, signed) = (mduc & 0x80 != 0, mduc & 0x40 != 0, mduc & 0x08 != 0);
        match a {
            // Multiplication starts when MDAL/MDAH are set; accumulation when
            // MDAH is set.
            0xFFFF1 | 0xFFFF3 if !div => {
                let (x, y) = (self.w16(MDAL), self.w16(MDAH));
                let p = if signed {
                    (x as i16 as i32 * y as i16 as i32) as u32
                } else {
                    x as u32 * y as u32
                };
                self.put16(MDBH, (p >> 16) as u16);
                self.put16(MDBL, p as u16);
                if mac && a == 0xFFFF3 {
                    let acc = self.w32(MDCL, MDCH);
                    let (r, of) = if signed {
                        let r = acc as i32 as i64 + p as i32 as i64;
                        (r as u32, r > i32::MAX as i64 || r < i32::MIN as i64)
                    } else {
                        let r = acc as u64 + p as u64;
                        (r as u32, r > u32::MAX as u64)
                    };
                    self.put16(MDCL, r as u16);
                    self.put16(MDCH, (r >> 16) as u16);
                    let neg = signed && (r as i32) < 0;
                    let f = mduc & !0x06 | (of as u8) << 2 | (neg as u8) << 1;
                    self.b[MDUC as usize] = f;
                }
            }
            // MACSF follows the sign of a newly written MDCH (signed MAC).
            0xF00E3 if mac && signed && !div => {
                let neg = self.b[0xF00E3] & 0x80 != 0;
                self.b[MDUC as usize] = mduc & !0x02 | (neg as u8) << 1;
            }
            MDUC if div && mduc & 1 != 0 => {
                let n = self.w32(MDAL, MDAH);
                let d = self.w32(MDBL, MDBH);
                let (q, r) = if d == 0 {
                    (u32::MAX, u32::MAX)
                } else {
                    (n / d, n % d)
                };
                self.put16(MDAL, q as u16);
                self.put16(MDAH, (q >> 16) as u16);
                self.put16(MDCL, r as u16);
                self.put16(MDCH, (r >> 16) as u16);
                let now = self.now;
                if let Some(m) = &mut self.mdu {
                    m.div_done_at = now + 16;
                }
            }
            _ => {}
        }
    }

    #[inline]
    pub fn write8(&mut self, a: u32, v: u8) {
        let a = a & MASK;
        if self.mdu.is_some() && (a >= 0xFFFF0 && a < 0xFFFF8 || (0xF00E0..=0xF00E8).contains(&a)) {
            self.b[a as usize] = v;
            self.mdu_store(a);
            return;
        }
        if a >= self.map.ram_start && a < 0xFFF00 {
            self.store(a, v);
            return;
        }
        match self.region(a) {
            Region::Sfr => self.write_sfr(a, v),
            Region::Rom | Region::Mirror => match self.map.rom_writes {
                RomWrites::Trap => {
                    self.event = true;
                    self.fault
                        .get_or_insert(Fault::RomWrite { addr: a, val: v });
                }
                RomWrites::Ignore => {}
                RomWrites::Through => {
                    let t = if self.region(a) == Region::Mirror {
                        self.mirror(a)
                    } else {
                        a
                    };
                    self.store(t, v);
                }
            },
            Region::Reserved if self.map.strict => {
                self.event = true;
                self.fault.get_or_insert(Fault::Unmapped {
                    addr: a,
                    write: true,
                });
            }
            _ => self.store(a, v),
        }
    }

    #[inline(always)]
    pub fn write16(&mut self, a: u32, v: u16) {
        let a = a & !1 & MASK;
        // Plain RAM below the register banks, not decoded as code, not
        // watched: just the store (as two write8s would do).
        if a >= self.map.ram_start
            && a < GPR_LO
            && a > MDUC
            && self.watch.is_empty()
            && self.code_mark[a as usize] | self.code_mark[(a | 1) as usize] == 0
        {
            self.put16_at(a, v);
            return;
        }
        self.write16_slow(a, v);
    }

    #[inline(never)]
    fn write16_slow(&mut self, a: u32, v: u16) {
        self.write8(a, v as u8);
        self.write8(a + 1, (v >> 8) as u8);
    }

    fn write_sfr(&mut self, a: u32, v: u8) {
        match a {
            // PSW: the CPU recomputes the register bank (at the event).
            SFR_PSW => self.event = true,
            SFR_SDR00 => {
                // libgloss's init_uart0 writes 0 to SDR00 once (baud setup in
                // the high byte); rl78-elf-gdb drops a first zero byte too.
                if !(v == 0 && !self.uart_seen) {
                    self.uart.push(v);
                }
                self.uart_seen = true;
            }
            SFR_PMC if v == 0x42 => {
                self.syscall = true;
                self.event = true;
                return; // PMC keeps its value
            }
            0xFFFF8 => {
                if v & 1 != 0 {
                    self.event = true;
                    self.fault.get_or_insert(Fault::OddSp { val: v as u16 });
                }
            }
            SFR_CS | SFR_ES => {
                self.b[a as usize] = v & 0x0F;
                return;
            }
            _ => {}
        }
        self.b[a as usize] = v;
    }

    /// Load bytes without side effects (ELF loading, tests).
    pub fn poke(&mut self, a: u32, data: &[u8]) {
        let a = a as usize;
        self.b[a..a + data.len()].copy_from_slice(data);
    }
}
