# rl78sim

A fast instruction-level simulator for the Renesas RL78 (S1, S2 and S3
cores). It runs an ELF file from the reset vector and tells you exactly how
many instructions and CPU clocks it took, where they were spent, and whether
the program did anything a real part would not allow.

It is a single command-line program written in Rust, with no licence server
and no dependencies beyond a Rust toolchain.

## What it is good for

- **Measuring code.** Clock counts follow the RL78 Family Software Manual,
  including the cases the simpler simulators leave out (flash read as data,
  branch taken/not taken, skipped instructions, the wait after writing a
  register that the next instruction addresses through). CC-RL's EEMBC
  CoreMark measures 577,093 clocks per iteration here; Renesas publishes
  582,411 cycles for the hardware, a difference of 0.9%.
- **Finding where the time goes.** One option gives clocks and instructions
  per function and per instruction address, for the whole run or for a region
  between two symbols.
- **Running tests.** The program's UART0 output goes to stdout and its exit
  code becomes the simulator's, so a test is a plain shell command. Start-up
  takes about 2 ms and runs are deterministic.
- **Catching layout mistakes.** By default a store to flash or to the flash
  mirror, or an access to reserved address space, stops the run with a
  message instead of silently corrupting something. A mislinked `.data`
  section shows up at its first store rather than as a wrong result later.
- **Speed.** About 110 million simulated instructions per second with every
  counter on: a 258-million-instruction CoreMark run takes 2.3 s.

It does not model interrupts or peripherals (see [Limits](#limits)), so it
is for compiler, library and algorithm work, not for firmware that depends
on timers or I/O.

## Build

```sh
cargo build --release        # the binary is target/release/rl78sim
```

## Use

Run a program:

```sh
rl78sim prog.elf             # UART0 output on stdout; exit code = the program's
```

Measure the region between two symbols (addresses work too):

```sh
rl78sim --start bench_begin --stop bench_end --stats - prog.elf
```

`--stats` writes JSON to a file, or to stderr with `-`:

```json
{
  "core": "s3",
  "exit_code": 0,
  "region": {
    "instrs": 12960426,
    "clocks": 17454271,
    "branches_taken": 173911,
    "branches_not_taken": 172,
    "skipped": 140933,
    "flash_data_reads": 696320,
    "hazard_waits": 1,
    "ram_fetch_instrs": 0
  },
  "region_mix": { "movw": 6487524, "xch": 1044484, "addw": 1044502, "...": 0 },
  "stop": "reached 0x021ea",
  "total": { "...": 0 }
}
```

`region` counts from the first time `--start` is reached until `--stop`;
`total` counts the whole run. `region_mix` is the number of times each
mnemonic ran. `hazard_waits` are the one-clock waits of Software Manual
7.2.3, and `ram_fetch_instrs` are instructions executed from RAM.

Profile it:

```sh
rl78sim --start bench_begin --stop bench_end --profile prof.json prog.elf
```

`prof.json` holds `functions` (name, instructions, clocks, sorted by clocks)
and `pcs` (one `[address, instructions, clocks]` entry per instruction that
ran). Joining `pcs` with a disassembly gives an annotated listing of a hot
loop.

Debug it:

```sh
rl78sim --trace prog.elf 2>trace.txt     # every instruction: PC, disassembly, registers
rl78sim --watch 0xffe20 prog.elf         # report every store to an address
rl78sim --dump 0xf8000:64 prog.elf       # hex dump of memory when the run ends
rl78sim --max-clocks 100000000 prog.elf  # give up on a program that hangs
```

A trace line looks like this:

```
021f4: or a, x                      AX=0010 BC=1111 DE=1111 HL=1111 SP=fe20 PSW=06 ES=f CS=0
```

`rl78sim --help` lists every option.

## What a program needs

Nothing special for a program that already runs under `rl78-elf-gdb`'s
simulator.

- **Output:** a byte written to the UART0 data register SDR00 (FFF10H)
  appears on stdout. This is what libgloss's `_write` does.
- **Exit:** `HALT` or `STOP` ends the run with the exit code in A (libgloss's
  `_exit`). A branch to itself ends it with the code in X (newlib's `nosys`
  `_exit`). `BRK` with an unprogrammed vector stops the run.
- **Verdicts:** when the result is reported by reaching one of several labels
  rather than by an exit code, `--verdict pass --verdict fail` keeps running
  after `--stop` and reports which was reached. `--cstr sym` prints the
  string at a symbol when the run ends.

## Cores

`--core s1|s2|s3` selects the clock table and the instruction set. Without
it the ELF header flags decide (`EF_RL78_CPU_*`), and S3 is the default. An
instruction the core does not have (MULHU on S2, SEL on S1) stops the run as
illegal.

For an S2 object that uses it, the RL78/G13 multiplier/divider at FFFF0H is
modelled; `--mdu true|false` overrides that.

## Memory maps

| Region | Addresses |
|---|---|
| Code flash | 00000H - EFFFFH |
| Mirror of flash 01000H onward | F1000H - F7FFFH |
| RAM (the register banks are its top 32 bytes) | F8000H - FFEFFH |

A `.modelinfo` section in the ELF (libgloss's `*_modelinfo.ld`) changes the
layout.

- `--map strict` (the default) has hardware semantics: stores to flash or
  the mirror and accesses to reserved space stop the run.
- `--map vendor` behaves like `rl78-elf-gdb`: stores go through to flash and
  nothing is unmapped. libgloss's own `rl78-sim.ld` puts RAM at F5700H,
  inside the mirror, so programs linked with `-fsim` need this mode.
- The 2nd SFRs are F0000H-F07FFH, as the manual has them on every RL78;
  `--map vendor` extends them to F0FFFH like `rl78-elf-gdb`. A mirror that
  starts at F0800H (the RL78/G15's) takes precedence.
- `--rom-writes trap|ignore|through` overrides what a store to flash does.
  `ignore` is what hardware does.
- `--strict` also stops on undefined behaviour (16-bit address overflow in
  based addressing, a misaligned word access) instead of counting it.

Memory starts with the same contents as under `rl78-elf-gdb` (FFH in flash,
33H in F0000H-FFFFFH, 11H in the register banks), so runs are reproducible.
On hardware RAM is undefined at power-on.

## Accuracy

- **Against hardware:** CC-RL's EEMBC CoreMark, 577,093 clocks per iteration
  against the published 582,411 cycles (-0.9%). Counting instructions plus
  branch penalties alone gives 539,735 (-7.3%).
- **Against `rl78-elf-gdb`:** `tools/lockstep.py` compares register state
  after every instruction. A complete newlib program (111,000 instructions)
  and CoreMark agree. `tools/fuzz_isa.py` builds random programs from the
  whole encoding table; 300 of 300 programs of about 1,900 instructions
  agree.
- **Against the manuals:** `tests/asm/*.s` cover flags, multiply, divide and
  multiply-accumulate including overflow, shifts, stack frames, addressing
  modes, register banks, the G13 multiplier/divider, traps and clock counts.
  The expected values are worked out from the manuals, not copied from a
  simulator run.

### The clock model

- The operation-list clocks for the selected core (Software Manual Tables
  5-5 to 5-8).
- The "note 2" clocks when code flash is read as data, also through the
  mirror.
- Taken and not-taken clocks for conditional branches.
- A skipped instruction costs 1 clock (2 with an `ES:` prefix).
- One wait when an instruction addresses memory through a register, or uses
  SP or CS, that the previous instruction wrote (7.2.3). Unoptimized code
  hits this on almost every stack access; without it CoreMark would read
  several percent low.
- Instructions fetched from RAM are charged the manual's maximum (2n+3
  clocks, 4n+6 on S1) and counted separately.

### Where `rl78-elf-gdb` differs from the manual

rl78sim follows the manual. `--quirks gdb` reproduces items 1 to 4 so that a
lockstep comparison can be exact.

1. MACH sets CY whenever MACR changes sign; the manual says on signed
   overflow.
2. CMPS sets CY like CMP; the manual says when the result is not 0, or A is
   0, or X is 0.
3. MOVS always clears CY; the manual says CY is set when A is 0 or the
   source is 0.
4. PUSH PSW and POP PSW use the low stack byte for PSW; the manual says
   (SP-1) for PUSH and (SP+1) for POP, with 00H in the other byte.
5. AC comes from a generic before/after heuristic; the manual says the
   carry or borrow between bits 3 and 4.
6. MACR is not reset; the manual says 0000H.
7. Stores to the flash mirror go through to flash; on hardware the mirror is
   read-only.
8. A misaligned word access is a fatal trap; hardware ignores address bit 0.

## Embedding it

The library is what the command line is built on, and it can stand in for a
part: a debugger's tests can run a whole session against it.

- `MemMap` describes a device's layout (flash size, mirror, RAM start, end
  of the 2nd SFRs); `Memory::new(map)`, then `Cpu::new(mem, core)`.
- `Cpu::step` runs one instruction; `Cpu::run` and `run_until` run to a
  clock limit, an address or a stop.
- `Memory::poke` writes memory from outside: a debugger's memory writes,
  flash programming. The CPU drops instructions it decoded from those bytes
  before running the next one. After writing `Memory::b` directly, call
  `Memory::mark_written`.
- An `Io` hook (`Memory::set_io`) models peripherals: it sees every data
  read and store of the SFRs (except the CPU registers at FFFF8H-FFFFFH)
  and the 2nd SFRs, can supply the value read or take the store, gets the
  memory to change other state, and can end the run with
  `Memory::stop_for_io` (`Stop::Io`). `Memory::io_mut::<T>()` gives the
  hook back to the embedder.

`tests/embed.rs` shows each of these.

## Limits

- No interrupts.
- No peripherals other than the UART0 data register and the G13
  multiplier/divider; an embedder adds others with an `Io` hook.
- Clock counts are CPU clocks from the manual's tables. Anything outside
  the core (oscillator start-up, flash wait states at high frequency) is not
  modelled.

## Where the instruction table comes from

`isa/rl78.tsv` lists all 1,224 encodings with their S1, S2 and S3 clocks. It
is extracted from the Software Manual (R01US0015EJ0230 Rev.2.30) by
`tools/extract_manual.py` and `tools/join_isa.py`, which checks that every
encoding matches exactly one operation-list row of the same length.
`tools/check_encodings.py` assembles every row with `llvm-mc` and compares
the bytes; all 1,224 agree. Instruction semantics are from chapter 6 of the
Software Manual, the multiplier/divider from chapter 14 of the RL78/G13
hardware manual (R01UH0146). The manuals are Renesas documents and are not
part of this repository.

## Working on the simulator

The tests need `llvm-mc` and `ld.lld` from an LLVM with the RL78 target, for
example <https://github.com/AddisonH/rl78-llvm>. The two differential tools
also need `rl78-elf-gdb`.

```sh
RL78_LLVM_BIN=<llvm build>/bin cargo test --release   # unit and assembly tests
tools/check_encodings.py                              # instruction table against llvm-mc
tools/fuzz_isa.py --seeds 300 --length 600            # random programs against rl78-elf-gdb
tools/lockstep.py prog.elf                            # one program against rl78-elf-gdb
```

An assembly test is a `.s` file with its expectations in comments
(`EXPECT:`, `EXPECT-MEM:`, `STOP:`, `UART:`, and `CLOCKS:` between the
labels `t0` and `t1`); `tests/asm.rs` describes them.

The speed comes from a decode cache, a per-core table of data precomputed
for each encoding (`OpInfo`), specialised handlers for the frequent
encodings, and a single event flag for everything rare. `Cpu::step_impl` is
the place to start reading.

## Licence

Licensed under either of the [Apache License, Version 2.0](LICENSE-APACHE)
or the [MIT license](LICENSE-MIT), at your option. The RL78 manuals the
instruction table was extracted from are Renesas documents and are not
covered.
