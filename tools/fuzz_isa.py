#!/usr/bin/env python3
"""Differential instruction fuzzer: random RL78 programs, run in rl78sim and
in the rl78-elf-gdb simulator, compared after every instruction
(tools/lockstep.py).

Each program draws instructions from isa/rl78.tsv (every encoding the manual
lists), with operands steered into scratch memory: before an instruction
that addresses memory through registers, its base/index registers are loaded
with an address inside a RAM window. Memory results are made visible at the
end by reading both windows back through AX. Control transfer instructions
are limited to ones whose target is the next instruction (conditional
branches with a zero displacement, skips).

  tools/fuzz_isa.py [--seeds N] [--first S] [--length L] [--keep DIR]

Exit status 1 if any program diverges (the failing program is kept).
"""
import argparse
import os
import random
import re
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
TABLE = os.path.join(HERE, "..", "isa", "rl78.tsv")
# Directory with llvm-mc and ld.lld: RL78_LLVM_BIN, else wherever llvm-mc is
# on PATH.
LLVM = os.environ.get("RL78_LLVM_BIN") or os.path.dirname(
    shutil.which("llvm-mc") or "llvm-mc")

WIN = 0xFA000          # !addr16 / based / indirect window (256 bytes)
SADDR_LO, SADDR_HI = 0xFFE20, 0xFFEDF   # saddr window (below the banks)
STACK = 0xFDF00
SFRS = [0xFFF20, 0xFFF21, 0xFFF22, 0xFFF23, 0xFFF24, 0xFFF25,  # port modes
        0xFFFF0, 0xFFFF1, 0xFFFF2, 0xFFFF3]                    # MACR
SFR_WORDS = [0xFFF20, 0xFFF22, 0xFFF24, 0xFFFF0, 0xFFFF2]

# Not generated: control flow with real targets, CPU state changes that
# would leave the scratch windows, and HALT/STOP (end of test).
SKIP_MN = {"CALL", "CALLT", "BRK", "RET", "RETI", "RETB", "BR", "HALT",
           "STOP", "EI", "DI"}


def load_table():
    rows = [l.rstrip("\n").split("\t") for l in open(TABLE)][1:]
    out = []
    for mn, opnd, code, *_ in rows:
        if mn in SKIP_MN:
            continue
        if re.search(r"\bSP\b", opnd) and mn in ("MOVW", "ADDW", "SUBW"):
            continue  # moving SP outside the stack window
        if re.match(r"(ES|CS),", opnd) or opnd in ("ES", "CS"):
            continue  # ES/CS writes
        if mn == "XCH" and "ES" in opnd.split(", ")[0]:
            continue
        # Nothing that reads PSW.AC: rl78-elf-gdb computes AC differently
        # from the manual (see lockstep.py).
        if re.search(r"\bPSW\.4\b", opnd) and mn not in ("SET1", "CLR1"):
            continue
        if (mn, opnd) in (("MOV", "A, PSW"), ("PUSH", "PSW")):
            continue
        out.append((mn, opnd))
    return out


class Gen:
    def __init__(self, rng):
        self.r = rng
        self.lines = []

    def emit(self, s):
        self.lines.append("\t" + s)

    def win(self, align=1, span=0):
        a = WIN + self.r.randrange(0, 0x100 - span)
        return a & ~(align - 1)

    def setup_regs(self, opnd, word):
        """Point the registers an operand addresses through at the window.
        Returns replacement text for the operand."""
        al = 2 if word else 1
        r = self.r
        t = opnd
        if "[HL+B]" in t or "[HL+C]" in t:
            idx = "b" if "[HL+B]" in t else "c"
            self.emit(f"movw hl, #{self.win(al, 0x40) & 0xFFFF:#x}")
            self.emit(f"mov {idx}, #{r.randrange(0, 0x20) * al:#x}")
        if "word[BC]" in t:
            self.emit(f"movw bc, #{r.randrange(0, 0x40) * al:#x}")
            t = t.replace("word[BC]", f"{self.win(al, 0x80) & 0xFFFF:#x}[bc]")
        for reg in ("B", "C"):
            if f"word[{reg}]" in t:
                self.emit(f"mov {reg.lower()}, #{r.randrange(0, 0x40) * al:#x}")
                t = t.replace(f"word[{reg}]", f"{self.win(al, 0x80) & 0xFFFF:#x}[{reg.lower()}]")
        for rp in ("DE", "HL"):
            if f"[{rp}+byte]" in t:
                self.emit(f"movw {rp.lower()}, #{self.win(al, 0x80) & 0xFFFF:#x}")
                t = t.replace(f"[{rp}+byte]", f"[{rp.lower()}+{r.randrange(0, 0x40) * al:#x}]")
            if f"[{rp}]" in t:
                self.emit(f"movw {rp.lower()}, #{self.win(al) & 0xFFFF:#x}")
        if "[SP+byte]" in t:
            t = t.replace("[SP+byte]", f"[sp+{r.randrange(0, 0x40) * al:#x}]")
        return t

    def instance(self, mn, opnd, after_skip=False):
        r = self.r
        word = mn in ("MOVW", "XCHW", "ADDW", "SUBW", "CMPW", "ONEW", "CLRW",
                      "INCW", "DECW", "SHLW", "SHRW", "SARW", "ROLWC", "PUSH", "POP")
        t = self.setup_regs(opnd, word)
        if "ES:" in t and not after_skip:
            self.emit("mov es, #0xf")
        al = 2 if word else 1
        # Parenthesised: llvm-mc turns "!0x1234.3" into a symbol reference
        # and "!4660.3" into a float.
        t = t.replace("ES:!addr16", f"es:!({self.win(al) & 0xFFFF:#x})")
        t = t.replace("!addr16", f"!({self.win(al) & 0xFFFF:#x})")
        t = t.replace("#byte", f"#{r.randrange(256):#x}")
        t = t.replace("#word", f"#{r.randrange(65536):#x}")
        if "saddrp" in t:
            t = t.replace("saddrp", f"{r.randrange(SADDR_LO, SADDR_HI, 2):#x}")
        if "saddr" in t:
            t = t.replace("saddr", f"{r.randrange(SADDR_LO, SADDR_HI + 1):#x}")
        if "sfrp" in t:
            t = t.replace("sfrp", f"{r.choice(SFR_WORDS):#x}")
        if "sfr" in t:
            t = t.replace("sfr", f"{r.choice(SFRS):#x}")
        t = t.replace("$!addr20", "$!1f").replace("$addr20", "$1f")
        m = re.match(r"\[00([89AB][0-9A-F])h\]", t)
        if m:
            t = f"[0x00{m.group(1)}]"
        t = t.replace("ES:", "es:")
        asm = f"{mn.lower()} {t.lower()}".strip()
        # llvm-mc spells the 1f/1b numeric label references in lower case;
        # put the label after the instruction.
        self.emit(asm.replace("$!1f", "$!1f").replace("$1f", "$1f"))
        if "1f" in asm:
            self.lines.append("1:")

    def program(self, table, n):
        r = self.r
        L = self.lines
        L += [".section .vec,\"a\"", "\t.short _start",
              ".text", ".global _start", "_start:"]
        self.emit(f"movw sp, #{STACK & 0xFFFF:#x}")
        self.emit("mov es, #0xf")
        # Random initial contents for the windows (RAM is not loadable).
        for a in list(range(WIN, WIN + 0x100)) + list(range(STACK - 0x80, STACK + 0x40)):
            self.emit(f"mov !({a & 0xFFFF:#x}), #{r.randrange(256):#x}")
        # rl78-elf-gdb does not reset MACR (the manual: 0000H).
        self.emit("clrw ax")
        self.emit("movw 0xffff0, ax")
        self.emit("movw 0xffff2, ax")
        for a in range(SADDR_LO, SADDR_HI + 1):
            self.emit(f"mov {a:#x}, #{r.randrange(256):#x}")
        for rp in ("ax", "bc", "de", "hl"):
            self.emit(f"movw {rp}, #{r.randrange(65536):#x}")
        after_skip = False
        for _ in range(n):
            mn, opnd = r.choice(table)
            # The instruction after a skip must not need setup code, or the
            # skip would hit the setup instead.
            while after_skip and re.search(r"\[(HL|DE)|word\[|\[SP", opnd):
                mn, opnd = r.choice(table)
            if after_skip and "ES:" in opnd:
                pass  # "mov es, #0xf" is redundant (ES is always 0FH)
            self.instance(mn, opnd, after_skip)
            after_skip = mn.startswith("SK")
        # Read the windows back through AX so memory differences show up.
        self.emit("sel rb0")
        for a in range(WIN, WIN + 0x100, 2):
            self.emit(f"movw ax, !{a & 0xFFFF:#x}")
        for a in range(SADDR_LO, SADDR_HI + 1, 2):
            self.emit(f"movw ax, {a:#x}")
        for a in range(0xFFEE0, 0xFFF00, 2):
            self.emit(f"movw ax, {a:#x}")
        for a in SFR_WORDS:
            self.emit(f"movw ax, {a:#x}")
        for a in range(STACK - 0x80, STACK + 0x40, 2):
            self.emit(f"movw ax, !{a & 0xFFFF:#x}")
        self.emit("halt")
        return "\n".join(L) + "\n"


LDS = f"""
SECTIONS {{
  .vec 0 : {{ KEEP(*(.vec)) }}
  .text 0x200 : {{ *(.text) }}
}}
"""


def build(src, d):
    s, o, e, ld = (os.path.join(d, x) for x in ("t.s", "t.o", "t.elf", "t.ld"))
    open(s, "w").write(src)
    open(ld, "w").write(LDS)
    subprocess.run([f"{LLVM}/llvm-mc", "-triple=rl78", "-filetype=obj", s, "-o", o],
                   check=True, capture_output=True, text=True)
    subprocess.run([f"{LLVM}/ld.lld", "-T", ld, o, "-o", e],
                   check=True, capture_output=True, text=True)
    return e


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--seeds", type=int, default=20)
    ap.add_argument("--first", type=int, default=1)
    ap.add_argument("--length", type=int, default=400)
    ap.add_argument("--keep", default=None, help="directory for failing programs")
    a = ap.parse_args()
    table = load_table()
    bad = 0
    for seed in range(a.first, a.first + a.seeds):
        g = Gen(random.Random(seed))
        src = g.program(table, a.length)
        with tempfile.TemporaryDirectory() as d:
            try:
                elf = build(src, d)
            except subprocess.CalledProcessError as e:
                print(f"seed {seed}: build failed: {e.stderr.strip()[:300]}")
                bad += 1
                continue
            p = subprocess.run([sys.executable, os.path.join(HERE, "lockstep.py"),
                                elf, "--steps", "1000000", "--start", "0x200"],
                               capture_output=True, text=True)
            ok = p.returncode == 0
            print(f"seed {seed}: {'ok' if ok else 'DIVERGED'}: "
                  f"{p.stdout.strip().splitlines()[-1] if ok else ''}", flush=True)
            if not ok:
                bad += 1
                print(p.stdout + p.stderr)
                if a.keep:
                    os.makedirs(a.keep, exist_ok=True)
                    shutil.copy(elf, os.path.join(a.keep, f"seed{seed}.elf"))
                    open(os.path.join(a.keep, f"seed{seed}.s"), "w").write(src)
    print(f"{a.seeds - bad}/{a.seeds} programs agree")
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
