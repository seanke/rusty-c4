// A relationship pipe in one of four styles (q.x): 0 a glowing neon tube with bright
// pulses; 1 an inked outline on paper with ink dashes flowing; 2 a flat trail of redstone
// dust with the signal pulsing along it; 3 a white technical outline on blueprint blue;
// 4 a plain grey line that turns editor blue when highlighted; 5 a road with lane markings.
// `uv.x` is distance along the pipe in pipe radii, `uv.y` goes round it.

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::{view, globals}

struct Pipe {
    // neon colour (linear), w: glow intensity
    color: vec4<f32>,
    // highlight, dim, pulse spacing (radii), pulse speed (radii per second)
    p: vec4<f32>,
    // style
    q: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> pipe: Pipe;

fn hash1(x: f32) -> f32 {
    return fract(sin(x * 127.1) * 43758.5453);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let hl = pipe.p.x;
    let dim = pipe.p.y;
    let style = i32(pipe.q.x + 0.5);
    let n = normalize(in.world_normal);
    let to_eye = normalize(view.world_position.xyz - in.world_position.xyz);
    let facing = clamp(dot(n, to_eye), 0.0, 1.0);
    let spacing = pipe.p.z;
    let phase = fract((in.uv.x - globals.time * pipe.p.w) / spacing);

    if style == 1 || style == 3 {
        // An outlined tube: lines along the silhouette, paper inside, dashes flowing. Ink on
        // paper for the sketch; white on blue for the blueprint.
        var ink = mix(vec3<f32>(0.1, 0.11, 0.18), vec3<f32>(0.8, 0.18, 0.12), hl);
        var paper = vec3<f32>(0.97, 0.95, 0.9);
        var faded = vec3<f32>(0.86, 0.83, 0.77);
        if style == 3 {
            ink = mix(vec3<f32>(0.9, 0.95, 1.0), vec3<f32>(1.0, 0.85, 0.3), hl);
            paper = vec3<f32>(0.006, 0.033, 0.147);
            faded = vec3<f32>(0.04, 0.16, 0.48);
        }
        let edge = 1.0 - smoothstep(0.28, 0.45, facing);
        let dash = step(phase, 0.22) * smoothstep(0.55, 0.75, facing);
        var col = mix(paper, ink, max(edge, dash * 0.85));
        col = mix(col, faded, 0.6 * dim);
        return vec4<f32>(col, 1.0);
    }
    if style == 2 {
        // Redstone dust: a dull red trail with darker grains, lit up in bright pulses as the
        // signal runs along it.
        let cell = floor(in.uv.x * 2.0);
        let across = floor(in.uv.y * 8.0);
        let grain = hash1(cell * 3.1 + across * 17.0);
        var col = vec3<f32>(0.36, 0.02, 0.01) * (0.7 + 0.45 * floor(grain * 3.0) / 3.0);
        let signal = step(fract((cell / 2.0 - globals.time * pipe.p.w) / spacing), 0.18);
        let lit = max(signal, 0.55 * hl);
        col = mix(col, vec3<f32>(1.0, 0.12, 0.04) * (0.8 + 0.3 * grain), lit);
        col = col * select(0.75, 1.0, n.y > 0.5);
        col = col * (1.0 - 0.6 * dim);
        return vec4<f32>(col, 1.0);
    }
    if style == 5 {
        // A road: grainy asphalt with white edge lines and a dashed yellow centre line,
        // seen on the top of the flat strip. `across` runs 0..1 over each face of it.
        let across = fract(in.uv.y * 4.0);
        let grain = hash1(floor(in.uv.x * 6.0) * 13.0 + floor(across * 12.0));
        var col = vec3<f32>(0.25, 0.26, 0.28) * (0.9 + 0.15 * grain);
        if n.y > 0.5 {
            let edge = across < 0.07 || across > 0.93;
            let centre = abs(across - 0.5) < 0.025 && fract(in.uv.x / 3.0) < 0.55;
            col = select(col, vec3<f32>(0.92, 0.92, 0.9), edge);
            col = select(col, mix(vec3<f32>(0.98, 0.8, 0.2), vec3<f32>(1.0, 0.95, 0.6), hl), centre);
        } else {
            col = vec3<f32>(0.58, 0.58, 0.56);
        }
        col = mix(col, vec3<f32>(0.2, 0.45, 0.85), 0.25 * hl);
        col = mix(col, vec3<f32>(0.5, 0.62, 0.42), 0.6 * dim);
        return vec4<f32>(pow(col, vec3<f32>(2.2)), 1.0);
    }
    if style == 4 {
        // Flat grey, a little shading so it reads as round; blue when highlighted, with a
        // soft lighter pulse running along it.
        var col = mix(vec3<f32>(0.4, 0.42, 0.45), vec3<f32>(0.22, 0.58, 1.0), hl) * (0.72 + 0.28 * facing);
        let pulse = smoothstep(0.0, 0.04, phase) * (1.0 - smoothstep(0.04, 0.16, phase));
        col = mix(col, vec3<f32>(0.31, 0.76, 1.0), pulse * 0.75);
        col = mix(col, vec3<f32>(0.118), 0.65 * dim);
        return vec4<f32>(pow(col, vec3<f32>(2.2)), 1.0);
    }

    let glow = pipe.color.w * (1.0 + 1.2 * hl) * (1.0 - 0.75 * dim);
    let neon = pipe.color.rgb;
    // A hot core facing the eye, softening toward the silhouette.
    var col = neon * (0.25 + 0.85 * pow(facing, 1.5)) * glow;
    let pulse = smoothstep(0.0, 0.04, phase) * (1.0 - smoothstep(0.04, 0.22, phase));
    col += mix(neon, vec3<f32>(1.0), 0.45) * pulse * 2.2 * glow;
    return vec4<f32>(col, 1.0);
}
