// Mönster som ritas på GPU:n varje bildruta, till en källas textur.
// Rör sig i takt med projektets tempo.

struct GenUniform {
    // x = mönster (Pattern), y = tid i taktslag × fart, zw = bildförhållande (bredd/höjd, 1)
    params: vec4<f32>,
    color_a: vec4<f32>,
    color_b: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: GenUniform;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    let x = f32((vi << 1u) & 2u);
    let y = f32(vi & 2u);
    var out: VsOut;
    out.clip = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, y);
    return out;
}

const TAU: f32 = 6.2831853;

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let s = f * f * (3.0 - 2.0 * f);
    let a = hash(i);
    let b = hash(i + vec2<f32>(1.0, 0.0));
    let c = hash(i + vec2<f32>(0.0, 1.0));
    let d = hash(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, s.x), mix(c, d, s.x), s.y);
}

fn fbm(p: vec2<f32>) -> f32 {
    var v = 0.0;
    var amp = 0.5;
    var q = p;
    for (var k = 0; k < 5; k++) {
        v += amp * noise(q);
        q = q * 2.03 + vec2<f32>(1.7, 9.2);
        amp *= 0.5;
    }
    return v;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let kind = u32(u.params.x + 0.5);
    let t = u.params.y;
    // Koordinater med rätt proportioner, origo i mitten.
    let p = (in.uv - vec2<f32>(0.5)) * vec2<f32>(u.params.z, 1.0);
    let a = u.color_a.rgb;
    let b = u.color_b.rgb;
    var mixv = 0.0;
    var glow = 0.0;
    switch kind {
        case 0u: {
            // Gradient som sveper diagonalt.
            mixv = 0.5 + 0.5 * sin((p.x + p.y) * 3.0 - t * TAU);
        }
        case 1u: {
            // Ränder som glider, med mjuka kanter.
            let s = fract((p.x * 0.7 + p.y * 0.3) * 6.0 - t);
            mixv = smoothstep(0.4, 0.5, s) - smoothstep(0.9, 1.0, s);
        }
        case 2u: {
            let tt = t * TAU * 0.5;
            let v = sin(p.x * 6.0 + tt) + sin(p.y * 7.0 - tt * 1.3) + sin((p.x + p.y) * 5.0 + tt * 0.7)
                + sin(length(p) * 10.0 - tt * 1.7);
            mixv = 0.5 + 0.5 * sin(v * 1.6);
        }
        case 3u: {
            // Tunnel: ringar som strömmar mot betraktaren.
            let r = max(length(p), 0.001);
            let ang = atan2(p.y, p.x) / TAU;
            let depth = 0.25 / r + t;
            let check = step(0.5, fract(depth * 2.0)) != step(0.5, fract(ang * 8.0));
            mixv = select(0.0, 1.0, check);
            glow = smoothstep(0.0, 0.6, r);
        }
        case 4u: {
            let q = p * 3.0 + vec2<f32>(t * 0.6, t * 0.25);
            mixv = smoothstep(0.25, 0.8, fbm(q + fbm(q + vec2<f32>(t * 0.2, 0.0))));
        }
        default: {
            // Ringar som pulserar ut från mitten.
            let r = length(p);
            let wave = fract(r * 3.0 - t);
            mixv = smoothstep(0.0, 0.15, wave) * (1.0 - smoothstep(0.15, 0.5, wave));
        }
    }
    var rgb = mix(a, b, mixv);
    if (kind == 3u) {
        rgb *= glow;
    }
    return vec4<f32>(rgb, 1.0);
}
