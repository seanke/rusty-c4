// Signed-distance card: rounded body (or a LikeC4 shape), soft drop shadow, optional
// "stack" plate for elements that have children, hover glow and a crisp border.
// Everything is anti-aliased with screen-space derivatives so it stays sharp at any zoom.

#import bevy_sprite::mesh2d_vertex_output::VertexOutput

struct Card {
    fill: vec4<f32>,
    border: vec4<f32>,
    glow: vec4<f32>,
    // w, h, corner radius, border width
    dims: vec4<f32>,
    // opacity, shape index, shadow alpha, stacked alpha
    p: vec4<f32>,
    // quad padding, shape mix (0 = plain rect, 1 = full shape), container (slab top), unused
    q: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> card: Card;

fn sd_box(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let rr = min(r, min(b.x, b.y));
    let q = abs(p) - b + vec2<f32>(rr);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - rr;
}

// Exact distance to an ellipse (Inigo Quilez). The cheap approximation drifts far from
// the edge, which made glows and shadows bulge oddly around cylinders and queues.
fn sd_ellipse(p_in: vec2<f32>, ab_in: vec2<f32>) -> f32 {
    var p = abs(p_in);
    var ab = ab_in;
    if p.x > p.y {
        p = p.yx;
        ab = ab.yx;
    }
    let l = ab.y * ab.y - ab.x * ab.x;
    if abs(l) < 1e-3 {
        return length(p) - ab.x;
    }
    let m = ab.x * p.x / l;
    let m2 = m * m;
    let n = ab.y * p.y / l;
    let n2 = n * n;
    let c = (m2 + n2 - 1.0) / 3.0;
    let c3 = c * c * c;
    let q = c3 + m2 * n2 * 2.0;
    let d = c3 + m2 * n2;
    let g = m + m * n2;
    var co: f32;
    if d < 0.0 {
        let h = acos(clamp(q / c3, -1.0, 1.0)) / 3.0;
        let s = cos(h);
        let t = sin(h) * sqrt(3.0);
        let rx = sqrt(max(-c * (s + t + 2.0) + m2, 0.0));
        let ry = sqrt(max(-c * (s - t + 2.0) + m2, 0.0));
        co = (ry + sign(l) * rx + abs(g) / max(rx * ry, 1e-6) - m) / 2.0;
    } else {
        let h = 2.0 * m * n * sqrt(d);
        let s = sign(q + h) * pow(abs(q + h), 1.0 / 3.0);
        let u = sign(q - h) * pow(abs(q - h), 1.0 / 3.0);
        let rx = -s - u - c * 4.0 + 2.0 * m2;
        let ry = (s - u) * sqrt(3.0);
        let rm = sqrt(rx * rx + ry * ry);
        co = (ry / sqrt(max(rm - rx, 1e-6)) + 2.0 * g / max(rm, 1e-6) - m) / 2.0;
    }
    co = clamp(co, 0.0, 1.0);
    let r = ab * vec2<f32>(co, sqrt(1.0 - co * co));
    return length(r - p) * sign(p.y - r.y);
}

fn smin(a: f32, b: f32, k: f32) -> f32 {
    let h = clamp(0.5 + 0.5 * (b - a) / k, 0.0, 1.0);
    return mix(b, a, h) - k * h * (1.0 - h);
}

fn over(dst: vec4<f32>, src: vec4<f32>) -> vec4<f32> {
    let a = src.a + dst.a * (1.0 - src.a);
    if a <= 0.00001 {
        return vec4<f32>(0.0);
    }
    let rgb = (src.rgb * src.a + dst.rgb * dst.a * (1.0 - src.a)) / a;
    return vec4<f32>(rgb, a);
}

fn coverage(d: f32, aa: f32) -> f32 {
    return clamp(0.5 - d / aa, 0.0, 1.0);
}

// x: body distance, y: distance to inner decoration strokes (lids, toolbars, notches)
fn shape_sdf(p: vec2<f32>, half: vec2<f32>, r: f32, shape: i32) -> vec2<f32> {
    let rect = sd_box(p, half, r);
    var d = rect;
    var deco = 1e5;
    if shape == 1 {
        // person: a head resting on the card
        let head = length(p - vec2<f32>(0.0, half.y + 4.0)) - 30.0;
        d = smin(rect, head, 12.0);
    } else if shape == 2 {
        // cylinder / storage
        let eh = min(16.0, half.y * 0.25);
        let body = sd_box(p, vec2<f32>(half.x, half.y - eh), 0.0);
        let top_c = vec2<f32>(0.0, half.y - eh);
        let top = sd_ellipse(p - top_c, vec2<f32>(half.x, eh));
        let bot = sd_ellipse(p + top_c, vec2<f32>(half.x, eh));
        d = min(body, min(top, bot));
        deco = abs(top);
    } else if shape == 3 {
        // queue: a cylinder on its side
        let ew = min(18.0, half.x * 0.15);
        let body = sd_box(p, vec2<f32>(half.x - ew, half.y), 0.0);
        let rc = vec2<f32>(half.x - ew, 0.0);
        let right = sd_ellipse(p - rc, vec2<f32>(ew, half.y));
        let left = sd_ellipse(p + rc, vec2<f32>(ew, half.y));
        d = min(body, min(left, right));
        deco = abs(right);
    } else if shape == 4 {
        // browser: toolbar line and three dots
        let bar = abs(p.y - (half.y - 26.0));
        var dots = 1e5;
        for (var i = 0; i < 3; i++) {
            let c = vec2<f32>(-half.x + 20.0 + f32(i) * 14.0, half.y - 13.0);
            dots = min(dots, length(p - c) - 4.0);
        }
        deco = min(bar, max(dots, 0.0));
    } else if shape == 5 {
        // mobile: rounder corners and a speaker notch
        d = sd_box(p, half, max(r, 28.0));
        deco = max(sd_box(p - vec2<f32>(0.0, half.y - 14.0), vec2<f32>(24.0, 2.5), 2.5), 0.0);
    }
    return vec2<f32>(d, deco);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let size = card.dims.xy;
    let half = size * 0.5;
    let r = card.dims.z;
    let pad = card.q.x;
    let mix_shape = card.q.y;
    let shape = i32(card.p.y + 0.5);

    let full = size + vec2<f32>(2.0 * pad);
    let p = (in.uv - vec2<f32>(0.5)) * full * vec2<f32>(1.0, -1.0);
    let aa = max(fwidth(p.x), 1e-4);
    let bw = max(card.dims.w, aa);

    let rect_d = sd_box(p, half, r);
    let sd = shape_sdf(p, half, r, shape);
    let d = mix(rect_d, sd.x, mix_shape);

    var col = vec4<f32>(0.0);

    // Soft effects (shadow, glow) fade to nothing before the quad's edge so they never clip.
    let edge = min(full.x * 0.5 - abs(p.x), full.y * 0.5 - abs(p.y));
    let soft = smoothstep(0.0, pad * 0.55, edge);

    // soft drop shadow
    let so = vec2<f32>(0.0, -10.0);
    let sdist = mix(sd_box(p - so, half, r), shape_sdf(p - so, half, r, shape).x, mix_shape);
    let shadow = card.p.z * (1.0 - smoothstep(-18.0, 34.0, sdist)) * 0.5 * soft;
    col = over(col, vec4<f32>(0.0, 0.0, 0.02, shadow));

    // a second card peeking out behind: "there is more inside"
    if card.p.w > 0.001 {
        let po = p - vec2<f32>(9.0, -9.0);
        let pd = sd_box(po, half, r);
        let plate = vec4<f32>(card.fill.rgb * 0.55, card.fill.a * card.p.w * coverage(pd, aa));
        col = over(col, plate);
        let pb = abs(pd + bw * 0.5) - bw * 0.5;
        col = over(col, vec4<f32>(card.border.rgb, card.border.a * 0.5 * card.p.w * coverage(pb, aa)));
    }

    // hover glow
    if card.glow.a > 0.001 {
        let g = card.glow.a * exp(-max(d, 0.0) / 16.0) * (1.0 - coverage(d, aa)) * soft;
        col = over(col, vec4<f32>(card.glow.rgb, g));
    }

    // body
    let inside = coverage(d, aa);
    col = over(col, vec4<f32>(card.fill.rgb, card.fill.a * inside));

    // subtle top highlight to give the card some depth
    let sheen = clamp((p.y / half.y) * 0.5 + 0.5, 0.0, 1.0);
    col = over(col, vec4<f32>(1.0, 1.0, 1.0, 0.05 * sheen * sheen * inside * card.fill.a));

    // decoration strokes
    let deco = coverage(sd.y - bw * 0.5, aa) * inside * mix_shape;
    col = over(col, vec4<f32>(card.border.rgb, card.border.a * 0.8 * deco));

    // border
    let bd = abs(d + bw * 0.5) - bw * 0.5;
    col = over(col, vec4<f32>(card.border.rgb, card.border.a * coverage(bd, aa)));

    // Containers: the top of a solid slab. A lit lip along the top edge, a fine bevel just
    // inside the border, and a quiet header strip with a hairline under the title.
    let slab = card.q.z;
    if slab > 0.001 {
        let depth_in = -d;
        let uvn = p / max(half, vec2<f32>(1.0));
        let bevel_w = 7.0;
        let bevel = coverage(abs(depth_in - bevel_w) - 0.4 * bw, aa) * inside;
        col = over(col, vec4<f32>(1.0, 1.0, 1.0, 0.06 * bevel * slab));
        let lip = coverage(bd, aa) * smoothstep(0.1, 0.95, uvn.y);
        col = over(col, vec4<f32>(1.0, 1.0, 1.0, 0.35 * lip * slab));
        let header_h = 62.0;
        let in_header = step(half.y - header_h, p.y) * step(bevel_w, depth_in);
        col = over(col, vec4<f32>(1.0, 1.0, 1.0, 0.025 * in_header * slab));
        let fade = 1.0 - smoothstep(-0.2, 0.9, uvn.x);
        let hairline = coverage(abs(p.y - (half.y - header_h)) - 0.5 * bw * 0.6, aa)
            * step(bevel_w + 14.0, depth_in);
        col = over(col, vec4<f32>(1.0, 1.0, 1.0, 0.14 * hairline * fade * slab));
    }

    return vec4<f32>(col.rgb, col.a * card.p.x);
}
