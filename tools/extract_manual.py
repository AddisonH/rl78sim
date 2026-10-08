#!/usr/bin/env python3
"""Extract the RL78 instruction tables from the RL78 Family Software Manual
(R01US0015EJ0230, Rev.2.30) into isa/*.tsv.

  pdftotext -layout r01us0015ej0230-rl78.pdf manual.txt
  tools/extract_manual.py manual.txt isa/manual
  tools/join_isa.py isa/manual isa/rl78.tsv

Outputs
  formats.tsv  Table 5-8 (instruction formats): mnemonic, operands, opcode
               bytes. One row per encoding (the manual lists every register,
               bit number and CALLT slot separately).
  s1.tsv, s2.tsv, s3.tsv
               Tables 5-5/5-6/5-7 (operation lists): mnemonic, operand
               pattern, bytes, clocks (note 1 = RAM/SFR/no data access),
               clocks (note 2 = code flash data access), flags Z AC CY.
               Branch clocks are "not taken/taken".

The manual is not redistributed; the TSVs are the facts the simulator needs.
"""
import re
import sys

BYTE = re.compile(r"^[0-9A-F]{2}$")
FIELD = {"data", "datal", "datah", "adr", "adrl", "adrh", "adrs", "saddr",
         "sfr"}
# pdftotext maps the manual's Symbol-font glyphs to the private use area.
GLYPHS = str.maketrans({"": "-", "": "<-", "": "->",
                        "": "<->", "": "*", "": "&",
                        "": "|", "": "+", "": " ",
                        "": "*", "": "*", "−": "-",
                        "–": "-", "↑": "^"})


def clean(line, table_dash=False):
    """table_dash: a lone '-' is an empty opcode column (Table 5-8)."""
    line = line.replace("<R>", "   ").translate(GLYPHS)
    if table_dash:
        line = re.sub(r"(?<=\s)-(?=\s|$)", " ", line)
    line = re.sub(r"\s*Note\s*\d*", lambda m: " " * max(2, len(m.group())),
                  line)
    return line.rstrip()


def cells(line):
    return [c for c in re.split(r"\s{2,}", line.strip()) if c]


def pages(lines, title):
    """Yield the lines of every page of the table whose caption starts with
    `title`, without the page headers and footers."""
    on = False
    for l in lines:
        if title in l:
            on = True
            continue
        if not on:
            continue
        if re.match(r"\s*(R01US0015EJ0230|Apr 22, 2022|RL78 Family)", l):
            on = False
            continue
        yield l


def formats(lines):
    rows = []
    mnem = None
    first = None    # column of the "1st" opcode byte on this page
    for raw in pages(lines, "Table 5-8. List of Instruction Formats"):
        l = clean(raw, table_dash=True)
        if re.match(r"\s*1st\s+2nd", l):
            first = l.index("1st")
            continue
        if not l.strip() or "Mnemonic" in l or first is None:
            continue
        if re.match(r"\s*(Notes?|Remark|Caution|\d\.)\s", l):
            continue
        # Opcode cells start under "1st" (an operand such as "BC" or "saddr"
        # would also pass for an opcode cell).
        cut = min(first - 4, len(l))
        while 0 < cut < len(l) and l[cut - 1] != " ":
            cut -= 1
        c = cells(l[:cut])
        ops = l[cut:].split()
        if not all(BYTE.match(o) or o in FIELD for o in ops):
            if not re.search(r"[a-z]{4}", l):   # prose notes are expected
                print(f"bad opcode cells: {l!r}", file=sys.stderr)
            continue
        reg = r"(A|X|B|C|D|E|H|L|AX|BC|DE|HL|CY|PSW|SP|RB\d)"
        if not ops:
            # A mnemonic on a line of its own ("SEL" above "RB0 ... 61 CF").
            if len(c) == 1 and re.fullmatch(r"[A-Z][A-Z0-9]*", c[0]) and \
                    not re.fullmatch(reg, c[0]):
                mnem = c[0]
            continue
        if len(c) >= 2 and re.fullmatch(r"[A-Z][A-Z0-9]*", c[0]):
            mnem, opnd = c[0], " ".join(c[1:])
        elif len(c) == 1 and re.fullmatch(r"[A-Z][A-Z0-9]*", c[0]) and \
                not re.fullmatch(reg, c[0]):
            mnem, opnd = c[0], ""
        else:
            opnd = " ".join(c)
        if mnem is None:
            continue
        opnd = re.sub(r"\s*,\s*", ", ", opnd).replace("ES: ", "ES:").strip()
        if opnd == "operands":
            continue
        rows.append((mnem, opnd, " ".join(ops)))
    return rows


def oplist(lines, title, mnemonics):
    """Column-sliced parse of an operation-list table. Each page's header row
    gives the column positions; rows may wrap (operands above or below the
    bytes/clocks line, flags on a continuation line)."""
    rows = []
    mnem = None
    cols = None
    pending = None      # operand text on a line of its own, before its row
    cont = False        # last row's operands end in ',' and continue below
    for raw in pages(lines, title):
        l = clean(raw)
        if "Mnemonic" in l and "Operands" in l:
            cols = dict(mn=l.index("Mnemonic"), op=l.index("Operands"),
                        by=l.index("Bytes"))
            continue
        if cols is None or not l.strip():
            continue
        fh = re.search(r"\b(Z)\s+(AC)\s+(CY)\s*$", l)
        if fh:
            # flags header: remember where the flag columns are
            cols["fl"] = [fh.start(1), fh.start(2) + 1, fh.start(3) + 1]
            continue
        if re.match(r"\s*(Notes?|Remark|Caution|\d\.|Group)\s", l) or \
                re.match(r"\s*Note 1\s+Note 2", raw):
            continue
        # The mnemonic sits under the "Mnemonic" header; group words
        # ("Unconditional", "8-bit data") sit left of it.
        lim = cols["mn"] + 10
        found = [mm for mm in re.finditer(r"[A-Z][A-Z0-9]+", l)
                 if mm.start() <= lim and mm.group() in mnemonics and
                 mm.start() >= cols["mn"] - 4]
        start = 0
        if found:
            mnem = found[-1].group()
            start = found[-1].end()
        else:
            start = min(len(l), lim + 1)
            while start > 0 and l[start - 1] != " ":
                start -= 1
        rest = l[start:]
        # The byte count sits under "Bytes" ("ROR A, 1  2  1": the operand
        # "1" is not it).
        m, opnd_text = None, ""
        for bm in re.finditer(r"(?<=\s)[1-6](?=\s)", l + " "):
            if cols["by"] - 3 <= bm.start() <= cols["by"] + 8 and \
                    bm.start() >= start:
                # "XCH A, r": "1 (r = X)" / "2 (other than r = X)"
                m = re.match(r"^(?P<bytes>[1-6])(?:\s+\([^)]*\)?)?\s+"
                             r"(?P<clk>\d+(?:/\d+)?)(?:\s+(?P<clk2>\d+(?:/\d+)?))?"
                             r"(?:\s{2,}(?P<rest>.*))?$", l[bm.start():])
                opnd_text = l[start:bm.start()]
                break
        # Flag marks, each assigned to the nearest of the Z/AC/CY columns.
        flags = ""
        if "fl" in cols:
            fl = ["", "", ""]
            for fm in re.finditer(r"(?<!\S)([×R01])(?=\s|$)", l):
                d = [abs(fm.start() - c) for c in cols["fl"]]
                k = d.index(min(d))
                if min(d) <= 3:
                    fl[k] = fm.group(1).replace("×", "x")
            if any(fl):
                flags = fl
        if m and (opnd_text.strip() or pending is not None or found):
            opnd = opnd_text.strip()
            if re.match(r"^[A-Z][a-z]", opnd):   # wrapped group label
                opnd = ""
            if not opnd and pending is not None:
                opnd = pending
            pending = None
            if mnem is None:
                continue
            rows.append([mnem, opnd, m.group("bytes"), m.group("clk"),
                         m.group("clk2") or "", ",".join(flags or ["", "", ""])])
            cont = opnd.endswith(",")
            continue
        text = rest[:max(0, cols["by"] - start)].strip()
        # Flash clocks ("6/7") wrapped onto the line below their row.
        wc = re.search(r"(?<=\s)(\d+/\d+)(?=\s|$)", l[cols["by"] + 8:])
        if rows and wc and not rows[-1][4] and not m:
            rows[-1][4] = wc.group(1)
        if rows and flags and rows[-1][5] == ",,":
            rows[-1][5] = ",".join(flags)
        if not text or not re.match(r"^[A-Z$!\[#a-z]", text) or \
                re.match(r"^[A-Z][a-z]", text) or re.search(r"<-|\(|PC ", text):
            continue
        if cont:
            rows[-1][1] += " " + text
            cont = rows[-1][1].endswith(",")
        else:
            pending = text
    for r in rows:
        r[1] = re.sub(r"\s*,\s*", ", ", r[1]).replace("ES: ", "ES:").strip()
    return [tuple(r) for r in rows]


def main():
    src, out = sys.argv[1], sys.argv[2]
    lines = open(src, encoding="utf-8").read().split("\n")
    fmts = formats(lines)
    with open(f"{out}/formats.tsv", "w") as f:
        f.write("mnemonic\toperands\topcode\n")
        for r in fmts:
            f.write("\t".join(r) + "\n")
    mnemonics = {r[0] for r in fmts}
    for core, n in (("s1", "5-5"), ("s2", "5-6"), ("s3", "5-7")):
        with open(f"{out}/{core}.tsv", "w") as f:
            f.write("mnemonic\toperands\tbytes\tclocks\tclocks_flash\tflags\n")
            for r in oplist(lines, f"Table {n}. Operation List of RL78-", mnemonics):
                f.write("\t".join(r) + "\n")


if __name__ == "__main__":
    main()
