// En mappad yta, ritad som trianglar från en vertexbuffer.
//
// Varje vertex har sin position på utgången (`pos`) och en punkt `p`.
// Per pixel räknas ytans egna koordinater fram, `param = h2 · p` (0..1 över
// ytan), och sedan källans texturkoordinat, `uv = h · param`. Att det görs per
// pixel ger korrekt perspektiv utan synlig diagonal söm:
//   Fyrhörn/ellips: p = pos, h2 = utgång → enhetskvadrat.
//   Mesh/triangel:  p = param direkt, h2 = identitet.
//   h = enhetskvadrat → källans utsnitt.
//
// En valfri polygonmask (i utgångens koordinater) tonar ut ytan med mjuk kant.
// Färgen lämnas förmultiplicerad med alfa så att alla blandningslägen blir rätt.

struct SurfaceUniform {
    h: mat3x3<f32>,
    h2: mat3x3<f32>,
    // x = opacitet, y = antal maskpunkter (0 = ingen mask), z = mjuk kant (px), w = 1 om inverterad
    params: vec4<f32>,
    // xy = utgångens upplösning i pixlar, z = form (1 ellips, 2 klot, 3 cylinder, 4 kon, 5 linje), w = nyansvridning (radianer)
    res: vec4<f32>,
    // Färgjustering: x = ljusstyrka, y = kontrast, z = gamma, w = mättnad
    color: vec4<f32>,
    // Effekt: x = sort (EffectKind), y = styrka 0..1, z = fas 0..1, w = 1 för triangel
    effect: vec4<f32>,
    // Maskpunkter, två per vec4 (xy, zw), normaliserade.
    mask: array<vec4<f32>, 16>,
};

@group(0) @binding(0) var<uniform> u: SurfaceUniform;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) p: vec2<f32>,
    @location(1) pos: vec2<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec2<f32>, @location(1) p: vec2<f32>) -> VsOut {
    var out: VsOut;
    out.clip = vec4<f32>(pos.x * 2.0 - 1.0, 1.0 - pos.y * 2.0, 0.0, 1.0);
    out.p = p;
    out.pos = pos;
    return out;
}

fn mask_point(i: u32) -> vec2<f32> {
    let v = u.mask[i / 2u];
    let p = select(v.xy, v.zw, i % 2u == 1u);
    return p * u.res.xy;
}

// Signerat avstånd i pixlar till maskpolygonen (negativt inuti).
fn mask_distance(q: vec2<f32>) -> f32 {
    let n = u32(u.params.y);
    var d = dot(q - mask_point(0u), q - mask_point(0u));
    var s = 1.0;
    var j = n - 1u;
    for (var i = 0u; i < n; i++) {
        let vi = mask_point(i);
        let vj = mask_point(j);
        let e = vj - vi;
        let w = q - vi;
        let b = w - e * clamp(dot(w, e) / max(dot(e, e), 1e-6), 0.0, 1.0);
        d = min(d, dot(b, b));
        let c0 = q.y >= vi.y;
        let c1 = vj.y > q.y;
        let c2 = e.x * w.y > e.y * w.x;
        if ((c0 && c1 && c2) || (!c0 && !c1 && !c2)) {
            s = -s;
        }
        j = i;
    }
    return s * sqrt(d);
}

fn mask_alpha(pos: vec2<f32>) -> f32 {
    if (u.params.y < 3.0) {
        return 1.0;
    }
    let d = mask_distance(pos * u.res.xy);
    // Minst en pixels kant så att masken alltid är kantutjämnad.
    let f = max(u.params.z, 1.0);
    if (u.params.w > 0.5) {
        return clamp(d / f + 0.5 / f, 0.0, 1.0);
    }
    return clamp(-d / f + 0.5 / f, 0.0, 1.0);
}

// Samma beräkning som `ColorAdjust::apply` i lc-core (testas där).
fn adjust(c: vec3<f32>) -> vec3<f32> {
    var rgb = pow(max((c - 0.5) * u.color.y + 0.5 + u.color.x, vec3<f32>(0.0)), vec3<f32>(1.0 / u.color.z));
    let luma = dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    rgb = mix(vec3<f32>(luma), rgb, u.color.w);
    let k = vec3<f32>(0.57735027);
    let s = sin(u.res.w);
    let co = cos(u.res.w);
    rgb = rgb * co + cross(k, rgb) * s + k * dot(k, rgb) * (1.0 - co);
    return clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0));
}

const TAU: f32 = 6.2831853;
const PI: f32 = 3.1415927;

fn shape_mode() -> u32 {
    return u32(u.res.z + 0.5);
}

// Ellips och klot är runda i bild.
fn is_round() -> bool {
    let m = shape_mode();
    return m == 1u || m == 2u;
}

// 3D-former: från punkten på ytan i bild till var den hamnar i bilden som
// lindas runt objektet. Projektorn ser objektet framifrån, så den synliga
// halvan av bilden täcker objektets framsida utan att dras ut mot kanterna.
fn wrap3d(p: vec2<f32>) -> vec2<f32> {
    switch shape_mode() {
        case 2u: {
            // Klot: longitud och latitud på framsidan.
            let d = (p - vec2<f32>(0.5)) * 2.0;
            let z = sqrt(max(1.0 - dot(d, d), 0.0));
            let lon = atan2(d.x, z);
            let lat = asin(clamp(d.y, -1.0, 1.0));
            return vec2<f32>(0.5 + lon / PI, 0.5 + lat / PI);
        }
        case 3u: {
            // Cylinder: vinkeln runt axeln.
            return vec2<f32>(0.5 + asin(clamp(p.x * 2.0 - 1.0, -1.0, 1.0)) / PI, p.y);
        }
        case 4u: {
            // Kon med spetsen i (0.5, 0): bredden växer nedåt.
            let half = max(p.y * 0.5, 1e-4);
            let x = clamp((p.x - 0.5) / half, -1.0, 1.0);
            return vec2<f32>(0.5 + asin(x) / PI, p.y);
        }
        default: {
            return p;
        }
    }
}

// Källans färg vid ytans koordinater `param` (genomskinligt utanför utsnittet).
fn sample_param(param: vec2<f32>) -> vec4<f32> {
    let q = u.h * vec3<f32>(param, 1.0);
    let uv = q.xy / q.z;
    let c = textureSampleLevel(tex, samp, uv, 0.0);
    let inside = step(0.0, uv.x) * step(uv.x, 1.0) * step(0.0, uv.y) * step(uv.y, 1.0);
    return vec4<f32>(c.rgb, c.a * inside);
}

// Speglar koordinater utanför 0..1 tillbaka in (som ett speglat kakel).
fn mirror(p: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(1.0) - abs(vec2<f32>(1.0) - fract(p * 0.5) * 2.0);
}

fn rotate(p: vec2<f32>, a: f32) -> vec2<f32> {
    let c = cos(a);
    let s = sin(a);
    return vec2<f32>(c * p.x - s * p.y, s * p.x + c * p.y);
}

// Effekter som flyttar bilden inne i ytan.
fn warp(p: vec2<f32>, kind: u32, amount: f32, phase: f32) -> vec2<f32> {
    let d = p - vec2<f32>(0.5);
    switch kind {
        case 1u: {
            // Kalejdoskop: 2–12 speglade tårtbitar som vrider sig.
            let n = floor(2.0 + amount * 10.0);
            let seg = TAU / n;
            var a = atan2(d.y, d.x) + phase * TAU;
            a = abs(a - seg * floor(a / seg) - seg * 0.5);
            return mirror(vec2<f32>(0.5) + vec2<f32>(cos(a), sin(a)) * length(d));
        }
        case 2u: {
            let n = 1.0 + floor(amount * 7.0);
            return fract(p * n + vec2<f32>(phase, 0.0));
        }
        case 3u: {
            return fract(p + vec2<f32>(phase, 0.0) * max(amount, 0.05) * 4.0);
        }
        case 4u: {
            // Inzoomat så att hörnen aldrig blir tomma.
            return vec2<f32>(0.5) + rotate(d, phase * TAU) / (1.0 + 0.42 * amount);
        }
        case 5u: {
            let s = 1.0 + amount * 0.35 * (0.5 + 0.5 * cos(phase * TAU));
            return vec2<f32>(0.5) + d / s;
        }
        default: {
            return p;
        }
    }
}

// Avstånd (0..0.5) till ytans kant i ytans koordinater, för konturglöden.
fn edge_distance(p: vec2<f32>) -> f32 {
    if (is_round()) {
        return (1.0 - length(p - vec2<f32>(0.5)) * 2.0) * 0.5;
    }
    if (u.effect.w > 0.5) {
        // Triangel med hörnen (0.5, 0), (1, 1), (0, 1).
        // Vänster sida 2x + y − 1 = 0, höger sida 1 − 2x + y = 0, botten y = 1.
        let left = (2.0 * p.x + p.y - 1.0) / sqrt(5.0);
        let right = (1.0 - 2.0 * p.x + p.y) / sqrt(5.0);
        return min(min(left, right), 1.0 - p.y);
    }
    return min(min(p.x, 1.0 - p.x), min(p.y, 1.0 - p.y));
}

// Läget runt ytans kant, 0..1 ett varv medurs från övre vänstra hörnet.
fn perimeter(p: vec2<f32>) -> f32 {
    if (is_round() || u.effect.w > 0.5) {
        return fract(atan2(p.y - 0.5, p.x - 0.5) / TAU + 0.75);
    }
    let d = vec4<f32>(p.y, 1.0 - p.x, 1.0 - p.y, p.x);
    let m = min(min(d.x, d.y), min(d.z, d.w));
    if (m == d.x) {
        return p.x * 0.25;
    }
    if (m == d.y) {
        return 0.25 + p.y * 0.25;
    }
    if (m == d.z) {
        return 0.5 + (1.0 - p.x) * 0.25;
    }
    return 0.75 + (1.0 - p.y) * 0.25;
}

// Mediets färgton men alltid starkt lysande; där mediet är svart blir det vitt.
fn bright_hue(rgb: vec3<f32>) -> vec3<f32> {
    let peak = max(max(rgb.r, rgb.g), rgb.b);
    return mix(vec3<f32>(1.0), rgb / max(peak, 0.001), smoothstep(0.02, 0.2, peak));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let q2 = u.h2 * vec3<f32>(in.p, 1.0);
    let param = q2.xy / q2.z;
    // Var på bilden punkten hamnar (lindad runt 3D-former); effekterna
    // flyttar sedan bilden, så att "Rulla" snurrar objektet.
    let tp = wrap3d(param);
    let kind = u32(u.effect.x + 0.5);
    let amount = u.effect.y;
    let phase = u.effect.z;

    var c: vec4<f32>;
    if (kind == 6u) {
        // Oskärpa: ett ringformat urval runt punkten.
        let radius = amount * 0.03;
        c = sample_param(tp);
        for (var k = 0; k < 12; k++) {
            let a = f32(k) * TAU / 12.0;
            c += sample_param(tp + vec2<f32>(cos(a), sin(a)) * radius);
            c += sample_param(tp + vec2<f32>(cos(a + 0.26), sin(a + 0.26)) * radius * 0.5);
        }
        c /= 25.0;
    } else {
        c = sample_param(warp(tp, kind, amount, phase));
    }
    var rgb = adjust(c.rgb);
    var alpha = c.a;
    switch kind {
        case 7u: {
            rgb = mix(rgb, vec3<f32>(1.0) - rgb, amount);
        }
        case 9u: {
            // Blinka: kort blixt i början av varje varv.
            let flash = select(0.0, 1.0, phase < 0.5);
            alpha *= mix(1.0, flash, amount);
        }
        case 10u: {
            // Konturglöd: bara nära kanten, plus ett ljus som springer runt.
            let d = edge_distance(param);
            let width = 0.01 + amount * 0.12;
            let glow = exp(-max(d, 0.0) / width * 2.5);
            let around = fract(atan2(param.y - 0.5, param.x - 0.5) / TAU + 0.5);
            let dist = abs(fract(around - phase + 0.5) - 0.5);
            let runner = exp(-dist * dist * 300.0);
            // Mediets färgton, men alltid starkt lysande; där mediet är svart lyser kanten vit.
            let peak = max(max(rgb.r, rgb.g), rgb.b);
            let hue = mix(vec3<f32>(1.0), rgb / max(peak, 0.001), smoothstep(0.02, 0.2, peak));
            alpha *= clamp(glow * (0.55 + 0.9 * runner), 0.0, 1.0);
            rgb = min(mix(hue, vec3<f32>(1.0), runner * 0.5), vec3<f32>(1.0));
        }
        case 11u: {
            // Jagande ljus: punkter i ett band längs kanten som springer runt.
            let d = edge_distance(param);
            let px = max(d, 0.0) / max(fwidth(d), 1e-6);
            let width = 4.0 + amount * 36.0;
            let band = 1.0 - smoothstep(width * 0.6, width, px);
            let f = fract((perimeter(param) - phase) * 24.0);
            let dot_ = 1.0 - smoothstep(0.12, 0.32, abs(f - 0.5));
            alpha *= band * (0.12 + 0.88 * dot_);
            rgb = mix(bright_hue(rgb), vec3<f32>(1.0), dot_ * 0.35);
        }
        case 12u: {
            if (shape_mode() == 5u) {
                // Ritad linje: ett lysrör – vit kärna i mitten, sken ut mot kanterna.
                let across = abs(param.y - 0.5) * 2.0;
                let core = 1.0 - smoothstep(0.1 + amount * 0.25, 0.35 + amount * 0.3, across);
                let glow = exp(-across * across * 3.0);
                let pulse = 0.8 + 0.2 * cos(phase * TAU);
                alpha *= clamp(core + glow * 0.7, 0.0, 1.0) * pulse;
                rgb = mix(bright_hue(rgb), vec3<f32>(1.0), core * 0.6);
                break;
            }
            // Neonram: skarp linje vid kanten med mjukt sken inåt, pulserar i takt.
            let d = edge_distance(param);
            let px = max(d, 0.0) / max(fwidth(d), 1e-6);
            let line = 2.0 + amount * 10.0;
            let core = 1.0 - smoothstep(line, line + 1.5, px);
            let glow = exp(-px / (6.0 + amount * 30.0));
            let pulse = 0.8 + 0.2 * cos(phase * TAU);
            alpha *= clamp(core + glow * 0.6, 0.0, 1.0) * pulse;
            rgb = mix(bright_hue(rgb), vec3<f32>(1.0), core * 0.5);
        }
        case 13u: {
            // Ljussvep: en ljusstrimma glider över mediet från vänster till höger.
            let at = phase * 1.4 - 0.2;
            let w = 0.03 + amount * 0.12;
            let x = (param.x - at) / w;
            let bar = exp(-x * x);
            rgb = min(rgb + vec3<f32>(bar), vec3<f32>(1.0));
            alpha = max(alpha, bar);
        }
        default: {}
    }
    // Ellips: kantutjämnad cirkel i ytans koordinater.
    let r = length(param - vec2<f32>(0.5)) * 2.0;
    let w = max(fwidth(r), 1e-4);
    let ellipse = select(1.0, 1.0 - smoothstep(1.0 - w, 1.0 + w, r), is_round());
    let a = alpha * u.params.x * ellipse * mask_alpha(in.pos);
    return vec4<f32>(rgb * a, a);
}
