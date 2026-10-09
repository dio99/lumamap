#!/usr/bin/env python3
"""Genererar exempelvideorna i examples/media/.

Alla videor är procedurgenererade (ingen extern media, inga licensfrågor)
och loopar sömlöst: animationen är periodisk eller tonas över i sig själv.

Kräver numpy och ffmpeg (med libx264). Matrix kräver även Pillow.

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
    # Kontroll av loopen före kodning: steget sista → första bildrutan ska
    # vara som ett vanligt steg mellan två bildrutor.
    first = prev = None
    steps = []
    for i, f in enumerate(frames):
        if f.dtype != np.uint8:
            f = (np.clip(f, 0, 1) * 255).astype(np.uint8)
        small = f[::8, ::8].astype(np.float32)
        if prev is None:
            first = small
        else:
            steps.append(np.abs(small - prev).mean())
        prev = small
        p.stdin.write(np.ascontiguousarray(f).tobytes())
        if i % FPS == 0:
            print(f"\r{name}: {i // FPS + 1}/{SECONDS} s", end="", flush=True)
    p.stdin.close()
    p.wait()
    size_mb = (OUT / f"{name}.mp4").stat().st_size / 1e6
    seam, typical, worst = np.abs(first - prev).mean(), np.median(steps), max(steps)
    ok = "ok" if seam <= worst * 1.05 else "HOPP I LOOPEN"
    print(f"\r{name}: klar ({size_mb:.1f} MB), skarv {seam:.2f} (vanligt steg {typical:.2f}, max {worst:.2f}) {ok}")


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


def water():
    """Vattenkrusningar: ljusbrytningar (kaustik) på botten, med ringar där
    droppar slår ned. Kaustiken bygger på en känd shader-teknik där en punkt
    vrids om flera gånger; alla hastigheter är heltal så att den loopar."""
    rng = np.random.default_rng(5)
    ys, xs = np.mgrid[0:H, 0:W].astype(np.float32)
    px0 = xs / H * TAU * 0.9 - 250.0
    py0 = ys / H * TAU * 0.9 - 250.0
    # Droppar: läge och tidpunkt (andel av loopen). Upprepas varje loop.
    drops = [(rng.uniform(0.1, 0.9) * W, rng.uniform(0.15, 0.85) * H, k / 7 + rng.uniform(0, 0.08)) for k in range(7)]
    base = palette([(0, (0.0, 0.08, 0.16)), (1, (0.0, 0.22, 0.32))], ys / H)

    def frame(i):
        a = TAU * i / N
        ring = np.zeros((H, W), np.float32)
        for dx, dy, t0 in drops:
            age = ((i / N - t0) % 1.0) * SECONDS
            if age > 4.0:
                continue
            r = np.hypot(xs - dx, ys - dy) / H
            front = 0.05 + age * 0.22
            env = np.exp(-((r - front) / 0.05) ** 2) * np.exp(-age * 0.9) * (r < front + 0.1)
            ring += np.sin((r - front) * 90) * env
        px = px0 + ring * 0.25
        py = py0 + ring * 0.25
        ix, iy = px.copy(), py.copy()
        c = np.ones_like(px)
        inten = 0.005
        for n, speed in enumerate((1, -2, 1, 2, -1)):
            t = a * speed + n * 1.7
            ix, iy = px + np.cos(t - ix) + np.sin(t + iy), py + np.sin(t - iy) + np.cos(t + ix)
            c += 1.0 / np.hypot(px / (np.sin(ix + t) / inten), py / (np.cos(iy + t) / inten))
        c /= 5.0
        c = 1.17 - np.power(np.abs(c), 1.4)
        light = np.clip(np.abs(c) ** 8, 0, 1.5)
        img = base * (1 + 0.25 * ring[..., None]) + light[..., None] * np.array([0.55, 0.9, 1.0])
        return glow(img, 0.3, 6)

    encode("water", (frame(i) for i in range(N)), crf=26)


def rain():
    """Regn på ett fönster: suddiga stadsljus bakom glaset, droppar som
    bryter ljuset (visar bakgrunden upp och ned) och droppar som rinner ned."""
    rng = np.random.default_rng(9)
    ys, xs = np.mgrid[0:H, 0:W].astype(np.float32)

    def lights(seed_rng, count):
        img = palette([(0, (0.01, 0.02, 0.06)), (1, (0.04, 0.03, 0.08))], ys / H)
        cols = np.array([[1.0, 0.6, 0.2], [1.0, 0.25, 0.15], [0.9, 0.9, 1.0], [0.2, 0.8, 0.9], [1.0, 0.4, 0.7], [1.0, 0.85, 0.4]])
        for _ in range(count):
            cx, cy = seed_rng.uniform(0, W), seed_rng.uniform(H * 0.25, H * 1.05)
            r = seed_rng.uniform(18, 70)
            d = np.hypot(xs - cx, ys - cy)
            disc = np.clip((r - d) / 3, 0, 1) * (0.55 + 0.45 * np.clip(d / r, 0, 1))
            img += disc[..., None] * cols[seed_rng.integers(len(cols))] * seed_rng.uniform(0.15, 0.5)
        return img

    layer_a, layer_b = lights(rng, 45), lights(rng, 45)
    sharp_a, sharp_b = blur(layer_a, 2), blur(layer_b, 2)
    soft_a, soft_b = blur(layer_a, 9), blur(layer_b, 9)

    still = [(rng.uniform(0, W), rng.uniform(0, H), 2 + 10 * rng.uniform(0, 1) ** 3, rng.uniform(0, 1)) for _ in range(650)]  # mest små, några stora
    running = [(rng.uniform(30, W - 30), rng.uniform(0, 1), int(rng.integers(1, 4)), rng.uniform(9, 15), rng.uniform(0, TAU))
               for _ in range(18)]

    def draw_drop(img, sharp, cx, cy, r, stretch=1.0, alpha=1.0):
        x0, x1 = int(max(cx - r - 2, 0)), int(min(cx + r + 2, W))
        y0, y1 = int(max(cy - r * stretch - 2, 0)), int(min(cy + r * stretch + 2, H))
        if x0 >= x1 or y0 >= y1:
            return
        yy, xx = np.mgrid[y0:y1, x0:x1].astype(np.float32)
        dx, dy = (xx - cx) / r, (yy - cy) / (r * stretch)
        d2 = dx * dx + dy * dy
        inside = np.clip((1 - d2) * r / 1.5, 0, 1) * alpha
        # Droppen är en lins: bakgrunden syns förminskad och upp och ned.
        sx = np.clip(cx - dx * r * 4, 0, W - 1).astype(int)
        sy = np.clip(cy - dy * r * 4 * stretch, 0, H - 1).astype(int)
        rim = np.clip((d2 - 0.55) * 2.2, 0, 1)[..., None]  # mörk kant där ljuset bryts bort
        spot = np.clip(0.6 - ((dx + 0.3) ** 2 + (dy + 0.35) ** 2) * 9, 0, 1)[..., None]  # högdager
        lens = sharp[sy, sx] * (1.6 - 0.4 * d2[..., None]) * (1 - 0.85 * rim) + spot * 0.8
        region = img[y0:y1, x0:x1]
        img[y0:y1, x0:x1] = region * (1 - inside[..., None]) + lens * inside[..., None]

    def frame(i):
        t = i / N
        w = 0.5 + 0.5 * np.sin(TAU * t)
        soft = soft_a * w + soft_b * (1 - w)
        sharp = sharp_a * w + sharp_b * (1 - w)
        img = soft.copy()
        span = H + 200
        # Rinnande droppar lämnar en klar strimma i glaset ovanför sig.
        heads = []
        for x0, y0, laps, r, ph in running:
            cy = (y0 + laps * t) % 1.0 * span - 100
            cx = x0 + 5 * np.sin(TAU * t * 3 + ph) + 3 * np.sin(cy / 37)
            heads.append((cx, cy, r))
            top = int(max(cy - 260, 0)); bottom = int(min(cy, H))
            if bottom > top:
                yy = np.arange(top, bottom)
                fade = np.clip((yy - (cy - 260)) / 260, 0, 1)[:, None]
                xx = np.arange(int(max(cx - r * 0.5, 0)), int(min(cx + r * 0.5, W)))
                if len(xx):
                    img[top:bottom, xx[0]:xx[-1] + 1] = (img[top:bottom, xx[0]:xx[-1] + 1] * (1 - 0.7 * fade[..., None])
                                                         + sharp[top:bottom, xx[0]:xx[-1] + 1] * 0.7 * fade[..., None])
        for cx, cy, r, ph in still:
            life = (t + ph) % 1.0
            alpha = np.clip(min(life, 0.85 - life) * 12, 0, 1) if life < 0.85 else 0.0
            if alpha > 0:
                draw_drop(img, sharp, cx, cy, r, 1.0, alpha)
        for cx, cy, r in heads:
            draw_drop(img, sharp, cx, cy, r, 1.25)
        vignette = 1 - 0.35 * (((xs - W / 2) / W) ** 2 + ((ys - H / 2) / H) ** 2)[..., None] * 2
        return img * vignette

    encode("rain", (frame(i) for i in range(N)), crf=26)


def matrix():
    """Matrix: gröna tecken som rinner nedåt i kolumner. Katakana om
    typsnittet finns, annars siffror och bokstäver."""
    from PIL import Image, ImageDraw, ImageFont

    rng = np.random.default_rng(1)
    cell = 24
    cols, rows = W // cell, H // cell
    chars = [chr(c) for c in range(0x30A2, 0x30F3)] + list("0123456789Z:=*+<>")
    font = None
    for path in ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf"):
        try:
            font = ImageFont.truetype(path, cell - 2)
            break
        except OSError:
            pass
    if font is None:
        chars = list("0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ:.=*+-<>|")
        font = ImageFont.load_default(cell - 2)
    atlas = np.zeros((len(chars), cell, cell), np.float32)
    for k, ch in enumerate(chars):
        im = Image.new("L", (cell, cell))
        ImageDraw.Draw(im).text((cell / 2, cell / 2), ch, fill=255, font=font, anchor="mm")
        atlas[k] = np.asarray(im, np.float32)[:, ::-1] / 255  # spegelvänt, som i filmen
    glyphs = rng.integers(0, len(chars), (rows, cols))
    flicker = rng.random((rows, cols)) < 0.06
    # Två strömmar per kolumn; hastighet = antal varv per loop.
    trail = 22
    streams = [(c, rng.uniform(0, 1), int(rng.integers(1, 4))) for c in range(cols) for _ in range(2)]
    green = np.array([0.15, 1.0, 0.35])

    def frame(i):
        t = i / N
        inten = np.zeros((rows, cols), np.float32)
        head = np.zeros((rows, cols), np.float32)
        r = np.arange(rows)
        for c, off, laps in streams:
            pos = (off + laps * t) % 1.0 * (rows + trail) - 1
            d = pos - r
            on = (d >= 0) & (d < trail)
            inten[on, c] = np.maximum(inten[on, c], (1 - d[on] / trail) ** 1.6)
            h = (d >= 0) & (d < 1)
            head[h, c] = 1.0
        # Några tecken byter form, 60 gånger per loop.
        g = glyphs.copy()
        step = (i * 60) // N
        g[flicker] = (glyphs[flicker] * 7 + step * 13) % len(chars)
        tiles = atlas[g]  # rows × cols × cell × cell
        mask = tiles.transpose(0, 2, 1, 3).reshape(rows * cell, cols * cell)
        level = np.repeat(np.repeat(inten, cell, 0), cell, 1)
        hd = np.repeat(np.repeat(head, cell, 0), cell, 1)
        img = mask[..., None] * (level[..., None] * green + hd[..., None] * np.array([0.9, 1.0, 0.9]) * 1.4)
        img = np.pad(img, ((0, H - img.shape[0]), (0, W - img.shape[1]), (0, 0)))
        return glow(img, 0.8, 8)

    encode("matrix", (frame(i) for i in range(N)), crf=26)


def lightgrid():
    """Ljusnät för fasader: fönsterkarmar som lyser, ljuspulser som springer
    längs linjerna och fönster som tänds i diagonala vågor med skiftande färg.
    Rutnätet passar att mappa på en husfasad (ställ in med mesh eller fyrhörn)."""
    ys, xs = np.mgrid[0:H, 0:W].astype(np.float32)
    ncol, nrow = 10, 6
    mx, my = W * 0.04, H * 0.06
    gx = np.linspace(mx, W - mx, ncol + 1)
    gy = np.linspace(my, H - my, nrow + 1)
    dvx = np.min(np.abs(xs[..., None] - gx), axis=-1)
    dhy = np.min(np.abs(ys[..., None] - gy), axis=-1)
    inside_x = (xs > gx[0] - 2) & (xs < gx[-1] + 2)
    inside_y = (ys > gy[0] - 2) & (ys < gy[-1] + 2)
    vline = np.exp(-(dvx / 2.0) ** 2) * inside_y
    hline = np.exp(-(dhy / 2.0) ** 2) * inside_x
    col = np.clip(np.searchsorted(gx, xs) - 1, 0, ncol - 1)
    row = np.clip(np.searchsorted(gy, ys) - 1, 0, nrow - 1)
    cw, ch = gx[1] - gx[0], gy[1] - gy[0]
    # Fönsterruta: lite innanför karmen.
    pane = (dvx > 7) & (dhy > 7) & inside_x & inside_y
    lx = (xs - gx[0]) / (gx[-1] - gx[0])
    ly = (ys - gy[0]) / (gy[-1] - gy[0])

    def frame(i):
        a = TAU * i / N
        # Ljuspulser: en per linje, olika fart (hela varv per loop).
        pulse_h = np.zeros((H, W), np.float32)
        for j in range(nrow + 1):
            p = ((j * 0.37 + (1 + j % 3) * i / N) % 1.0)
            d = np.abs(((lx - p) + 0.5) % 1.0 - 0.5)
            pulse_h += np.exp(-(d / 0.035) ** 2) * (np.abs(ys - gy[j]) < 3)
        pulse_v = np.zeros((H, W), np.float32)
        for k in range(ncol + 1):
            p = ((k * 0.61 + (1 + k % 2) * i / N) % 1.0)
            d = np.abs(((ly - p) + 0.5) % 1.0 - 0.5)
            pulse_v += np.exp(-(d / 0.06) ** 2) * (np.abs(xs - gx[k]) < 3)
        frame_lines = np.maximum(vline, hline)
        # Fönstren: diagonal våg, två gånger per loop, färgen vandrar ett varv.
        phase = (col + row * 0.7) / (ncol + nrow)
        wave = np.maximum(0, np.cos(2 * a - phase * TAU * 1.5)) ** 2.5
        hue = (i / N + phase * 0.5) % 1.0
        rgb = 0.5 + 0.5 * np.cos(TAU * (hue[..., None] + np.array([0.0, 0.33, 0.67])))
        img = pane[..., None] * wave[..., None] * rgb * 0.55
        img += frame_lines[..., None] * np.array([0.25, 0.45, 0.6])
        img += np.clip(pulse_h + pulse_v, 0, 1.5)[..., None] * np.array([0.9, 0.95, 1.0])
        return glow(img, 1.0, 10)

    encode("lightgrid", (frame(i) for i in range(N)))


def galaxy():
    """Galax: spiralgalax med två armar som roterar. Armarna är exakt
    symmetriska, så ett halvt varv per loop gör den sömlös."""
    rng = np.random.default_rng(42)
    # Hälften av stjärnorna; den andra hälften är samma vridna ett halvt varv.
    n_arm, n_bulge = 45000, 12000
    r = rng.power(0.55, n_arm) * 0.95 + 0.04
    pitch = np.tan(np.radians(16))
    theta = np.log(r) / pitch + rng.normal(0, 0.16 + 0.12 * r, n_arm)
    r = r * rng.normal(1, 0.04, n_arm)
    rb = np.abs(rng.normal(0, 0.12, n_bulge))
    tb = rng.uniform(0, TAU, n_bulge)
    radius = np.concatenate([r, rb])
    angle = np.concatenate([theta, tb])
    colour = np.concatenate([
        np.where(rng.random(n_arm)[:, None] < 0.03, [[1.0, 0.45, 0.7]], [[0.7, 0.8, 1.0]]) * rng.uniform(0.3, 1.0, (n_arm, 1)),
        np.array([[1.0, 0.85, 0.6]]) * rng.uniform(0.3, 1.0, (n_bulge, 1)),
    ])
    radius = np.concatenate([radius, radius])
    angle = np.concatenate([angle, angle + np.pi])
    colour = np.concatenate([colour, colour])
    tilt, turn = 0.55, np.radians(-25)
    scale = H * 0.85
    ys, xs = np.mgrid[0:H, 0:W].astype(np.float32)
    background = np.zeros((H, W, 3), np.float32)
    k = 1500
    background[rng.integers(0, H, k), rng.integers(0, W, k)] = rng.uniform(0.1, 0.7, (k, 1))
    core = np.exp(-(((xs - W / 2) / (H * 0.07)) ** 2 + ((ys - H / 2) / (H * 0.05)) ** 2))[..., None] * np.array([1.0, 0.8, 0.55])

    def frame(i):
        phi = np.pi * i / N  # ett halvt varv per loop
        a = angle + phi
        x, y = radius * np.cos(a), radius * np.sin(a) * tilt
        x, y = x * np.cos(turn) - y * np.sin(turn), x * np.sin(turn) + y * np.cos(turn)
        px = (W / 2 + x * scale).astype(int)
        py = (H / 2 + y * scale).astype(int)
        ok = (px >= 0) & (px < W) & (py >= 0) & (py < H)
        img = background.copy()
        np.add.at(img, (py[ok], px[ok]), colour[ok] * 0.35)
        # Lätt mjukning: tiotusentals gnistrande punkter går nästan inte att komprimera.
        img = box_blur(img, 1) * 1.6
        img = glow(img, 1.6, 6) + core * 0.9
        return glow(img, 0.5, 24)

    encode("galaxy", (frame(i) for i in range(N)), crf=28)


VIDEOS = {
    "space": space, "fire": fire, "plasma": plasma, "aurora": aurora, "neon": neon,
    "water": water, "rain": rain, "matrix": matrix, "lightgrid": lightgrid, "galaxy": galaxy,
}

if __name__ == "__main__":
    names = sys.argv[1:] or list(VIDEOS)
    for n in names:
        if n not in VIDEOS:
            sys.exit(f"Okänd video: {n}. Finns: {', '.join(VIDEOS)}")
    for n in names:
        VIDEOS[n]()
