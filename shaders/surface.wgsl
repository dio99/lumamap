// En mappad yta. Fyra hörn på utgången (dst) ritas som två trianglar.
// Texturkoordinaten räknas fram per pixel med homografin `h` (utgång → källa),
// vilket ger korrekt perspektiv utan synlig diagonal söm.

struct SurfaceUniform {
    h: mat3x3<f32>,
    dst01: vec4<f32>,
    dst23: vec4<f32>,
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
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    var corners = array<u32, 6>(0u, 1u, 2u, 0u, 2u, 3u);
    var dst = array<vec2<f32>, 4>(u.dst01.xy, u.dst01.zw, u.dst23.xy, u.dst23.zw);
    let p = dst[corners[vi]];
    var out: VsOut;
    out.clip = vec4<f32>(p.x * 2.0 - 1.0, 1.0 - p.y * 2.0, 0.0, 1.0);
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
