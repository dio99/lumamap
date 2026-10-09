// YUV → RGB på GPU:n. Video kommer som luminans (Y) i full upplösning och
// färg (U, V) i halv; resultatet skrivs till källans RGBA-textur så att resten
// av renderaren och editorns förhandsvisning inte behöver veta om formatet.
//
//   NV12: plan_a = UV växelvis (rg), plan_b används inte.
//   I420: plan_a = U, plan_b = V.

struct YuvUniform {
    // rgb = M · (yuv − offset), M som tre kolumner.
    m0: vec4<f32>,
    m1: vec4<f32>,
    m2: vec4<f32>,
    offset: vec4<f32>,
    // x = 0 för NV12, 1 för I420
    mode: vec4<f32>,
};

@group(0) @binding(0) var plane_y: texture_2d<f32>;
@group(0) @binding(1) var plane_a: texture_2d<f32>;
@group(0) @binding(2) var plane_b: texture_2d<f32>;
@group(0) @binding(3) var samp: sampler;
@group(0) @binding(4) var<uniform> u: YuvUniform;

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

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let y = textureSample(plane_y, samp, in.uv).r;
    let a = textureSample(plane_a, samp, in.uv);
    let b = textureSample(plane_b, samp, in.uv).r;
    let uv = select(a.rg, vec2<f32>(a.r, b), u.mode.x > 0.5);
    let yuv = vec3<f32>(y, uv) - u.offset.xyz;
    let m = mat3x3<f32>(u.m0.xyz, u.m1.xyz, u.m2.xyz);
    return vec4<f32>(clamp(m * yuv, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
