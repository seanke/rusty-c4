// An extruded element block in the Tron style: a near-black body with neon edges. The top
// face carries a bright rim, a soft inner glow and a small shape marker; an opened block (a
// platform for its children) gets a fine neon grid instead. Sides glow along their top and
// bottom edges. The mesh puts local top-face coordinates (x, up-the-screen) in `uv` and
// (height fraction, face: 0 top / 1 side) in `uv_b`.

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::view

struct Block {
    // neon colour (linear), w: glow intensity
    neon: vec4<f32>,
    // body colour (linear)
    body: vec4<f32>,
    // half width, half depth, corner radius, height (world units)
    dims: vec4<f32>,
    // hover, selected, shape index, opened
    p: vec4<f32>,
    // line width, grid spacing, dim, unused
    q: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> b: Block;

fn sd_box(p: vec2<f32>, half: vec2<f32>, r: f32) -> f32 {
    let rr = min(r, min(half.x, half.y));
    let q = abs(p) - half + vec2<f32>(rr);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - rr;
}

fn line(d: f32, w: f32, aa: f32) -> f32 {
    return clamp(0.5 - (abs(d) - w * 0.5) / aa, 0.0, 1.0);
}

// Distance to a small marker for the element's shape, drawn in the top-left corner.
fn marker(p: vec2<f32>, half: vec2<f32>, k: f32, shape: i32) -> f32 {
    let c = vec2<f32>(-half.x + 30.0 * k, half.y - 24.0 * k);
    let q = p - c;
    if shape == 1 {
        // person: head and shoulders
        let head = abs(length(q - vec2<f32>(0.0, 4.0 * k)) - 4.0 * k);
        let body = abs(length((q + vec2<f32>(0.0, 9.0 * k)) * vec2<f32>(1.0, 1.4)) - 8.0 * k);
        return min(head, max(body, -(q.y + 6.0 * k)));
    } else if shape == 2 {
        // storage: stacked discs
        let e = vec2<f32>(1.0, 3.2);
        let a = abs(length((q - vec2<f32>(0.0, 5.0 * k)) * e) - 9.0 * k);
        let m = abs(length(q * e) - 9.0 * k);
        let z = abs(length((q + vec2<f32>(0.0, 5.0 * k)) * e) - 9.0 * k);
        return min(a, min(m, z)) / 1.6;
    } else if shape == 3 {
        // queue: a pipe with a ring at its end
        let tube = abs(abs(q.y) - 5.0 * k);
        let along = max(tube, abs(q.x) - 10.0 * k);
        let ring = abs(length((q - vec2<f32>(10.0 * k, 0.0)) * vec2<f32>(2.0, 1.0)) - 10.0 * k) / 2.0;
        return min(along, ring);
    } else if shape == 4 {
        // browser: a toolbar across the top with three dots
        let bar = abs(p.y - (half.y - 34.0 * k));
        var dots = 1e5;
        for (var i = 0; i < 3; i++) {
            dots = min(dots, abs(length(p - vec2<f32>(-half.x + (20.0 + f32(i) * 13.0) * k, half.y - 17.0 * k)) - 3.0 * k));
        }
        return min(bar, dots);
    } else if shape == 5 {
        // mobile: a speaker notch
        let n = sd_box(p - vec2<f32>(0.0, half.y - 16.0 * k), vec2<f32>(22.0 * k, 2.0 * k), 2.0 * k);
        return abs(n);
    }
    return 1e5;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let local = in.uv;
    let v = in.uv_b.x;
    let face = in.uv_b.y;
    let half = b.dims.xy;
    let r = b.dims.z;
    let h = b.dims.w;
    let lw = b.q.x;
    let hover = b.p.x;
    let sel = b.p.y;
    let shape = i32(b.p.z + 0.5);
    let opened = b.p.w;
    let dim = b.q.z;
    let neon = b.neon.rgb;
    let glow = b.neon.w * (1.0 + 0.9 * hover + 0.7 * sel) * (1.0 - 0.7 * dim);

    let n = normalize(in.world_normal);
    let to_eye = normalize(view.world_position.xyz - in.world_position.xyz);
    let fres = pow(1.0 - clamp(dot(n, to_eye), 0.0, 1.0), 3.0);
    let light = normalize(vec3<f32>(0.25, 1.0, 0.45));
    var col = b.body.rgb * (0.5 + 0.5 * max(dot(n, light), 0.0));
    col += neon * fres * 0.08 * glow;

    if face < 0.5 {
        let d = sd_box(local, half, r);
        let aa = max(fwidth(local.x), 1e-5) * 1.2;
        // Bright rim just inside the edge, and a glow that soaks a little way in.
        let rim = line(d + lw * 1.4, lw, aa);
        let inner = exp(d / (lw * 7.0));
        col += neon * (rim * 2.4 + inner * 0.22) * glow;
        // An opened block is a platform: a fine grid where its children stand.
        if opened > 0.01 {
            let s = b.q.y;
            let g = abs(fract(local / s - 0.5) - 0.5) * s;
            let gaa = max(fwidth(local), vec2<f32>(1e-5));
            let gl = max(clamp(0.5 - (g.x - 0.03 * s) / gaa.x, 0.0, 1.0),
                         clamp(0.5 - (g.y - 0.03 * s) / gaa.y, 0.0, 1.0));
            let inside = smoothstep(-lw * 2.0, -lw * 6.0, d);
            col += neon * gl * 0.16 * opened * inside * glow;
        }
        let k = half.x / 160.0;
        let m = marker(local, half, k, shape);
        col += neon * line(m, lw * 0.8, aa) * 1.4 * glow * (1.0 - opened);
    } else {
        let y = v * h;
        let aa = max(fwidth(y), 1e-5) * 1.2;
        let top_edge = line(h - y - lw * 0.7, lw * 1.2, aa);
        let bottom_edge = line(y - lw * 0.6, lw, aa);
        let haze = exp(-(h - y) / (lw * 10.0)) * 0.3 + exp(-y / (lw * 8.0)) * 0.18;
        col = col * 0.75 + neon * (top_edge * 2.2 + bottom_edge * 1.3 + haze) * glow;
    }
    return vec4<f32>(col, 1.0);
}
