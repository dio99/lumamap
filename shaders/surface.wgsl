// En mappad yta, ritad som trianglar från en vertexbuffer.
//
// Varje vertex har sin position på utgången (`pos`) och en punkt `p` som
// homografin `h` avbildar på källan. Texturkoordinaten räknas fram per pixel,
// vilket ger korrekt perspektiv utan synlig diagonal söm:
//   Fyrhörn: p = pos, h = utgång → källa.
//   Mesh:    p = (u, v) i meshen, h = enhetskvadrat → källans utsnitt.
//
// En valfri polygonmask (i utgångens koordinater) tonar ut ytan med mjuk kant.

struct SurfaceUniform {
    h: mat3x3<f32>,
    // x = opacitet, y = antal maskpunkter (0 = ingen mask), z = mjuk kant (px), w = 1 om inverterad
    params: vec4<f32>,
    // xy = utgångens upplösning i pixlar
    res: vec4<f32>,
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

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let q = u.h * vec3<f32>(in.p, 1.0);
    let uv = q.xy / q.z;
    let c = textureSampleLevel(tex, samp, uv, 0.0);
    // Utanför källans utsnitt blir det genomskinligt.
    let inside = step(0.0, uv.x) * step(uv.x, 1.0) * step(0.0, uv.y) * step(uv.y, 1.0);
    return vec4<f32>(c.rgb, c.a * u.params.x * inside * mask_alpha(in.pos));
}
