//! Zooming into a single card or relationship: a detail card grows out of it, the camera
//! moves in, and everything else is set back.

use bevy::{
    prelude::*,
    sprite::Anchor,
    text::{LineBreak, TextBounds, TextLayoutInfo},
};

use crate::camera::Cam;
use crate::model::{Id, Model};
use crate::render::{lin, CardMaterial, CardUniform, Spring};
use crate::scene::{
    text_font, Detail, Diagram, EdgeSet, Elem, ElemIndex, Fonts, SceneAssets, ScaledText, TextRes,
    ACCENT, MUTED, PAD, TEXT,
};

const WIDTH: f32 = 640.0;
const INSET: Vec2 = Vec2::new(40.0, 34.0);
/// Above every diagram layer.
const Z: f32 = 160.0;
/// Room left between a relationship's two ends and its detail card below them.
const GAP: f32 = 56.0;

#[derive(Resource, Default)]
pub struct DetailView {
    /// How far the rest of the diagram is set back (0..1).
    pub push: Spring<f32>,
    /// Elements that stay in front while a detail is open.
    pub keep: Vec<Id>,
    /// Where the open detail card is, for hit testing.
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
    /// Where it grew from, and shrinks back into.
    origin: Rect,
    /// A relationship card hangs below its two ends: the y of its top edge, and those ends.
    hang: Option<(f32, Rect)>,
    fitted: bool,
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
    Color::srgba(0.92, 0.94, 1.0, alpha)
}

fn element_spans(model: &Model, id: Id) -> (Vec<Span>, Color) {
    let el = &model.elements[id];
    let color = Color::srgb(el.color[0], el.color[1], el.color[2]);
    let mut out = Vec::new();
    let header = [el.kind.to_uppercase(), el.technology.clone()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("  ·  ");
    out.push(span(header, 12.5, FontWeight::SEMIBOLD, color.mix(&Color::WHITE, 0.5)));
    out.push(span(format!("\n{}", el.title), 32.0, FontWeight::BOLD, TEXT));
    if !el.description.is_empty() {
        out.push(span(format!("\n\n{}", el.description), 17.0, FontWeight::NORMAL, soft(0.85)));
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
        out.push(span(format!("\n\n{head}\n"), 12.0, FontWeight::SEMIBOLD, MUTED));
        let n = lines.len();
        let shown: Vec<String> = lines.into_iter().take(14).collect();
        out.push(span(shown.join("\n"), 15.5, FontWeight::NORMAL, soft(0.88)));
        if n > 14 {
            out.push(span(format!("\nand {} more", n - 14), 14.0, FontWeight::NORMAL, MUTED));
        }
    }
    out.push(span(format!("\n\n{}", el.fqn), 12.0, FontWeight::NORMAL, soft(0.35)));
    (out, color)
}

fn relation_spans(model: &Model, a: Id, b: Id) -> (Vec<Span>, Color) {
    let mut out = vec![
        span("RELATIONSHIP", 12.5, FontWeight::SEMIBOLD, ACCENT.mix(&Color::WHITE, 0.3)),
        span(
            format!("\n{}  →  {}", model.elements[a].title, model.elements[b].title),
            28.0,
            FontWeight::BOLD,
            TEXT,
        ),
    ];
    // A line between two cards can stand for several relations between things inside them.
    let underlying: Vec<_> = model
        .relations
        .iter()
        .filter(|r| model.is_ancestor_or_self(a, r.source) && model.is_ancestor_or_self(b, r.target))
        .collect();
    for r in underlying.iter().take(12) {
        let title = if r.title.is_empty() { "(untitled)" } else { r.title.as_str() };
        out.push(span(format!("\n\n{title}"), 18.0, FontWeight::SEMIBOLD, TEXT));
        let mut via = format!(
            "\n{}  →  {}",
            model.elements[r.source].title, model.elements[r.target].title
        );
        if !r.technology.is_empty() {
            via.push_str(&format!("   [{}]", r.technology));
        }
        out.push(span(via, 14.5, FontWeight::NORMAL, soft(0.6)));
    }
    if underlying.len() > 12 {
        out.push(span(
            format!("\n\nand {} more", underlying.len() - 12),
            14.0,
            FontWeight::NORMAL,
            MUTED,
        ));
    }
    (out, ACCENT)
}

/// Rough height before the text has been laid out, so the camera can start moving at once.
fn estimate_height(spans: &[Span]) -> f32 {
    let chars_per_line = 62.0;
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

#[allow(clippy::too_many_arguments)]
fn spawn_card(
    commands: &mut Commands,
    assets: &SceneAssets,
    mats: &mut Assets<CardMaterial>,
    fonts: &Fonts,
    k: f32,
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
        ))
        .id();
    let quad = commands
        .spawn((
            Mesh2d(assets.quad.clone()),
            MeshMaterial2d(material.clone()),
            Transform::default(),
            ChildOf(root),
        ))
        .id();
    let wrap = WIDTH - INSET.x * 2.0;
    let mut iter = spans.into_iter();
    let first = iter.next().unwrap_or_else(|| span("", 12.0, FontWeight::NORMAL, TEXT));
    let text = commands
        .spawn((
            Text2d::new(first.text),
            text_font(fonts, first.size, first.weight, k),
            TextColor(first.color.with_alpha(0.0)),
            TextLayout::new(Justify::Left, LineBreak::WordBoundary),
            TextBounds::new_horizontal(wrap * k),
            ScaledText {
                size: first.size,
                bounds: Some(wrap),
            },
            Anchor::TOP_LEFT,
            Transform::from_xyz(0.0, 0.0, 0.5).with_scale(Vec3::splat(1.0 / k)),
            ChildOf(root),
        ))
        .id();
    let mut texts = vec![(text, first.color)];
    for s in iter {
        let e = commands
            .spawn((
                TextSpan::new(s.text),
                text_font(fonts, s.size, s.weight, k),
                TextColor(s.color.with_alpha(0.0)),
                ScaledText {
                    size: s.size,
                    bounds: None,
                },
                ChildOf(text),
            ))
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
        size: Spring::new(origin.size()),
        opacity: Spring::new(0.0),
        origin,
        hang: None,
        fitted: false,
    }
}

fn frame_camera(cam: &mut Cam, rect: Rect, extra: Option<Rect>) {
    let fit = extra.map_or(rect, |e| e.union(rect));
    cam.fit(fit, false);
    cam.vanish.target = rect.center() - cam.center.target;
}

#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    time: Res<Time>,
    diagram: Res<Diagram>,
    mut view: ResMut<DetailView>,
    mut cam: ResMut<Cam>,
    elems: Query<&Elem>,
    index: Res<ElemIndex>,
    edges: Res<EdgeSet>,
    fonts: Res<Fonts>,
    text_res: Res<TextRes>,
    assets: Res<SceneAssets>,
    mut mats: ResMut<Assets<CardMaterial>>,
    mut transforms: Query<&mut Transform>,
    infos: Query<&TextLayoutInfo>,
    mut colors: Query<&mut TextColor>,
) {
    let dt = time.delta_secs().min(0.05);
    let model = &diagram.model;
    let screen_rect = |id: Id| -> Option<Rect> {
        let e = index.0.get(&model.elements.get(id)?.fqn)?;
        elems.get(*e).ok().map(|el| el.screen_rect(&cam))
    };

    if diagram.detail != view.shown {
        view.shown = diagram.detail;
        if let Some(mut old) = view.card.take() {
            old.pos.target = old.origin.center();
            old.size.target = old.origin.size();
            old.opacity.target = 0.0;
            view.closing.push(old);
        }
        let opened = match diagram.detail {
            Some(Detail::Element(id)) => screen_rect(id).map(|origin| {
                let (spans, accent) = element_spans(model, id);
                (spans, accent, origin, None, vec![id])
            }),
            Some(Detail::Relation(a, b)) => screen_rect(a).zip(screen_rect(b)).map(|(ra, rb)| {
                let ends = ra.union(rb);
                let (spans, accent) = relation_spans(model, a, b);
                let fa = &model.elements[a].fqn;
                let fb = &model.elements[b].fqn;
                let origin = edges
                    .label_rect(fa, fb)
                    .unwrap_or_else(|| Rect::from_center_size(ends.center(), Vec2::splat(24.0)));
                (spans, accent, origin, Some((ends.min.y - GAP, ends)), vec![a, b])
            }),
            None => None,
        };
        match opened {
            Some((spans, accent, origin, hang, keep)) => {
                let h = estimate_height(&spans);
                let mut card = spawn_card(
                    &mut commands,
                    &assets,
                    &mut mats,
                    &fonts,
                    text_res.k,
                    spans,
                    accent,
                    origin,
                );
                let center = match hang {
                    Some((top, _)) => Vec2::new(origin.center().x, top - h / 2.0),
                    None => origin.center(),
                };
                card.pos.target = center;
                card.size.target = Vec2::new(WIDTH, h);
                card.opacity.target = 1.0;
                card.hang = hang;
                frame_camera(
                    &mut cam,
                    Rect::from_center_size(center, Vec2::new(WIDTH, h)),
                    hang.map(|(_, ends)| ends),
                );
                view.card = Some(card);
                view.keep = keep;
                view.push.target = 1.0;
            }
            None => view.push.target = 0.0,
        }
    }
    view.push.step(dt, 9.0, 1.0);
    if view.push.value < 0.01 && view.card.is_none() {
        view.keep.clear();
    }

    // Grow the open card to fit its text once that has been laid out.
    if let Some(card) = view.card.as_mut() {
        if let Ok(info) = infos.get(card.text) {
            if info.size.y > 0.0 {
                let h = info.size.y / text_res.k + INSET.y * 2.0;
                card.size.target.y = h;
                if let Some((top, _)) = card.hang {
                    card.pos.target.y = top - h / 2.0;
                }
                if !card.fitted {
                    card.fitted = true;
                    let rect = Rect::from_center_size(card.pos.target, card.size.target);
                    frame_camera(&mut cam, rect, card.hang.map(|(_, ends)| ends));
                }
            }
        }
    }

    let k = text_res.k;
    // Split the borrow so the open and closing cards can be animated together.
    let view = &mut *view;
    let open = view.card.iter_mut().map(|c| (c, true));
    let closing = view.closing.iter_mut().map(|c| (c, false));
    let mut done = Vec::new();
    for (card, is_open) in open.chain(closing) {
        card.pos.step(dt, 11.0, 0.85);
        card.size.step(dt, 11.0, 0.85);
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
            t.scale = Vec3::splat(1.0 / k);
        }
        // Text appears once the card is nearly full size, so it never spills out.
        let grown = ((size.x / WIDTH - 0.8) / 0.2).clamp(0.0, 1.0);
        for &(e, c) in &card.texts {
            if let Ok(mut tc) = colors.get_mut(e) {
                tc.0 = c.with_alpha(c.alpha() * op * grown);
            }
        }
        if let Some(mat) = mats.get_mut(&card.material) {
            mat.u = CardUniform {
                fill: lin(Color::srgb(0.07, 0.08, 0.11)),
                border: lin(card.accent.mix(&Color::WHITE, 0.25).with_alpha(0.75)),
                glow: lin(card.accent).truncate().extend(0.22),
                dims: Vec4::new(size.x, size.y, 20.0, 1.5),
                p: Vec4::new(op, 0.0, 1.4, 0.0),
                q: Vec4::new(PAD, 0.0, 0.0, 0.0),
            };
        }
        if !is_open && op < 0.01 && card.opacity.target == 0.0 {
            commands.entity(card.root).despawn();
            done.push(card.root);
        }
    }
    view.closing.retain(|c| !done.contains(&c.root));
    view.rect = view
        .card
        .as_ref()
        .map(|c| Rect::from_center_size(c.pos.value, c.size.value));
}
