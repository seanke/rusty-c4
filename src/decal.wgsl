// Painted text lying on a surface: a premultiplied texture, tinted (a little over 1 so it
// glows), optionally on a rounded pill backing.

#import bevy_pbr::forward_io::VertexOutput

struct Decal {
    // rgb: tint (HDR), a: opacity
    tint: vec4<f32>,
    // pill backing colour; a = 0 for none
    bg: vec4<f32>,
    // width / height of the decal, pill border alpha, unused, unused
    p: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> decal: Decal;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var samp: sampler;

fn sd_box(p: vec2<f32>, half: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - half + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let t = textureSample(tex, samp, in.uv);
    var rgb = t.rgb * decal.tint.rgb;
    var a = t.a;
    if decal.bg.a > 0.0 {
        let aspect = decal.p.x;
        let p = (in.uv - vec2<f32>(0.5)) * vec2<f32>(aspect, 1.0);
        let half = vec2<f32>(aspect * 0.5, 0.5);
        let d = sd_box(p, half, 0.5);
        let aa = max(fwidth(p.y), 1e-5);
        let cov = clamp(0.5 - d / aa, 0.0, 1.0);
        let edge = clamp(0.5 - (abs(d + aa) - aa * 0.6) / aa, 0.0, 1.0) * decal.p.y;
        let bg = mix(decal.bg.rgb, vec3<f32>(0.2, 0.9, 1.0), edge);
        let bga = max(decal.bg.a * cov, edge);
        rgb = rgb + bg * bga * (1.0 - a);
        a = a + bga * (1.0 - a);
    }
    return vec4<f32>(rgb, a) * decal.tint.a;
}
