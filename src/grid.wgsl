// Background: a soft vignette with a two-level dot grid that fades smoothly between levels
// as you zoom.

#import bevy_sprite::mesh2d_vertex_output::VertexOutput

struct Grid {
    base: vec4<f32>,
    center: vec4<f32>,
    dot: vec4<f32>,
    // spacing, dot radius in px, unused, unused
    params: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> grid: Grid;

fn dots(world: vec2<f32>, spacing: f32, radius_px: f32, aa: f32) -> f32 {
    let f = (fract(world / spacing) - vec2<f32>(0.5)) * spacing;
    let d = length(f) - radius_px * aa;
    return clamp(0.5 - d / aa, 0.0, 1.0);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let world = in.world_position.xy;
    let aa = max(fwidth(world.x), 1e-4);

    let v = length((in.uv - vec2<f32>(0.5)) * vec2<f32>(1.0, 0.8));
    var col = mix(grid.center.rgb, grid.base.rgb, smoothstep(0.0, 0.75, v));

    let s1 = grid.params.x;
    let s2 = s1 * 5.0;
    let s3 = s2 * 5.0;
    let r = grid.params.y;
    let f1 = smoothstep(9.0, 22.0, s1 / aa);
    let f2 = smoothstep(9.0, 22.0, s2 / aa);
    let f3 = smoothstep(9.0, 22.0, s3 / aa);
    let a = max(max(dots(world, s1, r, aa) * f1, dots(world, s2, r * 1.15, aa) * f2 * 1.3), dots(world, s3, r * 1.3, aa) * f3 * 1.6);
    col = mix(col, grid.dot.rgb, clamp(a * grid.dot.a, 0.0, 1.0));
    return vec4<f32>(col, 1.0);
}
