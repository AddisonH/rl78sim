//! The RL78 instruction set, loaded from `isa/rl78.tsv` (extracted from the
//! RL78 Family Software Manual R01US0015EJ0230, Tables 5-5..5-8, and checked
//! against llvm-mc by `tools/check_encodings.py`).
//!
//! Each row is one encoding: mnemonic, concrete operands (registers, bit
//! numbers and CALLT slots are spelled out), opcode bytes (hex bytes followed
//! by operand fields) and clocks per core.

use std::fmt;

const TABLE: &str = include_str!("../isa/rl78.tsv");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Mn {
    Mov,
    Xch,
    Oneb,
    Clrb,
    Movs,
    Movw,
    Xchw,
    Onew,
    Clrw,
    Add,
    Addc,
    Sub,
    Subc,
    And,
    Or,
    Xor,
    Cmp,
    Cmp0,
    Cmps,
    Addw,
    Subw,
    Cmpw,
    Mulu,
    Mulhu,
    Mulh,
    Divhu,
    Divwu,
    Machu,
    Mach,
    Inc,
    Dec,
    Incw,
    Decw,
    Shr,
    Shrw,
    Shl,
    Shlw,
    Sar,
    Sarw,
    Ror,
    Rol,
    Rorc,
    Rolc,
    Rolwc,
    Mov1,
    And1,
    Or1,
    Xor1,
    Set1,
    Clr1,
    Not1,
    Call,
    Callt,
    Brk,
    Ret,
    Reti,
    Retb,
    Push,
    Pop,
    Br,
    Bc,
    Bnc,
    Bz,
    Bnz,
    Bh,
    Bnh,
    Bt,
    Bf,
    Btclr,
    Skc,
    Sknc,
    Skz,
    Sknz,
    Skh,
    Sknh,
    Sel,
    Nop,
    Ei,
    Di,
    Halt,
    Stop,
}

impl Mn {
    /// Number of variants (NAMES lists them in declaration order; the
    /// mnemonic_count test checks both).
    pub const COUNT: usize = NAMES.len();

    pub fn from_index(i: usize) -> Mn {
        assert!(i < Self::COUNT);
        // SAFETY: Mn is a fieldless enum with COUNT variants numbered 0..COUNT.
        unsafe { std::mem::transmute::<u8, Mn>(i as u8) }
    }

    fn parse(s: &str) -> Option<Mn> {
        use Mn::*;
        Some(match s {
            "MOV" => Mov,
            "XCH" => Xch,
            "ONEB" => Oneb,
            "CLRB" => Clrb,
            "MOVS" => Movs,
            "MOVW" => Movw,
            "XCHW" => Xchw,
            "ONEW" => Onew,
            "CLRW" => Clrw,
            "ADD" => Add,
            "ADDC" => Addc,
            "SUB" => Sub,
            "SUBC" => Subc,
            "AND" => And,
            "OR" => Or,
            "XOR" => Xor,
            "CMP" => Cmp,
            "CMP0" => Cmp0,
            "CMPS" => Cmps,
            "ADDW" => Addw,
            "SUBW" => Subw,
            "CMPW" => Cmpw,
            "MULU" => Mulu,
            "MULHU" => Mulhu,
            "MULH" => Mulh,
            "DIVHU" => Divhu,
            "DIVWU" => Divwu,
            "MACHU" => Machu,
            "MACH" => Mach,
            "INC" => Inc,
            "DEC" => Dec,
            "INCW" => Incw,
            "DECW" => Decw,
            "SHR" => Shr,
            "SHRW" => Shrw,
            "SHL" => Shl,
            "SHLW" => Shlw,
            "SAR" => Sar,
            "SARW" => Sarw,
            "ROR" => Ror,
            "ROL" => Rol,
            "RORC" => Rorc,
            "ROLC" => Rolc,
            "ROLWC" => Rolwc,
            "MOV1" => Mov1,
            "AND1" => And1,
            "OR1" => Or1,
            "XOR1" => Xor1,
            "SET1" => Set1,
            "CLR1" => Clr1,
            "NOT1" => Not1,
            "CALL" => Call,
            "CALLT" => Callt,
            "BRK" => Brk,
            "RET" => Ret,
            "RETI" => Reti,
            "RETB" => Retb,
            "PUSH" => Push,
            "POP" => Pop,
            "BR" => Br,
            "BC" => Bc,
            "BNC" => Bnc,
            "BZ" => Bz,
            "BNZ" => Bnz,
            "BH" => Bh,
            "BNH" => Bnh,
            "BT" => Bt,
            "BF" => Bf,
            "BTCLR" => Btclr,
            "SKC" => Skc,
            "SKNC" => Sknc,
            "SKZ" => Skz,
            "SKNZ" => Sknz,
            "SKH" => Skh,
            "SKNH" => Sknh,
            "SEL" => Sel,
            "NOP" => Nop,
            "EI" => Ei,
            "DI" => Di,
            "HALT" => Halt,
            "STOP" => Stop,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        // Debug spelling, lower-cased, is the assembler mnemonic.
        NAMES[self as usize]
    }
}

const NAMES: &[&str] = &[
    "mov", "xch", "oneb", "clrb", "movs", "movw", "xchw", "onew", "clrw", "add", "addc", "sub",
    "subc", "and", "or", "xor", "cmp", "cmp0", "cmps", "addw", "subw", "cmpw", "mulu", "mulhu",
    "mulh", "divhu", "divwu", "machu", "mach", "inc", "dec", "incw", "decw", "shr", "shrw", "shl",
    "shlw", "sar", "sarw", "ror", "rol", "rorc", "rolc", "rolwc", "mov1", "and1", "or1", "xor1",
    "set1", "clr1", "not1", "call", "callt", "brk", "ret", "reti", "retb", "push", "pop", "br",
    "bc", "bnc", "bz", "bnz", "bh", "bnh", "bt", "bf", "btclr", "skc", "sknc", "skz", "sknz",
    "skh", "sknh", "sel", "nop", "ei", "di", "halt", "stop",
];

/// 8-bit register numbers are the offsets within a register bank:
/// X=0 A=1 C=2 B=3 E=4 D=5 L=6 H=7. 16-bit pairs: AX=0 BC=2 DE=4 HL=6.
pub mod reg {
    pub const X: u8 = 0;
    pub const A: u8 = 1;
    pub const C: u8 = 2;
    pub const B: u8 = 3;
    pub const E: u8 = 4;
    pub const D: u8 = 5;
    pub const L: u8 = 6;
    pub const H: u8 = 7;
    pub const AX: u8 = 0;
    pub const BC: u8 = 2;
    pub const DE: u8 = 4;
    pub const HL: u8 = 6;
    pub const R8: [&str; 8] = ["x", "a", "c", "b", "e", "d", "l", "h"];
    pub const R16: [&str; 4] = ["ax", "bc", "de", "hl"];
}

/// Where a memory or bit operand's address comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mem {
    /// !addr16 (F0000H-FFFFFH, or ES:addr16)
    Abs16,
    /// saddr / saddrp: FFE20H-FFF1FH
    Saddr,
    /// sfr / sfrp: FFF00H-FFFFFH
    Sfr,
    /// [DE] / [HL]
    Ind(u8),
    /// [DE+byte] / [HL+byte] / [SP+byte] (the register pair, or 8 = SP)
    IndOff(u8),
    /// [HL+B] / [HL+C] (the 8-bit index register)
    HlIdx(u8),
    /// word[B] / word[C] (8-bit index) / word[BC]
    Based(u8),
    BasedBc,
}

pub const SP_BASE: u8 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bit {
    A,
    Psw,
    Cy,
    /// a memory byte (saddr, sfr, !addr16, [HL])
    Mem(Mem),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opnd {
    R8(u8),
    R16(u8),
    Sp,
    Psw,
    Cs,
    Es,
    Imm8,
    Imm16,
    /// A memory operand; `es` = ES: prefixed.
    M {
        m: Mem,
        es: bool,
    },
    /// A bit operand: base, bit number, ES: prefix.
    B {
        b: Bit,
        n: u8,
        es: bool,
    },
    /// !!addr20
    Abs20,
    /// $addr20 (8-bit displacement)
    Rel8,
    /// $!addr20 (16-bit displacement)
    Rel16,
    /// Immediate shift/rotate count (also the "1" in ROR A, 1)
    Count(u8),
    /// CALLT [00xxh]: the table address
    Callt(u8),
    /// SEL RBn
    Bank(u8),
}

/// Operand fields that follow the fixed opcode bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Data,
    DataL,
    DataH,
    Adr,
    AdrL,
    AdrH,
    AdrS,
    Saddr,
    Sfr,
}

/// Clocks for one core: (condition not met, condition met); equal for
/// instructions that do not branch or skip.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Clk {
    pub nt: u8,
    pub t: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Core {
    S1,
    S2,
    S3,
}

#[derive(Clone, Debug)]
pub struct OpDef {
    pub mn: Mn,
    pub ops: Vec<Opnd>,
    pub fixed: Vec<u8>,
    pub fields: Vec<Field>,
    pub len: u8,
    /// S3 clocks (note 1: RAM/SFR/no data access).
    pub clk: Clk,
    /// S3 clocks when reading code/data flash as data (note 2), if listed.
    pub clk_flash: Option<Clk>,
    /// S2 and S1 clocks; None where the core lacks the instruction.
    pub clk_s2: Option<Clk>,
    pub clk_s1: Option<Clk>,
    /// S2 and S1 clocks when reading flash as data (note 2), if listed.
    pub clk_flash_s2: Option<Clk>,
    pub clk_flash_s1: Option<Clk>,
    /// The manual's operand text, for diagnostics.
    pub text: String,
}

impl OpDef {
    pub fn clocks(&self, core: Core) -> Option<Clk> {
        match core {
            Core::S3 => Some(self.clk),
            Core::S2 => self.clk_s2,
            Core::S1 => self.clk_s1,
        }
    }

    /// Clocks when the instruction reads code flash as data (note 2): the
    /// listed flash clocks, else the normal ones.
    pub fn clocks_flash(&self, core: Core) -> Option<Clk> {
        let flash = match core {
            Core::S3 => self.clk_flash,
            Core::S2 => self.clk_flash_s2,
            Core::S1 => self.clk_flash_s1,
        };
        flash.or(self.clocks(core))
    }
}

fn parse_clk(s: &str) -> Option<Clk> {
    if s.is_empty() {
        return None;
    }
    let mut it = s.split('/').map(|x| x.parse::<u8>().unwrap());
    let nt = it.next().unwrap();
    let t = it.next().unwrap_or(nt);
    Some(Clk { nt, t })
}

fn r8(s: &str) -> Option<u8> {
    reg::R8
        .iter()
        .position(|r| r.eq_ignore_ascii_case(s))
        .map(|i| i as u8)
}

fn r16(s: &str) -> Option<u8> {
    reg::R16
        .iter()
        .position(|r| r.eq_ignore_ascii_case(s))
        .map(|i| 2 * i as u8)
}

fn parse_mem(s: &str) -> Option<Mem> {
    Some(match s {
        "!addr16" => Mem::Abs16,
        "saddr" | "saddrp" => Mem::Saddr,
        "sfr" | "sfrp" => Mem::Sfr,
        "[DE]" => Mem::Ind(reg::DE),
        "[HL]" => Mem::Ind(reg::HL),
        "[DE+byte]" => Mem::IndOff(reg::DE),
        "[HL+byte]" => Mem::IndOff(reg::HL),
        "[SP+byte]" => Mem::IndOff(SP_BASE),
        "[HL+B]" => Mem::HlIdx(reg::B),
        "[HL+C]" => Mem::HlIdx(reg::C),
        "word[B]" => Mem::Based(reg::B),
        "word[C]" => Mem::Based(reg::C),
        "word[BC]" => Mem::BasedBc,
        _ => return None,
    })
}

fn parse_opnd(s: &str) -> Result<Opnd, String> {
    let (es, body) = match s.strip_prefix("ES:") {
        Some(b) => (true, b.trim()),
        None => (false, s),
    };
    // bit operands: X.n
    if let Some((base, n)) = body.rsplit_once('.') {
        if let Ok(n) = n.parse::<u8>() {
            let b = match base {
                "A" => Bit::A,
                "PSW" => Bit::Psw,
                "CY" => Bit::Cy,
                _ => Bit::Mem(parse_mem(base).ok_or(format!("bit base {base}"))?),
            };
            return Ok(Opnd::B { b, n, es });
        }
    }
    if let Some(m) = parse_mem(body) {
        return Ok(Opnd::M { m, es });
    }
    if es {
        return Err(format!("ES: on {body}"));
    }
    if let Some(r) = r8(body) {
        return Ok(Opnd::R8(r));
    }
    if let Some(r) = r16(body) {
        return Ok(Opnd::R16(r));
    }
    if let Some(h) = body.strip_prefix("[00").and_then(|x| x.strip_suffix("h]")) {
        return Ok(Opnd::Callt(
            u8::from_str_radix(h, 16).map_err(|e| e.to_string())?,
        ));
    }
    if let Some(n) = body.strip_prefix("RB") {
        return Ok(Opnd::Bank(n.parse().map_err(|_| format!("bank {body}"))?));
    }
    if let Ok(n) = body.parse::<u8>() {
        return Ok(Opnd::Count(n));
    }
    Ok(match body {
        "SP" => Opnd::Sp,
        "PSW" => Opnd::Psw,
        "CS" => Opnd::Cs,
        "ES" => Opnd::Es,
        "CY" => Opnd::B {
            b: Bit::Cy,
            n: 0,
            es: false,
        },
        "#byte" => Opnd::Imm8,
        "#word" => Opnd::Imm16,
        "!!addr20" => Opnd::Abs20,
        "$addr20" => Opnd::Rel8,
        "$!addr20" => Opnd::Rel16,
        _ => return Err(format!("operand {body:?}")),
    })
}

fn parse_field(s: &str) -> Option<Field> {
    Some(match s {
        "data" => Field::Data,
        "datal" => Field::DataL,
        "datah" => Field::DataH,
        "adr" => Field::Adr,
        "adrl" => Field::AdrL,
        "adrh" => Field::AdrH,
        "adrs" => Field::AdrS,
        "saddr" => Field::Saddr,
        "sfr" => Field::Sfr,
        _ => return None,
    })
}

/// Decoded operand field values of one instruction instance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Fields {
    /// #byte, or #word
    pub imm: u16,
    /// adr (8-bit offset or displacement)
    pub adr: u8,
    /// adrl/adrh(/adrs): 16-bit address, displacement, or 20-bit address
    pub addr: u32,
    pub saddr: u8,
    pub sfr: u8,
}

pub struct Isa {
    pub ops: Vec<OpDef>,
    nodes: Vec<Node>,
}

struct Node {
    next: [u16; 256],
    leaf: Option<u16>,
}

impl Node {
    fn new() -> Node {
        Node {
            next: [0; 256],
            leaf: None,
        }
    }
}

/// A decoded instruction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Insn {
    pub op: u16,
    pub len: u8,
    pub f: Fields,
}

impl Isa {
    pub fn load() -> Isa {
        Isa::parse(TABLE).expect("isa/rl78.tsv")
    }

    pub fn parse(table: &str) -> Result<Isa, String> {
        let mut ops = Vec::new();
        for (ln, line) in table.lines().enumerate().skip(1) {
            let c: Vec<&str> = line.split('\t').collect();
            let err = |e: String| format!("rl78.tsv:{}: {e}", ln + 1);
            let mn = Mn::parse(c[0]).ok_or_else(|| err(format!("mnemonic {}", c[0])))?;
            let ops_v = if c[1].is_empty() {
                vec![]
            } else {
                c[1].split(", ")
                    .map(parse_opnd)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(err)?
            };
            let mut fixed = Vec::new();
            let mut fields = Vec::new();
            for tok in c[2].split_whitespace() {
                if let Some(f) = parse_field(tok) {
                    fields.push(f);
                } else {
                    if !fields.is_empty() {
                        return Err(err("opcode byte after a field".into()));
                    }
                    fixed.push(u8::from_str_radix(tok, 16).map_err(|e| err(e.to_string()))?);
                }
            }
            let len = (fixed.len() + fields.len()) as u8;
            ops.push(OpDef {
                mn,
                ops: ops_v,
                fixed,
                fields,
                len,
                clk: parse_clk(c[3]).ok_or_else(|| err("no S3 clocks".into()))?,
                clk_flash: parse_clk(c[4]),
                clk_s2: parse_clk(c[5]),
                clk_s1: parse_clk(c[6]),
                clk_flash_s2: c.get(7).and_then(|x| parse_clk(x)),
                clk_flash_s1: c.get(8).and_then(|x| parse_clk(x)),
                text: c[1].to_string(),
            });
        }
        let mut nodes = vec![Node::new()];
        for (i, op) in ops.iter().enumerate() {
            let mut n = 0usize;
            for &b in &op.fixed {
                let nx = nodes[n].next[b as usize];
                n = if nx == 0 {
                    nodes.push(Node::new());
                    let k = nodes.len() - 1;
                    nodes[n].next[b as usize] = k as u16;
                    k
                } else {
                    nx as usize
                };
            }
            if let Some(prev) = nodes[n].leaf {
                // EI/DI are SET1/CLR1 PSW.7 (same bytes and clocks); keep the
                // operand-less spelling.
                let p = &ops[prev as usize];
                let alias = |a: &OpDef, b: &OpDef| {
                    matches!((a.mn, b.mn), (Mn::Set1, Mn::Ei) | (Mn::Clr1, Mn::Di))
                        && a.text == "PSW.7"
                        && a.clk == b.clk
                };
                if alias(p, op) {
                    nodes[n].leaf = Some(i as u16);
                    continue;
                }
                if alias(op, p) {
                    continue;
                }
                return Err(format!(
                    "duplicate encoding {:02X?}: {:?} {} / {:?} {}",
                    op.fixed, ops[prev as usize].mn, ops[prev as usize].text, op.mn, op.text
                ));
            }
            nodes[n].leaf = Some(i as u16);
        }
        Ok(Isa { ops, nodes })
    }

    /// Decode the instruction whose bytes `fetch(0)`, `fetch(1)`, ... yields.
    /// The longest fixed-byte prefix with an entry wins.
    #[inline]
    pub fn decode(&self, mut fetch: impl FnMut(u32) -> u8) -> Option<Insn> {
        let mut n = 0usize;
        let mut best = None;
        let mut i = 0u32;
        loop {
            if let Some(l) = self.nodes[n].leaf {
                best = Some(l);
            }
            let nx = self.nodes[n].next[fetch(i) as usize];
            if nx == 0 {
                break;
            }
            n = nx as usize;
            i += 1;
        }
        let op = best?;
        let d = &self.ops[op as usize];
        let mut f = Fields::default();
        let mut at = d.fixed.len() as u32;
        for &fl in &d.fields {
            let b = fetch(at);
            at += 1;
            match fl {
                Field::Data => f.imm = b as u16,
                Field::DataL => f.imm = (f.imm & 0xFF00) | b as u16,
                Field::DataH => f.imm = (f.imm & 0x00FF) | (b as u16) << 8,
                Field::Adr => f.adr = b,
                Field::AdrL => f.addr |= b as u32,
                Field::AdrH => f.addr |= (b as u32) << 8,
                Field::AdrS => f.addr |= ((b & 0x0F) as u32) << 16,
                Field::Saddr => f.saddr = b,
                Field::Sfr => f.sfr = b,
            }
        }
        Some(Insn { op, len: d.len, f })
    }
}

/// Effective address of a saddr operand byte.
#[inline]
pub fn saddr_addr(b: u8) -> u32 {
    if b >= 0x20 {
        0xFFE00 + b as u32
    } else {
        0xFFF00 + b as u32
    }
}

/// Disassembly in llvm-objdump's style (for traces and decoder tests).
pub struct Disasm<'a> {
    pub isa: &'a Isa,
    pub insn: Insn,
    /// Address of the instruction (for branch targets).
    pub pc: u32,
}

impl fmt::Display for Disasm<'_> {
    fn fmt(&self, w: &mut fmt::Formatter<'_>) -> fmt::Result {
        let d = &self.isa.ops[self.insn.op as usize];
        let f = &self.insn.f;
        write!(w, "{}", d.mn.name())?;
        let next = self.pc + d.len as u32;
        for (i, o) in d.ops.iter().enumerate() {
            write!(w, "{}", if i == 0 { " " } else { ", " })?;
            // CALL/BR !addr16 target 00000H-0FFFFH, not the F0000H page.
            if matches!(d.mn, Mn::Call | Mn::Br)
                && *o
                    == (Opnd::M {
                        m: Mem::Abs16,
                        es: false,
                    })
            {
                write!(w, "!0x{:x}", f.addr)?;
                continue;
            }
            fmt_opnd(w, o, f, next)?;
        }
        Ok(())
    }
}

fn fmt_mem(w: &mut fmt::Formatter<'_>, m: &Mem, f: &Fields) -> fmt::Result {
    match m {
        Mem::Abs16 => write!(w, "!0x{:x}", 0xF0000 | f.addr),
        Mem::Saddr => write!(w, "0x{:x}", saddr_addr(f.saddr)),
        Mem::Sfr => write!(w, "0x{:x}", 0xFFF00 + f.sfr as u32),
        Mem::Ind(r) => write!(w, "[{}]", reg::R16[*r as usize / 2]),
        Mem::IndOff(SP_BASE) => write!(w, "[sp+{}]", f.adr),
        Mem::IndOff(r) => write!(w, "[{}+{}]", reg::R16[*r as usize / 2], f.adr),
        Mem::HlIdx(r) => write!(w, "[hl+{}]", reg::R8[*r as usize]),
        Mem::Based(r) => write!(w, "{}[{}]", f.addr, reg::R8[*r as usize]),
        Mem::BasedBc => write!(w, "{}[bc]", f.addr),
    }
}

fn fmt_opnd(w: &mut fmt::Formatter<'_>, o: &Opnd, f: &Fields, next: u32) -> fmt::Result {
    match o {
        Opnd::R8(r) => write!(w, "{}", reg::R8[*r as usize]),
        Opnd::R16(r) => write!(w, "{}", reg::R16[*r as usize / 2]),
        Opnd::Sp => write!(w, "sp"),
        Opnd::Psw => write!(w, "psw"),
        Opnd::Cs => write!(w, "cs"),
        Opnd::Es => write!(w, "es"),
        Opnd::Imm8 | Opnd::Imm16 => write!(w, "#{}", f.imm),
        Opnd::M { m, es } => {
            if *es {
                write!(w, "es:")?;
            }
            if *es && *m == Mem::Abs16 {
                write!(w, "!0x{:x}", f.addr)
            } else {
                fmt_mem(w, m, f)
            }
        }
        Opnd::B { b, n, es } => {
            if *es {
                write!(w, "es:")?;
            }
            match b {
                Bit::A => write!(w, "a.{n}"),
                Bit::Psw => write!(w, "psw.{n}"),
                Bit::Cy => write!(w, "cy"),
                Bit::Mem(Mem::Abs16) if *es => write!(w, "!0x{:x}.{n}", f.addr),
                Bit::Mem(m) => {
                    fmt_mem(w, m, f)?;
                    write!(w, ".{n}")
                }
            }
        }
        Opnd::Abs20 => write!(w, "!!0x{:x}", f.addr),
        Opnd::Rel8 => write!(
            w,
            "$0x{:x}",
            next.wrapping_add(f.adr as i8 as u32) & 0xFFFFF
        ),
        Opnd::Rel16 => write!(
            w,
            "$!0x{:x}",
            next.wrapping_add(f.addr as u16 as i16 as u32) & 0xFFFFF
        ),
        Opnd::Count(n) => write!(w, "{n}"),
        Opnd::Callt(a) => write!(w, "[0x{:04x}]", *a as u32),
        Opnd::Bank(n) => write!(w, "rb{n}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mnemonic_count() {
        assert_eq!(Mn::Stop as usize + 1, Mn::COUNT);
        for i in 0..Mn::COUNT {
            assert_eq!(format!("{:?}", Mn::from_index(i)).to_lowercase(), NAMES[i]);
        }
    }

    #[test]
    fn table_loads() {
        let isa = Isa::load();
        assert_eq!(isa.ops.len(), 1224);
    }

    #[test]
    fn decodes_longest_prefix() {
        let isa = Isa::load();
        let dec = |b: &[u8]| {
            let i = isa
                .decode(|k| b.get(k as usize).copied().unwrap_or(0))
                .unwrap();
            let d = &isa.ops[i.op as usize];
            (d.mn, d.text.clone(), i.len)
        };
        assert_eq!(dec(&[0xCE, 0xFB, 0x01]), (Mn::Mulhu, "".into(), 3));
        assert_eq!(dec(&[0xCE, 0xFA, 0x12]), (Mn::Mov, "PSW, #byte".into(), 3));
        assert_eq!(dec(&[0xCE, 0x30, 0x12]), (Mn::Mov, "sfr, #byte".into(), 3));
        assert_eq!(
            dec(&[0x11, 0x8F, 0x34, 0x12]),
            (Mn::Mov, "A, ES:!addr16".into(), 4)
        );
        assert_eq!(dec(&[0x61, 0xEF]), (Mn::Sel, "RB2".into(), 2));
    }
}
