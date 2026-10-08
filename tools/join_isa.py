#!/usr/bin/env python3
"""Join isa/formats.tsv (encodings) with isa/s{1,2,3}.tsv (bytes, clocks,
flags) into isa/rl78.tsv: one row per encoding. Every encoding must match
exactly one S3 operation-list pattern, and the byte counts must agree; this
is the consistency check on the manual extraction.

  tools/join_isa.py isa/manual isa/rl78.tsv
"""
import re
import sys

R8 = "X|A|C|B|E|D|L|H"
SUBST = [
    (r"\bRBn\b", r"RB[0-3]"),
    (r"\brp\b", r"(?:AX|BC|DE|HL)"),
    (r"\br\b", rf"(?:{R8})"),
    (r"\.bit\b", r".[0-7]"),
    (r"\bcnt\b", r"(?:[1-9]|1[0-5])"),
    (r"\[addr5\]", r"\[00[89AB][0-9A-F]h\]"),
]


def pattern(p):
    if p == "-":
        p = ""
    keys = []
    for i, (a, b) in enumerate(SUBST):
        p = re.sub(a, lambda _m, i=i: f"\x00{i}\x00", p)
    rx = re.escape(p)
    for i, (a, b) in enumerate(SUBST):
        rx = rx.replace(re.escape(f"\x00{i}\x00"), b).replace(
            f"\x00{i}\x00", b)
    return re.compile("^" + rx + "$")


def load(path):
    rows = [l.rstrip("\n").split("\t") for l in open(path)]
    return rows[1:]


def main():
    d, dst = sys.argv[1], sys.argv[2]
    fmts = load(f"{d}/formats.tsv")
    ops = {c: [(r, pattern(r[1])) for r in load(f"{d}/{c}.tsv")]
           for c in ("s1", "s2", "s3")}
    out, bad = [], 0
    for mn, opnd, code in fmts:
        if mn == "PREFIX":   # the ES: byte (11H); the decoder handles it
            continue
        if re.fullmatch(r"(BC|DE|HL), SP", opnd) and code.endswith("adrl adrh"):
            # MOVW rp, SP is MOVW rp, !addr16 with SP's address (FFFF8H).
            code = code.replace("adrl adrh", "F8 FF")
        nbytes = len(code.split())
        found = {}
        for c in ops:
            hits = [r for r, rx in ops[c] if r[0] == mn and rx.match(opnd)]
            # "MOV A, r" excludes r = A; several patterns can match a
            # concrete operand, so prefer the one with the right length.
            hits = [r for r in hits if int(r[2]) == nbytes] or hits
            if len(hits) > 1:
                exact = [r for r in hits if r[1] == opnd]
                hits = exact or hits[:1]
            found[c] = hits[0] if hits else None
        s3 = found["s3"]
        # "XCH A, r" is listed once: 1 byte for X, 2 for the others.
        if s3 is None or (int(s3[2]) != nbytes and
                          (mn, s3[1]) != ("XCH", "A, r")):
            bad += 1
            print(f"unmatched: {mn} {opnd} [{code}] -> {s3}", file=sys.stderr)
            continue
        clk = lambda c: found[c][3] if found[c] else ""
        clk_flash = lambda c: found[c][4] if found[c] else ""
        out.append((mn, opnd, code, s3[3], s3[4], clk("s2"), clk("s1"),
                    clk_flash("s2"), clk_flash("s1")))
    with open(dst, "w") as f:
        # Flags are not carried over: the operation lists' flag columns do not
        # survive pdftotext reliably. Flag behaviour comes from chapter 6.
        f.write("mnemonic\toperands\topcode\tclocks\tclocks_flash"
                "\tclocks_s2\tclocks_s1\tclocks_flash_s2\tclocks_flash_s1\n")
        for r in out:
            f.write("\t".join(r) + "\n")
    # Every S3 pattern should be used.
    used = {(r[0], r[1]) for r in out}
    for r, rx in ops["s3"]:
        if not any(m == r[0] and rx.match(o) for m, o, *_ in out):
            print(f"unused S3 row: {r}", file=sys.stderr)
            bad += 1
    print(f"{len(out)} encodings, {bad} problems", file=sys.stderr)
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
