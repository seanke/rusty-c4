//! The city style's traffic: little cars driving along each road, in the direction of the
//! relationships it carries (both ways, in their own lanes, where they go both ways).

use std::collections::HashMap;

use bevy::prelude::*;

use crate::pipes::{Pipe, DUST_THIN, ROAD_WIDE};
use crate::scene::Diagram;
use crate::theme::{Style, Theme};

#[derive(Component)]
pub struct Car {
    pipe: Entity,
    forward: bool,
    /// Distance along the road, in units of its radius (so it keeps its place as the road
    /// grows or moves).
    along: f32,
    /// Radii per second.
    speed: f32,
}

/// The shared meshes and paint for cars.
pub struct CarAssets {
    body: Handle<Mesh>,
    cabin: Handle<Mesh>,
    roof: Handle<Mesh>,
    glass: Handle<StandardMaterial>,
    paint: Vec<Handle<StandardMaterial>>,
}

fn assets(meshes: &mut Assets<Mesh>, mats: &mut Assets<StandardMaterial>) -> CarAssets {
    let flat = |c: Color| StandardMaterial {
        base_color: c,
        unlit: true,
        ..default()
    };
    CarAssets {
        // Forward is -z.
        body: meshes.add(Cuboid::new(0.62, 0.36, 1.3)),
        cabin: meshes.add(Cuboid::new(0.54, 0.3, 0.66)),
        roof: meshes.add(Cuboid::new(0.56, 0.06, 0.6)),
        glass: mats.add(flat(Color::srgb(0.16, 0.2, 0.27))),
        paint: [
            Color::srgb(0.85, 0.16, 0.14),
            Color::srgb(0.16, 0.38, 0.85),
            Color::srgb(1.0, 0.78, 0.1),
            Color::srgb(0.95, 0.95, 0.93),
            Color::srgb(0.12, 0.13, 0.15),
            Color::srgb(0.1, 0.62, 0.55),
            Color::srgb(0.95, 0.5, 0.12),
            Color::srgb(0.6, 0.62, 0.66),
        ]
        .into_iter()
        .map(|c| mats.add(flat(c)))
        .collect(),
    }
}

/// A point and direction `d` along a polyline, measured from its start.
fn at(pts: &[Vec3], d: f32) -> Vec3 {
    let mut left = d.max(0.0);
    for w in pts.windows(2) {
        let l = w[0].distance(w[1]);
        if left <= l {
            return w[0].lerp(w[1], left / l.max(1e-6));
        }
        left -= l;
    }
    pts.last().copied().unwrap_or_default()
}

fn length(pts: &[Vec3]) -> f32 {
    pts.windows(2).map(|w| w[0].distance(w[1])).sum()
}

/// Which ways a road's traffic goes: along it (from its `from` block), and back.
fn directions(pipe: &Pipe, diagram: &Diagram) -> (bool, bool) {
    let model = &diagram.model;
    let mut there = false;
    let mut back = false;
    for &(a, _) in &pipe.links {
        if model.path(Some(a)).contains(&pipe.from) {
            there = true;
        } else {
            back = true;
        }
    }
    if !there && !back {
        there = true;
    }
    (there, back)
}

#[allow(clippy::too_many_arguments)]
pub fn traffic(
    mut commands: Commands,
    time: Res<Time>,
    theme: Res<Theme>,
    diagram: Res<Diagram>,
    pipes: Query<(Entity, &Pipe)>,
    mut cars: Query<(&mut Car, &mut Transform, &mut Visibility)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut fleet: Local<HashMap<Entity, Vec<Entity>>>,
    mut kit: Local<Option<CarAssets>>,
) {
    let on = theme.style == Style::City && diagram.loaded;
    // Clear the roads of cars whose road has gone (or everything, out of the city).
    fleet.retain(|pipe, list| {
        let keep = on && pipes.get(*pipe).is_ok_and(|(_, p)| !p.dying);
        if !keep {
            for &c in list.iter() {
                commands.entity(c).despawn();
            }
        }
        keep
    });
    if !on {
        return;
    }
    let kit = kit.get_or_insert_with(|| assets(&mut meshes, &mut mats));

    // New roads get their cars, spaced out along them.
    for (e, p) in &pipes {
        if fleet.contains_key(&e) || !p.ready() || p.radius <= 0.0 || p.samples.len() < 2 {
            continue;
        }
        let len = length(&p.samples) / p.radius;
        let (there, back) = directions(p, &diagram);
        let per_way = (len / 16.0).floor().clamp(1.0, 6.0) as usize;
        let mut list = Vec::new();
        let seed = e.index_u32();
        for (way, forward) in [(there, true), (back, false)] {
            if !way {
                continue;
            }
            for i in 0..per_way {
                let h = seed.wrapping_mul(2654435761).wrapping_add((i as u32 + forward as u32 * 7) * 40503);
                let r = (h >> 8) as f32 / (1u32 << 24) as f32;
                let paint = kit.paint[(h as usize >> 3) % kit.paint.len()].clone();
                let car = commands
                    .spawn((
                        Car {
                            pipe: e,
                            forward,
                            along: len * (i as f32 + 0.3 * r) / per_way as f32,
                            speed: 4.0 + 2.5 * r,
                        },
                        Transform::default(),
                        Visibility::Hidden,
                    ))
                    .with_children(|c| {
                        c.spawn((Mesh3d(kit.body.clone()), MeshMaterial3d(paint.clone()), Transform::from_xyz(0.0, 0.18, 0.0)));
                        c.spawn((Mesh3d(kit.cabin.clone()), MeshMaterial3d(kit.glass.clone()), Transform::from_xyz(0.0, 0.48, 0.1)));
                        c.spawn((Mesh3d(kit.roof.clone()), MeshMaterial3d(paint), Transform::from_xyz(0.0, 0.64, 0.1)));
                    })
                    .id();
                list.push(car);
            }
        }
        fleet.insert(e, list);
    }

    // Drive.
    let dt = time.delta_secs().min(0.05);
    for (mut car, mut tf, mut vis) in &mut cars {
        let Ok((_, p)) = pipes.get(car.pipe) else { continue };
        if !p.ready() || p.radius <= 0.0 {
            *vis = Visibility::Hidden;
            continue;
        }
        let r = p.radius;
        let total = length(&p.samples);
        let len = total / r;
        car.along = (car.along + car.speed * dt) % len.max(1e-3);
        let d = if car.forward { car.along * r } else { total - car.along * r };
        let pos = at(&p.samples, d);
        // Heading, from a little either side, so cars turn smoothly at corners.
        let ahead = if car.forward { r } else { -r };
        let dir = (at(&p.samples, d + ahead) - at(&p.samples, d - ahead)).normalize_or(Vec3::X);
        // On the level, up is up; on a wall, the road faces out from it.
        let flat = Vec3::new(dir.x, 0.0, dir.z);
        let up = if dir.y.abs() < 0.7 || flat.length() > 0.2 {
            Vec3::Y
        } else {
            let out = at(&p.samples, d - ahead * 3.0) - at(&p.samples, d + ahead * 3.0);
            let h = Vec3::new(out.x, 0.0, out.z).normalize_or(Vec3::Z);
            if dir.y < 0.0 { -h } else { h }
        };
        let right = dir.cross(up).normalize_or(Vec3::X);
        let size = r * 1.7;
        // On the road's top surface, in the right-hand lane.
        let lane = right * ROAD_WIDE * r * 0.5;
        let top = up * (-(1.0 - 2.0 * DUST_THIN) * r);
        tf.translation = pos + top + lane;
        *tf = Transform::from_translation(tf.translation).looking_to(dir, up).with_scale(Vec3::splat(size));
        *vis = Visibility::Inherited;
    }
}
