// The floor everything stands on, in one of three styles (p.w): 0 a dark plane with a neon
// grid fading into the distance; 1 paper with faint graph lines and grain; 2 pixel grass
// fading into sky haze.

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::view

struct Floor {
    // grid colour (linear), w: intensity; for the blocky style, the sky
    color: vec4<f32>,
    // minor spacing (or pixel size), major every n minor, fade distance, style
    p: vec4<f32>,
    // base colour
    base: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> fl: Floor;

fn grid(pos: vec2<f32>, aa: vec2<f32>, spacing: f32, width: f32) -> f32 {
    let g = abs(fract(pos / spacing - 0.5) - 0.5) * spacing;
    // Thin out lines that would be under a pixel apart.
    let px = spacing / max(aa.x, aa.y);
    let fade = smoothstep(4.0, 14.0, px);
    let l = max(clamp(0.5 - (g.x - width * 0.5) / aa.x, 0.0, 1.0),
                clamp(0.5 - (g.y - width * 0.5) / aa.y, 0.0, 1.0));
    return l * fade;
}

fn hash2(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2<f32>(123.34, 456.21));
    let r = q + dot(q, q + 45.32);
    return fract(r.x * r.y);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let pos = in.world_position.xz;
    let aa = max(fwidth(pos), vec2<f32>(1e-5)) * 1.2;
    let s = fl.p.x;
    let major = fl.p.y;
    let style = i32(fl.p.w + 0.5);
    let dist = distance(view.world_position.xz, pos);
    let far = exp(-dist / fl.p.z);

    if style == 1 {
        let grain = 1.0 - 0.035 * hash2(floor(pos / 2.0));
        let minor_l = grid(pos, aa, s, s * 0.02);
        let major_l = grid(pos, aa, s * major, s * 0.04);
        var col = fl.base.rgb * grain;
        col = mix(col, fl.color.rgb, (minor_l * 0.12 + major_l * 0.22) * far);
        return vec4<f32>(col, 1.0);
    }
    if style == 2 {
        let cell = floor(pos / s);
        let shade = 0.85 + 0.25 * floor(hash2(cell) * 4.0) / 4.0;
        var col = fl.base.rgb * shade;
        // Faint seams between grass blocks.
        let vox = fract(pos / (s * major));
        if min(min(vox.x, 1.0 - vox.x), min(vox.y, 1.0 - vox.y)) * major < 1.0 {
            col = col * 0.9;
        }
        let pixel_fade = smoothstep(1.0, 3.0, s / max(aa.x, aa.y));
        col = mix(fl.base.rgb, col, pixel_fade);
        col = mix(fl.color.rgb, col, far);
        return vec4<f32>(col, 1.0);
    }

    let minor_l = grid(pos, aa, s, s * 0.025);
    let major_l = grid(pos, aa, s * major, s * 0.06);
    let neon = fl.color.rgb * fl.color.w;
    let col = fl.base.rgb + neon * (minor_l * 0.18 + major_l * 0.55) * far;
    return vec4<f32>(col, 1.0);
}
