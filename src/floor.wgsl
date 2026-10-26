// The floor everything stands on, in one of four styles (p.w): 0 a dark plane with a neon
// grid fading into the distance; 1 paper with faint graph lines and grain; 2 rippling pixel
// water fading into sky haze; 3 blueprint blue with a white drafting grid.

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::{view, globals}

struct Floor {
    // grid colour (linear), w: intensity; for the blocky style, the sky
    color: vec4<f32>,
    // minor spacing (or pixel size), major every n minor, fade distance, style
    p: vec4<f32>,
    // base colour
    base: vec4<f32>,
    // blocky style: island footprints (centre x, z, half size x, z), and how many (n.x)
    islands: array<vec4<f32>, 48>,
    n: vec4<f32>,
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

fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash2(i), hash2(i + vec2<f32>(1.0, 0.0)), u.x),
               mix(hash2(i + vec2<f32>(0.0, 1.0)), hash2(i + vec2<f32>(1.0, 1.0)), u.x), u.y);
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
    if style == 3 {
        // Cyanotype: slightly mottled blue, a fine grid and a stronger one every few squares.
        let mottle = 1.0 + 0.06 * (hash2(floor(pos / 160.0)) - 0.5) + 0.03 * (hash2(floor(pos / 9.0)) - 0.5);
        let minor_l = grid(pos, aa, s, s * 0.03);
        let major_l = grid(pos, aa, s * major, s * 0.08);
        var col = fl.base.rgb * mottle;
        col = mix(col, fl.color.rgb, (minor_l * 0.16 + major_l * 0.34) * far);
        return vec4<f32>(col, 1.0);
    }
    if style == 2 {
        // Pixel water: two slow noise layers drifting past each other, posterised into a
        // few shades of blue, with the odd bright glint, fading into sky haze far off.
        // Round each island: a sandy beach, a line of foam, and shallows that lighten
        // toward the shore, with lily pads floating in them.
        let cell = floor(pos / s);
        let t = globals.time;
        let centre = (cell + 0.5) * s;
        var shore = 1e9;
        for (var i = 0; i < i32(fl.n.x); i++) {
            let r = fl.islands[i];
            let q = abs(centre - r.xy) - r.zw;
            shore = min(shore, length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0));
        }
        // In pixels, with a ragged edge.
        let dp = shore / s + (vnoise(cell / 4.0 + 3.0) - 0.5) * 3.0;
        let shade = hash2(cell + 1.3);
        if dp < 3.0 {
            let sand = vec3<f32>(0.86, 0.8, 0.58) * (0.9 + 0.13 * floor(shade * 4.0) / 4.0) * select(1.0, 0.86, dp > 2.0);
            return vec4<f32>(mix(fl.color.rgb, pow(sand, vec3<f32>(2.2)), far), 1.0);
        }
        if dp < 4.2 && hash2(cell + floor(t * 3.0) * 1.7) > 0.35 {
            return vec4<f32>(mix(fl.color.rgb, pow(vec3<f32>(0.86, 0.93, 1.0), vec3<f32>(2.2)), far), 1.0);
        }
        let a = vnoise(cell / 9.0 + vec2<f32>(t * 0.35, t * 0.12));
        let b = vnoise(cell / 5.0 - vec2<f32>(t * 0.2, t * 0.31) + 13.0);
        let w = floor((a * 0.6 + b * 0.4) * 5.0) / 5.0;
        var col = fl.base.rgb * (0.66 + 0.6 * w);
        let glint = step(0.985, hash2(cell + floor(t * 2.0) * 7.31)) * step(0.55, w);
        col = mix(col, fl.base.rgb * 1.6 + 0.08, glint * 0.6);
        if dp < 26.0 {
            let shallow = floor((1.0 - dp / 26.0) * 4.0) / 4.0;
            let lagoon = pow(vec3<f32>(0.26, 0.6, 0.86), vec3<f32>(2.2)) * (0.75 + 0.5 * w);
            col = mix(col, lagoon, shallow * 0.9);
            // Lily pads: a round pad with a notch, now and then with a flower.
            let pad_cell = floor(cell / 5.0);
            if dp > 6.0 && dp < 20.0 && hash2(pad_cell + 21.0) < 0.06 {
                let c = pad_cell * 5.0 + 2.0 + floor(vec2<f32>(hash2(pad_cell + 1.0), hash2(pad_cell + 2.0)) * 1.5);
                let o = cell - c;
                let notch = o.x > 0.0 && abs(o.y) < 0.5;
                if length(o) < 1.7 && !notch {
                    var pad = vec3<f32>(0.2, 0.5, 0.14) * (0.85 + 0.25 * shade);
                    if all(o == vec2<f32>(0.0)) && hash2(pad_cell + 5.0) < 0.3 {
                        pad = vec3<f32>(0.98, 0.6, 0.8);
                    }
                    col = pow(pad, vec3<f32>(2.2));
                }
            }
        }
        let pixel_fade = smoothstep(1.0, 3.0, s / max(aa.x, aa.y));
        col = mix(fl.base.rgb * 0.95, col, pixel_fade);
        col = mix(fl.color.rgb, col, far);
        return vec4<f32>(col, 1.0);
    }

    let minor_l = grid(pos, aa, s, s * 0.025);
    let major_l = grid(pos, aa, s * major, s * 0.06);
    let neon = fl.color.rgb * fl.color.w;
    let col = fl.base.rgb + neon * (minor_l * 0.18 + major_l * 0.55) * far;
    return vec4<f32>(col, 1.0);
}
