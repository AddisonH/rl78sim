//! ELF loading: PT_LOAD segments at their physical (load) addresses, the
//! symbol table, and the libgloss `.modelinfo` memory description.

use crate::mem::{MemMap, Memory};
use anyhow::{Context, Result, bail};
use object::{Object, ObjectSection, ObjectSegment, ObjectSymbol, SymbolKind};

pub struct Image {
    pub entry: u64,
    /// ELF e_flags (EF_RL78_*: CPU 100H = S1, 300H = S2, 308H = S3; bit 0
    /// = the multiplier/divider unit is used).
    pub flags: u32,
    /// (address, name), sorted by address; code symbols only.
    pub funcs: Vec<(u32, String)>,
    /// All named symbols, for --start/--stop lookups.
    pub syms: Vec<(u32, String)>,
    pub modelinfo: Option<ModelInfo>,
    segments: Vec<(u32, Vec<u8>)>,
}

/// libgloss `rl78_sim_*_modelinfo.ld`: family, ROM size, RAM start, mirror
/// ROM start, mirror RAM start, mirror size (six little-endian words).
#[derive(Clone, Copy, Debug)]
pub struct ModelInfo {
    pub family: u32,
    pub rom_size: u32,
    pub ram_start: u32,
    pub mirror_rom: u32,
    pub mirror_ram: u32,
    pub mirror_size: u32,
}

impl ModelInfo {
    pub fn apply(&self, m: &mut MemMap) {
        m.rom_end = self.rom_size;
        m.ram_start = self.ram_start;
        m.mirror_src = self.mirror_rom;
        m.mirror_start = self.mirror_ram;
        m.mirror_len = self.mirror_size;
    }
}

impl Image {
    pub fn read(path: &str) -> Result<Image> {
        let data = std::fs::read(path).with_context(|| format!("reading {path}"))?;
        let obj = object::File::parse(&*data).with_context(|| format!("parsing {path}"))?;
        if obj.architecture() != object::Architecture::Unknown {
            // object has no RL78 architecture; anything it recognises is wrong.
            bail!("{path}: not an RL78 ELF ({:?})", obj.architecture());
        }
        let mut segments = Vec::new();
        if let object::File::Elf32(ref e) = obj {
            use object::read::elf::ProgramHeader;
            let endian = e.endian();
            for ph in e.elf_program_headers() {
                if ph.p_type(endian) != object::elf::PT_LOAD || ph.p_filesz(endian) == 0 {
                    continue;
                }
                let bytes = ph
                    .data(endian, &*data)
                    .map_err(|_| anyhow::anyhow!("segment data"))?;
                segments.push((ph.p_paddr(endian), bytes.to_vec()));
            }
        } else {
            for seg in obj.segments() {
                let (off, _) = seg.file_range();
                let _ = off;
                segments.push((seg.address() as u32, seg.data()?.to_vec()));
            }
        }
        let mut syms = Vec::new();
        let mut funcs = Vec::new();
        for s in obj.symbols() {
            let Ok(name) = s.name() else { continue };
            if name.is_empty() || s.is_undefined() {
                continue;
            }
            let a = s.address() as u32;
            syms.push((a, name.to_string()));
            let is_code = s.kind() == SymbolKind::Text
                || s.section_index()
                    .and_then(|i| obj.section_by_index(i).ok())
                    .map(|sec| sec.kind() == object::SectionKind::Text)
                    .unwrap_or(false);
            if is_code && !name.starts_with(".L") && !name.starts_with('$') {
                funcs.push((a, name.to_string()));
            }
        }
        funcs.sort();
        funcs.dedup_by_key(|f| f.0);
        let modelinfo = obj.section_by_name(".modelinfo").and_then(|s| {
            let d = s.data().ok()?;
            if d.len() < 24 {
                return None;
            }
            let w = |i: usize| u32::from_le_bytes(d[i * 4..i * 4 + 4].try_into().unwrap());
            Some(ModelInfo {
                family: w(0),
                rom_size: w(1),
                ram_start: w(2),
                mirror_rom: w(3),
                mirror_ram: w(4),
                mirror_size: w(5),
            })
        });
        let flags = match obj.flags() {
            object::FileFlags::Elf { e_flags, .. } => e_flags,
            _ => 0,
        };
        Ok(Image {
            entry: obj.entry(),
            flags,
            funcs,
            syms,
            modelinfo,
            segments,
        })
    }

    /// Load the ROM segments. Segments in RAM/SFR space (F0000H and up) are
    /// skipped, as rl78-elf-gdb does: on hardware only flash is programmed and
    /// RAM contents come from the startup code.
    pub fn load(&self, mem: &mut Memory) -> Vec<String> {
        let mut warnings = Vec::new();
        for (a, bytes) in &self.segments {
            if *a as usize + bytes.len() > 0xF0000 {
                warnings.push(format!(
                    "segment at {a:#07x} ({} bytes) is not in ROM and was not loaded",
                    bytes.len()
                ));
                continue;
            }
            mem.poke(*a, bytes);
        }
        warnings
    }

    /// Core the object was built for (None: not recorded).
    pub fn core(&self) -> Option<crate::isa::Core> {
        use crate::isa::Core;
        match self.flags & 0x308 {
            0x100 => Some(Core::S1),
            0x300 => Some(Core::S2),
            0x308 => Some(Core::S3),
            _ => None,
        }
    }

    /// Built to use the G13 multiplier/divider (EF_RL78_FU_EXIST).
    pub fn uses_mdu(&self) -> bool {
        self.flags & 1 != 0
    }

    /// Address of a symbol (with or without the leading underscore the RL78
    /// ABI adds), or a number.
    pub fn lookup(&self, s: &str) -> Option<u32> {
        if let Some(h) = s.strip_prefix("0x") {
            return u32::from_str_radix(h, 16).ok();
        }
        if let Ok(n) = s.parse() {
            return Some(n);
        }
        let us = format!("_{s}");
        self.syms
            .iter()
            .find(|(_, n)| n == s)
            .or_else(|| self.syms.iter().find(|(_, n)| *n == us))
            .map(|x| x.0)
    }

    /// Function containing `pc`.
    pub fn func_of(&self, pc: u32) -> &str {
        match self.funcs.partition_point(|f| f.0 <= pc) {
            0 => "?",
            i => &self.funcs[i - 1].1,
        }
    }
}
