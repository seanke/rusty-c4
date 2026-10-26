//! Toolbar icons, drawn here as clean 1px outline strokes on a 16-unit grid (in the spirit
//! of VS Code's codicons) and rasterised into small anti-aliased images.

use std::collections::HashMap;

use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Icon {
    Folder,
    File,
    Save,
    Refresh,
    Sidebar,
    List,
    ChevronLeft,
    ChevronRight,
    ChevronDown,
    ZoomIn,
    ZoomOut,
    Fit,
    Palette,
    Close,
}

impl Icon {
    pub const ALL: [Icon; 14] = [
        Icon::Folder,
        Icon::File,
        Icon::Save,
        Icon::Refresh,
        Icon::Sidebar,
        Icon::List,
        Icon::ChevronLeft,
        Icon::ChevronRight,
        Icon::ChevronDown,
        Icon::ZoomIn,
        Icon::ZoomOut,
        Icon::Fit,
        Icon::Palette,
        Icon::Close,
    ];
}

/// What an icon is made of, in 16 × 16 units (y down).
enum Shape {
    /// An open (or, if the first and last points match, closed) run of straight strokes.
    Lines(Vec<Vec2>),
    /// A circle's outline, or part of it (angles in degrees, clockwise from +x).
    Arc(Vec2, f32, f32, f32),
    /// A filled dot.
    Dot(Vec2, f32),
}

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

fn lines(points: &[(f32, f32)]) -> Shape {
    Shape::Lines(points.iter().map(|&(x, y)| v(x, y)).collect())
}

fn shapes(icon: Icon) -> Vec<Shape> {
    match icon {
        Icon::Folder => vec![lines(&[
            (1.5, 3.5),
            (6.0, 3.5),
            (7.5, 5.0),
            (14.5, 5.0),
            (14.5, 13.0),
            (1.5, 13.0),
            (1.5, 3.5),
        ])],
        Icon::File => vec![
            lines(&[(3.5, 1.5), (9.5, 1.5), (12.5, 4.5), (12.5, 14.5), (3.5, 14.5), (3.5, 1.5)]),
            lines(&[(9.5, 1.5), (9.5, 4.5), (12.5, 4.5)]),
        ],
        Icon::Save => vec![
            lines(&[(2.5, 2.5), (11.5, 2.5), (13.5, 4.5), (13.5, 13.5), (2.5, 13.5), (2.5, 2.5)]),
            lines(&[(5.0, 2.5), (5.0, 5.5), (10.5, 5.5), (10.5, 2.5)]),
            lines(&[(4.5, 13.5), (4.5, 9.0), (11.5, 9.0), (11.5, 13.5)]),
        ],
        Icon::Refresh => vec![
            Shape::Arc(v(8.0, 8.5), 5.0, -70.0, 220.0),
            lines(&[(8.5, 1.0), (10.6, 3.5), (8.0, 5.5)]),
        ],
        Icon::Sidebar => vec![
            lines(&[(1.5, 2.5), (14.5, 2.5), (14.5, 13.5), (1.5, 13.5), (1.5, 2.5)]),
            lines(&[(9.5, 2.5), (9.5, 13.5)]),
        ],
        Icon::List => vec![
            lines(&[(6.0, 4.0), (14.5, 4.0)]),
            lines(&[(6.0, 8.0), (14.5, 8.0)]),
            lines(&[(6.0, 12.0), (14.5, 12.0)]),
            Shape::Dot(v(2.8, 4.0), 1.1),
            Shape::Dot(v(2.8, 8.0), 1.1),
            Shape::Dot(v(2.8, 12.0), 1.1),
        ],
        Icon::ChevronLeft => vec![lines(&[(10.0, 3.0), (5.0, 8.0), (10.0, 13.0)])],
        Icon::ChevronRight => vec![lines(&[(6.0, 3.0), (11.0, 8.0), (6.0, 13.0)])],
        Icon::ChevronDown => vec![lines(&[(3.5, 6.0), (8.0, 10.5), (12.5, 6.0)])],
        Icon::ZoomIn => vec![
            Shape::Arc(v(6.8, 6.8), 4.8, 0.0, 360.0),
            lines(&[(10.3, 10.3), (14.5, 14.5)]),
            lines(&[(4.6, 6.8), (9.0, 6.8)]),
            lines(&[(6.8, 4.6), (6.8, 9.0)]),
        ],
        Icon::ZoomOut => vec![
            Shape::Arc(v(6.8, 6.8), 4.8, 0.0, 360.0),
            lines(&[(10.3, 10.3), (14.5, 14.5)]),
            lines(&[(4.6, 6.8), (9.0, 6.8)]),
        ],
        Icon::Fit => vec![
            lines(&[(1.5, 5.5), (1.5, 1.5), (5.5, 1.5)]),
            lines(&[(10.5, 1.5), (14.5, 1.5), (14.5, 5.5)]),
            lines(&[(14.5, 10.5), (14.5, 14.5), (10.5, 14.5)]),
            lines(&[(5.5, 14.5), (1.5, 14.5), (1.5, 10.5)]),
        ],
        Icon::Palette => vec![
            Shape::Arc(v(8.0, 8.0), 6.3, 0.0, 360.0),
            Shape::Dot(v(5.0, 6.0), 1.15),
            Shape::Dot(v(8.0, 4.4), 1.15),
            Shape::Dot(v(11.0, 6.0), 1.15),
            Shape::Arc(v(10.3, 10.6), 1.4, 0.0, 360.0),
        ],
        Icon::Close => vec![lines(&[(4.0, 4.0), (12.0, 12.0)]), lines(&[(12.0, 4.0), (4.0, 12.0)])],
    }
}

fn segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

fn arc(p: Vec2, c: Vec2, r: f32, from: f32, to: f32) -> f32 {
    let d = p - c;
    let ang = d.y.atan2(d.x).to_degrees();
    let within = |a: f32| {
        let (lo, mut hi, mut a) = (from, to, a);
        while hi < lo {
            hi += 360.0;
        }
        while a < lo {
            a += 360.0;
        }
        a <= hi || (hi - lo) >= 359.9
    };
    if within(ang) {
        (d.length() - r).abs()
    } else {
        let end = |deg: f32| c + Vec2::new(deg.to_radians().cos(), deg.to_radians().sin()) * r;
        p.distance(end(from)).min(p.distance(end(to)))
    }
}

/// Rasterise an icon at `px` pixels square: white, with coverage in alpha.
fn render(icon: Icon, px: u32) -> Image {
    let units = 16.0 / px as f32;
    let half_w = 0.55;
    let parts = shapes(icon);
    let mut data = vec![0u8; (px * px * 4) as usize];
    for y in 0..px {
        for x in 0..px {
            let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5) * units;
            let mut cover = 0.0f32;
            for s in &parts {
                let c = match s {
                    Shape::Lines(pts) => {
                        let d = pts.windows(2).map(|w| segment(p, w[0], w[1])).fold(f32::INFINITY, f32::min);
                        0.5 - (d - half_w) / units
                    }
                    Shape::Arc(c, r, a0, a1) => 0.5 - (arc(p, *c, *r, *a0, *a1) - half_w) / units,
                    Shape::Dot(c, r) => 0.5 - (p.distance(*c) - r) / units,
                };
                cover = cover.max(c.clamp(0.0, 1.0));
            }
            let i = ((y * px + x) * 4) as usize;
            data[i..i + 4].copy_from_slice(&[255, 255, 255, (cover * 255.0) as u8]);
        }
    }
    Image::new(
        Extent3d {
            width: px,
            height: px,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

#[derive(Resource)]
pub struct Icons(HashMap<Icon, Handle<Image>>);

impl Icons {
    pub fn get(&self, icon: Icon) -> Handle<Image> {
        self.0[&icon].clone()
    }
}

pub fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    // Drawn at 3× their 16-point size, so they stay sharp on high-density displays.
    let map = Icon::ALL.into_iter().map(|i| (i, images.add(render(i, 48)))).collect();
    commands.insert_resource(Icons(map));
}
