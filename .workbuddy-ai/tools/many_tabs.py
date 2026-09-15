"""Add terminal tabs one at a time until the strip overflows.

Drives the real window and reports, after each addition, where the "+" button
and the right-hand icon cluster actually are. Used to check two things at once:

  * whether the "+" stays reachable once the tab strip starts scrolling, and
  * whether the right-hand controls keep their position (they must, since the
    strip's width is bounded -- if they move, the bound is wrong).

Usage:
    many_tabs.py <n>
"""

import re
import subprocess
import sys

PY = r"C:/Users/Zaida/AppData/Local/Programs/Python/Python312/python.exe"
TOOLS = r"C:/My work folder/Snor/.workbuddy-ai/tools"
SHOT = "../screenshots/many_tabs.png"
WINDOW_PT = 1536.0

n = int(sys.argv[1]) if len(sys.argv) > 1 else 8


def run(*args):
    r = subprocess.run([PY, *args], cwd=TOOLS, capture_output=True, text=True)
    return r.stdout


def measure():
    """Return (groups, plus_x) for the current strip.

    Identifying the "+" is the fiddly part: the close crosses, the "+", the
    active dot and the digits in "powershell 2" are all narrow. They are told
    apart by width *and* by what follows:

      "+"  9.6pt  -> the status hint: the 7.2pt dot, or "click" at 24pt
      "x"  8.0pt  -> the next tab's 13.6pt ">_" glyph
      "2"  7.2pt  -> a close cross (8.0pt)
      dot  7.2pt  -> nothing until the chevron ~780pt away

    So: width >= 8.5, a following group within 45pt, and that group either
    <= 7.5pt (the dot) or >= 18pt (a word).
    """
    out = run("tab_bbox.py", SHOT, "340", "623")
    groups = []
    for line in out.splitlines():
        m = re.match(
            r"\s+x\s+(\d+)\.\.(\d+)\s+phys\s+w\s+([\d.]+)pt\s+centre \(\s*([\d.]+),\s*([\d.]+)\)pt",
            line,
        )
        if m:
            groups.append((float(m.group(4)), float(m.group(3))))
    plus_x = None
    for i in range(len(groups) - 1):
        cx, w = groups[i]
        ncx, nw = groups[i + 1]
        if 8.5 <= w <= 11.0 and (ncx - cx) < 45.0 and (nw <= 7.5 or nw >= 18.0):
            plus_x = cx
            break
    return groups, plus_x


def click(x, y):
    run("snor_ui_probe.py", "click", str(int(x)), str(int(y)))


run("snor_ui_probe.py", "shot", SHOT)
for i in range(n):
    groups, plus_x = measure()
    right = max((cx for cx, _ in groups), default=0.0)
    if plus_x is None:
        print(f"add {i}: could not locate '+' -- stopping")
        break
    print(f"add {i}: '+' at {plus_x:7.1f}pt   rightmost ink {right:7.1f}pt   groups {len(groups)}")
    click(plus_x, 498)
    import time
    time.sleep(2.2)
    run("snor_ui_probe.py", "shot", SHOT)

groups, plus_x = measure()
right = max((cx for cx, _ in groups), default=0.0)
print(f"final:  '+' at {plus_x if plus_x is None else round(plus_x, 1)}pt   "
      f"rightmost ink {right:.1f}pt   groups {len(groups)}   window {WINDOW_PT}pt")
