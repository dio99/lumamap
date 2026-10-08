// Kantblandning: ett sista helskärmspass som multiplicerar utgångens bild
// med en ramp mot kanterna där projektorer överlappar.
//
// Rampen är smoothstep i linjärt ljus, s(t) + s(1 − t) = 1, så två
// projektorer som överlappar summerar till jämn ljusstyrka. Bilden är kodad
// med projektorns gamma, så faktorn kodas likadant: pow(s, 1/gamma).
// Samma beräkning finns i `EdgeBlend::linear_factor` (lm-core) och testas där.

struct EdgeUniform {
    // Bredd som andel av bilden: x = vänster, y = höger, z = topp, w = botten.
    widths: vec4<f32>,
    // x = gamma
    params: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: EdgeUniform;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// En triangel som täcker hela bilden.
@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    let x = f32((vi << 1u) & 2u);
    let y = f32(vi & 2u);
    var out: VsOut;
    out.clip = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, y);
    return out;
}

fn ramp(d: f32, w: f32) -> f32 {
    if (w <= 0.0) {
        return 1.0;
    }
    return smoothstep(0.0, 1.0, clamp(d / w, 0.0, 1.0));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let p = in.uv;
    let linear = ramp(p.x, u.widths.x) * ramp(1.0 - p.x, u.widths.y) * ramp(p.y, u.widths.z) * ramp(1.0 - p.y, u.widths.w);
    let f = pow(linear, 1.0 / max(u.params.x, 0.1));
    return vec4<f32>(f, f, f, 1.0);
}
