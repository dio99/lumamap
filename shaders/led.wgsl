// LED-slingor: läser projektorbildens färg där varje lysdiod ligger.
// Ett medel över en liten ruta gör att lysdioden inte fladdrar på enstaka pixlar.

struct Params {
    count: u32,
};

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var<storage, read> points: array<vec2<f32>>;
@group(0) @binding(2) var<storage, read_write> colors: array<u32>;
@group(0) @binding(3) var<uniform> params: Params;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if (i >= params.count) {
        return;
    }
    let size = vec2<i32>(textureDimensions(src));
    let p = vec2<i32>(clamp(points[i], vec2<f32>(0.0), vec2<f32>(1.0)) * vec2<f32>(size - vec2<i32>(1)));
    var c = vec3<f32>(0.0);
    for (var dy = -2; dy <= 2; dy++) {
        for (var dx = -2; dx <= 2; dx++) {
            let q = clamp(p + vec2<i32>(dx * 2, dy * 2), vec2<i32>(0), size - vec2<i32>(1));
            c += textureLoad(src, q, 0).rgb;
        }
    }
    colors[i] = pack4x8unorm(vec4<f32>(c / 25.0, 1.0));
}
