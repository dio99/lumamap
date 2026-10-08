// En mappad yta, ritad som trianglar från en vertexbuffer.
//
// Varje vertex har sin position på utgången (`pos`) och en punkt `p` som
// homografin `h` avbildar på källan. Texturkoordinaten räknas fram per pixel,
// vilket ger korrekt perspektiv utan synlig diagonal söm:
//   Fyrhörn: p = pos, h = utgång → källa.
//   Mesh:    p = (u, v) i meshen, h = enhetskvadrat → källans utsnitt.

struct SurfaceUniform {
    h: mat3x3<f32>,
    // x = opacitet
    params: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: SurfaceUniform;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) p: vec2<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec2<f32>, @location(1) p: vec2<f32>) -> VsOut {
    var out: VsOut;
    out.clip = vec4<f32>(pos.x * 2.0 - 1.0, 1.0 - pos.y * 2.0, 0.0, 1.0);
    out.p = p;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let q = u.h * vec3<f32>(in.p, 1.0);
    let uv = q.xy / q.z;
    let c = textureSampleLevel(tex, samp, uv, 0.0);
    // Utanför källans utsnitt blir det genomskinligt.
    let inside = step(0.0, uv.x) * step(uv.x, 1.0) * step(0.0, uv.y) * step(uv.y, 1.0);
    return vec4<f32>(c.rgb, c.a * u.params.x * inside);
}
