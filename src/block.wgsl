// An extruded element block, in one of four styles (q.w): 0 Tron (near-black body, neon
// edges), 1 Sketch (paper with wobbly ink outlines and hatched sides), 2 Blocky (voxel
// sandbox materials picked by colour), 3 Blueprint (white linework drawn like walls on a floor plan). The mesh puts local top-face coordinates (x, up-the-screen) in `uv` and
// (height fraction, face: 0 top / 1 side) in `uv_b`.

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::view

struct Block {
    // Tron: neon colour (linear); Sketch: ink; Blocky: x is the material id. w: glow intensity
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

// --- Blocky materials -----------------------------------------------------------------
// Original 16-pixel textures in the spirit of a voxel sandbox game, made from noise and a
// few rules. `p` is in whole pixels (on sides, x along the face and y counted down from the
// top edge); `side` picks a material's side texture; `deco` (0..1) allows surface details
// like flowers, kept off the middle of a closed block where its label is.
// Ids: 0 grass, 1 oak planks, 2 cobblestone, 3 stone, 4 sand, 5 stone bricks, 6 bricks,
// 7 prismarine, 8 purpur, 9 quartz, 10 obsidian, 11 oak log, 12 leaves, 13 cactus.

fn q4(x: f32) -> f32 {
    return floor(x * 4.0) / 4.0;
}

// The voxel a pixel belongs to, and a per-voxel random number (for block variants).
fn voxel_hash(p: vec2<f32>, salt: f32) -> f32 {
    return hash2(floor(p / 16.0) + vec2<f32>(salt, salt * 0.37));
}

fn cobble(p: vec2<f32>, seed: f32) -> vec2<f32> {
    // Jittered points every 5 pixels; returns (gap to the second-nearest, stone id).
    let c = floor(p / 5.0);
    var d1 = 1e5;
    var d2 = 1e5;
    var id = 0.0;
    for (var j = -1; j <= 1; j++) {
        for (var i = -1; i <= 1; i++) {
            let g = c + vec2<f32>(f32(i), f32(j));
            let o = vec2<f32>(hash2(g + seed), hash2(g + seed + 17.0)) * 4.0;
            let dd = length(p + 0.5 - (g * 5.0 + 0.5 + o));
            if dd < d1 {
                d2 = d1;
                d1 = dd;
                id = hash2(g + seed + 5.0);
            } else if dd < d2 {
                d2 = dd;
            }
        }
    }
    return vec2<f32>(d2 - d1, id);
}

fn bricks(p: vec2<f32>, w: f32, hgt: f32) -> vec4<f32> {
    // (mortar 0/1, brick id, x within the brick, y within the brick); rows of `hgt` pixels,
    // bricks `w` wide, every other row offset by half a brick.
    let row = floor(p.y / hgt);
    let x = p.x + select(0.0, w * 0.5, fract(row * 0.5) > 0.25);
    let bx = x - floor(x / w) * w;
    let by = p.y - row * hgt;
    let mortar = select(0.0, 1.0, by < 1.0 || bx < 1.0);
    return vec4<f32>(mortar, hash2(vec2<f32>(floor(x / w), row)), bx, by);
}

// Bare rock: stone with veins of lighter and pinker rock, and gravel pockets.
fn rock(p: vec2<f32>, n1: f32) -> vec3<f32> {
    var c = vec3<f32>(0.5, 0.5, 0.51) * (0.84 + 0.22 * q4(n1));
    let vein = vnoise(p / 26.0 + 31.0);
    let kind = vnoise(p / 90.0 + 7.0);
    if vein > 0.72 {
        if kind < 0.33 {
            // granite: pink-brown, speckled
            c = vec3<f32>(0.6, 0.43, 0.37) * (0.85 + 0.25 * q4(hash2(p + 3.3)));
        } else if kind < 0.66 {
            // diorite: pale, with dark flecks
            c = select(vec3<f32>(0.76, 0.76, 0.77), vec3<f32>(0.42), hash2(p + 8.1) > 0.82) * (0.92 + 0.1 * q4(n1));
        } else {
            // andesite: mid grey, mottled
            c = vec3<f32>(0.56, 0.56, 0.55) * (0.82 + 0.26 * q4(hash2(p + 1.7)));
        }
    } else if vein < 0.12 {
        // gravel: loose stones of mixed shades
        c = vec3<f32>(0.48, 0.45, 0.44) * (0.65 + 0.55 * q4(hash2(p + 9.4)));
    }
    return c;
}

// Ore flecks in rock, if this pixel has any: rarer and richer the deeper it is.
// Returns (colour, 1 if ore).
fn ore(p: vec2<f32>, depth: f32, deep: bool) -> vec4<f32> {
    // Whole ore blocks: some voxels carry a few clumps of flecks.
    let block = floor(p / 16.0);
    if hash2(block + 71.0) > 0.11 {
        return vec4<f32>(0.0);
    }
    let cell = floor(p / 5.0);
    if hash2(cell + 12.0) > 0.6 {
        return vec4<f32>(0.0);
    }
    // A clump of a few pixels round a jittered centre.
    let c = cell * 5.0 + 1.0 + vec2<f32>(hash2(cell + 2.0), hash2(cell + 5.0)) * 3.0;
    let r = length(p + 0.5 - c);
    let speck = hash2(p + 13.0);
    if r > 1.8 || (r > 1.1 && speck < 0.45) {
        return vec4<f32>(0.0);
    }
    let pick = hash2(block + 33.0);
    var lit = vec3<f32>(0.14);
    var dark = vec3<f32>(0.06);
    if deep {
        if pick < 0.3 {
            lit = vec3<f32>(0.45, 0.95, 0.9);
            dark = vec3<f32>(0.16, 0.6, 0.66);
        } else if pick < 0.55 {
            lit = vec3<f32>(1.0, 0.2, 0.12);
            dark = vec3<f32>(0.58, 0.02, 0.02);
        } else if pick < 0.75 {
            lit = vec3<f32>(0.25, 0.4, 0.95);
            dark = vec3<f32>(0.1, 0.17, 0.55);
        } else if pick < 0.9 {
            lit = vec3<f32>(1.0, 0.86, 0.3);
            dark = vec3<f32>(0.72, 0.52, 0.08);
        } else {
            lit = vec3<f32>(0.3, 0.95, 0.5);
            dark = vec3<f32>(0.04, 0.55, 0.22);
        }
    } else {
        let rich = clamp(depth / 24.0, 0.0, 1.0) * 0.25;
        if pick < 0.42 - rich {
            lit = vec3<f32>(0.22);
            dark = vec3<f32>(0.07);
        } else if pick < 0.68 - rich {
            lit = vec3<f32>(0.88, 0.72, 0.6);
            dark = vec3<f32>(0.62, 0.44, 0.34);
        } else if pick < 0.86 - rich * 0.5 {
            lit = vec3<f32>(0.85, 0.5, 0.3);
            dark = vec3<f32>(0.32, 0.62, 0.5);
        } else if pick < 0.95 {
            lit = vec3<f32>(1.0, 0.86, 0.3);
            dark = vec3<f32>(0.72, 0.52, 0.08);
        } else {
            lit = vec3<f32>(0.25, 0.4, 0.95);
            dark = vec3<f32>(0.1, 0.17, 0.55);
        }
    }
    return vec4<f32>(select(dark, lit, speck > 0.5 && r < 1.3), 1.0);
}

// Deepslate: dark blue-grey rock in horizontal streaks.
fn deepslate(p: vec2<f32>, n1: f32) -> vec3<f32> {
    let streak = q4(hash2(vec2<f32>(floor(p.x / 3.0), p.y) + 4.0));
    return vec3<f32>(0.3, 0.3, 0.34) * (0.78 + 0.22 * streak + 0.08 * q4(n1));
}

// The grass top: a gently varying green with darker blades, and flowers where allowed.
fn grass_top(p: vec2<f32>, n1: f32, deco: f32) -> vec3<f32> {
    let tint = q4(vnoise(p / 90.0 + 2.0));
    let base = mix(vec3<f32>(0.34, 0.58, 0.21), vec3<f32>(0.45, 0.66, 0.24), tint);
    var c = base * (0.82 + 0.3 * q4(n1));
    // Patches of bare dirt and path, here and there.
    let worn = vnoise(p / 26.0 + 50.0);
    if worn > 0.89 && deco > 0.5 {
        return vec3<f32>(0.55, 0.4, 0.27) * (0.82 + 0.26 * q4(n1));
    }
    if hash2(p + 0.7) > 0.93 {
        c = base * 0.62;
    }
    if deco > 0.5 {
        // A flower in some 5-pixel cells: four petals round a centre, seen from above.
        let cell = floor(p / 5.0);
        let fh = hash2(cell + 91.0);
        if fh < 0.07 {
            let at = cell * 5.0 + 1.0 + floor(vec2<f32>(hash2(cell + 3.0), hash2(cell + 4.0)) * 3.0);
            let o = p - at;
            let kind = hash2(cell + 7.0);
            var petal = vec3<f32>(1.0, 0.88, 0.2);
            if kind < 0.25 {
                petal = vec3<f32>(0.9, 0.12, 0.1);
            } else if kind < 0.45 {
                petal = vec3<f32>(0.95, 0.95, 0.92);
            } else if kind < 0.6 {
                petal = vec3<f32>(0.35, 0.6, 1.0);
            } else if kind < 0.72 {
                petal = vec3<f32>(0.95, 0.55, 0.85);
            } else if kind < 0.82 {
                petal = vec3<f32>(1.0, 0.55, 0.15);
            }
            if all(o == vec2<f32>(0.0)) {
                return select(vec3<f32>(1.0, 0.85, 0.2), vec3<f32>(0.25, 0.18, 0.08), kind < 0.6 && kind > 0.25);
            }
            if abs(o.x) + abs(o.y) == 1.0 {
                return petal;
            }
        }
    }
    return c;
}

fn texel(mat: i32, p: vec2<f32>, side: bool, deco: f32, rows: f32) -> vec3<f32> {
    let n1 = hash2(p + f32(mat) * 31.7);
    let n2 = hash2(p * 1.31 + 9.1 + f32(mat));
    let u = p - floor(p / 16.0) * 16.0;
    let vh = voxel_hash(p, f32(mat));
    if mat == 0 {
        if !side {
            return grass_top(p, n1, deco);
        }
        // A cliff: grass hanging over the top in a ragged fringe, a band of dirt, rock
        // studded with ore, and deepslate further down. Each boundary wanders a little.
        let fringe = 2.0 + floor(hash2(vec2<f32>(p.x, 3.0)) * 2.6);
        if p.y < fringe {
            return vec3<f32>(0.34, 0.57, 0.21) * (0.78 + 0.32 * q4(n1));
        }
        let dirt_to = 6.0 + floor(vnoise(vec2<f32>(p.x / 4.0, 1.0)) * 3.0);
        if p.y < dirt_to {
            var dirt = vec3<f32>(0.53, 0.38, 0.26) * (0.82 + 0.26 * q4(n1));
            if n2 > 0.9 {
                dirt = vec3<f32>(0.38, 0.27, 0.18);
            } else if n2 < 0.06 {
                dirt = vec3<f32>(0.62, 0.48, 0.34);
            }
            return dirt;
        }
        // Bedrock along the very bottom of a tall cliff.
        let bed = rows - 1.0 - floor(hash2(vec2<f32>(p.x, 11.0)) * 3.0);
        if rows > 40.0 && p.y >= bed {
            return vec3<f32>(0.22) * (0.5 + 0.9 * q4(n1));
        }
        let deep_at = max(22.0, rows * 0.55) + floor(vnoise(vec2<f32>(p.x / 6.0, 9.0)) * 4.0);
        let deep = p.y >= deep_at;
        let o = ore(p, p.y, deep);
        if o.w > 0.5 {
            return o.rgb;
        }
        if deep {
            return deepslate(p, n1);
        }
        return rock(p, n1);
    }
    if mat == 1 {
        // Planks four pixels tall with dark seams, butt joints staggered, a little grain
        // and the odd knot. Some blocks are a paler or a darker wood.
        let row = floor(p.y / 4.0);
        let joint = select(0.0, 8.0, fract(row * 0.5) > 0.25) + floor(hash2(vec2<f32>(row, 2.0)) * 2.0) * 4.0;
        let along = p.x - joint;
        let seam = fract(p.y / 4.0) * 4.0 > 2.5 || (along - floor(along / 16.0) * 16.0) < 1.0;
        let grain = 0.88 + 0.16 * q4(hash2(vec2<f32>(floor(p.x / 3.0), p.y)));
        var wood = vec3<f32>(0.66, 0.52, 0.32);
        var gap = vec3<f32>(0.44, 0.34, 0.2);
        if vh < 0.12 {
            wood = vec3<f32>(0.8, 0.72, 0.52);
            gap = vec3<f32>(0.6, 0.53, 0.37);
        } else if vh < 0.22 {
            wood = vec3<f32>(0.46, 0.33, 0.2);
            gap = vec3<f32>(0.3, 0.21, 0.12);
        }
        if seam {
            return gap;
        }
        if n2 > 0.985 {
            return gap * 0.9;
        }
        return wood * grain;
    }
    if mat == 2 || mat == 10 {
        let c = cobble(p, f32(mat));
        if mat == 10 {
            // Obsidian: near-black with purple glints; some blocks weep bright tears.
            var o = vec3<f32>(0.07, 0.05, 0.11) * (0.8 + 0.5 * q4(c.y));
            if n1 > 0.93 {
                o = vec3<f32>(0.36, 0.22, 0.5);
            }
            if vh < 0.15 && n2 > 0.9 {
                o = vec3<f32>(0.75, 0.35, 1.0);
            }
            return o;
        }
        if c.x < 0.9 {
            return vec3<f32>(0.3, 0.3, 0.3) * (0.85 + 0.2 * n2);
        }
        var stone = vec3<f32>(0.47 + 0.16 * q4(c.y) + 0.06 * (q4(n1) - 0.5));
        // Moss creeping over some of the stones.
        let moss = vnoise(p / 9.0 + 40.0);
        if moss > 0.6 && hash2(p + 2.2) < (moss - 0.6) * 4.0 {
            stone = vec3<f32>(0.36, 0.5, 0.24) * (0.8 + 0.3 * q4(n1));
        }
        // A lit upper-left on each stone.
        return stone * select(1.0, 1.1, c.x < 1.6 && n2 > 0.5);
    }
    if mat == 3 {
        let o = ore(p, 12.0, false);
        if o.w > 0.5 && side {
            return o.rgb;
        }
        return rock(p, n1);
    }
    if mat == 4 {
        // Sand, and here and there a block of layered sandstone.
        if vh < 0.15 {
            let band = fract(u.y / 4.0) * 4.0 < 1.0;
            return vec3<f32>(0.85, 0.79, 0.58) * select(0.95 + 0.07 * q4(n1), 0.84, band);
        }
        return vec3<f32>(0.86, 0.81, 0.6) * (0.9 + 0.13 * q4(n1));
    }
    if mat == 5 {
        let br = bricks(p, 8.0, 4.0);
        if br.x > 0.5 {
            return vec3<f32>(0.32, 0.32, 0.33);
        }
        var c = vec3<f32>(0.5 + 0.08 * (br.y - 0.5)) * (0.9 + 0.14 * q4(n1));
        // Some bricks are cracked, some mossy.
        if br.y < 0.12 {
            let crack = abs(br.z - 1.5 - br.w * 1.4 - floor(br.y * 30.0) % 3.0);
            if crack < 0.6 {
                c = vec3<f32>(0.3);
            }
        } else if br.y > 0.86 && hash2(p + 6.1) > 0.35 {
            c = vec3<f32>(0.37, 0.5, 0.26) * (0.85 + 0.25 * q4(n1));
        }
        // Chiseled blocks: a carved ring.
        if vh < 0.06 {
            let e = min(min(u.x, u.y), min(15.0 - u.x, 15.0 - u.y));
            c = vec3<f32>(0.55) * select(1.0, 0.68, e == 0.0 || e == 3.0) * (0.92 + 0.1 * q4(n1));
        }
        return c;
    }
    if mat == 6 {
        let br = bricks(p, 8.0, 4.0);
        if br.x > 0.5 {
            return vec3<f32>(0.62, 0.6, 0.57) * (0.9 + 0.1 * n2);
        }
        return vec3<f32>(0.58, 0.28, 0.21) * (0.85 + 0.22 * br.y) * (0.92 + 0.1 * q4(n1));
    }
    if mat == 7 {
        // Prismarine, with blocks of prismarine bricks, dark prismarine and the odd
        // glowing sea lantern.
        if vh < 0.025 {
            // Sea lantern: a pale glowing block with darker frosted corners.
            let cq = abs(u - 7.5);
            let corner = cq.x > 4.0 && cq.y > 4.0;
            let frame = min(min(u.x, u.y), min(15.0 - u.x, 15.0 - u.y)) < 1.0;
            let glow = vec3<f32>(0.88, 0.98, 0.95) * (0.95 + 0.08 * q4(n1));
            return select(glow * 1.1, vec3<f32>(0.6, 0.78, 0.74), corner || frame);
        }
        if vh < 0.18 {
            let br = bricks(p, 8.0, 4.0);
            return select(vec3<f32>(0.42, 0.68, 0.62) * (0.9 + 0.12 * q4(n1)), vec3<f32>(0.26, 0.45, 0.42), br.x > 0.5);
        }
        if vh < 0.26 {
            let line = u.x == 0.0 || u.y == 0.0 || u.y == 8.0;
            return select(vec3<f32>(0.2, 0.36, 0.32) * (0.85 + 0.2 * q4(n1)), vec3<f32>(0.12, 0.24, 0.22), line);
        }
        let blot = q4(vnoise(p / 3.5 + 4.0));
        let c = mix(vec3<f32>(0.34, 0.6, 0.55), vec3<f32>(0.3, 0.48, 0.58), blot);
        return c * (0.88 + 0.16 * q4(n1));
    }
    if mat == 8 {
        // Purpur: 8-pixel tiles with a dark border and a lighter middle; some blocks are
        // fluted pillars.
        let base = vec3<f32>(0.66, 0.48, 0.66) * (0.9 + 0.12 * q4(n1));
        if vh < 0.18 {
            let flute = u.x - floor(u.x / 4.0) * 4.0;
            return base * select(1.04, 0.74, flute == 0.0);
        }
        let t = u - floor(u / 8.0) * 8.0;
        let edge = min(min(t.x, t.y), min(7.0 - t.x, 7.0 - t.y));
        if edge < 1.0 {
            return base * 0.72;
        }
        return base * select(1.0, 1.06, edge > 1.5);
    }
    if mat == 11 {
        // Oak log: rough vertical bark on the sides, growth rings on the cut top.
        if side {
            let ridge = q4(hash2(vec2<f32>(p.x, floor(p.y / 3.0)) + 5.0));
            return vec3<f32>(0.42, 0.31, 0.19) * (0.75 + 0.35 * ridge);
        }
        let r = floor(length(u - 7.5) / 1.6);
        return select(vec3<f32>(0.68, 0.54, 0.33), vec3<f32>(0.52, 0.4, 0.24), r % 2.0 == 0.0) * (0.92 + 0.1 * q4(n1));
    }
    if mat == 13 {
        // Cactus: ribbed green sides with pale spines, a lighter cut top.
        if side {
            let rib = p.x - floor(p.x / 4.0) * 4.0;
            var c = vec3<f32>(0.3, 0.52, 0.18) * select(1.0, 0.72, rib == 0.0) * (0.9 + 0.12 * q4(n1));
            if rib == 2.0 && n2 > 0.8 {
                c = vec3<f32>(0.85, 0.85, 0.7);
            }
            return c;
        }
        let e = min(min(u.x, u.y), min(15.0 - u.x, 15.0 - u.y));
        return vec3<f32>(0.42, 0.62, 0.26) * select(1.0, 0.8, e < 2.0) * (0.92 + 0.1 * q4(n1));
    }
    if mat == 12 {
        // Leaves: dark green clumps with deep gaps between them, and the odd apple.
        let clump = q4(hash2(floor(p / 2.0) + 3.0));
        var c = vec3<f32>(0.24, 0.46, 0.16) * (0.72 + 0.45 * clump);
        if n1 > 0.82 {
            c = vec3<f32>(0.1, 0.22, 0.07);
        }
        if vh < 0.25 && n2 > 0.992 {
            c = vec3<f32>(0.85, 0.12, 0.1);
        }
        return c;
    }
    // Quartz: smooth and pale with a faint bevel; some blocks chiseled, some pillars.
    let edge = min(min(u.x, u.y), min(15.0 - u.x, 15.0 - u.y));
    var c = vec3<f32>(0.92, 0.89, 0.84) * (0.95 + 0.06 * q4(n1));
    if vh < 0.12 {
        return c * select(1.0, 0.82, edge == 2.0 || edge == 5.0);
    }
    if vh < 0.24 {
        let line = u.x - floor(u.x / 5.0) * 5.0;
        return c * select(1.0, 0.86, line == 0.0);
    }
    return c * select(1.0, 0.9, edge < 1.0);
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

    // --- Blocky: Minecraft-like voxels -----------------------------------------------------
    if style == 2 {
        // An opened block takes on its children's pixel size, so the whole world shares one
        // voxel grid at the level you are looking at.
        let px = select(4.0 * scale, b.q.y / 5.0, opened > 0.5);
        let mat = i32(b.neon.x + 0.5);
        // Pixel coordinates: world-aligned on top, counted down from the top edge on the
        // sides (so side textures, like grass's fringe, hang from the top).
        var p = floor(wp.xz / px);
        var light = 1.0;
        let side = face > 0.5;
        if side {
            let side_x = select(wp.x, wp.z, abs(n.x) > abs(n.z));
            p = vec2<f32>(floor(side_x / px), floor(h * (1.0 - v) / px));
            light = select(0.8, 0.6, abs(n.x) > abs(n.z));
        }
        // Where texture pixels shrink below screen pixels, read coarser ones so the surface
        // doesn't fizz with noise.
        let on_screen = px / max(length(fwidth(wp)), 1e-6);
        let lod = select(select(1.0, 4.0, on_screen < 1.2), 16.0, on_screen < 0.3);
        p = floor(p / lod) * lod + floor(lod * 0.5);
        // Surface details stay off the middle of a closed block, where its label is.
        let in_label = abs(local.x) < half.x * 0.85 && abs(local.y) < half.y * 0.55;
        let deco = select(1.0, 0.0, in_label && opened < 0.5);
        var col = texel(mat, p, side, deco, floor(h / px));
        if face < 0.5 {
            // A pixel icon for the element's shape, and the thin black selection box.
            let snapped = (floor(local / px) + 0.5) * px;
            let m = marker(snapped, half, k, shape);
            if m < px * 0.7 && opened < 0.5 {
                col = col * 0.4;
            }
            let rim = select(0.0, 1.0, d > -px * 0.8);
            col = mix(col, vec3<f32>(0.0), rim * max(hover, sel) * 0.9);
        } else {
            let y = v * h;
            let edge = select(0.0, 1.0, h - y < px * 0.6 || y < px * 0.6);
            col = mix(col, vec3<f32>(0.0), edge * max(hover, sel) * 0.9);
            // A soft shadow where the side meets the ground.
            col = col * mix(0.72, 1.0, clamp(y / (px * 3.0), 0.0, 1.0));
        }
        col = col * light * (1.0 - 0.5 * dim);
        // The textures are authored in sRGB; the target wants linear light.
        return vec4<f32>(pow(col, vec3<f32>(2.2)), 1.0);
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
