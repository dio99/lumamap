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


# ---------- Jul och halloween ----------

def font(names, size):
    """Första typsnittet som finns; annars Pillows inbyggda."""
    from PIL import ImageFont
    for name in names:
        for d in ("/usr/share/fonts/opentype/urw-base35", "/usr/share/fonts/truetype/dejavu", "/usr/share/fonts/truetype/liberation"):
            try:
                return ImageFont.truetype(f"{d}/{name}", size)
            except OSError:
                pass
    return ImageFont.load_default(size)


def polygon_mask(polys, size=(W, H)):
    """Fyller polygoner (listor av (x, y) i pixlar) till en mask 0..1."""
    from PIL import Image, ImageDraw
    im = Image.new("L", size, 0)
    d = ImageDraw.Draw(im)
    for p in polys:
        d.polygon([(float(x), float(y)) for x, y in p], fill=255)
    return np.asarray(im, np.float32) / 255.0


def sprite(core, halo, size=None):
    """En ljuspunkt: skarp kärna och mjuk gloria (radier i pixlar)."""
    size = size or int(halo * 3)
    yy, xx = np.mgrid[-size:size + 1, -size:size + 1].astype(np.float32)
    d = np.hypot(xx, yy)
    return np.clip(1.4 - d / core, 0, 1) + 0.45 * np.exp(-(d / halo) ** 2)


def stamp(img, xs, ys, colors, spr):
    """Lägger ut `spr` (en sprite) i färg på varje punkt."""
    s = spr.shape[0] // 2
    for x, y, c in zip(xs.astype(int), ys.astype(int), colors):
        x0, x1, y0, y1 = max(x - s, 0), min(x + s + 1, img.shape[1]), max(y - s, 0), min(y + s + 1, img.shape[0])
        if x0 >= x1 or y0 >= y1:
            continue
        img[y0:y1, x0:x1] += spr[y0 - (y - s):y1 - (y - s), x0 - (x - s):x1 - (x - s), None] * c
    return img


def splat(img, xs, ys, colors, radius):
    """Mjuka prickar: lägger färg i punkterna och suddar till runda fläckar."""
    layer = np.zeros_like(img)
    xi, yi = xs.astype(int), ys.astype(int)
    ok = (xi >= 0) & (xi < img.shape[1]) & (yi >= 0) & (yi < img.shape[0])
    np.add.at(layer, (yi[ok], xi[ok]), colors[ok])
    return img + blur(layer, radius) * (2 * radius + 1) ** 2 * 0.35 if radius > 0 else img + layer


def snow():
    """Snö: flingor i tre djup som faller och vajar. Närmast är stora och mjuka."""
    rng = np.random.default_rng(21)
    ys, xs = np.mgrid[0:H, 0:W].astype(np.float32)
    sky = palette([(0, (0.01, 0.02, 0.07)), (0.7, (0.03, 0.06, 0.16)), (1, (0.08, 0.12, 0.25))], ys / H)
    layers = [  # (antal, varv per loop, svaj px, oskärpa, ljusstyrka)
        (900, 1, 6, 0, 0.55),
        (380, 2, 14, 1, 0.8),
        (90, 3, 30, 6, 1.6),
    ]
    flakes = [(rng.uniform(0, W, n), rng.uniform(0, 1, n), rng.uniform(0, TAU, n), rng.integers(1, 3, n), laps, sway, r, b)
              for n, laps, sway, r, b in layers]

    def frame(i):
        t = i / N
        img = sky.copy()
        for x0, y0, ph, k, laps, sway, r, b in flakes:
            y = ((y0 + laps * t) % 1.0) * (H + 60) - 30
            x = (x0 + sway * np.sin(TAU * k * t + ph)) % W
            col = np.full((len(x), 3), b, np.float32) * np.array([0.9, 0.95, 1.0])
            img = splat(img, x, y, col, r)
        return glow(img, 0.25, 6)

    encode("snow", (frame(i) for i in range(N)))


def xmas_lights():
    """Julgransljus på girlanger som hänger i bågar; färgerna jagar och blinkar.
    Bra längs en takfot eller runt ett fönster."""
    rng = np.random.default_rng(24)
    ys, xs = np.mgrid[0:H, 0:W].astype(np.float32)
    colors = np.array([[1.0, 0.15, 0.1], [0.1, 1.0, 0.25], [0.2, 0.45, 1.0], [1.0, 0.8, 0.15], [1.0, 0.4, 0.9]], np.float32)
    bulbs = []  # (x, y, färg, index)
    wire = np.zeros((H, W), np.float32)
    for row, top in enumerate([0.12, 0.38, 0.64, 0.9]):
        for seg in range(3):  # tre bågar per rad
            x0, x1 = seg * W / 3, (seg + 1) * W / 3
            sag = H * 0.09
            t = np.linspace(0, 1, 400)
            cx, cy = x0 + (x1 - x0) * t, top * H - sag + sag * 4 * (t - 0.5) ** 2
            ok = (cy >= 0) & (cy < H) & (cx >= 0) & (cx < W)
            wire[cy[ok].astype(int), cx[ok].astype(int)] = 1.0
            for k, tt in enumerate(np.linspace(0.04, 0.96, 9)):
                bx, by = x0 + (x1 - x0) * tt, top * H - sag + sag * 4 * (tt - 0.5) ** 2 + 10
                bulbs.append((bx, by, colors[(len(bulbs)) % len(colors)], len(bulbs)))
    wire = blur(wire[..., None], 1)[..., 0] * 3
    base = wire[..., None] * np.array([0.05, 0.12, 0.05])
    bx = np.array([b[0] for b in bulbs], np.float32)
    by = np.array([b[1] for b in bulbs], np.float32)
    bc = np.array([b[2] for b in bulbs], np.float32)
    idx = np.arange(len(bulbs))
    twinkle = rng.uniform(0, TAU, len(bulbs))

    bulb = sprite(7, 22)

    def frame(i):
        t = i / N
        chase = np.maximum(0, np.cos(TAU * (idx / 15 - 4 * t))) ** 3  # jagar fyra varv per loop
        tw = 0.5 + 0.5 * np.sin(TAU * 6 * t + twinkle)
        level = 0.3 + 0.7 * np.maximum(chase, tw * 0.6)
        img = stamp(base.copy(), bx, by, bc * level[:, None], bulb)
        return glow(img, 0.6, 14)

    encode("xmas-lights", (frame(i) for i in range(N)))


def sparkle():
    """Glitter: guldkorn som singlar ned och glimmar, och stora stjärnor som blinkar."""
    rng = np.random.default_rng(27)
    ys, xs = np.mgrid[0:H, 0:W].astype(np.float32)
    bg = palette([(0, (0.03, 0.0, 0.02)), (1, (0.12, 0.02, 0.05))], np.hypot(xs - W / 2, ys - H / 2) / W) * 0.8
    n = 700
    x0, y0, ph = rng.uniform(0, W, n), rng.uniform(0, 1, n), rng.uniform(0, TAU, n)
    laps = rng.integers(1, 3, n)
    freq = rng.integers(3, 9, n)
    gold = np.array([1.0, 0.78, 0.35], np.float32) * rng.uniform(0.6, 1.0, (n, 1))
    stars = [(rng.uniform(0.08, 0.92) * W, rng.uniform(0.1, 0.9) * H, rng.uniform(18, 46), rng.integers(2, 5), rng.uniform(0, TAU)) for _ in range(9)]
    grain = sprite(2.5, 7)

    def frame(i):
        t = i / N
        y = ((y0 + laps * t) % 1.0) * (H + 20) - 10
        x = (x0 + 12 * np.sin(TAU * 2 * t + ph)) % W
        shine = np.maximum(0, np.sin(TAU * freq * t + ph)) ** 6
        img = stamp(bg.copy(), x, y, gold * (0.2 + 1.2 * shine[:, None]), grain)
        for sx, sy, r, f, p in stars:
            s = max(0.0, np.sin(TAU * f * t + p)) ** 2
            dx, dy = np.abs(xs - sx), np.abs(ys - sy)
            ray = np.exp(-dx / 2.5) * np.exp(-dy / (r * s + 1)) + np.exp(-dy / 2.5) * np.exp(-dx / (r * s + 1))
            img = img + (ray * s)[..., None] * np.array([1.0, 0.9, 0.6])
        return glow(img, 0.9, 10)

    encode("sparkle", (frame(i) for i in range(N)))


def pumpkin():
    """Pumpa: urholkad lykta med fladdrande ljus inuti. Fin på en ellipsyta."""
    ys, xs = np.mgrid[0:H, 0:W].astype(np.float32)
    cx, cy = W / 2, H * 0.56
    # Kroppen: fem ellipser (räfflor) bredvid varandra.
    body = np.zeros((H, W), np.float32)
    shade = np.zeros((H, W), np.float32)
    for k, (ox, rx) in enumerate([(-0.24, 0.17), (-0.12, 0.2), (0.0, 0.21), (0.12, 0.2), (0.24, 0.17)]):
        e = ((xs - (cx + ox * H)) / (rx * H)) ** 2 + ((ys - cy) / (0.36 * H)) ** 2
        inside = np.clip((1 - e) * 20, 0, 1)
        body = np.maximum(body, inside)
        shade = np.maximum(shade, inside * np.clip(1 - e, 0, 1) ** 0.5)
    stem = polygon_mask([[(cx - 22, cy - 0.33 * H), (cx + 18, cy - 0.33 * H), (cx + 34, cy - 0.47 * H), (cx + 6, cy - 0.49 * H)]])
    # Ansiktet: ögon, näsa och en tandad mun.
    s = H * 0.36
    def P(pts):
        return [(cx + x * s, cy + y * s) for x, y in pts]
    face = polygon_mask([
        P([(-0.55, -0.25), (-0.18, -0.35), (-0.3, 0.02)]),
        P([(0.55, -0.25), (0.18, -0.35), (0.3, 0.02)]),
        P([(0.0, -0.05), (-0.1, 0.12), (0.1, 0.12)]),
        P([(-0.62, 0.22), (-0.45, 0.3), (-0.35, 0.22), (-0.2, 0.33), (-0.05, 0.24), (0.1, 0.34), (0.25, 0.23),
           (0.4, 0.3), (0.62, 0.2), (0.45, 0.55), (0.2, 0.48), (0.05, 0.6), (-0.12, 0.49), (-0.3, 0.58), (-0.48, 0.48)]),
    ])
    orange = palette([(0, (0.12, 0.02, 0.0)), (0.6, (0.55, 0.18, 0.0)), (1, (0.85, 0.36, 0.04))], shade)
    # Mörk kant runt hålen, där skalet är tjockt.
    rim = np.clip(blur(face[..., None], 4)[..., 0] * 3, 0, 1) * (1 - face)
    pumpkin_rgb = orange * body[..., None] + stem[..., None] * np.array([0.25, 0.3, 0.08])
    face_glow = blur(face[..., None], 10)[..., 0]
    # Helt svart bakgrund: med blandningen Addera syns då bara pumpan.
    bg = np.zeros((H, W, 3), np.float32)

    def frame(i):
        t = i / N
        # Ljuslågan fladdrar: summa av heltalsfrekvenser, så att loopen går ihop.
        f = 0.82 + 0.08 * np.sin(TAU * 7 * t) + 0.06 * np.sin(TAU * 13 * t + 1) + 0.04 * np.sin(TAU * 29 * t + 2)
        candle = np.array([1.0, 0.75, 0.25]) * f
        img = bg + pumpkin_rgb * (0.75 + 0.25 * f) * (1 - 0.7 * rim[..., None])
        img = img * (1 - face[..., None]) + face[..., None] * (candle * 1.3 + 0.25)
        img = img + face_glow[..., None] * candle * 0.25
        return glow(img, 0.7, 16)

    encode("pumpkin", (frame(i) for i in range(N)))


def ghost_shape(x, y, size, t, phase):
    """Ett spöke: rundat huvud och kropp med vågig fåll (polygon i pixlar)."""
    pts = []
    for a in np.linspace(np.pi, 2 * np.pi, 24):  # huvudet
        pts.append((x + np.cos(a) * size, y + np.sin(a) * size))
    for k in range(25):  # fållen, vågar som rör sig
        u = k / 24
        px = x + size - 2 * size * u
        py = y + 1.6 * size + 0.18 * size * np.sin(TAU * (3 * u + 2 * t) + phase)
        pts.append((px, py))
    return pts


def ghosts():
    """Spöken som svävar genom dimma och guppar."""
    from PIL import Image, ImageDraw
    rng = np.random.default_rng(31)
    ys, xs = np.mgrid[0:H, 0:W].astype(np.float32)
    fog = periodic_noise(1024, 140, rng)
    sky = palette([(0, (0.0, 0.01, 0.02)), (1, (0.02, 0.06, 0.07))], ys / H)
    spooks = [(rng.uniform(0, 1), rng.uniform(0.25, 0.7) * H, rng.uniform(55, 110), int(rng.integers(1, 3)), rng.uniform(0, TAU)) for _ in range(5)]

    def frame(i):
        t = i / N
        f = fog[(ys.astype(int) + int(40 * np.sin(TAU * t))) % 1024, (xs.astype(int) + int(1024 * t)) % 1024]
        img = sky + (f ** 2)[..., None] * np.array([0.12, 0.2, 0.2]) * np.clip(ys / H * 1.5, 0, 1)[..., None]
        body = Image.new("L", (W, H), 0)
        faces = Image.new("L", (W, H), 0)
        d, df = ImageDraw.Draw(body), ImageDraw.Draw(faces)
        for x0, y0, size, laps, ph in spooks:
            x = ((x0 + laps * t) % 1.0) * (W + 4 * size) - 2 * size
            y = y0 + 25 * np.sin(TAU * 2 * t + ph)
            d.polygon([(float(a), float(b)) for a, b in ghost_shape(x, y, size, t, ph)], fill=200)
            for ex in (-0.35, 0.35):
                df.ellipse([x + ex * size - 0.13 * size, y - 0.15 * size, x + ex * size + 0.13 * size, y + 0.2 * size], fill=255)
            df.ellipse([x - 0.15 * size, y + 0.35 * size, x + 0.15 * size, y + 0.65 * size], fill=255)
        b = np.asarray(body, np.float32) / 255.0
        fc = np.asarray(faces, np.float32) / 255.0
        ghost = blur(b[..., None], 2)[..., 0] * (1 - fc)
        img = img * (1 - ghost[..., None] * 0.8) + ghost[..., None] * np.array([0.85, 0.95, 1.0])
        return glow(img, 0.6, 14)

    encode("ghosts", (frame(i) for i in range(N)))


def bat_shape(x, y, size, flap):
    """Fladdermus i siluett; `flap` −1..1 lyfter och sänker vingarna."""
    up = -flap * 0.5
    pts = [(0, -0.25), (0.12, -0.45), (0.18, -0.2), (0.45, -0.3 + up), (0.75, -0.55 + up * 1.6), (1.0, -0.2 + up * 1.4),
           (0.8, -0.05 + up), (0.62, 0.05 + up * 0.5), (0.4, 0.0), (0.2, 0.15), (0.0, 0.3)]
    right = [(x + px * size, y + py * size) for px, py in pts]
    left = [(x - px * size, y + py * size) for px, py in reversed(pts)]
    return right + left


def bats():
    """Fladdermöss som flaxar förbi fullmånen, med dimma nedtill."""
    from PIL import Image, ImageDraw
    rng = np.random.default_rng(33)
    ys, xs = np.mgrid[0:H, 0:W].astype(np.float32)
    sky = palette([(0, (0.02, 0.0, 0.06)), (0.7, (0.08, 0.02, 0.12)), (1, (0.18, 0.06, 0.12))], ys / H)
    mx, my, mr = W * 0.68, H * 0.36, H * 0.24
    md = np.hypot(xs - mx, ys - my)
    craters = periodic_noise(512, 40, rng)[ys.astype(int) % 512, xs.astype(int) % 512]
    moon = np.clip((mr - md) / 2, 0, 1)[..., None] * (np.array([1.0, 0.95, 0.75]) * (0.82 + 0.18 * craters[..., None]))
    halo = np.exp(-np.maximum(md - mr, 0) / (H * 0.08))[..., None] * np.array([0.35, 0.3, 0.25])
    stars = np.zeros((H, W), np.float32)
    stars[rng.integers(0, H, 300), rng.integers(0, W, 300)] = rng.uniform(0.2, 0.9, 300)
    background = sky + halo + glow(stars[..., None], 0.5, 3) * 0.6
    background = background * (1 - np.clip((mr - md) / 2, 0, 1)[..., None]) + moon
    fog = periodic_noise(1024, 160, rng)
    flock = [(rng.uniform(0, 1), rng.uniform(0.15, 0.75) * H, rng.uniform(22, 60), int(rng.integers(1, 3)), int(rng.integers(8, 14)), rng.uniform(0, TAU)) for _ in range(9)]

    def frame(i):
        t = i / N
        sil = Image.new("L", (W, H), 0)
        d = ImageDraw.Draw(sil)
        for x0, y0, size, laps, flaps, ph in flock:
            x = ((x0 + laps * t) % 1.0) * (W + 4 * size) - 2 * size
            y = y0 + 40 * np.sin(TAU * 2 * t + ph)
            d.polygon([(float(a), float(b)) for a, b in bat_shape(x, y, size, np.sin(TAU * flaps * t + ph))], fill=255)
        s = np.asarray(sil, np.float32)[..., None] / 255.0
        img = background * (1 - s)
        f = fog[ys.astype(int) % 1024, (xs.astype(int) + int(1024 * t)) % 1024]
        img = img + (f ** 2 * np.clip((ys / H - 0.6) * 2.5, 0, 1))[..., None] * np.array([0.3, 0.25, 0.35])
        return glow(img, 0.3, 10)

    encode("bats", (frame(i) for i in range(N)))


def eyes():
    """Glödande ögonpar i mörkret som blinkar och tittar åt sidan."""
    rng = np.random.default_rng(37)
    ys, xs = np.mgrid[0:H, 0:W].astype(np.float32)
    tints = np.array([[1.0, 0.85, 0.1], [1.0, 0.2, 0.1], [0.4, 1.0, 0.2]], np.float32)
    pairs = []
    while len(pairs) < 14:
        x, y = rng.uniform(0.08, 0.92) * W, rng.uniform(0.12, 0.88) * H
        if all(np.hypot(x - p[0], y - p[1]) > 160 for p in pairs):
            pairs.append((x, y, rng.uniform(14, 30), tints[rng.integers(0, 3)], rng.uniform(0, 1), int(rng.integers(2, 5)), rng.uniform(0, TAU)))

    def frame(i):
        t = i / N
        img = np.zeros((H, W, 3), np.float32)
        for x, y, r, tint, blink_at, looks, ph in pairs:
            # Blinkar kort vid sin tidpunkt (öppen annars), och syns inte hela tiden.
            d = abs(((t - blink_at) + 0.5) % 1.0 - 0.5)
            open_ = np.clip(d * 40, 0.05, 1.0)
            present = np.clip(np.sin(TAU * t + ph) * 3 + 1.5, 0, 1)
            if present <= 0:
                continue
            look = 0.35 * r * np.sin(TAU * looks * t + ph)
            for ex in (-1.6 * r, 1.6 * r):
                ex_x = x + ex
                e = ((xs - ex_x) / r) ** 2 + ((ys - y) / (r * 0.55 * open_)) ** 2
                iris = np.clip((1 - e) * 6, 0, 1)
                pupil = np.clip((1 - (((xs - ex_x - look) / (r * 0.18)) ** 2 + ((ys - y) / (r * 0.5)) ** 2)) * 6, 0, 1)
                img += ((iris * (1 - pupil)) * present)[..., None] * tint
        return glow(img, 1.4, 12)

    encode("eyes", (frame(i) for i in range(N)))


def cards():
    """Stillbilder: julkort och halloweenkort på svenska och engelska."""
    from PIL import Image, ImageDraw, ImageFilter
    rng = np.random.default_rng(41)
    w, h = 1920, 1080
    ys, xs = np.mgrid[0:h, 0:w].astype(np.float32)

    def save(name, bg, text, font_names, size, colour, glow_colour, extra=None):
        img = Image.fromarray((np.clip(bg, 0, 1) * 255).astype(np.uint8))
        if extra:
            extra(img)
        layer = Image.new("L", (w, h), 0)
        ImageDraw.Draw(layer).text((w / 2, h * 0.52), text, font=font(font_names, size), fill=255, anchor="mm")
        halo = layer.filter(ImageFilter.GaussianBlur(28))
        base = np.asarray(img, np.float32) / 255
        m = np.asarray(layer, np.float32)[..., None] / 255
        g = np.asarray(halo, np.float32)[..., None] / 255
        out = base + g * np.array(glow_colour) * 1.4
        out = out * (1 - m) + m * np.array(colour)
        Image.fromarray((np.clip(out, 0, 1) * 255).astype(np.uint8)).save(OUT / f"{name}.jpg", quality=92)
        print(f"{name}.jpg: klar")

    # Jul: djupröd bakgrund, snö och gyllene skrivstil.
    xmas_bg = palette([(0, (0.35, 0.02, 0.05)), (1, (0.08, 0.0, 0.02))], np.hypot(xs - w / 2, ys - h / 2) / (w * 0.6))
    flakes = np.zeros((h, w), np.float32)
    flakes[rng.integers(0, h, 1400), rng.integers(0, w, 1400)] = rng.uniform(0.3, 1.0, 1400)
    xmas_bg = xmas_bg + blur(flakes[..., None], 2) * 9
    script = ["Z003-MediumItalic.otf", "DejaVuSerif-BoldItalic.ttf"]
    save("god-jul", xmas_bg, "God Jul", script, 300, (1.0, 0.85, 0.45), (1.0, 0.6, 0.2))
    save("merry-christmas", xmas_bg, "Merry Christmas", script, 230, (1.0, 0.85, 0.45), (1.0, 0.6, 0.2))

    # Halloween: lila natt, måne och fladdermöss, orange text.
    hw_bg = palette([(0, (0.02, 0.0, 0.05)), (1, (0.14, 0.03, 0.16))], ys / h)
    md = np.hypot(xs - w * 0.8, ys - h * 0.22)
    hw_bg = hw_bg + np.clip((150 - md) / 3, 0, 1)[..., None] * np.array([0.9, 0.85, 0.6]) + np.exp(-np.maximum(md - 150, 0) / 90)[..., None] * 0.2

    def bats_on(img):
        d = ImageDraw.Draw(img)
        for _ in range(7):
            x, y, s = rng.uniform(0.1, 0.9) * w, rng.uniform(0.08, 0.35) * h, rng.uniform(30, 70)
            d.polygon([(float(a), float(b)) for a, b in bat_shape(x, y, s, rng.uniform(-1, 1))], fill=(5, 0, 10))

    heavy = ["URWBookman-Demi.otf", "DejaVuSerif-Bold.ttf"]
    save("glad-halloween", hw_bg, "Glad Halloween", heavy, 200, (1.0, 0.55, 0.05), (1.0, 0.35, 0.0), bats_on)
    save("happy-halloween", hw_bg, "Happy Halloween", heavy, 190, (1.0, 0.55, 0.05), (1.0, 0.35, 0.0), bats_on)

# ---------- Roliga animationer (som på sociala medier) ----------

def night_sky(rng, moon=(0.75, 0.3, 0.16)):
    """Natthimmel med stjärnor och en måne (läge och radie som andel av höjden)."""
    ys, xs = np.mgrid[0:H, 0:W].astype(np.float32)
    sky = palette([(0, (0.0, 0.01, 0.05)), (1, (0.04, 0.07, 0.18))], ys / H)
    stars = np.zeros((H, W), np.float32)
    stars[rng.integers(0, H, 400), rng.integers(0, W, 400)] = rng.uniform(0.2, 1.0, 400)
    sky = sky + glow(stars[..., None], 0.5, 3) * 0.7
    mx, my, mr = moon[0] * W, moon[1] * H, moon[2] * H
    md = np.hypot(xs - mx, ys - my)
    disc = np.clip((mr - md) / 2, 0, 1)[..., None]
    halo = np.exp(-np.maximum(md - mr, 0) / (H * 0.07))[..., None] * np.array([0.3, 0.3, 0.25])
    return sky * (1 - disc) + disc * np.array([1.0, 0.97, 0.85]) + halo


def santa():
    """Tomten i släden med fyra renar som flyger förbi månen."""
    from PIL import Image, ImageDraw
    rng = np.random.default_rng(51)
    sky = night_sky(rng, (0.62, 0.38, 0.2))
    span = 1.1 * W  # hela ekipaget, så att det är helt utanför bild vid loopens skarv
    trail = np.zeros((H, W, 3), np.float32)
    sparkle_spr = sprite(2, 6)

    def reindeer(d, x, y, s, step):
        d.ellipse([x - 0.55 * s, y - 0.22 * s, x + 0.55 * s, y + 0.22 * s], fill=255)  # kropp
        d.polygon([(x + 0.4 * s, y - 0.1 * s), (x + 0.7 * s, y - 0.55 * s), (x + 0.85 * s, y - 0.5 * s), (x + 0.6 * s, y)], fill=255)  # hals
        d.ellipse([x + 0.7 * s, y - 0.7 * s, x + 1.05 * s, y - 0.48 * s], fill=255)  # huvud
        for k in (0, 1):  # horn
            bx = x + (0.78 + 0.1 * k) * s
            d.line([(bx, y - 0.68 * s), (bx - 0.1 * s, y - 0.95 * s), (bx - 0.25 * s, y - 1.05 * s)], fill=255, width=max(3, int(s * 0.05)))
            d.line([(bx - 0.08 * s, y - 0.88 * s), (bx + 0.05 * s, y - 1.0 * s)], fill=255, width=max(3, int(s * 0.05)))
        for k, lx in enumerate((-0.4, -0.25, 0.3, 0.45)):  # ben i galopp
            a = 0.6 * np.sin(step + (k % 2) * np.pi)
            d.line([(x + lx * s, y + 0.1 * s), (x + lx * s + np.sin(a) * 0.45 * s, y + 0.1 * s + np.cos(a) * 0.45 * s)], fill=255, width=max(4, int(s * 0.07)))

    def frame(i):
        nonlocal trail
        t = i / N
        lead_x = t * (W + span) - 0.05 * W
        base_y = H * (0.55 - 0.25 * t) + 30 * np.sin(TAU * 2 * t)
        sil = Image.new("L", (W, H), 0)
        d = ImageDraw.Draw(sil)
        s = 70
        # Släden med tomten bakom renarna.
        sx, sy = lead_x - 0.95 * W * 0.5, base_y + 0.12 * (0.95 * W * 0.5)
        d.polygon([(sx - 120, sy - 30), (sx + 60, sy - 40), (sx + 80, sy + 20), (sx - 110, sy + 25)], fill=255)
        d.line([(sx - 140, sy + 40), (sx + 90, sy + 40), (sx + 120, sy + 15)], fill=255, width=8)
        d.ellipse([sx - 60, sy - 110, sx + 20, sy - 20], fill=255)  # tomten
        d.ellipse([sx - 45, sy - 150, sx + 5, sy - 100], fill=255)  # huvud
        d.polygon([(sx - 50, sy - 135), (sx + 10, sy - 140), (sx + 50, sy - 120)], fill=255)  # mössa
        d.ellipse([sx - 105, sy - 95, sx - 35, sy - 30], fill=255)  # säcken
        prev = (sx + 60, sy - 30)
        for k in range(4):
            rx = sx + 220 + k * 170
            ry = sy - 40 - k * 22 + 10 * np.sin(TAU * 6 * t + k)
            reindeer(d, rx, ry, s, TAU * 6 * t + k * 0.8)
            d.line([prev, (rx - 0.3 * s, ry)], fill=255, width=3)
            prev = (rx + 0.5 * s, ry - 0.1 * s)
        m = np.asarray(sil, np.float32)[..., None] / 255.0
        # Glittersvans efter släden som bleknar.
        trail *= 0.88
        stamp(trail, np.array([sx - 140.0 + rng.uniform(-20, 20) for _ in range(6)]),
              np.array([sy + 30.0 + rng.uniform(-25, 25) for _ in range(6)]), np.array([[1.0, 0.85, 0.4]] * 6), sparkle_spr)
        img = sky * (1 - m) + trail
        return glow(img, 0.5, 10)

    # Svansen behöver några varv för att se likadan ut i början som i slutet.
    frames = crossfade_loop(frame, N, FPS)
    encode("santa", frames)


def skeleton():
    """Ett dansande skelett."""
    from PIL import Image, ImageDraw

    def frame(i):
        t = i / N
        beat = TAU * 16 * t  # 16 dansrörelser per loop (= 80 BPM)
        cx = W / 2 + 120 * np.sin(TAU * 2 * t)
        hip = (cx, H * 0.6 + 18 * abs(np.sin(beat)))
        neck = (hip[0] + 20 * np.sin(beat), hip[1] - 230)
        im = Image.new("L", (W, H), 0)
        d = ImageDraw.Draw(im)
        bone = 16

        def limb(a, angle1, angle2, l1, l2):
            b = (a[0] + l1 * np.sin(angle1), a[1] + l1 * np.cos(angle1))
            c = (b[0] + l2 * np.sin(angle2), b[1] + l2 * np.cos(angle2))
            d.line([a, b], fill=255, width=bone)
            d.line([b, c], fill=255, width=bone - 3)
            for p in (b, c):
                d.ellipse([p[0] - 11, p[1] - 11, p[0] + 11, p[1] + 11], fill=255)

        d.line([hip, neck], fill=255, width=bone)  # ryggrad
        for k in range(4):  # revben
            y = neck[1] + 40 + k * 32
            w = 75 - k * 9
            mx = neck[0] + (hip[0] - neck[0]) * (k + 1) / 6
            d.arc([mx - w, y - 18, mx + w, y + 30], 200, 340, fill=255, width=9)
        d.ellipse([hip[0] - 60, hip[1] - 25, hip[0] + 60, hip[1] + 25], outline=255, width=12)  # bäcken
        # Armar som vinkar, ben som sparkar i takt.
        limb((neck[0] - 50, neck[1] + 20), np.pi * 0.8 + 0.9 * np.sin(beat), np.pi + 1.2 * np.sin(beat + 1), 95, 90)
        limb((neck[0] + 50, neck[1] + 20), -np.pi * 0.8 + 0.9 * np.sin(beat + np.pi), -np.pi + 1.2 * np.sin(beat + 2), 95, 90)
        limb((hip[0] - 35, hip[1] + 15), 0.35 * np.sin(beat), 0.5 * max(0.0, np.sin(beat)), 120, 115)
        limb((hip[0] + 35, hip[1] + 15), -0.35 * np.sin(beat), -0.5 * max(0.0, -np.sin(beat)), 120, 115)
        # Skallen som nickar.
        hx, hy = neck[0] + 10 * np.sin(beat), neck[1] - 75
        d.ellipse([hx - 62, hy - 70, hx + 62, hy + 55], fill=255)
        d.rectangle([hx - 38, hy + 30, hx + 38, hy + 75], fill=255)
        body = np.asarray(im, np.float32) / 255.0
        holes = Image.new("L", (W, H), 0)
        dh = ImageDraw.Draw(holes)
        for ex in (-26, 26):
            dh.ellipse([hx + ex - 20, hy - 25, hx + ex + 20, hy + 12], fill=255)
        dh.polygon([(hx, hy + 12), (hx - 10, hy + 32), (hx + 10, hy + 32)], fill=255)
        for k in range(-3, 4):
            dh.line([(hx + k * 10, hy + 48), (hx + k * 10, hy + 72)], fill=255, width=3)
        h = np.asarray(holes, np.float32) / 255.0
        sk = np.clip(body - h, 0, 1)
        img = sk[..., None] * np.array([0.92, 0.95, 0.85])
        return glow(img, 0.5, 8)

    encode("skeleton", (frame(i) for i in range(N)))


def bricks():
    """Fasadillusion: tegelstenarna faller ut ur väggen, ljus väller fram, och
    väggen murar upp sig igen. Mappa på en vägg eller ett hus."""
    from PIL import Image, ImageDraw
    rng = np.random.default_rng(57)
    ys, xs = np.mgrid[0:H, 0:W].astype(np.float32)
    bw, bh = 120, 48
    stones = []
    for r in range(H // bh + 1):
        off = 0 if r % 2 == 0 else -bw / 2
        for c in range(W // bw + 2):
            x, y = off + c * bw, r * bh
            shade = rng.uniform(0.75, 1.1)
            colour = np.array([0.55, 0.22, 0.14]) * shade
            # Fördröjning: de lossnar från mitten och utåt, lite slumpvis.
            dist = np.hypot((x + bw / 2 - W / 2) / W, (y + bh / 2 - H / 2) / H)
            stones.append((x, y, colour, dist * 0.5 + rng.uniform(0, 0.08), rng.uniform(-3, 3)))

    def stone_at(s, u):
        """Läge och vridning för stenen när den fallit andelen u (0 = på plats)."""
        x, y, colour, delay, spin = s
        fall = H * 1.3 * u * u
        return x, y + fall, spin * u, colour

    def frame(i):
        t = i / N
        # Bakom väggen: färger som strömmar.
        a = TAU * t
        v = np.sin(xs / 180 + a * 2) + np.sin(ys / 140 - a * 3) + np.sin((xs + ys) / 220 + a)
        light = 0.5 + 0.5 * np.cos(TAU * (v[..., None] / 6 + t + np.array([0.0, 0.33, 0.67])))
        im = Image.new("RGB", (W, H), (0, 0, 0))
        mask = Image.new("L", (W, H), 0)
        d, dm = ImageDraw.Draw(im), ImageDraw.Draw(mask)
        for s in stones:
            delay = s[3]
            # Faller under 0,1–0,45, väntar, flyger tillbaka under 0,6–0,95.
            if t < 0.5:
                u = np.clip((t - 0.1 - delay * 0.35) / 0.12, 0, 1)
            else:
                u = np.clip((0.95 - delay * 0.35 - t) / 0.12, 0, 1)
            if u >= 1:
                continue
            x, y, rot, colour = stone_at(s, u)
            cxs, cys = x + bw / 2, y + bh / 2
            corners = [(-bw / 2 + 2, -bh / 2 + 2), (bw / 2 - 2, -bh / 2 + 2), (bw / 2 - 2, bh / 2 - 2), (-bw / 2 + 2, bh / 2 - 2)]
            cr, sr = np.cos(rot), np.sin(rot)
            pts = [(cxs + px * cr - py * sr, cys + px * sr + py * cr) for px, py in corners]
            fill = tuple(int(255 * min(c, 1.0)) for c in colour)
            d.polygon(pts, fill=fill)
            dm.polygon(pts, fill=255)
            # Fogen (bruket) syns runt stenar som sitter kvar.
            if u == 0:
                dm.rectangle([x, y, x + bw, y + bh], fill=255)
        wall = np.asarray(im, np.float32) / 255.0
        m = np.asarray(mask, np.float32)[..., None] / 255.0
        mortar = np.array([0.32, 0.3, 0.28])
        covered = np.where(np.asarray(im).sum(axis=2, keepdims=True) > 0, wall, mortar)
        img = light * (1 - m) * 0.9 + covered * m
        return img

    encode("bricks", (frame(i) for i in range(N)))


def window():
    """Fönsterfilm: ett upplyst fönster där en siluett går förbi, stannar och
    vinkar. Mappa på ett riktigt fönster (gärna med vit gardin/papper bakom)."""
    from PIL import Image, ImageDraw
    ys, xs = np.mgrid[0:H, 0:W].astype(np.float32)
    room = palette([(0, (1.0, 0.75, 0.35)), (1, (0.75, 0.42, 0.15))], np.hypot(xs - W / 2, ys - H * 0.3) / W)
    # Gardiner i kanterna med veck.
    folds = 0.75 + 0.25 * np.cos(xs / 18)
    curtain = (np.clip((W * 0.14 - xs) / 30, 0, 1) + np.clip((xs - W * 0.86) / 30, 0, 1))[..., None]
    room = room * (1 - curtain) + curtain * np.array([0.55, 0.12, 0.1]) * folds[..., None]
    # Spröjsen: ett kors i fönstret.
    bars = ((np.abs(xs - W / 2) < 10) | (np.abs(ys - H * 0.45) < 10)).astype(np.float32)[..., None]

    def person(d, x, walk, wave):
        y = H * 0.32
        sway = 6 * np.sin(walk)
        d.ellipse([x - 45, y - 55 + sway, x + 45, y + 40 + sway], fill=255)  # huvud
        d.polygon([(x - 75, y + 60), (x + 75, y + 60), (x + 95, y + 380), (x - 95, y + 380)], fill=255)  # kropp
        d.ellipse([x - 80, y + 40, x + 80, y + 120], fill=255)  # axlar
        for side, ph in ((-1, 0.0), (1, np.pi)):
            a = 0.35 * np.sin(walk + ph)
            if side == 1 and wave > 0:
                a = 2.5 + 0.4 * np.sin(wave)  # armen uppåt och utåt, vinkar
            sx, sy = x + side * 75, y + 80
            ex, ey = sx + np.sin(a) * 160 * side, sy + np.cos(a) * 160
            d.line([(sx, sy), (ex, ey)], fill=255, width=36)
            d.ellipse([ex - 22, ey - 22, ex + 22, ey + 22], fill=255)
        for ph in (0.0, np.pi):
            a = 0.3 * np.sin(walk + ph)
            d.line([(x, y + 370), (x + np.sin(a) * 260, y + 370 + np.cos(a) * 260)], fill=255, width=46)

    def frame(i):
        t = i / N
        im = Image.new("L", (W, H), 0)
        d = ImageDraw.Draw(im)
        # Går in, stannar och vinkar, går ut åt andra hållet.
        if t < 0.35:
            x, walk, wave = -200 + (W / 2 + 200) * (t / 0.35), TAU * 12 * t, 0
        elif t < 0.6:
            x, walk, wave = W / 2, 0.0, TAU * 8 * (t - 0.35) / 0.25 * 1.0
        else:
            x, walk, wave = W / 2 + (W / 2 + 200) * ((t - 0.6) / 0.4), TAU * 12 * t, 0
        person(d, x, walk, wave)
        sil = np.asarray(im, np.float32)[..., None] / 255.0
        # Mjuk skugga (genom gardin/papper).
        sil = blur(sil, 3)
        img = room * (1 - 0.92 * sil)
        img = img * (1 - bars) + bars * np.array([0.12, 0.08, 0.05])
        return img

    encode("window", (frame(i) for i in range(N)))

VIDEOS = {
    "space": space, "fire": fire, "plasma": plasma, "aurora": aurora, "neon": neon,
    "water": water, "rain": rain, "matrix": matrix, "lightgrid": lightgrid, "galaxy": galaxy,
    # Jul och halloween
    "snow": snow, "xmas-lights": xmas_lights, "sparkle": sparkle,
    "pumpkin": pumpkin, "ghosts": ghosts, "bats": bats, "eyes": eyes,
    "cards": cards,
    # Roliga animationer
    "santa": santa, "skeleton": skeleton, "bricks": bricks, "window": window,
}

if __name__ == "__main__":
    names = sys.argv[1:] or list(VIDEOS)
    for n in names:
        if n not in VIDEOS:
            sys.exit(f"Okänd video: {n}. Finns: {', '.join(VIDEOS)}")
    for n in names:
        VIDEOS[n]()
