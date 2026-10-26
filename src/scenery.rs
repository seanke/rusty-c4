//! The blocky style's scenery: beaches and shallows round the islands (drawn by the floor
//! shader from their footprints), little islets out in the sea with trees or a cactus, and
//! blocky clouds drifting past. None of it can be clicked; it's there to make the world feel lived in.

use bevy::prelude::*;

use crate::render::{BlockMaterial, BlockUniform, FloorMaterial, MAX_ISLANDS};
use crate::scene::Diagram;
use crate::theme::{Style, Theme};
use crate::world::{prism, Geometry, View, BLOCK_H};

/// Everything spawned here, so it can all be cleared at once.
#[derive(Component)]
pub struct Scenery;

/// A cloud swaying slowly back and forth along x about where it was put.
#[derive(Component)]
pub struct Cloud {
    home: Vec3,
    reach: f32,
    period: f32,
    phase: f32,
}

/// What the scenery was last built for, and the islets it made.
#[derive(Default)]
pub struct Built {
    key: Option<(bool, u64, [i32; 4])>,
    islets: Vec<Rect>,
}

/// A small deterministic random sequence, so the same diagram always gets the same scenery.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next()
    }
}

/// A block of one material: `mat` as in `block.wgsl`, textured at `px` world units a pixel.
fn block(mat: f32, size: Vec2, height: f32, px: f32) -> BlockUniform {
    BlockUniform {
        neon: Vec4::new(mat, 0.0, 0.0, 0.0),
        body: Vec4::ONE,
        dims: Vec4::new(size.x * 0.5, size.y * 0.5, 0.3, height),
        // "Opened", so it's textured at the pixel size given (and gets flowers all over).
        p: Vec4::new(0.0, 0.0, 0.0, 1.0),
        q: Vec4::new(2.2, px * 5.0, 0.0, Style::Blocky.index()),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    time: Res<Time>,
    theme: Res<Theme>,
    diagram: Res<Diagram>,
    view: Res<View>,
    geometry: Res<Geometry>,
    floor: Query<&MeshMaterial3d<FloorMaterial>>,
    mut floors: ResMut<Assets<FloorMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut blocks: ResMut<Assets<BlockMaterial>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    scenery: Query<Entity, With<Scenery>>,
    mut clouds: Query<(&Cloud, &mut Transform)>,
    mut built: Local<Built>,
) {
    let on = theme.style == Style::Blocky && diagram.loaded;
    let b = geometry.root_bounds;
    let key = (on, diagram.resets, [b.min.x as i32, b.min.y as i32, b.max.x as i32, b.max.y as i32]);
    if built.key != Some(key) {
        built.key = Some(key);
        for e in &scenery {
            commands.entity(e).despawn();
        }
        built.islets.clear();
        if on && b.width() > 0.0 {
            spawn(&mut commands, &mut meshes, &mut blocks, &mut standard, b, &mut built.islets);
        }
    }

    // The islands the sea needs to know about: the top-level blocks, and the islets.
    let mut islands: Vec<Vec4> = Vec::new();
    if on {
        let model = &diagram.model;
        for (&id, g) in &geometry.geos {
            if model.elements.get(id).is_some_and(|e| e.parent.is_none()) && view.visible.contains(&id) {
                islands.push(Vec4::new(g.center.x, g.center.y, g.size.x * 0.5, g.size.y * 0.5));
            }
        }
        islands.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
        for r in &built.islets {
            islands.push(Vec4::new(r.center().x, r.center().y, r.half_size().x, r.half_size().y));
        }
        islands.truncate(MAX_ISLANDS);
    }
    for m in &floor {
        let Some(f) = floors.get(&m.0) else { continue };
        let n = islands.len() as f32;
        if f.u.n.x == n && f.u.islands[..islands.len()] == islands[..] {
            continue;
        }
        if let Some(f) = floors.get_mut(&m.0) {
            f.u.islands = [Vec4::ZERO; MAX_ISLANDS];
            f.u.islands[..islands.len()].copy_from_slice(&islands);
            f.u.n.x = n;
        }
    }

    let t = time.elapsed_secs();
    for (c, mut tf) in &mut clouds {
        let x = (t / c.period + c.phase).sin() * c.reach;
        tf.translation = c.home + Vec3::X * x;
    }
}

fn spawn(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    blocks: &mut Assets<BlockMaterial>,
    standard: &mut Assets<StandardMaterial>,
    bounds: Rect,
    islets: &mut Vec<Rect>,
) {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15 ^ (bounds.width() as u64).wrapping_mul(2654435761) ^ bounds.height() as u64);
    let reach = bounds.size().max_element();
    let keep_clear = bounds.inflate(140.0);

    // Islets: grass (sometimes sand) tufts out in the sea, most with a tree or two.
    let wanted = 10;
    let mut tries = 0;
    while islets.len() < wanted && tries < 400 {
        tries += 1;
        // Spread round the compass: each islet tries for its own slice first.
        let slot = (islets.len() as f32 + rng.range(0.15, 0.85) + (tries / 40) as f32 * 0.37) / wanted as f32;
        let ang = slot * std::f32::consts::TAU;
        let dir = Vec2::new(ang.cos(), ang.sin());
        let out = rng.range(220.0, 260.0 + reach * 0.45);
        // From the edge of the diagram's footprint in that direction, out a way.
        let half = bounds.half_size();
        let to_edge = (half.x / dir.x.abs().max(1e-3)).min(half.y / dir.y.abs().max(1e-3));
        let at = bounds.center() + dir * (to_edge + out);
        let size = Vec2::new(rng.range(1.0, 3.2).round() * 48.0, rng.range(1.0, 2.6).round() * 48.0);
        let rect = Rect::from_center_size(at, size);
        if !rect.intersect(keep_clear).is_empty() || islets.iter().any(|r| !r.inflate(70.0).intersect(rect).is_empty()) {
            continue;
        }
        islets.push(rect);
        let sandy = rng.next() < 0.25;
        let height = BLOCK_H * rng.range(0.8, 1.6);
        commands.spawn((
            Mesh3d(meshes.add(prism(size, 0.3))),
            MeshMaterial3d(blocks.add(BlockMaterial {
                u: block(if sandy { 4.0 } else { 0.0 }, size, height, 4.0),
            })),
            Transform::from_xyz(at.x, 0.0, at.y).with_scale(Vec3::new(1.0, height, 1.0)),
            Scenery,
        ));
        if sandy {
            if rng.next() < 0.75 {
                let spot = at + Vec2::new(rng.range(-0.25, 0.25), rng.range(-0.25, 0.25)) * size;
                let h = rng.range(26.0, 46.0);
                commands.spawn((
                    Mesh3d(meshes.add(prism(Vec2::splat(12.0), 0.2))),
                    MeshMaterial3d(blocks.add(BlockMaterial { u: block(13.0, Vec2::splat(12.0), h, 1.5) })),
                    Transform::from_xyz(spot.x, height, spot.y).with_scale(Vec3::new(1.0, h, 1.0)),
                    Scenery,
                ));
            }
            continue;
        }
        let trees = if size.x * size.y > 48.0 * 48.0 * 3.0 { 2 } else { 1 };
        for i in 0..trees {
            if rng.next() < 0.2 {
                continue;
            }
            let inset = (size * 0.5 - 26.0).max(Vec2::ZERO);
            let spot = at + Vec2::new(rng.range(-inset.x, inset.x), rng.range(-inset.y, inset.y)) * if i == 0 { 0.5 } else { 1.0 };
            tree(commands, meshes, blocks, Vec3::new(spot.x, height, spot.y), &mut rng);
        }
    }

    // Clouds: flat white slabs high up, kept off the diagram itself.
    let cloud = standard.add(StandardMaterial {
        base_color: Color::srgba(1.0, 1.0, 1.0, 0.82),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    let cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let mut placed: Vec<Rect> = Vec::new();
    let mut tries = 0;
    while placed.len() < 9 && tries < 300 {
        tries += 1;
        let ang = rng.range(0.0, std::f32::consts::TAU);
        let dir = Vec2::new(ang.cos(), ang.sin());
        let half = bounds.half_size();
        let to_edge = (half.x / dir.x.abs().max(1e-3)).min(half.y / dir.y.abs().max(1e-3));
        let at = bounds.center() + dir * (to_edge + rng.range(500.0, 700.0 + reach));
        let size = Vec2::new(rng.range(140.0, 320.0), rng.range(80.0, 170.0));
        let sway = rng.range(40.0, 120.0);
        let rect = Rect::from_center_size(at, size + Vec2::new(sway * 2.0, 0.0));
        if !rect.intersect(bounds.inflate(260.0)).is_empty() || placed.iter().any(|r| !r.inflate(60.0).intersect(rect).is_empty()) {
            continue;
        }
        placed.push(rect);
        let y = rng.range(230.0, 330.0);
        let home = Vec3::new(at.x, y, at.y);
        commands
            .spawn((
                Transform::from_translation(home),
                Visibility::default(),
                Cloud {
                    home,
                    reach: sway,
                    period: rng.range(18.0, 34.0),
                    phase: rng.range(0.0, std::f32::consts::TAU),
                },
                Scenery,
            ))
            .with_children(|c| {
                // A main slab and a couple of smaller ones, stepped like a pixel cloud.
                c.spawn((Mesh3d(cube.clone()), MeshMaterial3d(cloud.clone()), Transform::from_scale(Vec3::new(size.x, 16.0, size.y))));
                for _ in 0..2 {
                    let s = Vec2::new(size.x * rng.range(0.3, 0.55), size.y * rng.range(0.35, 0.6));
                    let o = (size - s) * 0.5 * Vec2::new(rng.range(-1.0, 1.0), rng.range(-1.0, 1.0));
                    c.spawn((
                        Mesh3d(cube.clone()),
                        MeshMaterial3d(cloud.clone()),
                        Transform::from_xyz(o.x, 0.0, o.y + size.y * 0.5 * rng.range(-0.4, 0.4)).with_scale(Vec3::new(s.x, 16.2, s.y + 30.0)),
                    ));
                }
            });
    }
}

/// An oak: a log with a two-tier canopy of leaves, standing at `base`.
fn tree(commands: &mut Commands, meshes: &mut Assets<Mesh>, blocks: &mut Assets<BlockMaterial>, base: Vec3, rng: &mut Rng) {
    let trunk_h = rng.range(44.0, 64.0);
    let wide = rng.range(40.0, 52.0).round();
    let parts = [
        (11.0, Vec2::splat(10.0), base.y, trunk_h),
        (12.0, Vec2::splat(wide), base.y + trunk_h - 18.0, 22.0),
        (12.0, Vec2::splat(wide * 0.6), base.y + trunk_h + 4.0, 14.0),
    ];
    for (mat, size, y, h) in parts {
        commands.spawn((
            Mesh3d(meshes.add(prism(size, 0.2))),
            MeshMaterial3d(blocks.add(BlockMaterial { u: block(mat, size, h, 2.0) })),
            Transform::from_xyz(base.x, y, base.z).with_scale(Vec3::new(1.0, h, 1.0)),
            Scenery,
        ));
    }
}
