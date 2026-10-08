#!/usr/bin/env python3
"""Run an RL78 ELF in rl78sim and in the rl78-elf-gdb simulator side by side
and compare the register state after every instruction.

  tools/lockstep.py ELF [--steps N] [--start ADDR|SYM] [--sim-args ...]

Compares PC, AX, BC, DE, HL, SP, PSW, ES and CS. PSW.AC is ignored unless
--compare-ac: rl78-elf-gdb derives AC from a before/after heuristic (and
for CMP from the source operand), not the manual's "borrow from bit 4 to bit
3"; rl78sim follows the manual, and AC is undefined after 16-bit
arithmetic anyway. Nothing a compiler emits reads AC. Reports the first
divergence with a few instructions of context and exits 1; exits 0 when N
instructions agree or both programs end together.
"""
import argparse
import os
import re
import subprocess
import sys

GDB = os.environ.get("RL78_GDB", os.path.expanduser(
    "~/.local/share/renesas/e2_studio/toolchains/llvm-17.0.1.202606-rl78-elf/"
    "llvm-for-renesas-rl78/bin/rl78-elf-gdb"))
SIM = os.environ.get("RL78SIM", os.path.join(
    os.path.dirname(os.path.abspath(__file__)), "..", "target", "release",
    "rl78sim"))
AC_DEFINED = {"add", "addc", "sub", "subc", "cmp", "cmp0", "cmps", "inc",
              "dec", "mach", "machu", "reti", "retb"}
FIELDS = ("pc", "ax", "bc", "de", "hl", "sp", "psw", "es", "cs")


def die_with_parent():
    import ctypes
    import signal
    ctypes.CDLL(None).prctl(1, signal.SIGKILL)
    if os.getppid() == 1:
        os._exit(1)


def gdb_states(elf, start, steps):
    script = f"""file {elf}
target sim
load
break *{start}
run
set $i = 0
while $i < {steps}
  printf "@@ %x %x %x %x %x %x %x %x %x\\n", $pc, $ax, $bc, $de, $hl, $sp, $psw.uint8, $es, $cs
  stepi
  set $i = $i + 1
end
"""
    import tempfile
    with tempfile.NamedTemporaryFile("w", suffix=".gdb") as f:
        f.write(script)
        f.flush()
        p = subprocess.run([GDB, "-nx", "-batch", "-q", "-x", f.name],
                           capture_output=True, text=True,
                           preexec_fn=die_with_parent, timeout=3600)
    out = re.sub(r"\x1b\[[0-9;]*m", "", p.stdout)
    states = []
    for l in out.splitlines():
        if l.startswith("@@ "):
            # gdb sign-extends int16/int8 registers
            v = [int(x, 16) & m for x, m in zip(l.split()[1:], (
                0xFFFFF, 0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF, 0xFF, 0xF, 0xF))]
            states.append(tuple(v))
    return states, out


TRACE = re.compile(r"^([0-9a-f]{5}): (.{28}) AX=(\w+) BC=(\w+) DE=(\w+) "
                   r"HL=(\w+) SP=(\w+) PSW=(\w+) ES=(\w+) CS=(\w+)")


def sim_states(elf, start, steps, extra):
    cmd = [SIM, "--trace", "--max-clocks", str(steps * 40 + 1000), *extra]
    if start is not None:
        cmd += ["--start", str(start)]
    p = subprocess.Popen(cmd + [elf], stdout=subprocess.DEVNULL,
                         stderr=subprocess.PIPE, text=True,
                         preexec_fn=die_with_parent)
    states, dis = [], []
    started = start is None
    for l in p.stderr:
        m = TRACE.match(l)
        if not m:
            continue
        pc = int(m.group(1), 16)
        if not started:
            if pc != start:
                continue
            started = True
        v = [pc] + [int(m.group(i), 16) for i in range(3, 11)]
        states.append(tuple(v))
        dis.append(m.group(2).strip())
        if len(states) > steps:
            break
    p.kill()
    p.wait()
    return states, dis


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("elf")
    ap.add_argument("--steps", type=int, default=100000)
    ap.add_argument("--start", default=None,
                    help="address to start comparing at (default: reset)")
    ap.add_argument("--compare-ac", action="store_true")
    ap.add_argument("--sim-args", nargs=argparse.REMAINDER, default=[])
    a = ap.parse_args()
    elf = os.path.abspath(a.elf)
    with open(elf, "rb") as f:
        f.seek(0)
    if a.start is None:
        # reset vector
        out = subprocess.run([SIM, "--max-clocks", "0", "--trace", elf],
                             capture_output=True, text=True).stderr
        m = TRACE.search(out) or re.search(r"^([0-9a-f]{5}):", out, re.M)
        start = int(m.group(1), 16)
    else:
        start = int(a.start, 0)
    g, gout = gdb_states(elf, start, a.steps)
    s, dis = sim_states(elf, start, a.steps, a.sim_args or ["--map", "vendor", "--quirks", "gdb"])
    # gdb's stepi executes a skipped instruction slot as part of the SKxx
    # step, while rl78sim traces the slot; drop rl78sim states whose PC gdb
    # never stopped at between the same neighbours.
    j = 0
    ac_undef = False
    for i in range(len(g)):
        if j >= len(s):
            break
        if g[i][0] != s[j][0] and j + 1 < len(s) and s[j + 1][0] == g[i][0] \
                and dis[j - 1].startswith("sk"):
            j += 1
        gi, sj = list(g[i]), list(s[j])
        # AC is undefined after ADDW/SUBW/CMPW until an instruction defines
        # it again.
        if j > 0:
            mn = dis[j - 1].split()[0]
            if mn in ("addw", "subw", "cmpw") and not dis[j - 1].startswith(
                    ("addw sp", "subw sp")):
                ac_undef = True
            elif mn in AC_DEFINED or re.match(r"(mov|pop) psw", dis[j - 1]):
                ac_undef = False
        if ac_undef or not a.compare_ac:
            gi[6] &= ~0x10
            sj[6] &= ~0x10
        if gi != sj:
            print(f"DIVERGENCE after {i} instructions")
            for k in range(max(0, j - 6), j):
                print(f"   {s[k][0]:05x}: {dis[k]}")
            print(f"   after {s[j-1][0]:05x} {dis[j-1] if j else ''}:")
            for name, x, y in zip(FIELDS, gi, sj):
                mark = "  <--" if x != y else ""
                print(f"     {name:>3} gdb={x:#x} rl78sim={y:#x}{mark}")
            sys.exit(1)
        j += 1
    print(f"{min(len(g), j)} instructions agree "
          f"(gdb {len(g)} states, rl78sim {len(s)})")


if __name__ == "__main__":
    main()
