#!/usr/bin/env python3
"""Genererar exempelvideorna i examples/media/.

Alla videor är procedurgenererade (ingen extern media, inga licensfrågor)
och loopar sömlöst: animationen är periodisk eller tonas över i sig själv.

Kräver numpy och ffmpeg (med libx264).

    python3 examples/media/generate.py            # alla
    python3 examples/media/generate.py fire neon  # bara några
"""

import os
import subprocess
import sys
from pathlib import Path

import numpy as np

W, H = 1280, 720
FPS = 30
# PREVIEW=1 gör korta versioner i en annan mapp, för att snabbt se hur de ser ut.
SECONDS = 2 if os.environ.get("PREVIEW") else 12
N = FPS * SECONDS
OUT = Path(os.environ["PREVIEW"]) if os.environ.get("PREVIEW") else Path(__file__).resolve().parent
TAU = 2 * np.pi


# ---------- Hjälpfunktioner ----------

def encode(name, frames, size=(W, H), scale_to=None, crf=24):
    """Skickar bildrutor (H×W×3, float 0..1 eller uint8) till ffmpeg."""
    w, h = size
    vf = ["format=yuv420p"]
    if scale_to:
        vf.insert(0, f"scale={scale_to[0]}:{scale_to[1]}:flags=bicubic")
    cmd = [
        "ffmpeg", "-loglevel", "error", "-y",
        "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", f"{w}x{h}", "-r", str(FPS), "-i", "-",
        "-vf", ",".join(vf),
        "-c:v", "libx264", "-preset", "slow", "-crf", str(crf), "-tune", "film",
        "-movflags", "+faststart",
        str(OUT / f"{name}.mp4"),
    ]
    p = subprocess.Popen(cmd, stdin=subprocess.PIPE)
    for i, f in enumerate(frames):
        if f.dtype != np.uint8:
            f = (np.clip(f, 0, 1) * 255).astype(np.uint8)
        p.stdin.write(np.ascontiguousarray(f).tobytes())
        if i % FPS == 0:
            print(f"\r{name}: {i // FPS + 1}/{SECONDS} s", end="", flush=True)
    p.stdin.close()
    p.wait()
    size_mb = (OUT / f"{name}.mp4").stat().st_size / 1e6
    print(f"\r{name}: klar ({size_mb:.1f} MB)        ")


def box_blur(img, r):
    """Box-blur med radie r längs båda axlarna (kanterna förlängs)."""
    if r < 1:
        return img
    for axis in (0, 1):
        pad = [(0, 0)] * img.ndim
        pad[axis] = (r + 1, r)
        c = np.cumsum(np.pad(img, pad, mode="edge"), axis=axis)
        hi = np.take(c, np.arange(2 * r + 1, c.shape[axis]), axis=axis)
        lo = np.take(c, np.arange(0, c.shape[axis] - 2 * r - 1), axis=axis)
        img = (hi - lo) / (2 * r + 1)
    return img


def blur(img, r):
    """Ungefär gaussisk oskärpa: tre box-blur efter varandra."""
    for _ in range(3):
        img = box_blur(img, r)
    return img


def glow(img, strength=1.0, radius=6):
    """Mjukt sken runt ljusa delar, beräknat i låg upplösning för fart."""
    small = img[::4, ::4]
    g = blur(small, max(1, radius // 4))
    g = np.repeat(np.repeat(g, 4, axis=0), 4, axis=1)[: img.shape[0], : img.shape[1]]
    return img + strength * g


def periodic_noise(size, feature, rng):
    """Mjukt brus som går runt i kanterna (för sömlös panorering).
    `feature` är ungefärlig storlek på fläckarna i pixlar."""
    white = rng.standard_normal((size, size))
    fy = np.fft.fftfreq(size)[:, None]
    fx = np.fft.fftfreq(size)[None, :]
    sigma = feature / 2
    f = np.fft.ifft2(np.fft.fft2(white) * np.exp(-2 * (np.pi * sigma) ** 2 * (fx**2 + fy**2))).real
    return (f - f.min()) / (f.max() - f.min())


def palette(stops, x):
    """Färg från en gradient: stops = [(läge, (r, g, b)), ...], x i 0..1."""
    pos = np.array([s[0] for s in stops])
    cols = np.array([s[1] for s in stops], dtype=np.float32)
    out = np.empty(x.shape + (3,), np.float32)
    for c in range(3):
        out[..., c] = np.interp(x, pos, cols[:, c])
    return out


def crossfade_loop(render, n, fade):
    """Gör en icke-periodisk animation sömlös: de sista `fade` bildrutorna
    tonas in över de första."""
    frames = [render(i) for i in range(n + fade)]
    for i in range(fade):
        a = i / fade
        frames[i] = frames[i] * a + frames[n + i] * (1 - a)
    return frames[:n]


# ---------- Videorna ----------

def space():
    """Rymden: flygning genom ett stjärnfält med färgad nebulosa."""
    rng = np.random.default_rng(7)
    tile = 1024
    # Två lager moln: stora lila och mindre turkosa, med lite fint stoft.
    big = periodic_noise(tile, 160, rng)
    mid = periodic_noise(tile, 60, rng)
    dust = periodic_noise(tile, 8, rng)
    nebula = (
        palette([(0, (0.0, 0.0, 0.0)), (0.6, (0.01, 0.0, 0.03)), (0.78, (0.16, 0.03, 0.24)), (0.9, (0.45, 0.12, 0.42)), (1, (0.8, 0.45, 0.65))],
                big * (0.75 + 0.25 * dust))
        + palette([(0, (0.0, 0.0, 0.0)), (0.6, (0.0, 0.02, 0.05)), (0.85, (0.05, 0.35, 0.5)), (1, (0.3, 0.8, 0.9))],
                  mid * (0.8 + 0.2 * dust)) * 0.45
    )
    count = 3500
    dirs = rng.uniform(-1, 1, (count, 2)) * np.array([W / H, 1.0])
    z0 = rng.uniform(0, 1, count)
    tint = rng.choice([[1.0, 1.0, 1.0], [0.7, 0.82, 1.0], [1.0, 0.88, 0.7]], count)
    ys, xs = np.mgrid[0:H, 0:W]
    focal = H * 0.22

    def frame(i):
        t = i / N
        # Nebulosan glider runt i en cirkel – periodiskt.
        ox = int(tile / 2 + 220 * np.cos(TAU * t))
        oy = int(tile / 2 + 140 * np.sin(TAU * t))
        img = nebula[(ys + oy) % tile, (xs + ox) % tile]
        # Stjärnorna rör sig mot betraktaren; z går runt ett varv per loop.
        # Varje stjärna ritas som en kort svans bakåt i tiden (rörelseoskärpa).
        stars = np.zeros((H, W, 3), np.float32)
        for k in range(6):
            z = (z0 - t + k * 0.004) % 1.0 + 0.015
            px = (W / 2 + dirs[:, 0] * focal / z).astype(int)
            py = (H / 2 + dirs[:, 1] * focal / z).astype(int)
            b = np.clip((1.015 - z) ** 3 * 2.2, 0, 2.0) * (1 - k / 6)
            ok = (px >= 0) & (px < W - 1) & (py >= 0) & (py < H - 1)
            for dy, dx in ((0, 0), (0, 1), (1, 0), (1, 1)):
                near = ok & ((dy + dx == 0) | (z < 0.25))  # nära stjärnor är större
                np.add.at(stars, (py[near] + dy, px[near] + dx), tint[near] * b[near, None])
        return glow(img + stars, 1.0, 10)

    # Tusentals rörliga stjärnor är svåra att komprimera – lite hårdare här.
    encode("space", (frame(i) for i in range(N)), crf=28)


def fire():
    """Eld: klassisk "Doom-eld". Varje cell skickar sin värme ett steg uppåt,
    lite åt sidan och lite svalare – samma slumptal styr båda, vilket ger
    flamtungor i stället för jämn glöd."""
    rng = np.random.default_rng(3)
    fw, fh, levels = 320, 180, 48
    heat = np.zeros((fh, fw), np.int32)
    pal = palette([(0, (0, 0, 0)), (0.15, (0.12, 0.01, 0.0)), (0.35, (0.55, 0.06, 0.0)),
                   (0.6, (0.95, 0.3, 0.02)), (0.8, (1.0, 0.65, 0.1)), (0.93, (1.0, 0.9, 0.45)), (1, (1.0, 1.0, 0.85))],
                  np.linspace(0, 1, levels + 1))
    xs = np.arange(fw)[None, :].repeat(fh - 1, 0)
    rows = np.arange(fh - 1)[:, None].repeat(fw, 1)

    def step(t):
        nonlocal heat
        # Glöden i botten: mestadels full, med långsamt vandrande svagare partier.
        base = 0.75 + 0.25 * np.sin(np.arange(fw) / 23 + t * 2.1) * np.sin(np.arange(fw) / 9 - t * 3.3)
        heat[-1, :] = np.clip(base * levels + rng.integers(-4, 2, fw), 0, levels)
        # Sidled −1, 0 eller +1 (symmetriskt, ingen vind) och avsvalning 0 eller 1.
        shift = rng.integers(-1, 2, (fh - 1, fw))
        cool = rng.integers(0, 2, (fh - 1, fw))
        target = np.clip(xs + shift, 0, fw - 1)
        new = heat.copy()
        new[rows, target] = np.maximum(heat[1:, :] - cool, 0)
        heat = new

    for k in range(300):
        step(k / FPS)

    def render(i):
        step(i / FPS)
        step((i + 0.5) / FPS)
        return box_blur(pal[heat], 1)

    frames = crossfade_loop(render, N, FPS)
    encode("fire", frames, size=(fw, fh), scale_to=(W, H))


def plasma():
    """Plasma: mjuka, flytande färger. Alla rörelser är hela varv per loop."""
    ys, xs = np.mgrid[0:H, 0:W].astype(np.float32)
    x, y = xs / H, ys / H
    cx, cy = W / H / 2, 0.5

    def frame(i):
        a = TAU * i / N
        v = (np.sin(x * 6 + a)
             + np.sin(y * 7 - 2 * a)
             + np.sin((x + y) * 5 + 3 * a)
             + np.sin(np.hypot(x - cx - 0.4 * np.cos(a), y - cy - 0.3 * np.sin(2 * a)) * 12 - 2 * a))
        h = (v / 8 + 0.5 + i / N) % 1.0
        rgb = 0.5 + 0.5 * np.cos(TAU * (h[..., None] + np.array([0.0, 0.33, 0.67])))
        return rgb ** 1.2

    encode("plasma", (frame(i) for i in range(N)))


def aurora():
    """Norrsken: gröna och lila draperier som böljar över en stjärnhimmel."""
    rng = np.random.default_rng(11)
    ys, xs = np.mgrid[0:H, 0:W].astype(np.float32)
    sky = palette([(0, (0.0, 0.01, 0.04)), (0.7, (0.01, 0.03, 0.08)), (1, (0.02, 0.06, 0.1))], ys / H)
    stars = np.zeros((H, W), np.float32)
    n = 900
    stars[rng.integers(0, H, n), rng.integers(0, W, n)] = rng.uniform(0.2, 1, n)
    sky = sky + glow(stars[..., None], 0.6, 4) * 0.8
    # Vertikala strålar: periodiskt brus längs x som rullar ett helt varv per loop.
    rays = periodic_noise(1024, 5, rng)[0] ** 1.5
    rays = np.interp(np.arange(W * 2) * 1024 / (W * 2), np.arange(1024), rays)
    x = xs[0] / W
    # Fjäll längst ned, med lite ljus från norrskenet på snön.
    ridge = H * (0.80 - 0.07 * np.sin(x * 3.1 + 1.0) - 0.04 * np.sin(x * 7.3 + 2.0) - 0.015 * np.sin(x * 23 + 0.5))
    mountain = ys > ridge[None, :]
    snow = np.clip(1 - (ys - ridge[None, :]) / (H * 0.05), 0, 1) * mountain

    def frame(i):
        a = TAU * i / N
        img = sky.copy()
        for k, (hue_low, hue_high, base, amp, speed) in enumerate([
            ((0.1, 1.0, 0.45), (0.5, 0.2, 0.8), 0.42, 0.10, 1),
            ((0.15, 0.9, 0.55), (0.55, 0.25, 0.75), 0.30, 0.07, -2),
        ]):
            centre = (base + amp * np.sin(x * 5 + speed * a + k)
                      + 0.04 * np.sin(x * 13 - 2 * speed * a)) * H
            shift = int((i / N) * W * 2 * (1 if k == 0 else -1)) % (W * 2)
            r = np.roll(rays, shift)[:W] * 0.8 + 0.2
            d = (ys - centre[None, :]) / H
            # Skarp nederkant, lång svans uppåt.
            curtain = np.where(d > 0, np.exp(-(d / 0.04) ** 2), np.exp(-(d / 0.22) ** 2)) * r[None, :]
            colour = palette([(0, hue_high), (1, hue_low)], np.clip(1 + d / 0.3, 0, 1))
            img += curtain[..., None] * colour * (0.9 - 0.25 * k)
        lit = img[int(H * 0.45)].mean(axis=0)  # himlens färg strax ovanför fjällen
        img = np.where(mountain[..., None], 0.015 + snow[..., None] * lit * 0.8, img)
        return glow(img, 0.4, 12)

    encode("aurora", (frame(i) for i in range(N)))


def neon():
    """Neon: synthwave-golv med rutnät som rör sig mot horisonten under en randig sol."""
    ys, xs = np.mgrid[0:H, 0:W].astype(np.float32)
    horizon = H * 0.58
    sky = palette([(0, (0.02, 0.0, 0.08)), (0.7, (0.25, 0.02, 0.3)), (1, (0.9, 0.2, 0.5))], ys / horizon)
    # Sol med ränder.
    sun_r = H * 0.26
    sd = np.hypot(xs - W / 2, ys - (horizon - sun_r * 0.35))
    sun_col = palette([(0, (1.0, 0.85, 0.2)), (1, (1.0, 0.2, 0.55))], np.clip((ys - (horizon - sun_r * 1.4)) / (sun_r * 1.4), 0, 1))
    stripes = ((ys - horizon) % 26) > np.clip((horizon - ys) / sun_r * 26, 0, 26)
    sun = (sd < sun_r)[..., None] * sun_col * np.where(ys < horizon - sun_r * 0.55, 1.0, stripes)[..., None]
    background = np.where((ys < horizon)[..., None], sky + sun, 0.0)
    below = ys > horizon
    # Golvets koordinater: djup ur skärmens y, sidled ur x.
    depth = np.where(below, H * 0.35 / np.maximum(ys - horizon, 1e-3), 0)
    lateral = (xs - W / 2) / W * depth * 4

    def frame(i):
        z = depth + 2.0 * i / N  # rutnätet flyttar exakt två rutor per loop
        lw = 0.035 + 0.02 * depth  # linjerna blir tunnare långt bort i bild
        lines_z = np.abs(((z + 0.5) % 1.0) - 0.5) < lw * 0.6
        lines_x = np.abs(((lateral + 0.5) % 1.0) - 0.5) < lw * 0.5
        fade = np.clip(1.2 - depth / 12, 0, 1)
        grid = ((lines_z | lines_x) & below) * fade
        img = background + grid[..., None] * np.array([1.0, 0.2, 0.9]) * 1.2
        # Horisontlinjen lyser.
        img += np.exp(-((ys - horizon) / 4) ** 2)[..., None] * np.array([1.0, 0.4, 0.9])
        return glow(img, 1.1, 10)

    encode("neon", (frame(i) for i in range(N)))


VIDEOS = {"space": space, "fire": fire, "plasma": plasma, "aurora": aurora, "neon": neon}

if __name__ == "__main__":
    names = sys.argv[1:] or list(VIDEOS)
    for n in names:
        if n not in VIDEOS:
            sys.exit(f"Okänd video: {n}. Finns: {', '.join(VIDEOS)}")
    for n in names:
        VIDEOS[n]()
