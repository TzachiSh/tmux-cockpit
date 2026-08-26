#!/usr/bin/env python3
"""ANSI (SGR only) -> HTML for a terminal screenshot. Handles 30-37/90-97, 40-47/100-107,
38;5;N / 48;5;N, 38;2;r;g;b / 48;2;r;g;b, 1 bold, 2 dim, 3 italic, 4 underline, 7 reverse, 0/22/23/24/27/39/49 resets."""
import html, re, sys

BASE = ["#45475a","#f38ba8","#a6e3a1","#f9e2af","#89b4fa","#f5c2e7","#94e2d5","#bac2de",
        "#585b70","#f38ba8","#a6e3a1","#f9e2af","#89b4fa","#f5c2e7","#94e2d5","#a6adc8"]
def c256(n):
    if n < 16: return BASE[n]
    if n < 232:
        n -= 16; r, g, b = n // 36, (n // 6) % 6, n % 6
        return "#%02x%02x%02x" % tuple(0 if v == 0 else 55 + 40 * v for v in (r, g, b))
    v = 8 + 10 * (n - 232); return "#%02x%02x%02x" % (v, v, v)

title = sys.argv[1] if len(sys.argv) > 1 else "screen"
text = sys.stdin.read()
st = {"fg": None, "bg": None, "b": False, "d": False, "i": False, "u": False, "r": False}
out = []
def span(s):
    if not s: return ""
    fg, bg = st["fg"], st["bg"]
    if st["r"]: fg, bg = (bg or "#1e1e2e"), (fg or "#cdd6f4")
    css = []
    if fg: css.append("color:" + fg)
    if bg: css.append("background:" + bg)
    if st["b"]: css.append("font-weight:bold")
    if st["d"]: css.append("opacity:.6")
    if st["i"]: css.append("font-style:italic")
    if st["u"]: css.append("text-decoration:underline")
    return '<span style="%s">%s</span>' % (";".join(css), html.escape(s)) if css else html.escape(s)
pos = 0
for m in re.finditer(r"\x1b\[([0-9;]*)m|\x1b\[[0-9;?]*[A-Za-z]|\x1b\][^\x07]*\x07", text):
    out.append(span(text[pos:m.start()])); pos = m.end()
    if m.group(1) is None: continue
    p = [int(x) if x else 0 for x in m.group(1).split(";")] or [0]
    i = 0
    while i < len(p):
        n = p[i]
        if n == 0: st.update(fg=None, bg=None, b=False, d=False, i=False, u=False, r=False)
        elif n == 1: st["b"] = True
        elif n == 2: st["d"] = True
        elif n == 3: st["i"] = True
        elif n == 4: st["u"] = True
        elif n == 7: st["r"] = True
        elif n == 22: st["b"] = st["d"] = False
        elif n == 23: st["i"] = False
        elif n == 24: st["u"] = False
        elif n == 27: st["r"] = False
        elif n == 39: st["fg"] = None
        elif n == 49: st["bg"] = None
        elif 30 <= n <= 37: st["fg"] = BASE[n - 30]
        elif 90 <= n <= 97: st["fg"] = BASE[n - 90 + 8]
        elif 40 <= n <= 47: st["bg"] = BASE[n - 40]
        elif 100 <= n <= 107: st["bg"] = BASE[n - 100 + 8]
        elif n in (38, 48) and i + 1 < len(p):
            key = "fg" if n == 38 else "bg"
            if p[i + 1] == 5 and i + 2 < len(p): st[key] = c256(p[i + 2]); i += 2
            elif p[i + 1] == 2 and i + 4 < len(p): st[key] = "#%02x%02x%02x" % (p[i + 2], p[i + 3], p[i + 4]); i += 4
        i += 1
out.append(span(text[pos:]))
body = "".join(out)
print("""<!doctype html><meta charset="utf-8"><title>%s</title>
<style>body{margin:0;background:#11111b;padding:18px}
.t{background:#1e1e2e;color:#cdd6f4;font:15px/1.25 "JetBrainsMono Nerd Font","DejaVu Sans Mono","Noto Sans Mono",monospace;
padding:14px 16px;border-radius:8px;display:inline-block;white-space:pre;box-shadow:0 8px 30px #0008}
.h{color:#6c7086;font:13px "DejaVu Sans",sans-serif;margin:0 0 8px 2px}</style>
<div class="h">%s</div><div class="t">%s</div>""" % (html.escape(title), html.escape(title), body))
