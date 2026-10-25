//! Block titles, drawn on a flat overlay over the 3D scene and sized by perspective, so
//! they stay crisp. A closed block shows its title (and, up close, its description) on
//! its top; an opened platform shows its name along the back of its top instead.

use std::collections::HashSet;

use bevy::{
    camera::visibility::RenderLayers,
    prelude::*,
    sprite::Anchor,
    text::{LineBreak, TextBounds},
};

use crate::camera::Cam;
use crate::model::{Id, Model};
use crate::scene::{Diagram, Fonts, OVERLAY, TEXT};
use crate::world::{Block, BlockIndex, Geometry, Heights, View, BLOCK_H};

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Card,
    Header,
}

#[derive(Component)]
pub struct Label {
    id: Id,
    kind: Kind,
    /// The font size the text is laid out at (it is then scaled to its on-screen size).
    q: f32,
    /// Each part of the text: entity, size relative to the title, colour, only up close.
    parts: Vec<(Entity, f32, Color, bool)>,
}

fn quantize(px: f32) -> f32 {
    2f32.powf((px.max(4.0).log2() * 3.0).round() / 3.0).clamp(6.0, 72.0)
}

fn spawn(commands: &mut Commands, fonts: &Fonts, model: &Model, id: Id, kind: Kind) {
    let el = &model.elements[id];
    let q = 18.0;
    let font = |em: f32, weight: FontWeight| TextFont {
        font: fonts.ui.clone(),
        font_size: q * em,
        weight,
        ..default()
    };
    let sub = [el.kind.to_uppercase(), el.technology.clone()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("  ·  ");
    let mut parts = Vec::new();
    let (anchor, justify) = match kind {
        Kind::Card => (Anchor::CENTER, Justify::Center),
        Kind::Header => (Anchor::TOP_LEFT, Justify::Left),
    };
    let root = commands
        .spawn((
            Text2d::new(el.title.clone()),
            font(1.0, FontWeight::BOLD),
            TextColor(TEXT.with_alpha(0.0)),
            TextLayout::new(justify, LineBreak::WordBoundary),
            TextBounds::new_horizontal(q * 13.5),
            anchor,
            Transform::from_xyz(0.0, 0.0, 1.0),
            Visibility::Hidden,
            RenderLayers::layer(OVERLAY),
        ))
        .id();
    parts.push((root, 1.0, TEXT, false));
    if !sub.is_empty() {
        let c = Color::srgba(0.75, 0.95, 1.0, 0.7);
        let e = commands
            .spawn((
                TextSpan::new(format!("\n{sub}")),
                font(0.58, FontWeight::SEMIBOLD),
                TextColor(c.with_alpha(0.0)),
                ChildOf(root),
            ))
            .id();
        parts.push((e, 0.58, c, false));
    }
    if kind == Kind::Card && !el.description.is_empty() {
        let c = Color::srgba(0.85, 0.92, 1.0, 0.8);
        let desc: String = if el.description.chars().count() > 90 {
            format!("{}…", el.description.chars().take(90).collect::<String>().trim_end())
        } else {
            el.description.clone()
        };
        let e = commands
            .spawn((
                TextSpan::new(format!("\n{desc}")),
                font(0.68, FontWeight::NORMAL),
                TextColor(c.with_alpha(0.0)),
                ChildOf(root),
            ))
            .id();
        parts.push((e, 0.68, c, true));
    }
    commands.entity(root).insert(Label { id, kind, q, parts });
}

/// Keep one card label (and, for blocks with children, one header label) per block.
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    diagram: Res<Diagram>,
    view: Res<View>,
    geometry: Res<Geometry>,
    heights: Res<Heights>,
    cam: Res<Cam>,
    fonts: Res<Fonts>,
    index: Res<BlockIndex>,
    blocks: Query<&Block>,
    mut labels: Query<(Entity, &mut Label, &mut Transform, &mut Visibility, &mut TextBounds)>,
    mut text: Query<(&mut TextFont, &mut TextColor)>,
    mut last_resets: Local<u64>,
) {
    let model = &diagram.model;
    if *last_resets != view.resets {
        *last_resets = view.resets;
        for (e, ..) in &labels {
            commands.entity(e).despawn();
        }
        return;
    }
    let mut have: HashSet<(Id, bool)> = HashSet::new();
    for (e, l, ..) in &labels {
        if !index.0.contains_key(&l.id) {
            commands.entity(e).despawn();
        } else {
            have.insert((l.id, l.kind == Kind::Header));
        }
    }
    for &id in index.0.keys() {
        if !have.contains(&(id, false)) {
            spawn(&mut commands, &fonts, model, id, Kind::Card);
        }
        if !model.elements[id].children.is_empty() && !have.contains(&(id, true)) {
            spawn(&mut commands, &fonts, model, id, Kind::Header);
        }
    }

    let focus_level: HashSet<Id> = model.children_of(view.focus).iter().copied().collect();
    for (_, mut l, mut tf, mut vis, mut bounds) in &mut labels {
        let Some(b) = index.0.get(&l.id).and_then(|e| blocks.get(*e).ok()) else { continue };
        let Some(g) = geometry.geos.get(&l.id) else { continue };
        let base = heights.base.get(&l.id).copied().unwrap_or(0.0);
        let h = heights.height.get(&l.id).copied().unwrap_or(0.0);
        let top = base + h + 0.5 * g.scale;
        let open = b.open.value.clamp(0.0, 1.0);
        let (at, em_world, fade) = match l.kind {
            Kind::Card => (Vec3::new(g.center.x, top, g.center.y), 21.0 * g.scale, 1.0 - open),
            Kind::Header => match geometry.inner.get(&l.id) {
                Some(inner) => (Vec3::new(inner.header.x, top, inner.header.y), 26.0 * inner.scale, open),
                None => (Vec3::ZERO, 0.0, 0.0),
            },
        };
        let grown = (h / (BLOCK_H * g.scale * 0.8)).clamp(0.0, 1.0);
        let screen = cam.world_to_overlay(at);
        let px = em_world * cam.px_per_unit(at);
        let level = if focus_level.contains(&l.id) || view.focus == Some(l.id) { 1.0 } else { 0.55 };
        // Fade out titles too small to read, and ones so close they'd fill the screen.
        let (too_big, over) = if level < 1.0 { (22.0, 8.0) } else { (46.0, 12.0) };
        let readable = ((px - 6.0) / 3.0).clamp(0.0, 1.0) * (1.0 - ((px - too_big) / over).clamp(0.0, 1.0));
        let alpha = fade * grown * level * readable * (1.0 - 0.6 * b.dim.value.clamp(0.0, 1.0));
        let Some(o) = screen.filter(|_| alpha > 0.01) else {
            *vis = Visibility::Hidden;
            continue;
        };
        *vis = Visibility::Inherited;
        let q = quantize(px);
        if q != l.q {
            l.q = q;
            for &(e, em, ..) in &l.parts {
                if let Ok((mut f, _)) = text.get_mut(e) {
                    f.font_size = q * em;
                }
            }
            bounds.width = Some(q * 13.5);
        }
        tf.translation = o.extend(1.0);
        tf.scale = Vec3::splat(px / q);
        let near = ((px - 15.0) / 4.0).clamp(0.0, 1.0);
        for &(e, _, c, close_only) in &l.parts {
            if let Ok((_, mut tc)) = text.get_mut(e) {
                let a = if close_only { alpha * near } else { alpha };
                tc.0 = c.with_alpha(c.alpha() * a);
            }
        }
    }
}
