// The floor everything stands on: a dark plane with a neon grid that fades into the
// distance, finer lines fading out as they get too dense to read.

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::view

struct Floor {
    // grid colour (linear), w: intensity
    color: vec4<f32>,
    // minor spacing, major every n minor, fade distance, unused
    p: vec4<f32>,
    // base colour
    base: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> fl: Floor;

fn grid(pos: vec2<f32>, spacing: f32, width: f32) -> f32 {
    let g = abs(fract(pos / spacing - 0.5) - 0.5) * spacing;
    let aa = max(fwidth(pos), vec2<f32>(1e-5)) * 1.2;
    // Thin out lines that would be under a pixel apart.
    let px = spacing / max(aa.x, aa.y);
    let fade = smoothstep(4.0, 14.0, px);
    let l = max(clamp(0.5 - (g.x - width * 0.5) / aa.x, 0.0, 1.0),
                clamp(0.5 - (g.y - width * 0.5) / aa.y, 0.0, 1.0));
    return l * fade;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let pos = in.world_position.xz;
    let s = fl.p.x;
    let major = fl.p.y;
    let minor_l = grid(pos, s, s * 0.025);
    let major_l = grid(pos, s * major, s * 0.06);
    let dist = distance(view.world_position.xz, pos);
    let far = exp(-dist / fl.p.z);
    let neon = fl.color.rgb * fl.color.w;
    var col = fl.base.rgb + neon * (minor_l * 0.18 + major_l * 0.55) * far;
    return vec4<f32>(col, 1.0);
}
