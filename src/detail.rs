//! Opening up a block or a pipe: the camera moves to it and a detail card grows out of it
//! on the overlay, listing everything known about it.

use bevy::{
    camera::visibility::RenderLayers,
    prelude::*,
    sprite::Anchor,
    text::{LineBreak, TextBounds, TextLayoutInfo},
};

use crate::camera::Cam;
use crate::model::{Id, Model};
use crate::pipes::Pipe;
use crate::render::{lin, neon, CardMaterial, CardUniform, Spring};
use crate::scene::{Detail, Diagram, Fonts, SceneAssets, ACCENT, MUTED, OVERLAY, TEXT};
use crate::world::{Geometry, Heights};

const WIDTH: f32 = 560.0;
const INSET: Vec2 = Vec2::new(34.0, 30.0);
const Z: f32 = 20.0;
const PAD: f32 = 60.0;

#[derive(Resource, Default)]
pub struct DetailView {
    /// Where the open card is on the overlay, for hit testing.
    pub rect: Option<Rect>,
    shown: Option<Detail>,
    card: Option<Card>,
    closing: Vec<Card>,
}

struct Card {
    root: Entity,
    quad: Entity,
    text: Entity,
    texts: Vec<(Entity, Color)>,
    material: Handle<CardMaterial>,
    accent: Color,
    pos: Spring<Vec2>,
    size: Spring<Vec2>,
    opacity: Spring<f32>,
    origin: Rect,
}

struct Span {
    text: String,
    size: f32,
    weight: FontWeight,
    color: Color,
}

fn span(text: impl Into<String>, size: f32, weight: FontWeight, color: Color) -> Span {
    Span {
        text: text.into(),
        size,
        weight,
        color,
    }
}

fn soft(alpha: f32) -> Color {
    Color::srgba(0.85, 0.95, 1.0, alpha)
}

fn element_spans(model: &Model, id: Id) -> (Vec<Span>, Color) {
    let el = &model.elements[id];
    let n = neon(el.color);
    let color = Color::linear_rgb(n.x, n.y, n.z);
    let mut out = Vec::new();
    let header = [el.kind.to_uppercase(), el.technology.clone()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("  ·  ");
    out.push(span(header, 12.0, FontWeight::SEMIBOLD, color));
    out.push(span(format!("\n{}", el.title), 30.0, FontWeight::BOLD, TEXT));
    if !el.description.is_empty() {
        out.push(span(format!("\n\n{}", el.description), 16.0, FontWeight::NORMAL, soft(0.85)));
    }
    for outgoing in [true, false] {
        let lines: Vec<String> = model
            .relations
            .iter()
            .filter_map(|r| {
                let (me, other) = if outgoing { (r.source, r.target) } else { (r.target, r.source) };
                if !model.is_ancestor_or_self(id, me) || model.is_ancestor_or_self(id, other) {
                    return None;
                }
                let arrow = if outgoing { "→" } else { "←" };
                let mut s = format!("{arrow}  {}", model.elements[other].title);
                if !r.title.is_empty() {
                    s.push_str(&format!("   ·   {}", r.title));
                }
                if !r.technology.is_empty() {
                    s.push_str(&format!("  [{}]", r.technology));
                }
                Some(s)
            })
            .collect();
        if lines.is_empty() {
            continue;
        }
        let head = if outgoing { "OUTGOING" } else { "INCOMING" };
        out.push(span(format!("\n\n{head}\n"), 11.5, FontWeight::SEMIBOLD, MUTED));
        let n = lines.len();
        let shown: Vec<String> = lines.into_iter().take(14).collect();
        out.push(span(shown.join("\n"), 14.5, FontWeight::NORMAL, soft(0.88)));
        if n > 14 {
            out.push(span(format!("\nand {} more", n - 14), 13.0, FontWeight::NORMAL, MUTED));
        }
    }
    out.push(span(format!("\n\n{}", el.fqn), 11.5, FontWeight::NORMAL, soft(0.35)));
    (out, color)
}

fn relation_spans(model: &Model, a: Id, b: Id) -> (Vec<Span>, Color) {
    let mut out = vec![
        span("RELATIONSHIP", 12.0, FontWeight::SEMIBOLD, ACCENT),
        span(
            format!("\n{}  →  {}", model.elements[a].title, model.elements[b].title),
            26.0,
            FontWeight::BOLD,
            TEXT,
        ),
    ];
    // A pipe between two blocks can stand for several relations between things inside them.
    let underlying: Vec<_> = model
        .relations
        .iter()
        .filter(|r| model.is_ancestor_or_self(a, r.source) && model.is_ancestor_or_self(b, r.target))
        .collect();
    for r in underlying.iter().take(12) {
        let title = if r.title.is_empty() { "(untitled)" } else { r.title.as_str() };
        out.push(span(format!("\n\n{title}"), 17.0, FontWeight::SEMIBOLD, TEXT));
        let mut via = format!("\n{}  →  {}", model.elements[r.source].title, model.elements[r.target].title);
        if !r.technology.is_empty() {
            via.push_str(&format!("   [{}]", r.technology));
        }
        out.push(span(via, 14.0, FontWeight::NORMAL, soft(0.6)));
    }
    if underlying.len() > 12 {
        out.push(span(format!("\n\nand {} more", underlying.len() - 12), 13.0, FontWeight::NORMAL, MUTED));
    }
    (out, ACCENT)
}

/// Rough height before the text has been laid out.
fn estimate_height(spans: &[Span]) -> f32 {
    let chars_per_line = 56.0;
    spans
        .iter()
        .map(|s| {
            s.text
                .split('\n')
                .map(|l| (l.chars().count() as f32 / chars_per_line).ceil().max(1.0))
                .sum::<f32>()
                * s.size
                * 1.25
        })
        .sum::<f32>()
        + INSET.y * 2.0
}

fn spawn_card(
    commands: &mut Commands,
    assets: &SceneAssets,
    mats: &mut Assets<CardMaterial>,
    fonts: &Fonts,
    spans: Vec<Span>,
    accent: Color,
    origin: Rect,
) -> Card {
    let material = mats.add(CardMaterial {
        u: CardUniform::default(),
    });
    let root = commands
        .spawn((
            Transform::from_translation(origin.center().extend(Z)),
            Visibility::Inherited,
            RenderLayers::layer(OVERLAY),
        ))
        .id();
    let quad = commands
        .spawn((
            Mesh2d(assets.quad.clone()),
            MeshMaterial2d(material.clone()),
            Transform::default(),
            RenderLayers::layer(OVERLAY),
            ChildOf(root),
        ))
        .id();
    let font = |s: &Span| TextFont {
        font: fonts.ui.clone(),
        font_size: s.size,
        weight: s.weight,
        ..default()
    };
    let mut iter = spans.into_iter();
    let first = iter.next().unwrap_or_else(|| span("", 12.0, FontWeight::NORMAL, TEXT));
    let text = commands
        .spawn((
            Text2d::new(first.text.clone()),
            font(&first),
            TextColor(first.color.with_alpha(0.0)),
            TextLayout::new(Justify::Left, LineBreak::WordBoundary),
            TextBounds::new_horizontal(WIDTH - INSET.x * 2.0),
            Anchor::TOP_LEFT,
            Transform::from_xyz(0.0, 0.0, 0.5),
            RenderLayers::layer(OVERLAY),
            ChildOf(root),
        ))
        .id();
    let mut texts = vec![(text, first.color)];
    for s in iter {
        let e = commands
            .spawn((TextSpan::new(s.text.clone()), font(&s), TextColor(s.color.with_alpha(0.0)), ChildOf(text)))
            .id();
        texts.push((e, s.color));
    }
    Card {
        root,
        quad,
        text,
        texts,
        material,
        accent,
        pos: Spring::new(origin.center()),
        size: Spring::new(origin.size().max(Vec2::splat(4.0))),
        opacity: Spring::new(0.0),
        origin,
    }
}

/// The screen rectangle (overlay coordinates) covering a block's top.
fn block_on_screen(cam: &Cam, geometry: &Geometry, heights: &Heights, id: Id) -> Option<Rect> {
    let g = geometry.geos.get(&id)?;
    let top = heights.base.get(&id).copied().unwrap_or(0.0) + heights.height.get(&id).copied().unwrap_or(0.0);
    let h = g.size / 2.0;
    let mut r: Option<Rect> = None;
    for (dx, dz) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        let p = cam.world_to_overlay(Vec3::new(g.center.x + h.x * dx, top, g.center.y + h.y * dz))?;
        let pr = Rect::from_center_size(p, Vec2::ZERO);
        r = Some(r.map_or(pr, |a| a.union(pr)));
    }
    r
}

#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    time: Res<Time>,
    diagram: Res<Diagram>,
    mut view: ResMut<DetailView>,
    mut cam: ResMut<Cam>,
    geometry: Res<Geometry>,
    heights: Res<Heights>,
    pipes: Query<&Pipe>,
    fonts: Res<Fonts>,
    assets: Res<SceneAssets>,
    mut mats: ResMut<Assets<CardMaterial>>,
    mut transforms: Query<&mut Transform>,
    infos: Query<&TextLayoutInfo>,
    mut colors: Query<&mut TextColor>,
) {
    let dt = time.delta_secs().min(0.05);
    let model = &diagram.model;
    // The card sits on the right; the subject is framed on the left.
    let card_x = cam.viewport.x / 2.0 - 32.0 - WIDTH / 2.0;

    if diagram.detail != view.shown {
        view.shown = diagram.detail;
        if let Some(mut old) = view.card.take() {
            old.pos.target = old.origin.center();
            old.size.target = old.origin.size().max(Vec2::splat(4.0));
            old.opacity.target = 0.0;
            view.closing.push(old);
        }
        let opened = match diagram.detail {
            Some(Detail::Element(id)) => {
                let (spans, accent) = element_spans(model, id);
                let origin = block_on_screen(&cam, &geometry, &heights, id);
                let subject = geometry.geos.get(&id).map(|g| {
                    let top = heights.base.get(&id).copied().unwrap_or(0.0) + heights.height.get(&id).copied().unwrap_or(0.0);
                    (Vec3::new(g.center.x, top, g.center.y), g.size)
                });
                origin.zip(subject).map(|(o, s)| (spans, accent, o, s))
            }
            Some(Detail::Relation(a, b)) => {
                let (spans, accent) = relation_spans(model, a, b);
                let mid = pipes
                    .iter()
                    .find(|p| p.key == (a, b))
                    .and_then(|p| p.samples.get(p.samples.len() / 2).copied());
                let ends = geometry.geos.get(&a).zip(geometry.geos.get(&b)).map(|(ga, gb)| ga.rect().union(gb.rect()));
                match (mid, ends) {
                    (Some(m), Some(r)) => cam
                        .world_to_overlay(m)
                        .map(|o| (spans, accent, Rect::from_center_size(o, Vec2::splat(20.0)), (Vec3::new(r.center().x, m.y, r.center().y), r.size()))),
                    _ => None,
                }
            }
            None => None,
        };
        if let Some((spans, accent, origin, (center, size))) = opened {
            let h = estimate_height(&spans).min(cam.viewport.y - 120.0);
            let mut card = spawn_card(&mut commands, &assets, &mut mats, &fonts, spans, accent, origin);
            card.pos.target = Vec2::new(card_x, 0.0);
            card.size.target = Vec2::new(WIDTH, h);
            card.opacity.target = 1.0;
            view.card = Some(card);
            // Frame the subject in the space left of the card.
            let free = (cam.viewport.x - WIDTH - 64.0) / cam.viewport.x;
            let want = Vec2::new(size.x / free.max(0.3), size.y) * 2.6;
            let shift = cam.right() * (want.x * (1.0 - free) * 0.5);
            cam.fit(center + shift, want, false);
        }
    }

    // Grow the open card to fit its text once laid out.
    if let Some(card) = view.card.as_mut() {
        if let Ok(info) = infos.get(card.text) {
            if info.size.y > 0.0 {
                card.size.target.y = (info.size.y + INSET.y * 2.0).min(cam.viewport.y - 100.0);
            }
        }
        card.pos.target.x = card_x;
    }

    let view = &mut *view;
    let open = view.card.iter_mut().map(|c| (c, true));
    let closing = view.closing.iter_mut().map(|c| (c, false));
    let mut done = Vec::new();
    for (card, is_open) in open.chain(closing) {
        card.pos.step(dt, 12.0, 0.9);
        card.size.step(dt, 12.0, 0.9);
        card.opacity.step(dt, if is_open { 10.0 } else { 14.0 }, 1.0);
        let op = card.opacity.value.clamp(0.0, 1.0);
        let size = card.size.value.max(Vec2::splat(1.0));
        if let Ok(mut t) = transforms.get_mut(card.root) {
            t.translation = card.pos.value.extend(Z);
        }
        if let Ok(mut t) = transforms.get_mut(card.quad) {
            t.scale = (size + Vec2::splat(2.0 * PAD)).extend(1.0);
        }
        if let Ok(mut t) = transforms.get_mut(card.text) {
            t.translation = (Vec2::new(-size.x, size.y) * 0.5 + Vec2::new(INSET.x, -INSET.y)).extend(0.5);
        }
        let grown = ((size.x / WIDTH - 0.85) / 0.15).clamp(0.0, 1.0);
        for &(e, c) in &card.texts {
            if let Ok(mut tc) = colors.get_mut(e) {
                tc.0 = c.with_alpha(c.alpha() * op * grown);
            }
        }
        if let Some(mat) = mats.get_mut(&card.material) {
            mat.u = CardUniform {
                fill: lin(Color::srgb(0.008, 0.02, 0.035)),
                border: lin(card.accent.with_alpha(0.9)),
                glow: lin(card.accent).truncate().extend(0.35),
                dims: Vec4::new(size.x, size.y, 14.0, 1.5),
                p: Vec4::new(op, 0.0, 1.0, 0.0),
                q: Vec4::new(PAD, 0.0, 0.0, 0.0),
            };
        }
        if !is_open && op < 0.01 && card.opacity.target == 0.0 {
            commands.entity(card.root).despawn();
            done.push(card.root);
        }
    }
    view.closing.retain(|c| !done.contains(&c.root));
    view.rect = view.card.as_ref().map(|c| Rect::from_center_size(c.pos.value, c.size.value));
}
