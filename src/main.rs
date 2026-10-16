use anyhow::{Context, Result};
use clap::Parser;
use rl78sim::cpu::{self, Counts, Cpu, Stop};
use rl78sim::elf::Image;
use rl78sim::isa::{Core, Disasm, reg};
use rl78sim::mem::{MemMap, Memory, RomWrites};
use std::collections::HashMap;
use std::io::Write;

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
enum CoreArg {
    S1,
    S2,
    S3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
enum QuirksArg {
    /// Follow the manual.
    None,
    /// Reproduce rl78-elf-gdb's deviations (MACH/CMPS/MOVS carry, PUSH/POP PSW
    /// slot).
    Gdb,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
enum MapArg {
    /// rl78-elf-gdb's simulator: mirror F1000H-F7FFFH, RAM from F8000H,
    /// stores to flash/mirror go through, nothing is unmapped.
    Vendor,
    /// Same layout with hardware semantics: stores to flash or the mirror
    /// and accesses to reserved space stop the simulation.
    Strict,
}

/// RL78 instruction-level simulator.
///
/// Runs an RL78 ELF from its reset vector until HALT/STOP, `_exit`, a
/// fault, or a limit; the program's UART0 output goes to stdout and its exit
/// code becomes ours. Clock counts follow the RL78 Software Manual.
#[derive(Parser, Debug)]
#[command(version)]
struct Args {
    elf: String,
    /// CPU core (clock table and instruction set); default: from the ELF
    /// header flags, else S3.
    #[arg(long, value_enum)]
    core: Option<CoreArg>,
    /// Model the RL78/G13 multiplier/divider at FFFF0H (default: when the
    /// ELF is an S2 object that uses it).
    #[arg(long)]
    mdu: Option<bool>,
    /// Memory map (a .modelinfo section in the ELF overrides the layout).
    #[arg(long, value_enum, default_value = "strict")]
    map: MapArg,
    /// Override what stores to flash/mirror do.
    #[arg(long, value_enum)]
    rom_writes: Option<RomWrites>,
    /// Instruction-semantics deviations to reproduce.
    #[arg(long, value_enum, default_value = "none")]
    quirks: QuirksArg,
    /// With -v, also print "total clocks: N".
    #[arg(long)]
    print_clocks: bool,
    /// Stop on undefined behaviour (16-bit address overflow in based
    /// addressing, misaligned word access) instead of counting it.
    #[arg(long)]
    strict: bool,
    /// Stop after this many clocks.
    #[arg(long, default_value_t = u64::MAX)]
    max_clocks: u64,
    /// Count only from the first time this symbol/address is reached...
    #[arg(long)]
    start: Option<String>,
    /// ...until this symbol/address is reached (the simulation then stops).
    #[arg(long)]
    stop: Option<String>,
    /// After --stop, keep running until one of these symbols is reached and
    /// report which ("verdict" in --stats; repeatable).
    #[arg(long)]
    verdict: Vec<String>,
    /// Report the NUL-terminated string at this symbol at the end ("strings"
    /// in --stats; repeatable).
    #[arg(long)]
    cstr: Vec<String>,
    /// Write counters as JSON to this file ("-" = stderr).
    #[arg(long)]
    stats: Option<String>,
    /// Write a per-function and per-PC profile (JSON) of the counted region.
    #[arg(long)]
    profile: Option<String>,
    /// Trace every instruction (PC, disassembly, registers) to stderr.
    #[arg(long)]
    trace: bool,
    /// Report every store to this address (repeatable; mirror stores are
    /// reported at the flash address they reach).
    #[arg(long, value_parser = parse_addr)]
    watch: Vec<u32>,
    /// Print memory (ADDR:LEN, hex bytes) to stderr when the run ends.
    #[arg(long, value_parser = parse_range)]
    dump: Vec<(u32, u32)>,
    /// Print "Exit code: N" at the end.
    #[arg(short)]
    v: bool,
    /// No warnings on stderr (for test harnesses that capture it).
    #[arg(short, long)]
    quiet: bool,
}

fn parse_addr(s: &str) -> Result<u32, String> {
    let r = match s.strip_prefix("0x") {
        Some(h) => u32::from_str_radix(h, 16),
        None => s.parse(),
    };
    r.map_err(|e| e.to_string())
}

fn parse_range(s: &str) -> Result<(u32, u32), String> {
    let (a, n) = s.split_once(':').ok_or("expected ADDR:LEN")?;
    Ok((parse_addr(a)?, parse_addr(n)?))
}

fn counts_json(n: &Counts) -> serde_json::Value {
    serde_json::json!({
        "instrs": n.instrs, "clocks": n.clocks, "skipped": n.skipped,
        "branches_taken": n.taken, "branches_not_taken": n.not_taken,
        "flash_data_reads": n.flash_data, "hazard_waits": n.hazards,
        "ram_fetch_instrs": n.ram_fetch,
    })
}

fn trace_line(cpu: &Cpu) -> String {
    let isa = cpu::isa();
    let pc = cpu.pc;
    let insn = isa.decode(|k| cpu.mem.fetch(pc + k));
    let dis = match insn {
        Some(i) => format!("{}", Disasm { isa, insn: i, pc }),
        None => "(illegal)".into(),
    };
    format!(
        "{pc:05x}: {dis:<28} AX={:04x} BC={:04x} DE={:04x} HL={:04x} SP={:04x} PSW={:02x} ES={:x} CS={:x}",
        cpu.r16(reg::AX),
        cpu.r16(reg::BC),
        cpu.r16(reg::DE),
        cpu.r16(reg::HL),
        cpu.sp(),
        cpu.psw(),
        cpu.mem.b[0xFFFFD],
        cpu.mem.b[0xFFFFC],
    )
}

fn run(a: Args) -> Result<i32> {
    let img = Image::read(&a.elf)?;
    let mut map = match a.map {
        MapArg::Vendor => MemMap::vendor(),
        MapArg::Strict => MemMap::default_strict(),
    };
    if let Some(mi) = &img.modelinfo {
        mi.apply(&mut map);
    }
    if let Some(w) = a.rom_writes {
        map.rom_writes = w;
    }
    let mut mem = Memory::new(map);
    for w in img.load(&mut mem) {
        if !a.quiet {
            eprintln!("rl78sim: warning: {}: {w}", a.elf);
        }
    }
    mem.watch = a.watch.clone();
    let core = match a.core {
        Some(CoreArg::S1) => Core::S1,
        Some(CoreArg::S2) => Core::S2,
        Some(CoreArg::S3) => Core::S3,
        None => img.core().unwrap_or(Core::S3),
    };
    if a.mdu.unwrap_or(core == Core::S2 && img.uses_mdu()) {
        mem.mdu = Some(Default::default());
    }
    let mut cpu = Cpu::new(mem, core);
    cpu.strict = a.strict;
    cpu.gdb_quirks = a.quirks == QuirksArg::Gdb;
    cpu.reset();
    // Start at the ELF entry point, as rl78-elf-gdb does (tests linked with
    // -nostartfiles have no reset vector); hardware uses the reset vector.
    if img.entry != 0 {
        if img.entry as u32 != cpu.pc && !a.quiet {
            eprintln!(
                "rl78sim: warning: {}: entry point {:#07x} differs from the reset vector ({:#06x})",
                a.elf, img.entry, cpu.pc
            );
        }
        cpu.pc = img.entry as u32;
    }
    let sym = |s: &Option<String>| -> Result<Option<u32>> {
        s.as_ref()
            .map(|s| img.lookup(s).with_context(|| format!("no symbol {s}")))
            .transpose()
    };
    let start = sym(&a.start)?;
    let stop = sym(&a.stop)?;
    if a.profile.is_some() {
        cpu.enable_profile();
    }

    let verdicts: Vec<(u32, &str)> = a
        .verdict
        .iter()
        .map(|v| {
            Ok((
                img.lookup(v).with_context(|| format!("no symbol {v}"))?,
                v.as_str(),
            ))
        })
        .collect::<Result<_>>()?;

    let out = std::io::stdout();
    let mut tracer = |c: &Cpu| eprintln!("{}", trace_line(c));
    // Run to one of `stops`, flushing UART output every 2^24 clocks.
    let mut go = |cpu: &mut Cpu, stops: &[u32]| -> Stop {
        loop {
            let chunk = cpu.n.clocks.saturating_add(1 << 24).min(a.max_clocks);
            let t: Option<&mut dyn FnMut(&Cpu)> = if a.trace { Some(&mut tracer) } else { None };
            let r = cpu.run_until(chunk, stops, t);
            if !cpu.mem.uart.is_empty() {
                let mut o = out.lock();
                let _ = o.write_all(&cpu.mem.uart);
                let _ = o.flush();
                cpu.mem.uart.clear();
            }
            match r {
                Stop::Limit if chunk < a.max_clocks => continue,
                r => return r,
            }
        }
    };

    // Phase 1: run to the start of the counted region (if any).
    let mut base = Counts::default();
    let mut base_mix = [0u64; 96];
    let mut never_started = false;
    let mut stop_reason = None;
    if let Some(s) = start {
        match go(&mut cpu, &[s]) {
            Stop::Breakpoint(_) => {
                base = cpu.n;
                base_mix = cpu.mix;
                if let Some(p) = &mut cpu.prof {
                    p.iter_mut().for_each(|e| *e = [0, 0]);
                }
            }
            st => {
                never_started = true;
                stop_reason = Some(st);
            }
        }
    }
    // Phase 2: run to the stop address / exit.
    let stop_reason = match stop_reason {
        Some(s) => s,
        None => go(&mut cpu, stop.as_slice()),
    };
    let region = cpu.n.sub(&base);
    // The profile covers the region only (not the verdict phase).
    let prof = cpu.prof.take();
    let region_mix = std::array::from_fn::<u64, 96, _>(|i| cpu.mix[i] - base_mix[i]);
    // Phase 3: which verdict symbol is reached first.
    let mut verdict = None;
    if !verdicts.is_empty() && matches!(stop_reason, Stop::Breakpoint(_)) {
        let addrs: Vec<u32> = verdicts.iter().map(|v| v.0).collect();
        if let Stop::Breakpoint(pc) = go(&mut cpu, &addrs) {
            verdict = verdicts.iter().find(|v| v.0 == pc).map(|v| v.1.to_string());
        }
    }
    let mut cstrs = Vec::new();
    for name in &a.cstr {
        let at = img
            .lookup(name)
            .with_context(|| format!("no symbol {name}"))?;
        let bytes: Vec<u8> = (0..)
            .map(|k| cpu.mem.fetch(at + k))
            .take_while(|&b| b != 0)
            .take(1 << 16)
            .collect();
        cstrs.push((name.clone(), String::from_utf8_lossy(&bytes).into_owned()));
    }
    for &(addr, len) in &a.dump {
        for row in (0..len).step_by(16) {
            let bytes: Vec<String> = (row..(row + 16).min(len))
                .map(|k| format!("{:02x}", cpu.mem.fetch(addr + k)))
                .collect();
            eprintln!("{:05x}: {}", addr + row, bytes.join(" "));
        }
    }

    let code = match &stop_reason {
        Stop::Exit(c) | Stop::SelfLoop { code: c } => *c as i32,
        Stop::Breakpoint(_) => 0,
        Stop::Limit => {
            println!("Simulation time limit reached");
            1
        }
        other => {
            eprintln!(
                "rl78sim: {}: stopped at pc {:#07x}: {}",
                a.elf,
                cpu.pc,
                describe(other)
            );
            1
        }
    };
    if (start.is_some() || stop.is_some())
        && !matches!(stop_reason, Stop::Breakpoint(_))
        && !a.quiet
    {
        eprintln!(
            "rl78sim: warning: never reached {}",
            if never_started {
                a.start.as_deref().unwrap()
            } else {
                a.stop.as_deref().unwrap_or("?")
            }
        );
    }
    if cpu.ub_count > 0 && !a.quiet {
        eprintln!(
            "rl78sim: warning: {} undefined-behaviour event(s); first: {}",
            cpu.ub_count,
            cpu.ub.as_deref().unwrap_or("?")
        );
    }
    if let Some(path) = &a.stats {
        let j = serde_json::json!({
            "elf": a.elf,
            "core": format!("{core:?}").to_lowercase(),
            "stop": describe(&stop_reason),
            "exit_code": code,
            "region": counts_json(&region),
            "total": counts_json(&cpu.n),
            "undefined_behaviour": cpu.ub_count,
            "region_mix": (0..rl78sim::isa::Mn::COUNT)
                .filter(|&i| region_mix[i] > 0)
                .map(|i| (rl78sim::isa::Mn::from_index(i).name().to_string(),
                          serde_json::json!(region_mix[i])))
                .collect::<serde_json::Map<_, _>>(),
            "verdict": verdict,
            "strings": cstrs.iter().map(|(k, v)| (k.clone(), serde_json::json!(v))).collect::<serde_json::Map<_, _>>(),
        });
        let s = serde_json::to_string_pretty(&j)?;
        if path == "-" {
            eprintln!("{s}");
        } else {
            std::fs::write(path, s + "\n")?;
        }
    }
    if let (Some(path), Some(prof)) = (&a.profile, &prof) {
        let mut funcs: HashMap<&str, [u64; 2]> = HashMap::new();
        let mut pcs = Vec::new();
        for (pc, e) in prof.iter().enumerate() {
            if e[0] == 0 && e[1] == 0 {
                continue;
            }
            let f = funcs.entry(img.func_of(pc as u32)).or_default();
            f[0] += e[0];
            f[1] += e[1];
            pcs.push(serde_json::json!([pc, e[0], e[1]]));
        }
        let mut fv: Vec<_> = funcs.into_iter().collect();
        fv.sort_by(|a, b| b.1[1].cmp(&a.1[1]).then(a.0.cmp(b.0)));
        let j = serde_json::json!({
            "functions": fv.iter().map(|(n, e)| serde_json::json!({"name": n, "instrs": e[0], "clocks": e[1]})).collect::<Vec<_>>(),
            "pcs": pcs,
        });
        std::fs::write(path, serde_json::to_string(&j)? + "\n")?;
    }
    if a.v {
        if a.print_clocks {
            println!("total clocks: {}", cpu.n.clocks);
        }
        println!("Exit code: {code}");
    }
    Ok(code)
}

fn describe(s: &Stop) -> String {
    match s {
        Stop::Exit(c) => format!("exit {c}"),
        Stop::SelfLoop { code } => format!("exit {code} (branch to self)"),
        Stop::Brk => "BRK with no handler".into(),
        Stop::Illegal { pc, bytes } => format!("illegal instruction at {pc:#07x}: {bytes:02x?}"),
        Stop::Fault(f) => f.to_string(),
        Stop::Undefined(u) => format!("undefined behaviour: {u}"),
        Stop::Limit => "clock limit".into(),
        Stop::Breakpoint(a) => format!("reached {a:#07x}"),
        Stop::Io => "stopped by a peripheral".into(),
    }
}

fn main() {
    let r = run(Args::parse());
    match r {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("rl78sim: {e:#}");
            std::process::exit(1);
        }
    }
}
