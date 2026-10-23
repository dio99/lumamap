#!/usr/bin/env python3
"""Sätter ihop LuxCanvas reklamfilm i tre format (stående 9:16, kvadrat, 16:9)
av klippen i target/promo/. Klippen spelas in med

    cargo run --release -p lc-render --example record -- examples/christmas.lmap target/promo/c_christmas.mp4 4 --cue 4

(se SHOTS nedan för vilka), och editor-klippet c_editor_raw.mp4 med en
skärminspelning (ffmpeg x11grab). Kräver ffmpeg och Ubuntu-typsnittet."""
import subprocess, sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SRC = ROOT / "target/promo"
WORK = SRC / "work"
ICON = str(ROOT / "packaging/linux/io.github.dio99.LuxCanvas.png")
FB = "/usr/share/fonts/truetype/ubuntu/Ubuntu-B.ttf"
FM = "/usr/share/fonts/truetype/ubuntu/Ubuntu-M.ttf"
FL = "/usr/share/fonts/truetype/ubuntu/Ubuntu-L.ttf"
ACCENT = "0x00beff"

FORMATS = {"vertical": (1080, 1920), "square": (1080, 1080), "landscape": (1920, 1080)}

# (klipp, start, längd, fart, rubrik)
SHOTS = [
    ("c_editor_raw.mp4", 0.3, 8.0, 1.5, "Drag the corners. Done."),
    ("c_facade.mp4", 1.5, 3.4, 1.0, "Façade illusions"),
    ("c_christmas.mp4", 0.8, 2.6, 1.0, "Christmas shows"),
    ("c_objects.mp4", 0.5, 2.6, 1.0, "Balls, pillars and cones"),
    ("c_geometry.mp4", 0.5, 2.6, 1.0, "Neon geometry - no video needed"),
    ("c_party.mp4", 0.5, 2.6, 1.0, "Party mode"),
    ("c_showcase.mp4", 0.5, 2.4, 1.0, "Any surface"),
    ("c_skeleton.mp4", 0.5, 2.2, 1.0, "Halloween"),
    ("c_scare.mp4", 6.4, 3.0, 1.0, ""),
]


def run(cmd):
    r = subprocess.run(cmd, capture_output=True, text=True)
    if r.returncode:
        print(" ".join(cmd)); print(r.stderr[-3000:]); sys.exit(1)


def txt(name, text):
    p = WORK / f"{name}.txt"
    p.write_text(text)
    return str(p)


def draw(textfile, font, size, x, y, color="white", box=False):
    s = f"drawtext=fontfile={font}:textfile={textfile}:fontsize={size}:fontcolor={color}:x={x}:y={y}"
    if box:
        s += ":box=1:boxcolor=black@0.55:boxborderw=18"
    return s + ":shadowcolor=black@0.6:shadowx=2:shadowy=2"


def card(fmt, name, lines, secs):
    """Intro/slutbild: mörk bakgrund, ikon och textrader [(text, font, storlek, färg)]."""
    w, h = FORMATS[fmt]
    icon = max(140, w // 6)
    total = sum(int(sz * 1.45) for _, _, sz, _ in lines) + icon + 40
    y = (h - total) // 2
    filters = [f"[1:v]scale={icon}:{icon}[ic]", f"[0:v][ic]overlay=(W-w)/2:{y}[b0]"]
    y += icon + 40
    last = "b0"
    for i, (t, font, size, color) in enumerate(lines):
        f = txt(f"{fmt}_{name}_{i}", t)
        filters.append(f"[{last}]{draw(f, font, size, '(w-text_w)/2', y, color)}[b{i+1}]")
        last = f"b{i+1}"
        y += int(size * 1.45)
    filters.append(f"[{last}]fade=in:st=0:d=0.4,fade=out:st={secs-0.4}:d=0.4,format=yuv420p[out]")
    out = WORK / f"{fmt}_{name}.mp4"
    run(["ffmpeg", "-y", "-loglevel", "error", "-f", "lavfi", "-i", f"color=c=0x0b0f14:s={w}x{h}:d={secs}:r=30",
         "-i", ICON, "-filter_complex", ";".join(filters), "-map", "[out]", "-t", str(secs),
         "-c:v", "libx264", "-preset", "medium", "-crf", "18", "-r", "30", str(out)])
    return out


def shot(fmt, i, clip, start, length, speed, caption):
    w, h = FORMATS[fmt]
    dur = length / speed
    src = SRC / clip
    pts = f"setpts=(PTS-STARTPTS)/{speed}"
    if fmt == "landscape":
        chain = f"[0:v]trim={start}:{start+length},{pts},scale={w}:{h}[v]"
        cap_y, cap_size = f"h-text_h-80", 54
        last = "v"
        graph = [chain]
    else:
        # Stående: klippet zoomas in lite (sidorna beskärs) så att det syns bättre i mobilen.
        cw, ch = w, (800 if fmt == "vertical" else 608)
        sw = max(cw, (int(ch * 16 / 9) + 1) // 2 * 2)
        bgw = int(h * 16 / 9)
        graph = [
            f"[0:v]trim={start}:{start+length},{pts},split[a][b]",
            f"[a]scale={bgw}:{h},crop={w}:{h},boxblur=30:3,eq=brightness=-0.25[bg]",
            f"[b]scale={sw}:{ch},crop={cw}:{ch}[fg]",
            f"[bg][fg]overlay=0:(H-h)/2[v]",
        ]
        last = "v"
        cap_y = f"(h-{ch})/2-text_h-60"
        cap_size = 64 if fmt == "vertical" else 52
        # Varumärket under klippet.
        b1 = txt(f"{fmt}_brand", "LuxCanvas")
        b2 = txt(f"{fmt}_brand2", "projection mapping for Linux")
        graph.append(f"[{last}]{draw(b1, FB, 56 if fmt == 'vertical' else 44, '(w-text_w)/2', f'(h+{ch})/2+60', ACCENT)},"
                     f"{draw(b2, FL, 34 if fmt == 'vertical' else 28, '(w-text_w)/2', f'(h+{ch})/2+60+90', 'white')}[vb]")
        last = "vb"
    if caption:
        c = txt(f"{fmt}_cap{i}", caption)
        x = "(w-text_w)/2" if fmt != "landscape" else "80"
        graph.append(f"[{last}]{draw(c, FM, cap_size, x, cap_y, 'white', box=(fmt == 'landscape'))}[vc]")
        last = "vc"
    graph.append(f"[{last}]fade=in:st=0:d=0.15,fade=out:st={dur-0.15}:d=0.15,format=yuv420p[out]")
    out = WORK / f"{fmt}_shot{i}.mp4"
    run(["ffmpeg", "-y", "-loglevel", "error", "-i", str(src), "-filter_complex", ";".join(graph),
         "-map", "[out]", "-r", "30", "-c:v", "libx264", "-preset", "medium", "-crf", "18", str(out)])
    return out


def build(fmt):
    w, h = FORMATS[fmt]
    big = {"vertical": 150, "square": 112, "landscape": 120}[fmt]
    parts = [card(fmt, "intro", [
        ("LuxCanvas", FB, big, ACCENT),
        ("Projection mapping for Linux", FM, big // 2.6, "white"),
        ("Free and open source", FL, big // 3.2, "0x8b96a3"),
    ], 3.0)]
    for i, s in enumerate(SHOTS):
        parts.append(shot(fmt, i, *s))
    parts.append(card(fmt, "outro", [
        ("LuxCanvas", FB, big, ACCENT),
        ("Free download for Linux", FM, big // 2.6, "white"),
        ("github.com/dio99/luxcanvas", FM, big // 3.0, "white"),
        ("AppImage · deb · rpm · Flatpak", FL, big // 3.6, "0x8b96a3"),
    ], 4.0))
    lst = WORK / f"{fmt}_list.txt"
    lst.write_text("".join(f"file '{p}'\n" for p in parts))
    out = SRC / f"LuxCanvas-promo-{fmt}-{w}x{h}.mp4"
    run(["ffmpeg", "-y", "-loglevel", "error", "-f", "concat", "-safe", "0", "-i", str(lst),
         "-c:v", "libx264", "-preset", "slow", "-crf", "20", "-pix_fmt", "yuv420p", "-movflags", "+faststart", str(out)])
    print(out)


WORK.mkdir(parents=True, exist_ok=True)
for fmt in (sys.argv[1:] or FORMATS):
    build(fmt)
