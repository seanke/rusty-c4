// A relationship pipe: a glowing neon tube with bright pulses flowing from source to
// target. `uv.x` is distance along the pipe in pipe radii, `uv.y` goes round it.

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::{view, globals}

struct Pipe {
    // neon colour (linear), w: glow intensity
    color: vec4<f32>,
    // highlight, dim, pulse spacing (radii), pulse speed (radii per second)
    p: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> pipe: Pipe;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let hl = pipe.p.x;
    let dim = pipe.p.y;
    let glow = pipe.color.w * (1.0 + 1.2 * hl) * (1.0 - 0.75 * dim);
    let neon = pipe.color.rgb;

    let n = normalize(in.world_normal);
    let to_eye = normalize(view.world_position.xyz - in.world_position.xyz);
    let facing = clamp(dot(n, to_eye), 0.0, 1.0);
    // A hot core facing the eye, softening toward the silhouette.
    var col = neon * (0.25 + 0.85 * pow(facing, 1.5)) * glow;

    let spacing = pipe.p.z;
    let phase = fract((in.uv.x - globals.time * pipe.p.w) / spacing);
    let pulse = smoothstep(0.0, 0.04, phase) * (1.0 - smoothstep(0.04, 0.22, phase));
    col += mix(neon, vec3<f32>(1.0), 0.45) * pulse * 2.2 * glow;
    return vec4<f32>(col, 1.0);
}
