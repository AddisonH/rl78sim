# rl78sim

An instruction-level simulator for the Renesas RL78 (S1/S2/S3 cores) with a
clock model taken from the RL78 Family Software Manual. It is meant for
benchmarking (exact instruction counts, per-PC profiles, real clock counts)
and for running compiler tests, in place of the simulator built into
`rl78-elf-gdb`. It is written in Rust.

The assembler and linker its tests use come from an LLVM with the RL78
target, for example <https://github.com/AddisonH/rl78-llvm>.

```sh
cargo build --release
target/release/rl78sim prog.elf                       # run; UART0 output on stdout, exit code = program's
target/release/rl78sim --start cm_region_start --stop cm_region_end \
    --stats - --profile prof.json prog.elf             # measure a region
target/release/rl78sim --trace prog.elf 2>trace.txt   # PC, disassembly, registers per instruction
```

Speed: ~110M instructions/s (a 258M-instruction CoreMark run in 2.3 s,
Embench at 90-117M/s) and ~2 ms startup, with every counter and the clock
model on. Where the time goes: the decode cache, a per-core
table of precomputed per-encoding data (`OpInfo`), specialised handlers for
the frequent encodings, and one event flag for everything rare (see
`Cpu::step_impl`).

## Where the facts come from

- `isa/rl78.tsv` — every encoding (1224) with its S1/S2/S3 clocks, extracted
  from the Software Manual (R01US0015EJ0230 Rev.2.30, Tables 5-5..5-8) by
  `tools/extract_manual.py` + `tools/join_isa.py` (which checks that every
  encoding matches exactly one operation-list row with the same length).
  `tools/check_encodings.py` assembles every row with llvm-mc and compares the
  bytes: 1224/1224 agree. The manuals are Renesas documents and are not part
  of this repository.
- Instruction semantics: chapter 6 of the Software Manual.
- Clock model (chapters 5 and 7): operation-list clocks for the selected
  core; "note 2" clocks when code flash is read as data (also through the
  mirror); not taken/taken for conditional branches; a skipped instruction
  costs 1 clock (2 with an ES: prefix); +1 wait when an instruction addresses
  memory through a register (or uses SP/CS) written by the previous
  instruction (7.2.3); instructions fetched from RAM are charged the manual's
  maximum (2n+3, S1 4n+6) and counted separately.
- G13 multiplier/divider (S2 objects with `EF_RL78_FU_EXIST`): R01UH0146
  chapter 14.
- Core selection: `--core`, else the ELF flags (`EF_RL78_CPU_*`,
  `EF_RL78_EI_EXIST`); instructions the core lacks (MULHU on S2, SEL on S1)
  stop the run as illegal.

## Accuracy

- **Hardware:** CC-RL's EEMBC CoreMark measures 577,093 clocks/iteration;
  Renesas publishes 582,411 cycles (−0.9%). The rl78-elf-gdb trace harness's
  floor said 539,735 (−7.3%).
- **Against rl78-elf-gdb** (`tools/lockstep.py`): register state compared
  after every instruction. A complete newlib program (111k instructions)
  and CoreMark agree; `tools/fuzz_isa.py` builds random programs from the
  whole encoding table (memory operands steered into scratch windows, memory
  read back at the end): 300/300 programs × ~1,900 instructions agree.
- **Against the manuals** (`tests/asm/*.s`, run by `cargo test`): flags,
  multiply/divide/MAC incl. overflow, shifts, stack frames, addressing modes,
  banks, the MDU (the G13 manual's own examples), traps, and clock counts.

### rl78-elf-gdb deviations from the manual (rl78sim follows the manual)

`--quirks gdb` reproduces 1-4 so lockstep comparison can be exact; AC is
masked instead (it is never read by compiled code).

1. MACH sets CY whenever MACR changes sign; the manual: signed overflow.
2. CMPS sets CY like CMP; the manual: result ≠ 0, or A = 0, or X = 0.
3. MOVS always clears CY; the manual: A = 0 or the source = 0.
4. PUSH PSW / POP PSW use the low stack byte for PSW; the manual: (SP-1) for
   PUSH, (SP+1) for POP, with 00H in the other byte.
5. AC is computed by a generic before/after heuristic (for CMP from the
   source operand); the manual: carry/borrow between bits 3 and 4.
6. MACR is not reset (the manual: 0000H).
7. Stores to the flash mirror go through to flash (hardware: read-only).
8. A misaligned word access is a fatal trap; hardware ignores address bit 0.

The register-write waits (7.2.3) matter: unoptimized code hits one on
almost every stack access, and without them the count for CC-RL's CoreMark
would be several percent below the published hardware figure instead of
0.9%.

## Memory maps

- `--map strict` (default): the rl78-elf-gdb layout (code flash to EFFFFH,
  mirror F1000H-F7FFFH ← 01000H, RAM from F8000H) with hardware semantics:
  stores to flash or the mirror and accesses to reserved space stop the run.
  A `.modelinfo` section (libgloss `*_modelinfo.ld`) sets the layout.
- `--map vendor`: stores go through to flash, 2nd-SFR reads return 11H, as
  rl78-elf-gdb does. libgloss's own
  `rl78-sim.ld` puts RAM at F5700H, inside the mirror, so `-fsim` programs
  need this mode.

Power-on contents match rl78-elf-gdb (FFH, 33H in F0000H-FFFFFH, 11H in the
register banks) so runs are reproducible; RAM is undefined on hardware.

I/O: UART0 SDR00 (FFF10H) → stdout (libgloss `_write`); HALT/STOP exit with
the code in A (libgloss `_exit`); a branch to itself exits with the code in
X (newlib's nosys `_exit`); BRK with an unprogrammed vector stops.
Not modelled: interrupts and peripherals other than the UART0 data register
and the G13 MDU.

## Tests

```sh
cargo test --release                  # unit + assembly tests; needs llvm-mc and ld.lld with
                                      # the RL78 target: RL78_LLVM_BIN=<llvm build>/bin
tools/check_encodings.py              # manual table vs llvm-mc
tools/fuzz_isa.py --seeds 300 --length 600   # differential fuzz vs rl78-elf-gdb
tools/lockstep.py prog.elf            # one program vs rl78-elf-gdb
```

An assembly test is a `.s` file with expectations in comments (`EXPECT:`,
`EXPECT-MEM:`, `STOP:`, `CLOCKS:` between labels `t0`/`t1`, `UART:`);
see `tests/asm.rs`. Expected values are worked out from the manuals, not
copied from a simulator run.
