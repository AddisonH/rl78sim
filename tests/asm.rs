//! Assembly tests: every `tests/asm/*.s` is assembled with llvm-mc, linked
//! with ld.lld, run, and checked against expectations written in its comments.
//! The expected values come from the RL78 Software Manual, not from a
//! simulator run.
//!
//! Directives (in `;` comments):
//!   ARGS: core=s1|s2|s3 map=strict|vendor strict quirks=gdb mdu
//!   EXPECT: AX=0x1234 BC=.. DE=.. HL=.. SP=.. PSW=.. ES=.. CS=.. MACR=..
//!           A=.. X=.. B=.. C=.. D=.. E=.. H=.. L=..        (at the end)
//!   EXPECT-MEM: 0xfa000 12 34 56 ...                       (at the end)
//!   STOP: exit N | fault | illegal | undefined | brk        (default: exit 0)
//!   CLOCKS: N   clocks from label t0 up to label t1 (t1 not executed)
//!   INSTRS: N   instructions in the same region
//!   UART: text  bytes written to UART0 (escapes: \n \r)
//!
//! Layout: .vec at 0, .text at 0x200, .rodata at 0x3000 (in the mirror at
//! F3000H), .ramcode copied by the test itself if it needs code in RAM.
//! Set RL78_LLVM_BIN to the directory with llvm-mc and ld.lld.

use rl78sim::cpu::{Cpu, Stop};
use rl78sim::elf::Image;
use rl78sim::isa::{Core, reg};
use rl78sim::mem::{MemMap, Memory};
use std::path::{Path, PathBuf};
use std::process::Command;

const LDS: &str = "SECTIONS {
  .vec 0 : { KEEP(*(.vec)) }
  .brkvec 0x7e : { KEEP(*(.brkvec)) }
  .callt0 0x80 : { KEEP(*(.callt0)) }
  .text 0x200 : { *(.text) }
  .rodata 0x3000 : { *(.rodata) }
}
";

fn llvm_bin() -> Option<PathBuf> {
    // RL78_LLVM_BIN, else the first directory on PATH with an llvm-mc.
    if let Ok(p) = std::env::var("RL78_LLVM_BIN") {
        let p = PathBuf::from(p);
        return p.join("llvm-mc").exists().then_some(p);
    }
    std::env::split_paths(&std::env::var_os("PATH")?).find(|d| d.join("llvm-mc").exists())
}

fn build(bin: &Path, src: &Path, out: &Path) -> Result<PathBuf, String> {
    let obj = out.join("t.o");
    let elf = out.join("t.elf");
    let lds = out.join("t.ld");
    std::fs::write(&lds, LDS).unwrap();
    let r = Command::new(bin.join("llvm-mc"))
        .args(["-triple=rl78", "-filetype=obj", "-o"])
        .arg(&obj)
        .arg(src)
        .output()
        .unwrap();
    if !r.status.success() {
        return Err(String::from_utf8_lossy(&r.stderr).into());
    }
    let r = Command::new(bin.join("ld.lld"))
        .arg("-T")
        .arg(&lds)
        .arg(&obj)
        .arg("-o")
        .arg(&elf)
        .output()
        .unwrap();
    if !r.status.success() {
        return Err(String::from_utf8_lossy(&r.stderr).into());
    }
    Ok(elf)
}

fn num(s: &str) -> u64 {
    match s.strip_prefix("0x") {
        Some(h) => u64::from_str_radix(h, 16).unwrap(),
        None => s.parse().unwrap(),
    }
}

fn run_one(bin: &Path, src: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(src).unwrap();
    let directive = |name: &str| -> Vec<String> {
        text.lines()
            .filter_map(|l| l.split_once(';').map(|x| x.1.trim()))
            .filter_map(|c| c.strip_prefix(name).map(|r| r.trim().to_string()))
            .collect()
    };
    let tmp = std::env::temp_dir().join(format!(
        "rl78sim-asm-{}-{}",
        std::process::id(),
        src.file_stem().unwrap().to_string_lossy()
    ));
    std::fs::create_dir_all(&tmp).unwrap();
    let elf = build(bin, src, &tmp)?;
    let img = Image::read(elf.to_str().unwrap()).map_err(|e| e.to_string())?;

    let mut map = MemMap::default_strict();
    let mut core = Core::S3;
    let mut strict = false;
    let mut quirks = false;
    let mut mdu = false;
    for a in directive("ARGS:")
        .iter()
        .flat_map(|l| l.split_whitespace().map(String::from).collect::<Vec<_>>())
    {
        match a.as_str() {
            "core=s1" => core = Core::S1,
            "core=s2" => core = Core::S2,
            "core=s3" => core = Core::S3,
            "map=vendor" => map = MemMap::vendor(),
            "map=strict" => map = MemMap::default_strict(),
            "strict" => strict = true,
            "quirks=gdb" => quirks = true,
            "mdu" => mdu = true,
            o => return Err(format!("unknown ARGS {o}")),
        }
    }
    let mut mem = Memory::new(map);
    let _ = img.load(&mut mem);
    if mdu {
        mem.mdu = Some(Default::default());
    }
    let mut cpu = Cpu::new(mem, core);
    cpu.strict = strict;
    cpu.gdb_quirks = quirks;
    cpu.reset();

    let limit = 10_000_000;
    let (t0, t1) = (img.lookup("t0"), img.lookup("t1"));
    let mut region = None;
    let stop = if let (Some(t0), Some(t1)) = (t0, t1) {
        match cpu.run(limit, Some(t0)) {
            Stop::Breakpoint(_) => {}
            s => return Err(format!("never reached t0: {s:?}")),
        }
        let base = cpu.n;
        match cpu.run(limit, Some(t1)) {
            Stop::Breakpoint(_) => {}
            s => return Err(format!("never reached t1: {s:?}")),
        }
        region = Some(cpu.n.sub(&base));
        cpu.run(limit, None)
    } else {
        cpu.run(limit, None)
    };

    let mut errs = Vec::new();
    let want_stop = directive("STOP:").pop().unwrap_or_else(|| "exit 0".into());
    let ok = match (want_stop.as_str(), &stop) {
        (w, Stop::Exit(c) | Stop::SelfLoop { code: c }) if w.starts_with("exit") => {
            num(w[4..].trim()) == *c as u64
        }
        ("fault", Stop::Fault(_)) | ("illegal", Stop::Illegal { .. }) => true,
        ("undefined", Stop::Undefined(_)) | ("brk", Stop::Brk) => true,
        _ => false,
    };
    if !ok {
        errs.push(format!("stop: want {want_stop}, got {stop:?}"));
    }
    for line in directive("EXPECT:") {
        for kv in line.split_whitespace() {
            let (k, v) = kv.split_once('=').ok_or(format!("bad EXPECT {kv}"))?;
            let want = num(v);
            let got = match k {
                "AX" => cpu.r16(reg::AX) as u64,
                "BC" => cpu.r16(reg::BC) as u64,
                "DE" => cpu.r16(reg::DE) as u64,
                "HL" => cpu.r16(reg::HL) as u64,
                "A" => cpu.r8(reg::A) as u64,
                "X" => cpu.r8(reg::X) as u64,
                "B" => cpu.r8(reg::B) as u64,
                "C" => cpu.r8(reg::C) as u64,
                "D" => cpu.r8(reg::D) as u64,
                "E" => cpu.r8(reg::E) as u64,
                "H" => cpu.r8(reg::H) as u64,
                "L" => cpu.r8(reg::L) as u64,
                "SP" => cpu.sp() as u64,
                "PSW" => cpu.psw() as u64,
                "ES" => cpu.mem.b[0xFFFFD] as u64,
                "CS" => cpu.mem.b[0xFFFFC] as u64,
                "MACR" => cpu.macr() as u64,
                _ => return Err(format!("unknown register {k}")),
            };
            if got != want {
                errs.push(format!("{k}: want {want:#x}, got {got:#x}"));
            }
        }
    }
    for line in directive("EXPECT-MEM:") {
        let mut it = line.split_whitespace();
        let addr = num(it.next().unwrap()) as u32;
        for (i, b) in it.enumerate() {
            let want = u8::from_str_radix(b, 16).map_err(|e| format!("EXPECT-MEM {b}: {e}"))?;
            let got = cpu.mem.fetch(addr + i as u32);
            if got != want {
                errs.push(format!(
                    "mem {:#07x}: want {want:02x}, got {got:02x}",
                    addr + i as u32
                ));
            }
        }
    }
    for (name, pick) in [("CLOCKS:", 0), ("INSTRS:", 1)] {
        if let Some(w) = directive(name).pop() {
            let r = region.ok_or("CLOCKS/INSTRS need labels t0 and t1")?;
            let got = if pick == 0 { r.clocks } else { r.instrs };
            if got != num(&w) {
                errs.push(format!("{name} want {w}, got {got} ({r:?})"));
            }
        }
    }
    if let Some(w) = directive("UART:").pop() {
        let want = w.replace("\\n", "\n").replace("\\r", "\r");
        if cpu.mem.uart != want.as_bytes() {
            errs.push(format!(
                "uart: want {want:?}, got {:?}",
                String::from_utf8_lossy(&cpu.mem.uart)
            ));
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs.join("\n    "))
    }
}

#[test]
fn asm_tests() {
    let Some(bin) = llvm_bin() else {
        if std::env::var_os("RL78SIM_SKIP_ASM").is_some() {
            eprintln!("asm tests skipped (no llvm-mc)");
            return;
        }
        panic!("llvm-mc not found: set RL78_LLVM_BIN (or RL78SIM_SKIP_ASM=1 to skip)");
    };
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/asm");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "s"))
        .collect();
    files.sort();
    let filter = std::env::var("ASM_TEST").ok();
    let mut failed = Vec::new();
    let mut n = 0;
    for f in &files {
        let name = f.file_name().unwrap().to_string_lossy().to_string();
        if filter.as_ref().is_some_and(|x| !name.contains(x.as_str())) {
            continue;
        }
        n += 1;
        match run_one(&bin, f) {
            Ok(()) => eprintln!("ok   {name}"),
            Err(e) => {
                eprintln!("FAIL {name}\n    {e}");
                failed.push(name);
            }
        }
    }
    assert!(
        failed.is_empty(),
        "{} of {n} asm tests failed: {failed:?}",
        failed.len()
    );
}
