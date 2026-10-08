#!/usr/bin/env python3
"""Cross-check isa/rl78.tsv (from the manual) against an RL78 assembler.

Every row is instantiated with fixed operand values, assembled with
`llvm-mc -triple=rl78 -show-encoding`, and the bytes compared with the
table's opcode pattern. Fixup bytes ('A' in llvm-mc's output: branch
displacements) match anything.

  tools/check_encodings.py [--llvm-mc PATH] [isa/rl78.tsv]

Exit status 1 on any mismatch or assembler error.
"""
import argparse
import os
import re
import subprocess
import sys

DEFAULT_MC = os.path.join(os.environ["RL78_LLVM_BIN"], "llvm-mc") \
    if "RL78_LLVM_BIN" in os.environ else "llvm-mc"

# operand placeholder -> (assembler text, field values)
SADDR, SFR = 0xFFE80, 0xFFF30


def instantiate(mn, opnd, code):
    """Return (asm line, expected byte list with None for wildcards)."""
    f = {}
    rel = "$" in opnd
    text = opnd

    def sub(pat, rep):
        nonlocal text
        text = re.sub(pat, rep, text)

    if "!!addr20" in text:
        sub(r"!!addr20", "!!0x12345")
        f.update(adrl=0x45, adrh=0x23, adrs=0x01)
    if "$!addr20" in text:
        sub(r"\$!addr20", "$!.Lt")
        f.update(adrl=None, adrh=None)
    if "$addr20" in text:
        sub(r"\$addr20", "$.Lt")
        f.update(adr=None)
    if "!addr16" in text:
        sub(r"!addr16", "!0x1234")
        f.update(adrl=0x34, adrh=0x12)
    if re.search(r"\bword\[", text):
        sub(r"\bword\[", "0x1234[")
        f.update(adrl=0x34, adrh=0x12)
    if "#word" in text:
        sub(r"#word", "#0x3456")
        f.update(datal=0x56, datah=0x34)
    if "#byte" in text:
        sub(r"#byte", "#0x12")
        f.update(data=0x12)
    if re.search(r"\+byte\]", text):
        sub(r"\+byte\]", "+0x5]")
        if not rel:
            f.update(adr=0x05)
    sub(r"\bsaddrp?\b", hex(SADDR))
    f.setdefault("saddr", SADDR & 0xFF)
    sub(r"\bsfrp?\b", hex(SFR))
    f.setdefault("sfr", SFR & 0xFF)
    m = re.match(r"\[00([89AB][0-9A-F])h\]", text)
    if m:
        text = f"[0x00{m.group(1)}]"
    asm = f"{mn.lower()} {text.lower()}".strip()
    asm = asm.replace("0x", "0x")
    exp = []
    for tok in code.split():
        if re.fullmatch(r"[0-9A-F]{2}", tok):
            exp.append(int(tok, 16))
        else:
            exp.append(f[tok] if tok in f else "?" + tok)
    return asm, exp


def assemble(mc, lines):
    src = "\n".join(lines) + "\n"
    p = subprocess.run([mc, "-triple=rl78", "-show-encoding"], input=src,
                       capture_output=True, text=True)
    return p.stdout, p.stderr


def parse_enc(out):
    encs = []
    for l in out.splitlines():
        m = re.search(r"encoding: \[([^\]]*)\]", l)
        if m:
            encs.append([None if b.strip() == "A" else int(b, 16)
                         for b in m.group(1).split(",")])
    return encs


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("table", nargs="?", default=os.path.join(
        os.path.dirname(__file__), "..", "isa", "rl78.tsv"))
    ap.add_argument("--llvm-mc", default=DEFAULT_MC)
    ap.add_argument("-v", action="store_true")
    a = ap.parse_args()
    rows = [l.rstrip("\n").split("\t") for l in open(a.table)][1:]
    insts = [(r, *instantiate(r[0], r[1], r[2])) for r in rows]
    bad = 0
    for r, asm, exp in insts:
        if any(isinstance(e, str) for e in exp):
            print(f"UNFILLED {r[0]} {r[1]} [{r[2]}] -> {exp}")
            bad += 1
            continue
        out, err = assemble(a.llvm_mc, [asm, ".Lt: nop"])
        got = parse_enc(out)[:-1]
        if err.strip() or len(got) != 1:
            print(f"ASMERR  {r[0]} {r[1]} [{r[2]}]: {asm!r}: "
                  f"{err.strip().splitlines()[0] if err.strip() else out}")
            bad += 1
            continue
        g = got[0]
        ok = len(g) == len(exp) and all(
            e is None or x is None or e == x for e, x in zip(exp, g))
        if not ok:
            print(f"MISMATCH {r[0]} {r[1]} [{r[2]}]: {asm!r}: manual "
                  f"{' '.join('..' if e is None else f'{e:02X}' for e in exp)}"
                  f" llvm-mc {' '.join('..' if x is None else f'{x:02X}' for x in g)}")
            bad += 1
        elif a.v:
            print(f"ok {asm}")
    print(f"{len(insts)} encodings checked, {bad} problems", file=sys.stderr)
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
