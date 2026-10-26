// An extruded element block, in one of four styles (q.w): 0 Tron (near-black body, neon
// edges), 1 Sketch (paper with wobbly ink outlines and hatched sides), 2 Blocky (pixel
// textured voxels), 3 Blueprint (white linework drawn like walls on a floor plan). The mesh puts local top-face coordinates (x, up-the-screen) in `uv` and
// (height fraction, face: 0 top / 1 side) in `uv_b`.

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::view

struct Block {
    // Tron: neon colour (linear); Sketch: ink; Blocky: unused. w: glow intensity
    neon: vec4<f32>,
    // body colour (linear)
    body: vec4<f32>,
    // half width, half depth, corner radius, height (world units)
    dims: vec4<f32>,
    // hover, selected, shape index, opened
    p: vec4<f32>,
    // line width, grid spacing, dim, style
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
    let scale = lw / 2.2;
    let hover = b.p.x;
    let sel = b.p.y;
    let shape = i32(b.p.z + 0.5);
    let opened = b.p.w;
    let dim = b.q.z;
    let style = i32(b.q.w + 0.5);
    let wp = in.world_position.xyz;
    let n = normalize(in.world_normal);
    // Screen-space derivatives, taken up front (uniform control flow).
    let aa = max(fwidth(local.x), 1e-5) * 1.2;
    let gaa = max(fwidth(local), vec2<f32>(1e-5));
    let yaa = max(fwidth(v * h), 1e-5) * 1.2;
    let d = sd_box(local, half, r);
    let k = half.x / 160.0;

    // --- Sketch: paper and ink ------------------------------------------------------------
    if style == 1 {
        let ink = b.neon.rgb;
        let paper = b.body.rgb;
        let grain = 1.0 - 0.05 * vnoise(wp.xz / (3.0 * scale) + wp.y);
        var col = paper * grain;
        var a_ink = 0.0;
        if face < 0.5 {
            // Two wobbly strokes round the edge, as if drawn twice by hand.
            let t = (local.x * 0.7 + local.y * 1.3) / (55.0 * scale);
            let w1 = (vnoise(vec2<f32>(t, 1.7)) - 0.5) * 4.0 * scale;
            let w2 = (vnoise(vec2<f32>(t * 1.4, 8.3)) - 0.5) * 4.0 * scale;
            a_ink = max(line(d + 2.0 * scale + w1, lw * 0.85, aa), 0.75 * line(d + 4.0 * scale + w2, lw * 0.6, aa));
            // Highlighter wash when hovered or selected.
            let inside = clamp(0.5 - d / aa, 0.0, 1.0);
            col = mix(col, vec3<f32>(1.0, 0.92, 0.35), (0.35 * hover + 0.3 * sel) * inside * (1.0 - opened));
            // An opened block becomes a sheet of faint pencil graph paper.
            if opened > 0.01 {
                let s = b.q.y;
                let g = abs(fract(local / s - 0.5) - 0.5) * s;
                let gl = max(clamp(0.5 - (g.x - 0.025 * s) / gaa.x, 0.0, 1.0),
                             clamp(0.5 - (g.y - 0.025 * s) / gaa.y, 0.0, 1.0));
                col = mix(col, vec3<f32>(0.45, 0.6, 0.85), gl * 0.18 * opened * inside);
            }
            let m = marker(local, half, k, shape);
            a_ink = max(a_ink, line(m, lw * 0.75, aa) * (1.0 - opened));
        } else {
            // Sides in shadow, hatched with diagonal pen strokes.
            let side_x = select(wp.x, wp.z, abs(n.x) > abs(n.z));
            let lit = select(0.8, 0.6, abs(n.x) > abs(n.z));
            col = col * mix(1.0, 0.94, 1.0 - lit);
            let hc = (side_x + wp.y * 1.3) / (7.0 * scale);
            let hatch = clamp(0.5 - (abs(fract(hc) - 0.5) - 0.07) / max(fwidth(hc), 1e-4), 0.0, 1.0);
            a_ink = hatch * (1.0 - lit) * 0.9;
            let y = v * h;
            a_ink = max(a_ink, line(h - y - lw * 0.6, lw * 0.9, yaa));
            a_ink = max(a_ink, line(y - lw * 0.5, lw * 0.8, yaa));
        }
        col = mix(col, ink, a_ink);
        col = mix(col, vec3<f32>(0.86, 0.83, 0.77), 0.55 * dim);
        return vec4<f32>(col, 1.0);
    }

    // --- Blocky: pixel-textured voxels ----------------------------------------------------
    if style == 2 {
        let px = 4.0 * scale;
        var coord = wp.xz;
        var light = 1.0;
        if face > 0.5 {
            let side_x = select(wp.x, wp.z, abs(n.x) > abs(n.z));
            coord = vec2<f32>(side_x, wp.y);
            light = select(0.8, 0.62, abs(n.x) > abs(n.z));
        }
        let cell = floor(coord / px);
        let noise = floor(hash2(cell + vec2<f32>(f32(shape) * 7.0, 3.0)) * 4.0) / 4.0;
        var shade = 0.8 + 0.25 * noise;
        // Planks for people, cobbles for storage: a little texture per kind of thing.
        if shape == 1 {
            let row = floor(cell.y / 4.0);
            let seam = select(1.0, 0.72, fract(cell.y / 4.0) < 0.2);
            shade = shade * seam * (0.92 + 0.08 * hash2(vec2<f32>(row, 1.0)));
        } else if shape == 2 {
            let blob = step(0.62, vnoise(cell / 2.5));
            shade = shade * (1.0 - 0.18 * blob);
        }
        // Seams between voxels every 16 pixels.
        let vox = fract(coord / (px * 16.0));
        let seam = min(min(vox.x, 1.0 - vox.x), min(vox.y, 1.0 - vox.y)) * 16.0;
        if seam < 1.0 {
            shade = shade * 0.78;
        }
        // An opened block becomes a stone floor for its contents to stand out on.
        let material = mix(b.body.rgb, vec3<f32>(0.42, 0.42, 0.44), 0.7 * opened);
        var col = material * shade * light;
        if face < 0.5 {
            // A pixel icon, and a dark selection outline round a hovered block.
            let snapped = (floor(local / px) + 0.5) * px;
            let m = marker(snapped, half, k, shape);
            if m < px * 0.7 && opened < 0.5 {
                col = col * 0.35;
            }
            let rim = select(0.0, 1.0, d > -px * 1.2);
            col = mix(col, vec3<f32>(0.02), rim * max(hover, sel) * 0.85);
        }
        col = col * (1.0 - 0.5 * dim);
        return vec4<f32>(col, 1.0);
    }

    // --- Blueprint: white technical linework on blue ----------------------------------------
    if style == 3 {
        let ink = b.neon.rgb;
        var col = b.body.rgb;
        var a_ink = 0.0;
        let hot = 0.35 * hover + 0.25 * sel;
        if face < 0.5 {
            // Drawn like a wall on a plan: an outer line, a thinner inner line, and the space
            // between them hatched with fine diagonals.
            let wall = 7.0 * scale;
            let outer = line(d + lw * 0.6, lw * (1.1 + 0.8 * hot), aa);
            let inner = line(d + wall, lw * 0.55, aa);
            let in_wall = step(-wall, d) * step(d, -lw * 1.2);
            let hc = (local.x + local.y) / (3.0 * scale);
            let hatch = clamp(0.5 - (abs(fract(hc) - 0.5) - 0.06) / max(fwidth(hc), 1e-4), 0.0, 1.0);
            a_ink = max(max(outer, inner), hatch * in_wall * 0.55);
            // An opened block is a drafting sheet with a finer grid.
            if opened > 0.01 {
                let s = b.q.y;
                let g = abs(fract(local / s - 0.5) - 0.5) * s;
                let gl = max(clamp(0.5 - (g.x - 0.02 * s) / gaa.x, 0.0, 1.0),
                             clamp(0.5 - (g.y - 0.02 * s) / gaa.y, 0.0, 1.0));
                let inside = step(d, -wall);
                a_ink = max(a_ink, gl * 0.16 * opened * inside);
            }
            let m = marker(local, half, k, shape);
            a_ink = max(a_ink, line(m, lw * 0.6, aa) * (1.0 - opened));
            // A faint wash when hovered or selected.
            let inside = clamp(0.5 - d / aa, 0.0, 1.0);
            col = mix(col, ink, 0.08 * hot * inside);
        } else {
            // Elevation: crisp top and bottom edges, faint construction lines between.
            let y = v * h;
            a_ink = max(line(h - y - lw * 0.5, lw * (1.0 + 0.8 * hot), yaa), line(y - lw * 0.5, lw * 0.8, yaa));
            let side_x = select(wp.x, wp.z, abs(n.x) > abs(n.z));
            let tick = abs(fract(side_x / (20.0 * scale) - 0.5) - 0.5) * 20.0 * scale;
            a_ink = max(a_ink, clamp(0.5 - (tick - lw * 0.2) / max(fwidth(side_x), 1e-4), 0.0, 1.0) * 0.18);
            col = col * select(0.92, 0.84, abs(n.x) > abs(n.z));
        }
        col = mix(col, ink, a_ink);
        col = mix(col, vec3<f32>(0.009, 0.06, 0.24), 0.55 * dim);
        return vec4<f32>(col, 1.0);
    }

    // --- Tron: dark body, neon edges -------------------------------------------------------
    let neon = b.neon.rgb;
    let glow = b.neon.w * (1.0 + 0.9 * hover + 0.7 * sel) * (1.0 - 0.7 * dim);
    let to_eye = normalize(view.world_position.xyz - wp);
    let fres = pow(1.0 - clamp(dot(n, to_eye), 0.0, 1.0), 3.0);
    let light = normalize(vec3<f32>(0.25, 1.0, 0.45));
    var col = b.body.rgb * (0.5 + 0.5 * max(dot(n, light), 0.0));
    col += neon * fres * 0.08 * glow;

    if face < 0.5 {
        // Bright rim just inside the edge, and a glow that soaks a little way in.
        let rim = line(d + lw * 1.4, lw, aa);
        let inner = exp(d / (lw * 7.0));
        col += neon * (rim * 2.4 + inner * 0.22) * glow;
        // An opened block is a platform: a fine grid where its children stand.
        if opened > 0.01 {
            let s = b.q.y;
            let g = abs(fract(local / s - 0.5) - 0.5) * s;
            let gl = max(clamp(0.5 - (g.x - 0.03 * s) / gaa.x, 0.0, 1.0),
                         clamp(0.5 - (g.y - 0.03 * s) / gaa.y, 0.0, 1.0));
            let inside = smoothstep(-lw * 2.0, -lw * 6.0, d);
            col += neon * gl * 0.16 * opened * inside * glow;
        }
        let m = marker(local, half, k, shape);
        col += neon * line(m, lw * 0.8, aa) * 1.4 * glow * (1.0 - opened);
    } else {
        let y = v * h;
        let top_edge = line(h - y - lw * 0.7, lw * 1.2, yaa);
        let bottom_edge = line(y - lw * 0.6, lw, yaa);
        let haze = exp(-(h - y) / (lw * 10.0)) * 0.3 + exp(-y / (lw * 8.0)) * 0.18;
        col = col * 0.75 + neon * (top_edge * 2.2 + bottom_edge * 1.3 + haze) * glow;
    }
    return vec4<f32>(col, 1.0);
}
