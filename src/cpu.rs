//! RL78 CPU: instruction semantics (RL78 Software Manual chapter 6) and the
//! clock model (chapter 5 operation lists, chapter 7 pipeline).
//!
//! Clock model, per executed instruction:
//! - the operation-list clocks for the selected core; "note 2" clocks when the
//!   instruction read code flash (or the mirror) as data; "not taken/taken"
//!   for conditional branches;
//! - a skipped instruction costs 1 clock (2 when ES:-prefixed) (6.14);
//! - +1 clock when the instruction addresses memory through a register (or
//!   uses SP/CS/the call register) written by the previous instruction
//!   (7.2.3: S2/S3 insert one wait; S1 one to two, modelled as one);
//! - fetching from RAM: the manual gives only a maximum (2n+3 on S2/S3, 4n+6
//!   on S1); that bound is charged and the count reported separately.
//!
//! Not modelled: interrupts and peripherals other than the UART0 output
//! register and the libgloss syscall trap.

use crate::isa::{self, Bit, Core, Insn, Isa, Mem, Mn, Opnd, SP_BASE, reg};
use crate::mem::{self, Memory, Region};
use std::sync::OnceLock;

pub fn isa() -> &'static Isa {
    static ISA: OnceLock<Isa> = OnceLock::new();
    ISA.get_or_init(Isa::load)
}

pub const PSW_CY: u8 = 0x01;
pub const PSW_AC: u8 = 0x10;
pub const PSW_Z: u8 = 0x40;
pub const PSW_IE: u8 = 0x80;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stop {
    /// HALT/STOP (libgloss `_exit`: exit code in A), or a branch to itself
    /// (newlib's nosys `_exit`: code in X).
    Exit(u8),
    SelfLoop {
        code: u8,
    },
    /// BRK with no handler installed (vector 0000H or erased FFFFH).
    Brk,
    Illegal {
        pc: u32,
        bytes: [u8; 4],
    },
    Fault(mem::Fault),
    /// Undefined behaviour on hardware (with `--strict`).
    Undefined(String),
    /// --max-clocks / --max-instrs reached.
    Limit,
    /// The stop address (--stop) was reached.
    Breakpoint(u32),
    /// An `Io` hook asked to stop (`Memory::stop_for_io`).
    Io,
}

#[derive(Clone, Copy, Default, Debug)]
pub struct Counts {
    pub instrs: u64,
    pub clocks: u64,
    pub skipped: u64,
    pub taken: u64,
    pub not_taken: u64,
    pub flash_data: u64,
    pub hazards: u64,
    pub ram_fetch: u64,
}

impl Counts {
    pub fn sub(&self, o: &Counts) -> Counts {
        Counts {
            instrs: self.instrs - o.instrs,
            clocks: self.clocks - o.clocks,
            skipped: self.skipped - o.skipped,
            taken: self.taken - o.taken,
            not_taken: self.not_taken - o.not_taken,
            flash_data: self.flash_data - o.flash_data,
            hazards: self.hazards - o.hazards,
            ram_fetch: self.ram_fetch - o.ram_fetch,
        }
    }
}


/// Specialised semantics for the most frequently executed encodings (the
/// rest go through the generic operand-driven `exec`). Register operands are
/// bank-relative numbers as in `Opnd`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Generic,
    MovRR { d: u8, s: u8 },
    MovRImm { d: u8 },
    XchRR { a: u8, b: u8 },
    MovwRR { d: u8, s: u8 },
    MovwRImm { d: u8 },
    /// MOVW rp, [SP+byte]
    MovwRSp { d: u8 },
    /// MOVW [SP+byte], rp
    MovwSpR { s: u8 },
    /// MOVW rp, [base] / [base+byte] (no ES:)
    MovwRInd { d: u8, base: u8, off: bool },
    /// MOVW [base] / [base+byte], rp (no ES:)
    MovwIndR { s: u8, base: u8, off: bool },
    XchwRR { a: u8, b: u8 },
    /// ADD/ADDC/SUB/SUBC/CMP/AND/OR/XOR r, r / r, #byte
    Alu8 { op: Mn, d: u8, s: Option<u8> },
    /// ADDW/SUBW/CMPW rp, rp / rp, #word
    Alu16 { op: Mn, d: u8, s: Option<u8> },
    IncwR { r: u8 },
    DecwR { r: u8 },
    PushR { r: u8 },
    PopR { r: u8 },
    /// BC/BNC/BZ/BNZ/BH/BNH $addr20
    Bcc { op: Mn },
    /// SKC/SKNC/SKZ/SKNZ/SKH/SKNH
    Sk { op: Mn },
}

/// Per-encoding data the execution loop needs, precomputed for one core.
#[derive(Clone, Copy)]
struct OpInfo {
    mn: Mn,
    o0: Option<Opnd>,
    o1: Option<Opnd>,
    kind: Kind,
    /// The core implements the instruction.
    ok: bool,
    clk: isa::Clk,
    /// Clocks when it reads flash as data (note 2).
    clk_flash: isa::Clk,
    /// ES:-prefixed (a skipped one costs 2 clocks).
    es_prefix: bool,
    /// Register-bank bytes (bit n = register n) used to form an address, and
    /// whether SP / CS are (manual 7.2.3).
    haz_regs: u8,
    haz_sp: bool,
    haz_cs: bool,
    /// Writes SP / CS (a following instruction addressing with it waits).
    sets_sp: bool,
    sets_cs: bool,
    /// Uses a register, SP or CS for addressing (may wait on a hazard).
    haz_any: bool,
    /// Fixed clocks (no branch/skip; `clk` = `clk.nt`), unless it reads
    /// flash, waits on a hazard or runs from RAM; None: always take the
    /// general path.
    fixed: Option<u8>,
}

impl OpInfo {
    fn new(d: &isa::OpDef, core: Core) -> OpInfo {
        let o0 = d.ops.first().copied();
        let o1 = d.ops.get(1).copied();
        // Hazard inputs, as `hazard` used to compute them per instruction.
        let mut haz_regs = 0u8;
        let mut haz_sp = false;
        let mut haz_cs = false;
        let bits = |regs: &[u8]| regs.iter().fold(0u8, |m, &r| m | 1 << r);
        for o in &d.ops {
            let m = match *o {
                Opnd::M { m, .. } => Some(m),
                Opnd::B { b: Bit::Mem(m), .. } => Some(m),
                _ => None,
            };
            match m {
                Some(Mem::Ind(r)) | Some(Mem::IndOff(r)) if r != SP_BASE => {
                    haz_regs |= bits(&[r, r + 1])
                }
                Some(Mem::IndOff(_)) => haz_sp = true,
                Some(Mem::HlIdx(r)) => haz_regs |= bits(&[reg::L, reg::H, r]),
                Some(Mem::Based(r)) => haz_regs |= bits(&[r]),
                Some(Mem::BasedBc) => haz_regs |= bits(&[reg::B, reg::C]),
                _ => {}
            }
        }
        match (d.mn, o0) {
            (Mn::Br, Some(Opnd::R16(_))) => {
                haz_regs |= bits(&[reg::X, reg::A]);
                haz_cs = true;
            }
            (Mn::Call, Some(Opnd::R16(_))) => {
                haz_regs = 0xFF;
                haz_cs = true;
                haz_sp = true;
            }
            (
                Mn::Call | Mn::Callt | Mn::Brk | Mn::Ret | Mn::Reti | Mn::Retb | Mn::Push | Mn::Pop,
                _,
            ) => haz_sp = true,
            _ => {}
        }
        let clk = d.clocks(core);
        let c = clk.unwrap_or_default();
        let fixed = (c.t == c.nt).then_some(c.nt);
        OpInfo {
            mn: d.mn,
            o0,
            o1,
            kind: Self::kind(d.mn, o0, o1),
            ok: clk.is_some(),
            clk: clk.unwrap_or_default(),
            clk_flash: d.clocks_flash(core).unwrap_or_default(),
            es_prefix: d.fixed[0] == 0x11,
            haz_regs,
            haz_sp,
            haz_cs,
            sets_sp: matches!(d.mn, Mn::Movw | Mn::Addw | Mn::Subw) && o0 == Some(Opnd::Sp),
            sets_cs: d.mn == Mn::Mov && o0 == Some(Opnd::Cs),
            haz_any: haz_regs != 0 || haz_sp || haz_cs,
            fixed,
        }
    }

    fn kind(mn: Mn, o0: Option<Opnd>, o1: Option<Opnd>) -> Kind {
        use Opnd::*;
        let ind = |m: Mem| match m {
            Mem::Ind(b) => Some((b, false)),
            Mem::IndOff(b) if b != SP_BASE => Some((b, true)),
            _ => None,
        };
        let sp = Mem::IndOff(SP_BASE);
        match (mn, o0, o1) {
            (Mn::Mov, Some(R8(d)), Some(R8(s))) => Kind::MovRR { d, s },
            (Mn::Mov, Some(R8(d)), Some(Imm8)) => Kind::MovRImm { d },
            (Mn::Xch, Some(R8(a)), Some(R8(b))) => Kind::XchRR { a, b },
            (Mn::Movw, Some(R16(d)), Some(R16(s))) => Kind::MovwRR { d, s },
            (Mn::Movw, Some(R16(d)), Some(Imm16)) => Kind::MovwRImm { d },
            (Mn::Movw, Some(R16(d)), Some(M { m, es: false })) if m == sp => Kind::MovwRSp { d },
            (Mn::Movw, Some(M { m, es: false }), Some(R16(s))) if m == sp => Kind::MovwSpR { s },
            (Mn::Movw, Some(R16(d)), Some(M { m, es: false })) if ind(m).is_some() => {
                let (base, off) = ind(m).unwrap();
                Kind::MovwRInd { d, base, off }
            }
            (Mn::Movw, Some(M { m, es: false }), Some(R16(s))) if ind(m).is_some() => {
                let (base, off) = ind(m).unwrap();
                Kind::MovwIndR { s, base, off }
            }
            (Mn::Xchw, Some(R16(a)), Some(R16(b))) => Kind::XchwRR { a, b },
            (
                Mn::Add | Mn::Addc | Mn::Sub | Mn::Subc | Mn::Cmp | Mn::And | Mn::Or | Mn::Xor,
                Some(R8(d)),
                Some(R8(s)),
            ) => Kind::Alu8 { op: mn, d, s: Some(s) },
            (
                Mn::Add | Mn::Addc | Mn::Sub | Mn::Subc | Mn::Cmp | Mn::And | Mn::Or | Mn::Xor,
                Some(R8(d)),
                Some(Imm8),
            ) => Kind::Alu8 { op: mn, d, s: None },
            (Mn::Addw | Mn::Subw | Mn::Cmpw, Some(R16(d)), Some(R16(s))) => {
                Kind::Alu16 { op: mn, d, s: Some(s) }
            }
            (Mn::Addw | Mn::Subw | Mn::Cmpw, Some(R16(d)), Some(Imm16)) => {
                Kind::Alu16 { op: mn, d, s: None }
            }
            (Mn::Incw, Some(R16(r)), None) => Kind::IncwR { r },
            (Mn::Decw, Some(R16(r)), None) => Kind::DecwR { r },
            (Mn::Push, Some(R16(r)), None) => Kind::PushR { r },
            (Mn::Pop, Some(R16(r)), None) => Kind::PopR { r },
            (Mn::Bc | Mn::Bnc | Mn::Bz | Mn::Bnz | Mn::Bh | Mn::Bnh, Some(Rel8), None) => {
                Kind::Bcc { op: mn }
            }
            (Mn::Skc | Mn::Sknc | Mn::Skz | Mn::Sknz | Mn::Skh | Mn::Sknh, None, None) => {
                Kind::Sk { op: mn }
            }
            _ => Kind::Generic,
        }
    }
}

/// The per-encoding table for a core (built once).
const OPS: usize = 2048;

fn op_info(core: Core) -> &'static [OpInfo; OPS] {
    static INFO: [OnceLock<Box<[OpInfo; OPS]>>; 3] = [OnceLock::new(), OnceLock::new(), OnceLock::new()];
    INFO[core as usize].get_or_init(|| {
        let ops = &isa().ops;
        assert!(ops.len() <= OPS);
        let v: Vec<OpInfo> = (0..OPS)
            .map(|i| OpInfo::new(&ops[i.min(ops.len() - 1)], core))
            .collect();
        v.into_boxed_slice().try_into().ok().unwrap()
    })
}

pub struct Cpu {
    pub mem: Memory,
    pub pc: u32,
    pub core: Core,
    pub n: Counts,
    /// Treat undefined behaviour (16-bit address overflow in based modes,
    /// misaligned word access) as an error.
    pub strict: bool,
    /// First undefined-behaviour event and how many there were.
    pub ub: Option<String>,
    pub ub_count: u64,
    /// Per-PC instructions and clocks (enable with `enable_profile`).
    pub prof: Option<Box<[[u64; 2]; mem::SIZE]>>,
    /// Executed instructions per mnemonic (index: `Mn as usize`).
    pub mix: [u64; 96],
    /// Reproduce rl78-elf-gdb's deviations from the manual (for lockstep
    /// comparison): MACH sets CY when MACR's sign changes, CMPS sets CY like
    /// CMP, MOVS clears CY, PUSH/POP PSW use the low stack byte for PSW.
    pub gdb_quirks: bool,
    /// Per-encoding data for `core` (indexed like `Isa::ops`; padded so an
    /// index masked to 11 bits needs no bounds check).
    info: &'static [OpInfo; OPS],
    /// Decoded instructions by address; `len == 0`: not decoded yet.
    cache: Box<[Insn; mem::SIZE]>,
    /// Register bank base address (kept in step with PSW.RBS: by set_psw,
    /// by `step_events` after a store to PSW, and on entering `step`/`run_*`).
    rb: u32,
    skip: bool,
    /// Stop requested by the instruction being executed (with `mem.event`).
    pending: Option<Stop>,
    /// Hazard state from the previous instruction.
    prev_gpr: u32,
    prev_sp: bool,
    prev_cs: bool,
}

/// Result of executing one instruction (a stop goes to `Cpu::pending`).
#[derive(Clone, Copy)]
struct Exec {
    next: u32,
    taken: bool,
}

impl Cpu {
    pub fn new(mem: Memory, core: Core) -> Cpu {
        Cpu {
            mem,
            pc: 0,
            core,
            n: Counts::default(),
            strict: false,
            ub: None,
            ub_count: 0,
            prof: None,
            mix: [0; 96],
            gdb_quirks: false,
            info: op_info(core),
            cache: mem::zeroed_box(),
            rb: mem::GPR_BANK0,
            pending: None,
            skip: false,
            prev_gpr: 0,
            prev_sp: false,
            prev_cs: false,
        }
    }

    pub fn enable_profile(&mut self) {
        self.prof = Some(mem::zeroed_box());
    }

    /// Reset: PC from the reset vector at 00000H, SP/PSW as after reset.
    pub fn reset(&mut self) {
        self.pc = self.mem.b[0] as u32 | (self.mem.b[1] as u32) << 8;
        // Reset values (manual 3.1.2, 3.3): PSW 06H, ES 0FH, CS 00H; PMC
        // 00H (mirror reads 00000H-0FFFFH).
        self.mem.b[mem::SFR_PSW as usize] = 0x06;
        self.mem.b[mem::SFR_PMC as usize] = 0x00;
        self.set_macr(0);
        self.mem.b[mem::SFR_ES as usize] = 0x0F;
        self.mem.b[mem::SFR_CS as usize] = 0x00;
    }

    // ---- registers -------------------------------------------------------

    #[inline]
    pub fn psw(&self) -> u8 {
        self.mem.b[mem::SFR_PSW as usize]
    }
    #[inline]
    fn set_psw(&mut self, v: u8) {
        let old = self.psw();
        self.mem.b[mem::SFR_PSW as usize] = v;
        if (old ^ v) & 0x28 != 0 {
            // RBS0/RBS1 changed (the instruction accesses no register after
            // writing PSW, so the new bank applies from the next one).
            self.rb = self.bank_base();
        }
    }
    #[inline]
    fn flag(&self, m: u8) -> bool {
        self.psw() & m != 0
    }
    #[inline]
    fn set_flag(&mut self, m: u8, v: bool) {
        let p = self.psw();
        self.set_psw(if v { p | m } else { p & !m });
    }
    #[inline]
    fn bank_base(&self) -> u32 {
        let p = self.psw();
        let rb = ((p >> 3) & 1) | ((p >> 4) & 2);
        mem::GPR_BANK0 - 8 * rb as u32
    }
    // Register access during an instruction: `rb` is the bank base for the
    // whole instruction (RBS changes only by writing all of PSW, after which
    // no instruction accesses a general register).
    #[inline]
    fn g8(&self, r: u8) -> u8 {
        self.mem.b[((self.rb + r as u32) & mem::MASK) as usize]
    }
    #[inline]
    fn sg8(&mut self, r: u8, v: u8) {
        let a = self.rb + r as u32;
        self.mem.wrote_gpr |= 1 << ((a - mem::GPR_LO) & 31);
        self.mem.b[(a & mem::MASK) as usize] = v;
    }
    /// Register pairs: `r` is even and `rb` 8-aligned, so the pair is an
    /// aligned word.
    #[inline]
    fn g16(&self, r: u8) -> u16 {
        self.mem.get16(self.rb + r as u32)
    }
    #[inline]
    fn sg16(&mut self, r: u8, v: u16) {
        let a = self.rb + r as u32;
        self.mem.wrote_gpr |= 3 << ((a - mem::GPR_LO) & 30);
        self.mem.put16_at(a, v);
    }

    #[inline]
    pub fn r8(&self, r: u8) -> u8 {
        self.mem.b[(self.bank_base() + r as u32) as usize]
    }
    #[inline]
    pub fn set_r8(&mut self, r: u8, v: u8) {
        let a = self.bank_base() + r as u32;
        self.mem.wrote_gpr |= 1 << (a - mem::GPR_LO);
        self.mem.b[a as usize] = v;
    }
    #[inline]
    pub fn r16(&self, r: u8) -> u16 {
        self.r8(r) as u16 | (self.r8(r + 1) as u16) << 8
    }
    #[inline]
    pub fn set_r16(&mut self, r: u8, v: u16) {
        self.set_r8(r, v as u8);
        self.set_r8(r + 1, (v >> 8) as u8);
    }
    #[inline]
    pub fn sp(&self) -> u16 {
        self.mem.get16(mem::SFR_SP)
    }
    #[inline]
    pub fn set_sp(&mut self, v: u16) {
        self.mem.write16(mem::SFR_SP, v);
    }
    #[inline]
    fn es(&self) -> u32 {
        (self.mem.b[mem::SFR_ES as usize] & 0xF) as u32
    }
    #[inline]
    fn cs(&self) -> u32 {
        (self.mem.b[mem::SFR_CS as usize] & 0xF) as u32
    }
    pub fn macr(&self) -> u32 {
        let b = &self.mem.b;
        u32::from_le_bytes([b[0xFFFF0], b[0xFFFF1], b[0xFFFF2], b[0xFFFF3]])
    }
    fn set_macr(&mut self, v: u32) {
        self.mem.b[0xFFFF0..0xFFFF4].copy_from_slice(&v.to_le_bytes());
    }

    #[cold]
    #[inline(never)]
    fn undefined(&mut self, what: String) {
        self.ub_count += 1;
        if self.strict {
            self.mem.event = true;
        }
        if self.ub.is_none() {
            self.ub = Some(format!("pc {:#07x}: {what}", self.pc));
        }
    }

    // ---- addressing ------------------------------------------------------

    /// Effective address of a memory operand.
    #[inline]
    fn ea(&mut self, m: Mem, es: bool, f: &isa::Fields) -> u32 {
        let near = |v: u32| 0xF0000 | (v & 0xFFFF);
        let based = |cpu: &mut Cpu, base: u32, off: u32| -> u32 {
            if es {
                ((cpu.es() << 16 | base) + off) & mem::MASK
            } else {
                let v = base + off;
                if v > 0xFFFF {
                    cpu.undefined(format!("address {base:#06x}+{off:#x} exceeds FFFFH"));
                }
                near(v)
            }
        };
        match m {
            Mem::Abs16 => {
                if es {
                    self.es() << 16 | f.addr
                } else {
                    near(f.addr)
                }
            }
            Mem::Saddr => isa::saddr_addr(f.saddr),
            Mem::Sfr => 0xFFF00 + f.sfr as u32,
            Mem::Ind(r) => {
                let v = self.g16(r) as u32;
                if es { self.es() << 16 | v } else { near(v) }
            }
            Mem::IndOff(SP_BASE) => {
                let v = self.sp() as u32 + f.adr as u32;
                if v > 0xFFFF {
                    self.undefined(format!("[sp+{}] exceeds FFFFH", f.adr));
                }
                near(v)
            }
            Mem::IndOff(r) => {
                let b = self.g16(r) as u32;
                based(self, b, f.adr as u32)
            }
            Mem::HlIdx(r) => {
                let b = self.g16(reg::HL) as u32;
                let o = self.g8(r) as u32;
                based(self, b, o)
            }
            Mem::Based(r) => {
                let o = self.g8(r) as u32;
                based(self, f.addr, o)
            }
            Mem::BasedBc => {
                let o = self.g16(reg::BC) as u32;
                based(self, f.addr, o)
            }
        }
    }

    fn rd8(&mut self, o: &Opnd, f: &isa::Fields) -> u8 {
        match *o {
            Opnd::R8(r) => self.g8(r),
            Opnd::Imm8 => f.imm as u8,
            Opnd::Psw => self.psw(),
            Opnd::Cs => self.cs() as u8,
            Opnd::Es => self.es() as u8,
            Opnd::M { m, es } => {
                let a = self.ea(m, es, f);
                self.mem.read8(a)
            }
            _ => unreachable!("8-bit read of {o:?}"),
        }
    }

    fn wr8(&mut self, o: &Opnd, f: &isa::Fields, v: u8) {
        match *o {
            Opnd::R8(r) => self.sg8(r, v),
            Opnd::Psw => self.set_psw(v),
            Opnd::Cs => self.mem.write8(mem::SFR_CS, v),
            Opnd::Es => self.mem.write8(mem::SFR_ES, v),
            Opnd::M { m, es } => {
                let a = self.ea(m, es, f);
                self.mem.write8(a, v)
            }
            _ => unreachable!("8-bit write of {o:?}"),
        }
    }

    #[inline(always)]
    fn word_addr(&mut self, a: u32) -> u32 {
        if a & 1 != 0 {
            self.undefined(format!("word access at odd address {a:#07x}"));
        }
        a
    }

    /// Address of [SP+byte] (as `ea`).
    #[inline(always)]
    fn ea_sp(&mut self, f: &isa::Fields) -> u32 {
        let v = self.sp() as u32 + f.adr as u32;
        if v > 0xFFFF {
            self.undefined(format!("[sp+{}] exceeds FFFFH", f.adr));
        }
        0xF0000 | (v & 0xFFFF)
    }

    fn rd16(&mut self, o: &Opnd, f: &isa::Fields) -> u16 {
        match *o {
            Opnd::R16(r) => self.g16(r),
            Opnd::Imm16 | Opnd::Imm8 => f.imm,
            Opnd::Sp => self.sp(),
            Opnd::M { m, es } => {
                let a = self.ea(m, es, f);
                let a = self.word_addr(a);
                self.mem.read16(a)
            }
            _ => unreachable!("16-bit read of {o:?}"),
        }
    }

    fn wr16(&mut self, o: &Opnd, f: &isa::Fields, v: u16) {
        match *o {
            Opnd::R16(r) => self.sg16(r, v),
            Opnd::Sp => self.set_sp(v),
            Opnd::M { m, es } => {
                let a = self.ea(m, es, f);
                let a = self.word_addr(a);
                self.mem.write16(a, v)
            }
            _ => unreachable!("16-bit write of {o:?}"),
        }
    }

    /// Byte holding a bit operand: (address or None for A/PSW/CY).
    fn bit_get(&mut self, o: &Opnd, f: &isa::Fields) -> bool {
        match *o {
            Opnd::B { b: Bit::Cy, .. } => self.flag(PSW_CY),
            Opnd::B { b: Bit::A, n, .. } => self.g8(reg::A) >> n & 1 != 0,
            Opnd::B { b: Bit::Psw, n, .. } => self.psw() >> n & 1 != 0,
            Opnd::B {
                b: Bit::Mem(m),
                n,
                es,
            } => {
                let a = self.ea(m, es, f);
                self.mem.read8(a) >> n & 1 != 0
            }
            _ => unreachable!("bit read of {o:?}"),
        }
    }

    fn bit_set(&mut self, o: &Opnd, f: &isa::Fields, v: bool) {
        let upd = |x: u8, n: u8| if v { x | 1 << n } else { x & !(1 << n) };
        match *o {
            Opnd::B { b: Bit::Cy, .. } => self.set_flag(PSW_CY, v),
            Opnd::B { b: Bit::A, n, .. } => {
                let x = self.g8(reg::A);
                self.sg8(reg::A, upd(x, n))
            }
            Opnd::B { b: Bit::Psw, n, .. } => {
                let x = self.psw();
                self.set_psw(upd(x, n))
            }
            Opnd::B {
                b: Bit::Mem(m),
                n,
                es,
            } => {
                let a = self.ea(m, es, f);
                // A read-modify-write of the whole byte; the read is not a
                // "flash data access" for timing purposes when it is RAM.
                let x = self.mem.read8(a);
                self.mem.write8(a, upd(x, n))
            }
            _ => unreachable!("bit write of {o:?}"),
        }
    }

    fn push8(&mut self, v: u8) {
        let sp = self.sp().wrapping_sub(1);
        self.mem.b[mem::SFR_SP as usize] = sp as u8;
        self.mem.b[mem::SFR_SP as usize + 1] = (sp >> 8) as u8;
        self.mem.write8(0xF0000 | sp as u32, v);
    }

    /// CALL/BRK frame: (SP-2) <- PC[19:16], (SP-3) <- PC[15:8], (SP-4) <- PC[7:0].
    fn push_pc(&mut self, ret: u32, psw: Option<u8>) {
        let sp = self.sp() as u32;
        if let Some(p) = psw {
            self.mem.write8(0xF0000 | ((sp - 1) & 0xFFFF), p);
        }
        self.mem
            .write8(0xF0000 | ((sp - 2) & 0xFFFF), (ret >> 16) as u8 & 0xF);
        self.mem
            .write8(0xF0000 | ((sp - 3) & 0xFFFF), (ret >> 8) as u8);
        self.mem.write8(0xF0000 | ((sp - 4) & 0xFFFF), ret as u8);
        self.set_sp(sp.wrapping_sub(4) as u16);
    }

    fn pop_pc(&mut self) -> (u32, u8) {
        let sp = self.sp() as u32;
        let rd = |c: &mut Cpu, o: u32| c.mem.read8(0xF0000 | ((sp + o) & 0xFFFF)) as u32;
        let pc = rd(self, 0) | rd(self, 1) << 8 | (rd(self, 2) & 0xF) << 16;
        let psw = rd(self, 3) as u8;
        self.set_sp(sp.wrapping_add(4) as u16);
        (pc, psw)
    }

    // ---- execution -------------------------------------------------------

    /// Decode the instruction at `pc` into the cache (None: illegal).
    #[cold]
    fn decode_miss(&mut self, pc: u32) -> Option<Insn> {
        let mem = &self.mem;
        let i = isa().decode(|k| mem.fetch(pc + k))?;
        for k in 0..i.len as u32 {
            self.mem.code_mark[((pc + k) & mem::MASK) as usize] = 1;
        }
        self.cache[(pc & mem::MASK) as usize] = i;
        Some(i)
    }

    /// Catch up with changes made from outside since the last instruction:
    /// PSW (the register bank) and code (`Memory::poke`, `mark_written`).
    #[inline]
    fn enter(&mut self) {
        self.rb = self.bank_base();
        if self.mem.wrote_code {
            self.flush_cache();
        }
    }

    fn flush_cache(&mut self) {
        for (a, m) in self.mem.code_mark.iter_mut().enumerate() {
            if *m != 0 {
                *m = 0;
                self.cache[a].len = 0;
            }
        }
        self.mem.wrote_code = false;
    }

    /// Execute one instruction (or one skipped instruction slot).
    pub fn step(&mut self) -> Option<Stop> {
        self.enter();
        if self.prof.is_some() {
            self.step_impl::<true>()
        } else {
            self.step_impl::<false>()
        }
    }

    /// `step` with or without the profile (`PROF` = `prof.is_some()`).
    #[inline(always)]
    fn step_impl<const PROF: bool>(&mut self) -> Option<Stop> {
        let pc = self.pc;
        let mut insn = self.cache[(pc & mem::MASK) as usize];
        if insn.len == 0 {
            match self.decode_miss(pc) {
                Some(i) => insn = i,
                None => return Some(self.illegal(pc)),
            }
        }
        let d = &self.info[insn.op as usize & (OPS - 1)];
        if !d.ok || self.skip {
            return self.step_skip(pc, insn.len, d);
        }
        self.mem.now = self.n.clocks;
        let hazard = d.haz_any && self.hazard(d);
        let ex = self.exec(d, &insn, pc);

        let clk = match d.fixed {
            Some(c) if !hazard && !self.mem.flash_read && pc < self.mem.map.ram_start => c as u64,
            _ => self.clocks(d, pc, ex.taken, hazard),
        };
        self.n.instrs += 1;
        self.n.clocks += clk;
        self.mix[d.mn as usize] += 1;
        if PROF {
            if let Some(p) = &mut self.prof {
                let e = &mut p[(pc & mem::MASK) as usize];
                e[0] += 1;
                e[1] += clk;
            }
        }

        // hazard state for the next instruction
        self.prev_gpr = if d.mn == Mn::Sel { u32::MAX } else { self.mem.wrote_gpr };
        self.mem.wrote_gpr = 0;
        self.prev_sp = d.sets_sp;
        self.prev_cs = d.sets_cs;

        self.pc = ex.next & mem::MASK;
        if self.mem.event {
            return self.step_events(pc);
        }
        None
    }

    /// A skipped instruction slot, or an instruction this core lacks.
    #[cold]
    #[inline(never)]
    fn step_skip(&mut self, pc: u32, len: u8, d: &OpInfo) -> Option<Stop> {
        if !d.ok {
            // Not implemented by this core (e.g. MULHU on S2, SEL on S1).
            return Some(self.illegal(pc));
        }
        self.skip = false;
        self.pc = (pc + len as u32) & mem::MASK;
        let clk = if d.es_prefix { 2 } else { 1 };
        self.n.skipped += 1;
        self.n.clocks += clk;
        self.prev_gpr = 0;
        self.prev_sp = false;
        self.prev_cs = false;
        if let Some(p) = &mut self.prof {
            p[(pc & mem::MASK) as usize][1] += clk;
        }
        None
    }

    /// Clocks of an executed instruction in general: branch taken or not,
    /// flash data read (note 2), register-write wait, fetch from RAM.
    #[inline(never)]
    fn clocks(&mut self, d: &OpInfo, pc: u32, taken: bool, hazard: bool) -> u64 {
        let flash = self.mem.flash_read;
        self.mem.flash_read = false;
        let base = if flash { d.clk_flash } else { d.clk };
        let mut clk = if taken { base.t } else { base.nt } as u64;
        if base.t != base.nt {
            if taken {
                self.n.taken += 1
            } else {
                self.n.not_taken += 1
            }
        }
        if flash {
            self.n.flash_data += 1;
        }
        if hazard {
            clk += 1;
            self.n.hazards += 1;
        }
        if pc >= self.mem.map.ram_start && self.mem.region(pc) == Region::Ram {
            clk = if self.core == isa::Core::S1 {
                4 * clk + 6
            } else {
                2 * clk + 3
            };
            self.n.ram_fetch += 1;
        }
        clk
    }

    #[cold]
    fn illegal(&self, pc: u32) -> Stop {
        let b = [0, 1, 2, 3].map(|k| self.mem.fetch(pc + k));
        Stop::Illegal { pc, bytes: b }
    }

    #[cold]
    fn report_watch(&mut self, pc: u32) {
        for (a, v) in std::mem::take(&mut self.mem.watch_hits) {
            eprintln!("rl78sim: watch: pc {pc:#07x} stored {v:#04x} to {a:#07x}");
        }
    }

    /// The rest of `step` after a store to code, a watched store, a fault,
    /// undefined behaviour with --strict or a stop.
    #[cold]
    fn step_events(&mut self, pc: u32) -> Option<Stop> {
        self.mem.event = false;
        // A store may have reached PSW through memory.
        self.rb = self.bank_base();
        let stop = self.pending.take();
        if !self.mem.watch_hits.is_empty() {
            self.report_watch(pc);
        }
        if self.mem.wrote_code {
            self.flush_cache();
        }
        if let Some(f) = self.mem.fault.take() {
            return Some(Stop::Fault(f));
        }
        if std::mem::take(&mut self.mem.io_stop) {
            return Some(Stop::Io);
        }
        if self.strict {
            if let Some(u) = self.ub.take() {
                return Some(Stop::Undefined(u));
            }
        }
        stop
    }

    /// Does this instruction use, for addressing, a register the previous
    /// instruction wrote (manual 7.2.3)?
    #[inline]
    fn hazard(&self, d: &OpInfo) -> bool {
        if self.prev_gpr == 0 && !self.prev_sp && !self.prev_cs {
            return false;
        }
        let need = (d.haz_regs as u32) << (self.rb - mem::GPR_LO);
        (need & self.prev_gpr) != 0 || (d.haz_sp && self.prev_sp) || (d.haz_cs && self.prev_cs)
    }

    fn rel(pc: u32, len: u8, f: &isa::Fields, wide: bool) -> u32 {
        let disp = if wide {
            f.addr as u16 as i16 as i32
        } else {
            f.adr as i8 as i32
        };
        (pc as i32 + len as i32 + disp) as u32 & mem::MASK
    }

    /// Branch/skip condition of BC..BNH / SKC..SKNH.
    #[inline]
    fn cond(&self, op: Mn) -> bool {
        let p = self.psw();
        let (z, c) = (p & PSW_Z != 0, p & PSW_CY != 0);
        match op {
            Mn::Bc | Mn::Skc => c,
            Mn::Bnc | Mn::Sknc => !c,
            Mn::Bz | Mn::Skz => z,
            Mn::Bnz | Mn::Sknz => !z,
            Mn::Bh | Mn::Skh => !(z || c),
            _ => z || c,
        }
    }

    /// Set the PSW bits in `mask` to those of `bits`.
    #[inline]
    fn set_flags(&mut self, mask: u8, bits: u8) {
        let p = self.psw();
        self.set_psw(p & !mask | bits);
    }

    #[inline(always)]
    fn exec(&mut self, d: &OpInfo, insn: &Insn, pc: u32) -> Exec {
        let f = &insn.f;
        let mut ex = Exec {
            next: pc + insn.len as u32,
            taken: false,
        };
        match d.kind {
            Kind::Generic => self.exec_generic(d, insn, pc, &mut ex),
            Kind::MovRR { d, s } => {
                let v = self.g8(s);
                self.sg8(d, v);
            }
            Kind::MovRImm { d } => self.sg8(d, f.imm as u8),
            Kind::XchRR { a, b } => {
                let (x, y) = (self.g8(a), self.g8(b));
                self.sg8(a, y);
                self.sg8(b, x);
            }
            Kind::MovwRR { d, s } => {
                let v = self.g16(s);
                self.sg16(d, v);
            }
            Kind::MovwRImm { d } => self.sg16(d, f.imm),
            Kind::MovwRSp { d } => {
                let a = self.ea_sp(f);
                let a = self.word_addr(a);
                let v = self.mem.read16(a);
                self.sg16(d, v);
            }
            Kind::MovwSpR { s } => {
                let v = self.g16(s);
                let a = self.ea_sp(f);
                let a = self.word_addr(a);
                self.mem.write16(a, v);
            }
            Kind::MovwRInd { d, base, off } => {
                let m = if off { Mem::IndOff(base) } else { Mem::Ind(base) };
                let a = self.ea(m, false, f);
                let a = self.word_addr(a);
                let v = self.mem.read16(a);
                self.sg16(d, v);
            }
            Kind::MovwIndR { s, base, off } => {
                let v = self.g16(s);
                let m = if off { Mem::IndOff(base) } else { Mem::Ind(base) };
                let a = self.ea(m, false, f);
                let a = self.word_addr(a);
                self.mem.write16(a, v);
            }
            Kind::XchwRR { a, b } => {
                let (x, y) = (self.g16(a), self.g16(b));
                self.sg16(a, y);
                self.sg16(b, x);
            }
            Kind::Alu8 { op, d, s } => {
                let a = self.g8(d) as u32;
                let b = match s {
                    Some(s) => self.g8(s),
                    None => f.imm as u8,
                } as u32;
                match op {
                    Mn::And | Mn::Or | Mn::Xor => {
                        let r = match op {
                            Mn::And => a & b,
                            Mn::Or => a | b,
                            _ => a ^ b,
                        } as u8;
                        self.set_flags(PSW_Z, if r == 0 { PSW_Z } else { 0 });
                        self.sg8(d, r);
                    }
                    _ => {
                        let c = matches!(op, Mn::Addc | Mn::Subc) as u32 & self.flag(PSW_CY) as u32;
                        let (r, cy, ac) = if matches!(op, Mn::Add | Mn::Addc) {
                            let r = a + b + c;
                            (r, r > 0xFF, (a & 0xF) + (b & 0xF) + c > 0xF)
                        } else {
                            let r = a.wrapping_sub(b).wrapping_sub(c);
                            (r, a < b + c, (a & 0xF) < (b & 0xF) + c)
                        };
                        let r8 = r as u8;
                        self.set_flags(
                            PSW_Z | PSW_AC | PSW_CY,
                            (r8 == 0) as u8 * PSW_Z | ac as u8 * PSW_AC | cy as u8 * PSW_CY,
                        );
                        if op != Mn::Cmp {
                            self.sg8(d, r8);
                        }
                    }
                }
            }
            Kind::Alu16 { op, d, s } => {
                let a = self.g16(d) as u32;
                let b = match s {
                    Some(s) => self.g16(s),
                    None => f.imm,
                } as u32;
                let (r, cy, ac) = if op == Mn::Addw {
                    (a + b, a + b > 0xFFFF, (a & 0xF) + (b & 0xF) > 0xF)
                } else {
                    (a.wrapping_sub(b), a < b, (a & 0xF) < (b & 0xF))
                };
                let r16 = r as u16;
                self.set_flags(
                    PSW_Z | PSW_AC | PSW_CY,
                    (r16 == 0) as u8 * PSW_Z | ac as u8 * PSW_AC | cy as u8 * PSW_CY,
                );
                if op != Mn::Cmpw {
                    self.sg16(d, r16);
                }
            }
            Kind::IncwR { r } => {
                let v = self.g16(r);
                self.sg16(r, v.wrapping_add(1));
            }
            Kind::DecwR { r } => {
                let v = self.g16(r);
                self.sg16(r, v.wrapping_sub(1));
            }
            Kind::PushR { r } => {
                let (h, l) = (self.g8(r + 1), self.g8(r));
                self.push8(h);
                self.push8(l);
            }
            Kind::PopR { r } => {
                let sp = self.sp() as u32;
                let lo = self.mem.read8(0xF0000 | sp);
                let hi = self.mem.read8(0xF0000 | ((sp + 1) & 0xFFFF));
                self.set_sp((sp + 2) as u16);
                self.sg8(r, lo);
                self.sg8(r + 1, hi);
            }
            Kind::Bcc { op } => {
                if self.cond(op) {
                    ex.taken = true;
                    ex.next = Self::rel(pc, insn.len, f, false);
                }
            }
            Kind::Sk { op } => self.skip = self.cond(op),
        }
        if self.mem.event && self.mem.syscall {
            self.mem.syscall = false;
            if let Some(s) = self.syscall() {
                self.stop_with(s);
            }
        }
        ex
    }

    /// End the run after the current instruction.
    #[cold]
    fn stop_with(&mut self, s: Stop) {
        self.pending = Some(s);
        self.mem.event = true;
    }

    #[inline(never)]
    fn exec_generic(&mut self, d: &OpInfo, insn: &Insn, pc: u32, ex: &mut Exec) {
        let f = &insn.f;
        let next = ex.next;
        let mn = d.mn;
        let o0 = d.o0;
        let o1 = d.o1;
        let a8 = |c: &mut Cpu| c.g8(reg::A);
        match mn {
            Mn::Mov => {
                let v = self.rd8(&o1.unwrap(), f);
                self.wr8(&o0.unwrap(), f, v);
            }
            Mn::Movw => {
                let v = self.rd16(&o1.unwrap(), f);
                self.wr16(&o0.unwrap(), f, v);
            }
            Mn::Xch => {
                let (x, y) = (o0.unwrap(), o1.unwrap());
                let (a, b) = (self.rd8(&x, f), self.rd8(&y, f));
                self.wr8(&x, f, b);
                self.wr8(&y, f, a);
            }
            Mn::Xchw => {
                let (x, y) = (o0.unwrap(), o1.unwrap());
                let (a, b) = (self.rd16(&x, f), self.rd16(&y, f));
                self.wr16(&x, f, b);
                self.wr16(&y, f, a);
            }
            Mn::Oneb => self.wr8(&o0.unwrap(), f, 1),
            Mn::Clrb => self.wr8(&o0.unwrap(), f, 0),
            Mn::Onew => self.wr16(&o0.unwrap(), f, 1),
            Mn::Clrw => self.wr16(&o0.unwrap(), f, 0),
            Mn::Movs => {
                // MOVS [HL+byte], X
                let v = self.rd8(&o1.unwrap(), f);
                self.wr8(&o0.unwrap(), f, v);
                let a = a8(self);
                self.set_flag(PSW_Z, v == 0);
                self.set_flag(PSW_CY, !self.gdb_quirks && (a == 0 || v == 0));
            }
            Mn::Add | Mn::Addc | Mn::Sub | Mn::Subc | Mn::Cmp | Mn::Cmps => {
                let (x, y) = (o0.unwrap(), o1.unwrap());
                let a = self.rd8(&x, f) as u32;
                let b = self.rd8(&y, f) as u32;
                let c = matches!(mn, Mn::Addc | Mn::Subc) as u32 & self.flag(PSW_CY) as u32;
                let (r, cy, ac) = if matches!(mn, Mn::Add | Mn::Addc) {
                    let r = a + b + c;
                    (r, r > 0xFF, (a & 0xF) + (b & 0xF) + c > 0xF)
                } else {
                    let r = a.wrapping_sub(b).wrapping_sub(c);
                    (r, a < b + c, (a & 0xF) < (b & 0xF) + c)
                };
                let r8 = r as u8;
                self.set_flag(PSW_Z, r8 == 0);
                self.set_flag(PSW_AC, ac);
                if mn == Mn::Cmps && !self.gdb_quirks {
                    let acc = a8(self);
                    self.set_flag(PSW_CY, r8 != 0 || acc == 0 || a == 0);
                } else {
                    self.set_flag(PSW_CY, cy);
                }
                if !matches!(mn, Mn::Cmp | Mn::Cmps) {
                    self.wr8(&x, f, r8);
                }
            }
            Mn::And | Mn::Or | Mn::Xor => {
                let (x, y) = (o0.unwrap(), o1.unwrap());
                let a = self.rd8(&x, f);
                let b = self.rd8(&y, f);
                let r = match mn {
                    Mn::And => a & b,
                    Mn::Or => a | b,
                    _ => a ^ b,
                };
                self.set_flag(PSW_Z, r == 0);
                self.wr8(&x, f, r);
            }
            Mn::Cmp0 => {
                let v = self.rd8(&o0.unwrap(), f);
                self.set_flag(PSW_Z, v == 0);
                self.set_flag(PSW_AC, false);
                self.set_flag(PSW_CY, false);
            }
            Mn::Addw | Mn::Subw | Mn::Cmpw => {
                let (x, y) = (o0.unwrap(), o1.unwrap());
                let a = self.rd16(&x, f) as u32;
                let b = self.rd16(&y, f) as u32;
                let (r, cy, ac) = if mn == Mn::Addw {
                    (a + b, a + b > 0xFFFF, (a & 0xF) + (b & 0xF) > 0xF)
                } else {
                    (a.wrapping_sub(b), a < b, (a & 0xF) < (b & 0xF))
                };
                let r16 = r as u16;
                if x == Opnd::Sp {
                    // ADDW/SUBW SP, #byte: flags unchanged.
                    self.set_sp(r16);
                } else {
                    self.set_flag(PSW_Z, r16 == 0);
                    self.set_flag(PSW_CY, cy);
                    // AC is "undefined" for 16-bit operations; this is the
                    // low-nibble carry/borrow.
                    self.set_flag(PSW_AC, ac);
                    if mn != Mn::Cmpw {
                        self.wr16(&x, f, r16);
                    }
                }
            }
            Mn::Mulu => {
                let r = a8(self) as u16 * self.g8(reg::X) as u16;
                self.sg16(reg::AX, r);
            }
            Mn::Mulhu | Mn::Mulh => {
                let (a, b) = (self.g16(reg::AX), self.g16(reg::BC));
                let r = if mn == Mn::Mulhu {
                    a as u32 * b as u32
                } else {
                    (a as i16 as i32 * b as i16 as i32) as u32
                };
                self.sg16(reg::AX, r as u16);
                self.sg16(reg::BC, (r >> 16) as u16);
            }
            Mn::Divhu => {
                let (a, b) = (self.g16(reg::AX), self.g16(reg::DE));
                if b == 0 {
                    self.sg16(reg::DE, a);
                    self.sg16(reg::AX, 0xFFFF);
                } else {
                    self.sg16(reg::AX, a / b);
                    self.sg16(reg::DE, a % b);
                }
            }
            Mn::Divwu => {
                let a = self.g16(reg::AX) as u32 | (self.g16(reg::BC) as u32) << 16;
                let b = self.g16(reg::DE) as u32 | (self.g16(reg::HL) as u32) << 16;
                let (q, r) = if b == 0 {
                    (0xFFFF_FFFF, a)
                } else {
                    (a / b, a % b)
                };
                self.sg16(reg::AX, q as u16);
                self.sg16(reg::BC, (q >> 16) as u16);
                self.sg16(reg::DE, r as u16);
                self.sg16(reg::HL, (r >> 16) as u16);
            }
            Mn::Machu => {
                let p = self.g16(reg::AX) as u64 * self.g16(reg::BC) as u64;
                let s = self.macr() as u64 + p;
                self.set_macr(s as u32);
                self.set_flag(PSW_CY, s > 0xFFFF_FFFF);
                self.set_flag(PSW_AC, false);
            }
            Mn::Mach => {
                let p = self.g16(reg::AX) as i16 as i64 * self.g16(reg::BC) as i16 as i64;
                let m = self.macr() as i32 as i64;
                let s = m + p;
                self.set_macr(s as u32);
                let cy = if self.gdb_quirks {
                    (m as i32 ^ s as i32) < 0
                } else {
                    s > i32::MAX as i64 || s < i32::MIN as i64
                };
                self.set_flag(PSW_CY, cy);
                self.set_flag(PSW_AC, (s as i32) < 0);
            }
            Mn::Inc | Mn::Dec => {
                let x = o0.unwrap();
                let v = self.rd8(&x, f);
                let (r, ac) = if mn == Mn::Inc {
                    (v.wrapping_add(1), v & 0xF == 0xF)
                } else {
                    (v.wrapping_sub(1), v & 0xF == 0)
                };
                self.set_flag(PSW_Z, r == 0);
                self.set_flag(PSW_AC, ac);
                self.wr8(&x, f, r);
            }
            Mn::Incw | Mn::Decw => {
                let x = o0.unwrap();
                let v = self.rd16(&x, f);
                let r = if mn == Mn::Incw {
                    v.wrapping_add(1)
                } else {
                    v.wrapping_sub(1)
                };
                self.wr16(&x, f, r);
            }
            Mn::Shl | Mn::Shr | Mn::Sar => {
                let x = o0.unwrap();
                let Some(Opnd::Count(n)) = o1 else {
                    unreachable!()
                };
                let v = self.rd8(&x, f);
                let (r, cy) = match mn {
                    Mn::Shl => (((v as u16) << n) as u8, v >> (8 - n) & 1),
                    Mn::Shr => (v >> n, v >> (n - 1) & 1),
                    _ => (((v as i8) >> n) as u8, v >> (n - 1) & 1),
                };
                self.set_flag(PSW_CY, cy != 0);
                self.wr8(&x, f, r);
            }
            Mn::Shlw | Mn::Shrw | Mn::Sarw => {
                let x = o0.unwrap();
                let Some(Opnd::Count(n)) = o1 else {
                    unreachable!()
                };
                let v = self.rd16(&x, f);
                let (r, cy) = match mn {
                    Mn::Shlw => (((v as u32) << n) as u16, v >> (16 - n) & 1),
                    Mn::Shrw => (v >> n, v >> (n - 1) & 1),
                    _ => (((v as i16) >> n) as u16, v >> (n - 1) & 1),
                };
                self.set_flag(PSW_CY, cy != 0);
                self.wr16(&x, f, r);
            }
            Mn::Ror | Mn::Rol | Mn::Rorc | Mn::Rolc => {
                let x = o0.unwrap();
                let v = self.rd8(&x, f);
                let c = self.flag(PSW_CY) as u8;
                let (r, cy) = match mn {
                    Mn::Ror => (v.rotate_right(1), v & 1),
                    Mn::Rol => (v.rotate_left(1), v >> 7),
                    Mn::Rorc => (v >> 1 | c << 7, v & 1),
                    _ => (v << 1 | c, v >> 7),
                };
                self.set_flag(PSW_CY, cy != 0);
                self.wr8(&x, f, r);
            }
            Mn::Rolwc => {
                let x = o0.unwrap();
                let v = self.rd16(&x, f);
                let c = self.flag(PSW_CY) as u16;
                self.set_flag(PSW_CY, v >> 15 != 0);
                self.wr16(&x, f, v << 1 | c);
            }
            Mn::Mov1 => {
                let v = self.bit_get(&o1.unwrap(), f);
                self.bit_set(&o0.unwrap(), f, v);
            }
            Mn::And1 | Mn::Or1 | Mn::Xor1 => {
                let b = self.bit_get(&o1.unwrap(), f);
                let c = self.flag(PSW_CY);
                let r = match mn {
                    Mn::And1 => c & b,
                    Mn::Or1 => c | b,
                    _ => c ^ b,
                };
                self.set_flag(PSW_CY, r);
            }
            Mn::Set1 => self.bit_set(&o0.unwrap(), f, true),
            Mn::Clr1 => self.bit_set(&o0.unwrap(), f, false),
            Mn::Not1 => {
                let c = self.flag(PSW_CY);
                self.set_flag(PSW_CY, !c);
            }
            Mn::Call => {
                let t = match o0.unwrap() {
                    Opnd::R16(r) => self.cs() << 16 | self.g16(r) as u32,
                    Opnd::Rel16 => Self::rel(pc, insn.len, f, true),
                    Opnd::M { m: Mem::Abs16, .. } => f.addr,
                    Opnd::Abs20 => f.addr,
                    o => unreachable!("call {o:?}"),
                };
                self.push_pc(next, None);
                ex.next = t;
            }
            Mn::Callt => {
                let Some(Opnd::Callt(a)) = o0 else {
                    unreachable!()
                };
                let t = self.mem.read16(a as u32) as u32;
                self.mem.flash_read = false; // table read is part of CALLT's timing
                self.push_pc(next, None);
                ex.next = t;
            }
            Mn::Brk => {
                let v = self.mem.read16(0x7E) as u32;
                self.mem.flash_read = false;
                if v == 0 || v == 0xFFFF {
                    // No handler: the vector is zero or erased flash.
                    self.stop_with(Stop::Brk);
                } else {
                    let p = self.psw();
                    self.push_pc(next, Some(p));
                    self.set_flag(PSW_IE, false);
                    ex.next = v;
                }
            }
            Mn::Ret | Mn::Reti | Mn::Retb => {
                let (t, p) = self.pop_pc();
                if mn != Mn::Ret {
                    self.set_psw(p);
                }
                ex.next = t;
            }
            Mn::Push => {
                match o0.unwrap() {
                    Opnd::Psw => {
                        let p = self.psw();
                        if self.gdb_quirks {
                            // rl78-elf-gdb pushes the PSW word (PSW, FFFFBH).
                            let hi = self.mem.b[0xFFFFB];
                            self.push8(hi);
                            self.push8(p);
                        } else {
                            self.push8(p);
                            self.push8(0);
                        }
                    }
                    Opnd::R16(r) => {
                        let (h, l) = (self.g8(r + 1), self.g8(r));
                        self.push8(h);
                        self.push8(l);
                    }
                    o => unreachable!("push {o:?}"),
                }
            }
            Mn::Pop => {
                let sp = self.sp() as u32;
                let lo = self.mem.read8(0xF0000 | sp);
                let hi = self.mem.read8(0xF0000 | ((sp + 1) & 0xFFFF));
                self.set_sp((sp + 2) as u16);
                match o0.unwrap() {
                    Opnd::Psw => self.set_psw(if self.gdb_quirks { lo } else { hi }),
                    Opnd::R16(r) => {
                        self.sg8(r, lo);
                        self.sg8(r + 1, hi);
                    }
                    o => unreachable!("pop {o:?}"),
                }
            }
            Mn::Br => {
                let t = match o0.unwrap() {
                    Opnd::R16(_) => self.cs() << 16 | self.g16(reg::AX) as u32,
                    Opnd::Rel8 => Self::rel(pc, insn.len, f, false),
                    Opnd::Rel16 => Self::rel(pc, insn.len, f, true),
                    Opnd::M { m: Mem::Abs16, .. } => f.addr,
                    Opnd::Abs20 => f.addr,
                    o => unreachable!("br {o:?}"),
                };
                if t == pc {
                    let code = self.g8(reg::X);
                    self.stop_with(Stop::SelfLoop { code });
                }
                ex.next = t;
            }
            Mn::Bc | Mn::Bnc | Mn::Bz | Mn::Bnz | Mn::Bh | Mn::Bnh => {
                let (z, c) = (self.flag(PSW_Z), self.flag(PSW_CY));
                let cond = match mn {
                    Mn::Bc => c,
                    Mn::Bnc => !c,
                    Mn::Bz => z,
                    Mn::Bnz => !z,
                    Mn::Bh => !(z || c),
                    _ => z || c,
                };
                if cond {
                    ex.taken = true;
                    ex.next = Self::rel(pc, insn.len, f, false);
                }
            }
            Mn::Bt | Mn::Bf | Mn::Btclr => {
                let b = o0.unwrap();
                let v = self.bit_get(&b, f);
                let cond = if mn == Mn::Bf { !v } else { v };
                if cond {
                    if mn == Mn::Btclr {
                        self.bit_set(&b, f, false);
                    }
                    ex.taken = true;
                    ex.next = Self::rel(pc, insn.len, f, false);
                }
            }
            Mn::Skc | Mn::Sknc | Mn::Skz | Mn::Sknz | Mn::Skh | Mn::Sknh => {
                let (z, c) = (self.flag(PSW_Z), self.flag(PSW_CY));
                self.skip = match mn {
                    Mn::Skc => c,
                    Mn::Sknc => !c,
                    Mn::Skz => z,
                    Mn::Sknz => !z,
                    Mn::Skh => !(z || c),
                    _ => z || c,
                };
            }
            Mn::Sel => {
                let Some(Opnd::Bank(n)) = o0 else {
                    unreachable!()
                };
                let p = self.psw() & !0x28;
                self.set_psw(p | (n & 1) << 3 | (n & 2) << 4);
            }
            Mn::Nop => {}
            Mn::Ei => self.set_flag(PSW_IE, true),
            Mn::Di => self.set_flag(PSW_IE, false),
            Mn::Halt | Mn::Stop => {
                let code = a8(self);
                self.stop_with(Stop::Exit(code))
            }
        }
    }

    /// libgloss simulator syscalls (`mov a, #N; mov 0xffffe, #0x42; skc`).
    /// Only exit is meaningful: libgloss's syscall stubs assume the GCC ABI
    /// (arguments on the stack) and stdout goes through UART0 instead.
    fn syscall(&mut self) -> Option<Stop> {
        match self.g8(reg::A) {
            1 => Some(Stop::Exit(self.g8(reg::X))),
            _ => {
                self.set_flag(PSW_CY, true);
                self.sg16(reg::AX, 0xFFFF);
                None
            }
        }
    }

    /// Run until a stop condition: `max_clocks` reached, PC at one of `stops`
    /// (before executing it), or the program stops. `trace` sees the CPU
    /// before every instruction.
    pub fn run_until(
        &mut self,
        max_clocks: u64,
        stops: &[u32],
        mut trace: Option<&mut dyn FnMut(&Cpu)>,
    ) -> Stop {
        if stops.is_empty() && trace.is_none() {
            return if self.prof.is_some() {
                self.run_fast::<true>(max_clocks)
            } else {
                self.run_fast::<false>(max_clocks)
            };
        }
        loop {
            if self.n.clocks >= max_clocks {
                return Stop::Limit;
            }
            if !self.skip && stops.contains(&self.pc) {
                return Stop::Breakpoint(self.pc);
            }
            if let Some(t) = trace.as_mut() {
                t(self);
            }
            if let Some(s) = self.step() {
                return s;
            }
        }
    }

    fn run_fast<const PROF: bool>(&mut self, max_clocks: u64) -> Stop {
        self.enter();
        while self.n.clocks < max_clocks {
            if let Some(s) = self.step_impl::<PROF>() {
                return s;
            }
        }
        Stop::Limit
    }

    pub fn run(&mut self, max_clocks: u64, stop_at: Option<u32>) -> Stop {
        let stops: &[u32] = match &stop_at {
            Some(a) => std::slice::from_ref(a),
            None => &[],
        };
        self.run_until(max_clocks, stops, None)
    }
}
