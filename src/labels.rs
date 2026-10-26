//! Titles painted on the blocks. A closed block shows its title, kind and description in
//! the middle of its top; as it opens, that slides to a header along the top edge so
//! everything standing on it can be seen.

use bevy::prelude::*;

use crate::model::{Id, Model};
use crate::paint::{span, Painter};
use crate::render::{DecalMaterial, DecalUniform};
use crate::scene::Diagram;
use crate::theme::{Style, Theme};
use crate::world::{Block, BlockIndex, Geometry, Heights, View, BLOCK_H};

/// Width of a card's text, in layout units (cards are 320 wide).
const CARD_TEXT: f32 = 276.0;

#[derive(Component)]
pub struct Label {
    id: Id,
    /// The painted card text and its size (layout units), and the header (for blocks with
    /// children).
    card: (Entity, Handle<DecalMaterial>, Vec2),
    header: Option<(Entity, Handle<DecalMaterial>, Vec2)>,
}

/// Shared flat quad (1 × 1 on the ground plane) that decals are drawn on.
#[derive(Resource)]
pub struct DecalQuad(pub Handle<Mesh>);

pub fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    commands.insert_resource(DecalQuad(meshes.add(Plane3d::default().mesh().size(1.0, 1.0))));
    commands.insert_resource(Painter::new());
}

fn sub_line(model: &Model, id: Id) -> String {
    let el = &model.elements[id];
    [el.kind.to_uppercase(), el.technology.clone()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("  ·  ")
}

/// Paint `spans` and put them on a decal.
pub fn decal(
    commands: &mut Commands,
    painter: &mut Painter,
    images: &mut Assets<Image>,
    mats: &mut Assets<DecalMaterial>,
    quad: &DecalQuad,
    spans: &[crate::paint::Span],
    width: f32,
    centered: bool,
    pill: bool,
) -> Option<(Entity, Handle<DecalMaterial>, Vec2)> {
    let painted = painter.paint(spans, width, centered)?;
    let size = painted.size;
    let texture = images.add(painted.image);
    let (e, material) = decal_with(commands, mats, quad, texture, size, painter.style, pill);
    Some((e, material, size))
}

/// A decal for text already painted (`size` in layout units).
pub fn decal_with(
    commands: &mut Commands,
    mats: &mut Assets<DecalMaterial>,
    quad: &DecalQuad,
    texture: Handle<Image>,
    size: Vec2,
    style: Style,
    pill: bool,
) -> (Entity, Handle<DecalMaterial>) {
    let (_, bg, edge) = style.tag();
    let corner = match style {
        Style::Blocky => 0.08,
        Style::Dark | Style::City => 0.2,
        _ => 0.5,
    };
    let material = mats.add(DecalMaterial {
        u: DecalUniform {
            tint: Vec4::new(1.0, 1.0, 1.0, 0.0),
            bg: if pill { bg } else { Vec4::ZERO },
            p: Vec4::new(size.x / size.y, if pill { 0.6 } else { 0.0 }, corner, 0.0),
            edge,
        },
        texture,
    });
    let e = commands
        .spawn((Mesh3d(quad.0.clone()), MeshMaterial3d(material.clone()), Transform::default(), Visibility::Hidden))
        .id();
    (e, material)
}

fn spawn(
    commands: &mut Commands,
    painter: &mut Painter,
    images: &mut Assets<Image>,
    mats: &mut Assets<DecalMaterial>,
    quad: &DecalQuad,
    model: &Model,
    id: Id,
) {
    let el = &model.elements[id];
    let sub = sub_line(model, id);
    let (title, cyan, soft) = painter.style.ink();
    let mut spans = vec![span(el.title.clone(), 22.0, 700, title)];
    if !sub.is_empty() {
        spans.push(span(format!("\n{sub}"), 12.0, 600, cyan));
    }
    if !el.description.is_empty() {
        spans.push(span(format!("\n{}", el.description), 14.0, 400, soft));
    }
    let Some(card) = decal(commands, painter, images, mats, quad, &spans, CARD_TEXT, true, false) else {
        return;
    };
    let header = (!el.children.is_empty())
        .then(|| {
            let mut spans = vec![span(el.title.clone(), 30.0, 700, title)];
            if !sub.is_empty() {
                spans.push(span(format!("    {sub}"), 15.0, 600, cyan));
            }
            decal(commands, painter, images, mats, quad, &spans, 1400.0, false, false)
        })
        .flatten();
    commands.spawn(Label { id, card, header });
}

/// Keep a label for every block, and lay it on the block's top.
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    diagram: Res<Diagram>,
    view: Res<View>,
    geometry: Res<Geometry>,
    heights: Res<Heights>,
    index: Res<BlockIndex>,
    blocks: Query<&Block>,
    (mut painter, mut images, quad): (ResMut<Painter>, ResMut<Assets<Image>>, Res<DecalQuad>),
    mut mats: ResMut<Assets<DecalMaterial>>,
    theme: Res<Theme>,
    labels: Query<(Entity, &Label)>,
    mut decals: Query<(&mut Transform, &mut Visibility)>,
    mut last_resets: Local<u64>,
) {
    let model = &diagram.model;
    let despawn = |commands: &mut Commands, e: Entity, l: &Label| {
        commands.entity(l.card.0).despawn();
        if let Some(h) = &l.header {
            commands.entity(h.0).despawn();
        }
        commands.entity(e).despawn();
    };
    if *last_resets != view.resets {
        *last_resets = view.resets;
        for (e, l) in &labels {
            despawn(&mut commands, e, l);
        }
        return;
    }
    let mut have = std::collections::HashSet::new();
    for (e, l) in &labels {
        if index.0.contains_key(&l.id) {
            have.insert(l.id);
        } else {
            despawn(&mut commands, e, l);
        }
    }
    for &id in index.0.keys() {
        if !have.contains(&id) {
            spawn(&mut commands, &mut painter, &mut images, &mut mats, &quad, model, id);
        }
    }

    for (_, l) in &labels {
        let Some(b) = index.0.get(&l.id).and_then(|e| blocks.get(*e).ok()) else { continue };
        let Some(g) = geometry.geos.get(&l.id) else { continue };
        let h = heights.height.get(&l.id).copied().unwrap_or(0.0);
        let top = heights.base.get(&l.id).copied().unwrap_or(0.0) + h + 0.4 * g.scale;
        let open = b.open.value.clamp(0.0, 1.0);
        let grown = (h / (BLOCK_H * g.scale * 0.8)).clamp(0.0, 1.0);
        let center = Vec2::new(g.center.x, g.center.y);

        // Where the header sits once open: along the top edge, left-aligned.
        let header = l.header.as_ref().zip(geometry.inner.get(&l.id)).map(|(hd, inner)| {
            let size = hd.2 * inner.scale;
            let at = inner.header + Vec2::new(size.x / 2.0, size.y / 2.0);
            (at, size)
        });
        let card_size = l.card.2 * g.scale;
        let (header_at, header_size) = header.unwrap_or((center, card_size));

        let place = |e: Entity, at: Vec2, size: Vec2, decals: &mut Query<(&mut Transform, &mut Visibility)>, alpha: f32| {
            if let Ok((mut tf, mut vis)) = decals.get_mut(e) {
                tf.translation = Vec3::new(at.x, top, at.y);
                tf.scale = Vec3::new(size.x, 1.0, size.y);
                *vis = if alpha > 0.01 { Visibility::Inherited } else { Visibility::Hidden };
            }
        };
        let tint = |m: &Handle<DecalMaterial>, alpha: f32, mats: &mut Assets<DecalMaterial>| {
            if let Some(mat) = mats.get_mut(m) {
                let t = theme.style.text_tint();
                mat.u.tint = Vec4::new(t, t, t * 1.04, alpha);
            }
        };
        // The card text slides toward the header and fades; the header fades in as it lands.
        let card_alpha = grown * (1.0 - open * 1.6).max(0.0);
        // Both keep their proportions: each scales uniformly between the two sizes.
        let at = center.lerp(header_at, open);
        let card_scale = 1.0 + ((header_size.y / card_size.y).min(1.0) - 1.0) * open;
        place(l.card.0, at, card_size * card_scale, &mut decals, card_alpha);
        tint(&l.card.1, card_alpha, &mut mats);
        if let Some(hd) = &l.header {
            let alpha = grown * open;
            let start = (card_size.x / header_size.x).min(1.0);
            let header_scale = start + (1.0 - start) * open;
            place(hd.0, at, header_size * header_scale, &mut decals, alpha);
            tint(&hd.1, alpha, &mut mats);
        }
    }
}
