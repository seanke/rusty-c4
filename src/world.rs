//! The 3D world. Every visible element is a block extruded from the surface it stands on:
//! the floor for top-level elements, or the top of its parent once that parent is open.
//! There is no focus: a block opens by itself once it is big enough on screen, rising into
//! a platform with its children extruded from its top, and closes again as you zoom away.
//! Nothing else moves or changes size, so zooming feels like moving through one world.

use std::collections::{HashMap, HashSet};

use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};

use crate::camera::Cam;
use crate::layout::{self, Role};
use crate::model::{Id, Model, Shape};
use crate::render::{lin, neon, BlockMaterial, BlockUniform, Spring};
use crate::scene::{Detail, Diagram};
use crate::theme::{Style, Theme};

/// Height of a closed block, in layout units (cards are 320 × 180).
pub const BLOCK_H: f32 = 34.0;
/// How much higher an opened block stands, in layout units.
pub const RISE: f32 = 80.0;
/// A block opens when it is this wide on screen (as a fraction of the window), and closes
/// again below the second, smaller width.
const OPEN_AT: f32 = 0.42;
const CLOSE_AT: f32 = 0.32;

/// Where an element's block stands, in world units: `center` and `size` on the ground
/// plane (x, z), and `scale`, world units per layout unit at its level.
#[derive(Clone, Copy, Debug)]
pub struct Geo {
    pub center: Vec2,
    pub size: Vec2,
    pub scale: f32,
}

impl Geo {
    pub fn rect(&self) -> Rect {
        Rect::from_center_size(self.center, self.size)
    }
}

/// How an element's children are laid out on its top.
#[derive(Clone, Copy, Debug)]
pub struct Inner {
    /// World units per layout unit for the children.
    pub scale: f32,
    /// Top-left of the title on the platform (x, z).
    pub header: Vec2,
}

#[derive(Resource, Default)]
pub struct Geometry {
    pub geos: HashMap<Id, Geo>,
    pub inner: HashMap<Id, Inner>,
    /// The floor's footprint of all top-level blocks.
    pub root_bounds: Rect,
}

/// Layout coordinates (y up the screen) to the ground plane (z toward the viewer).
fn to_ground(p: Vec2) -> Vec2 {
    Vec2::new(p.x, -p.y)
}

impl Geometry {
    pub fn reset(&mut self, model: &Model, aspect: f32) {
        self.geos.clear();
        self.inner.clear();
        let view = layout::compute(model, None, aspect);
        let mut bounds: Option<Rect> = None;
        for p in view.nodes.iter().filter(|p| p.role == Role::Card) {
            let g = Geo {
                center: to_ground(p.rect.center()),
                size: p.rect.size(),
                scale: 1.0,
            };
            bounds = Some(bounds.map_or(g.rect(), |b| b.union(g.rect())));
            self.geos.insert(p.id, g);
        }
        self.root_bounds = bounds.unwrap_or(Rect::from_center_size(Vec2::ZERO, layout::CARD));
    }

    /// Lay out `id`'s children on its top (once).
    pub fn ensure_children(&mut self, model: &Model, id: Id) {
        if self.inner.contains_key(&id) {
            return;
        }
        let Some(parent) = self.geos.get(&id).copied() else { return };
        let view = layout::compute(model, Some(id), parent.size.x / parent.size.y);
        let Some(frame) = view.get(id).map(|p| p.rect) else { return };
        let k = (parent.size.x / frame.width()).min(parent.size.y / frame.height()) * 0.97;
        let place = |p: Vec2| parent.center + to_ground(p - frame.center()) * k;
        for p in view.nodes.iter().filter(|p| p.role == Role::Card) {
            self.geos.insert(
                p.id,
                Geo {
                    center: place(p.rect.center()),
                    size: p.rect.size() * k,
                    scale: k,
                },
            );
        }
        self.inner.insert(
            id,
            Inner {
                scale: k,
                header: place(Vec2::new(frame.min.x + 30.0, frame.max.y - 22.0)),
            },
        );
    }

    /// World units per layout unit for things standing on `surface` (None: the floor).
    pub fn surface_scale(&self, surface: Option<Id>) -> f32 {
        surface.and_then(|s| self.inner.get(&s)).map_or(1.0, |i| i.scale)
    }
}

/// What is open and visible right now.
#[derive(Resource, Default)]
pub struct View {
    /// The opened blocks.
    pub opened: HashSet<Id>,
    pub visible: HashSet<Id>,
    /// Bumped when the open set changes; `resets` when everything is rebuilt.
    pub version: u64,
    pub resets: u64,
    synced: u64,
}

#[derive(Component)]
pub struct Block {
    pub id: Id,
    pub height: Spring<f32>,
    pub hover: Spring<f32>,
    pub dim: Spring<f32>,
    pub open: Spring<f32>,
    pub delay: f32,
    pub dying: bool,
    pub material: Handle<BlockMaterial>,
}

#[derive(Resource, Default)]
pub struct BlockIndex(pub HashMap<Id, Entity>);

/// Each visible block's base and current height, refreshed every frame.
#[derive(Resource, Default)]
pub struct Heights {
    pub base: HashMap<Id, f32>,
    pub height: HashMap<Id, f32>,
    /// Anything still rising or sinking.
    pub moving: bool,
}

impl Heights {
    /// Height of the top of a surface (None: the floor).
    pub fn top(&self, surface: Option<Id>) -> f32 {
        match surface {
            None => 0.0,
            Some(s) => self.base.get(&s).copied().unwrap_or(0.0) + self.height.get(&s).copied().unwrap_or(0.0),
        }
    }
}

/// Outline of a rounded rectangle centred on the origin, counter-clockwise (y up).
pub fn rounded_outline(size: Vec2, radius: f32) -> Vec<Vec2> {
    let half = size / 2.0;
    let r = radius.min(half.x).min(half.y).max(0.0);
    let segs = 5;
    let corners = [
        (Vec2::new(half.x - r, half.y - r), 0.0),
        (Vec2::new(-half.x + r, half.y - r), 0.5),
        (Vec2::new(-half.x + r, -half.y + r), 1.0),
        (Vec2::new(half.x - r, -half.y + r), 1.5),
    ];
    let mut out = Vec::with_capacity(4 * (segs + 1));
    for (c, start) in corners {
        for i in 0..=segs {
            let a = (start + 0.5 * i as f32 / segs as f32) * std::f32::consts::PI;
            out.push(c + Vec2::new(a.cos(), a.sin()) * r);
        }
    }
    out
}

/// A rounded-rectangle prism of unit height standing on y = 0, footprint `size` (x, z).
/// `uv` holds local top-face coordinates (x, up the screen); `uv_b` (height fraction, face).
fn prism(size: Vec2, radius: f32) -> Mesh {
    let outline = rounded_outline(size, radius);
    let n = outline.len();
    let mut pos = Vec::new();
    let mut nrm = Vec::new();
    let mut uv = Vec::new();
    let mut uvb = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    // Top face: a fan from the centre.
    let to3 = |p: Vec2, y: f32| [p.x, y, -p.y];
    pos.push(to3(Vec2::ZERO, 1.0));
    nrm.push([0.0, 1.0, 0.0]);
    uv.push([0.0, 0.0]);
    uvb.push([1.0, 0.0]);
    for p in &outline {
        pos.push(to3(*p, 1.0));
        nrm.push([0.0, 1.0, 0.0]);
        uv.push([p.x, p.y]);
        uvb.push([1.0, 0.0]);
    }
    for i in 0..n as u32 {
        idx.extend_from_slice(&[0, 1 + i, 1 + (i + 1) % n as u32]);
    }
    // Sides, with smooth normals round the corners.
    let normal_at = |i: usize| {
        let prev = outline[(i + n - 1) % n];
        let next = outline[(i + 1) % n];
        let t = (next - prev).normalize_or(Vec2::X);
        Vec2::new(t.y, -t.x)
    };
    let base = pos.len() as u32;
    for i in 0..n {
        let p = outline[i];
        let o = normal_at(i);
        for y in [0.0, 1.0] {
            pos.push(to3(p, y));
            nrm.push([o.x, 0.0, -o.y]);
            uv.push([p.x, p.y]);
            uvb.push([y, 1.0]);
        }
    }
    for i in 0..n as u32 {
        let a = base + i * 2;
        let b = base + ((i + 1) % n as u32) * 2;
        idx.extend_from_slice(&[a, b, b + 1, a, b + 1, a + 1]);
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, uvb);
    mesh.insert_indices(Indices::U32(idx));
    mesh
}

/// Rounded corners, except for the blocky style's square ones (a hair of rounding keeps
/// the side normals clean).
fn corner_radius(style: Style, scale: f32) -> f32 {
    match style {
        Style::Blocky => 0.3 * scale,
        _ => 16.0 * scale,
    }
}

fn target_height(opened: &HashSet<Id>, id: Id, g: &Geo) -> f32 {
    let h = if opened.contains(&id) { BLOCK_H + RISE } else { BLOCK_H };
    h * g.scale
}

/// Open the blocks that are big on screen and close the ones that have shrunk away.
pub fn auto_open(
    diagram: Res<Diagram>,
    mut view: ResMut<View>,
    mut geometry: ResMut<Geometry>,
    heights: Res<Heights>,
    cam: Res<Cam>,
) {
    if !diagram.loaded || view.resets != diagram.resets {
        return;
    }
    let model = &diagram.model;
    let screen = Rect::from_corners(Vec2::ZERO, cam.viewport);
    let mut open: HashSet<Id> = HashSet::new();
    // Parents before children, so a block can only open once its parent is open.
    let mut ids: Vec<Id> = view.visible.iter().copied().filter(|id| !model.elements[*id].children.is_empty()).collect();
    ids.sort_by_key(|id| model.path(Some(*id)).len());
    for id in ids {
        if let Some(p) = model.elements[id].parent {
            if !open.contains(&p) {
                continue;
            }
        }
        let Some(g) = geometry.geos.get(&id) else { continue };
        let top = heights.base.get(&id).copied().unwrap_or(0.0) + heights.height.get(&id).copied().unwrap_or(0.0);
        let h = g.size / 2.0;
        let mut r: Option<Rect> = None;
        for (dx, dz) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            if let Some(p) = cam.world_to_screen(Vec3::new(g.center.x + h.x * dx, top, g.center.y + h.y * dz)) {
                let pr = Rect::from_center_size(p, Vec2::ZERO);
                r = Some(r.map_or(pr, |a| a.union(pr)));
            }
        }
        let Some(r) = r else { continue };
        let on_screen = !r.intersect(screen).is_empty();
        let wide = r.width() / cam.viewport.x.max(1.0);
        let threshold = if view.opened.contains(&id) { CLOSE_AT } else { OPEN_AT };
        if on_screen && wide > threshold {
            open.insert(id);
        }
    }
    if open != view.opened {
        for &id in &open {
            geometry.ensure_children(model, id);
        }
        view.opened = open;
        view.version += 1;
    }
}

/// Bring the blocks in line with what is open: extrude the newly visible blocks and retract
/// the ones no longer visible.
#[allow(clippy::too_many_arguments)]
pub fn sync_view(
    mut commands: Commands,
    mut diagram: ResMut<Diagram>,
    mut view: ResMut<View>,
    mut geometry: ResMut<Geometry>,
    mut blocks: Query<(Entity, &mut Block)>,
    mut index: ResMut<BlockIndex>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<BlockMaterial>>,
    mut cam: ResMut<Cam>,
    theme: Res<Theme>,
) {
    let reset = view.resets != diagram.resets;
    if !reset && view.synced == view.version {
        return;
    }
    let intro = std::mem::take(&mut diagram.intro);
    let restyle = reset && std::mem::take(&mut diagram.restyle);
    let model = &diagram.model;
    if reset {
        for (e, _) in &blocks {
            commands.entity(e).despawn();
        }
        index.0.clear();
        let aspect = cam.viewport.x / cam.viewport.y.max(1.0);
        geometry.reset(model, aspect);
        view.opened.clear();
        view.resets = diagram.resets;
        view.version += 1;
        let r = geometry.root_bounds;
        // A change of style rebuilds the world in place; only a new model refits the view.
        if !restyle {
            cam.fit(Vec3::new(r.center().x, 0.0, r.center().y), r.size() + Vec2::splat(120.0), true);
        }
    }
    let opened = view.opened.clone();
    let mut visible: HashSet<Id> = model.roots.iter().copied().collect();
    for &id in &opened {
        visible.extend(model.elements[id].children.iter().copied());
    }

    // Retract what is no longer visible.
    for (_, mut b) in &mut blocks {
        if !visible.contains(&b.id) {
            b.dying = true;
            b.height.target = 0.0;
            b.open.target = 0.0;
            b.delay = 0.0;
        }
    }

    let center = geometry.root_bounds.center();
    let spread = geometry.root_bounds.size().length().max(1.0);
    for &id in &visible {
        let Some(g) = geometry.geos.get(&id).copied() else { continue };
        let h = target_height(&opened, id, &g);
        let open_t = if opened.contains(&id) { 1.0 } else { 0.0 };
        match index.0.get(&id).and_then(|e| blocks.get_mut(*e).ok()) {
            Some((_, mut b)) => {
                b.dying = false;
                b.height.target = h;
                b.open.target = open_t;
            }
            None => {
                let material = mats.add(BlockMaterial {
                    u: BlockUniform::default(),
                });
                let mesh = meshes.add(prism(g.size, corner_radius(theme.style, g.scale)));
                // New blocks extrude out of their surface: all together on first load
                // (staggered from the middle), or just after their platform starts rising.
                let delay = if intro {
                    0.1 + 0.45 * (g.center.distance(center) / spread)
                } else if reset {
                    0.0
                } else {
                    0.12
                };
                let start = if reset && !intro { h } else { g.scale * 0.2 };
                let mut height = Spring::new(start);
                height.target = h;
                let mut open = Spring::new(if reset && !intro { open_t } else { 0.0 });
                open.target = open_t;
                let e = commands
                    .spawn((
                        Mesh3d(mesh),
                        MeshMaterial3d(material.clone()),
                        Transform::from_xyz(g.center.x, 0.0, g.center.y).with_scale(Vec3::new(1.0, start, 1.0)),
                        Block {
                            id,
                            height,
                            hover: Spring::new(0.0),
                            dim: Spring::new(0.0),
                            open,
                            delay,
                            dying: false,
                            material,
                        },
                    ))
                    .id();
                index.0.insert(id, e);
            }
        }
    }
    view.visible = visible;
    view.synced = view.version;
}

/// The innermost open block under a point on the ground, if any.
pub fn open_under(model: &Model, view: &View, geometry: &Geometry, p: Vec2) -> Option<Id> {
    view.opened
        .iter()
        .copied()
        .filter(|id| geometry.geos.get(id).is_some_and(|g| g.rect().contains(p)))
        .max_by_key(|id| model.path(Some(*id)).len())
}

/// Keep the camera's pivot on the surface under it, so it rides up with rising platforms.
pub fn follow_surface(diagram: Res<Diagram>, view: Res<View>, geometry: Res<Geometry>, heights: Res<Heights>, mut cam: ResMut<Cam>) {
    let t = cam.target.target;
    let under = open_under(&diagram.model, &view, &geometry, Vec2::new(t.x, t.z));
    cam.target.target.y = heights.top(under);
}

/// Raise and lower blocks, stack them on their surfaces, and update their materials.
#[allow(clippy::too_many_arguments)]
pub fn animate(
    mut commands: Commands,
    time: Res<Time>,
    diagram: Res<Diagram>,
    geometry: Res<Geometry>,
    mut heights: ResMut<Heights>,
    mut index: ResMut<BlockIndex>,
    mut blocks: Query<(Entity, &mut Block, &mut Transform)>,
    mut mats: ResMut<Assets<BlockMaterial>>,
    theme: Res<Theme>,
) {
    let dt = time.delta_secs().min(0.05);
    let model = &diagram.model;
    heights.moving = false;
    heights.height.clear();
    for (_, mut b, _) in &mut blocks {
        if b.delay > 0.0 {
            b.delay -= dt;
        } else {
            b.height.step(dt, 9.0, 0.92);
            b.open.step(dt, 8.0, 1.0);
        }
        b.hover.step(dt, 20.0, 1.0);
        b.dim.step(dt, 14.0, 1.0);
        let scale = geometry.geos.get(&b.id).map_or(1.0, |g| g.scale);
        if (b.height.target - b.height.value).abs() > 0.01 * scale || b.height.vel.abs() > 0.05 * scale {
            heights.moving = true;
        }
        heights.height.insert(b.id, b.height.value.max(0.0));
    }

    // Each block stands on top of its parent's block (if that is open), else on the floor.
    let mut base: HashMap<Id, f32> = HashMap::new();
    fn base_of(model: &Model, h: &HashMap<Id, f32>, memo: &mut HashMap<Id, f32>, id: Id) -> f32 {
        if let Some(&b) = memo.get(&id) {
            return b;
        }
        let b = match model.elements[id].parent {
            Some(p) if h.contains_key(&p) => base_of(model, h, memo, p) + h[&p],
            _ => 0.0,
        };
        memo.insert(id, b);
        b
    }
    let ids: Vec<Id> = heights.height.keys().copied().collect();
    for id in ids {
        base_of(model, &heights.height, &mut base, id);
    }
    heights.base = base;

    let selected = match diagram.detail {
        Some(Detail::Element(id)) => Some(id),
        _ => None,
    };
    for (e, b, mut tf) in &mut blocks {
        let Some(g) = geometry.geos.get(&b.id).copied() else { continue };
        let h = b.height.value.max(0.0);
        if b.dying && b.height.target == 0.0 && h < 0.5 * g.scale {
            index.0.remove(&b.id);
            commands.entity(e).despawn();
            continue;
        }
        let base = heights.base.get(&b.id).copied().unwrap_or(0.0);
        tf.translation = Vec3::new(g.center.x, base, g.center.y);
        tf.scale = Vec3::new(1.0, h.max(1e-4), 1.0);

        let el = &model.elements[b.id];
        let glow = 2.4;
        let shape = match el.shape {
            Shape::Rectangle => 0.0,
            s => s.index(),
        };
        let child_scale = geometry.inner.get(&b.id).map_or(g.scale * 0.2, |i| i.scale);
        let base = Color::srgb(el.color[0], el.color[1], el.color[2]);
        let (edge, body) = match theme.style {
            // Neon edges over a near-black body.
            Style::Tron => (neon(el.color), lin(base.mix(&Color::srgb(0.01, 0.015, 0.03), 0.9))),
            // Ink in a darker shade of the colour, on pale tinted paper.
            Style::Sketch => (
                lin(base.mix(&Color::srgb(0.08, 0.09, 0.15), 0.55)).truncate(),
                lin(base.mix(&Color::srgb(0.98, 0.97, 0.93), 0.82)),
            ),
            // The colour itself, a touch brighter, as the block's material.
            Style::Blocky => (Vec3::ZERO, lin(base.mix(&Color::WHITE, 0.12))),
        };
        if let Some(m) = mats.get_mut(&b.material) {
            m.u = BlockUniform {
                neon: edge.extend(glow),
                body,
                dims: Vec4::new(g.size.x * 0.5, g.size.y * 0.5, corner_radius(theme.style, g.scale), h),
                p: Vec4::new(
                    b.hover.value.clamp(0.0, 1.0),
                    if selected == Some(b.id) { 1.0 } else { 0.0 },
                    shape,
                    b.open.value.clamp(0.0, 1.0),
                ),
                q: Vec4::new(2.2 * g.scale, 20.0 * child_scale, b.dim.value.clamp(0.0, 1.0), theme.style.index()),
            };
        }
    }
}

/// The nearest block under the ray (origin, direction), with its distance.
pub fn pick<'a>(
    heights: &Heights,
    geometry: &Geometry,
    blocks: impl Iterator<Item = (Entity, &'a Block)>,
    origin: Vec3,
    dir: Vec3,
) -> Option<(Entity, Id, f32)> {
    let mut best: Option<(Entity, Id, f32)> = None;
    for (e, b) in blocks {
        if b.dying {
            continue;
        }
        let Some(g) = geometry.geos.get(&b.id) else { continue };
        let base = heights.base.get(&b.id).copied().unwrap_or(0.0);
        let h = heights.height.get(&b.id).copied().unwrap_or(0.0);
        let min = Vec3::new(g.center.x - g.size.x / 2.0, base, g.center.y - g.size.y / 2.0);
        let max = Vec3::new(g.center.x + g.size.x / 2.0, base + h, g.center.y + g.size.y / 2.0);
        let inv = dir.recip();
        let t1 = (min - origin) * inv;
        let t2 = (max - origin) * inv;
        let tmin = t1.min(t2).max_element();
        let tmax = t1.max(t2).min_element();
        if tmax >= tmin.max(0.0) && best.is_none_or(|(_, _, t)| tmin < t) {
            best = Some((e, b.id, tmin));
        }
    }
    best
}
