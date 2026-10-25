//! Everything in the world: one entity per element (a card that can morph into a frame),
//! the relationship ribbons, and the view transitions that tie them together.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::SystemTime;

use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::NoFrustumCulling,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    sprite::Anchor,
    text::{LineBreak, TextBounds, TextLayoutInfo},
    window::{CursorIcon, PrimaryWindow, SystemCursorIcon},
};

use crate::camera::Cam;
use crate::layout::{self, Role, ViewLayout, CARD};
use crate::model::{Id, Model, Shape};
use crate::parser;
use crate::render::{lin, CardMaterial, CardUniform, Spring};

/// Room around each card's quad for its shadow, glow and person-shape head.
/// z of the front layer; each layer further back sits `Z_LAYER` lower. Edges between front
/// cards draw just above the front frame, edges reaching back just below it.
const Z_FRONT: f32 = 100.0;
const Z_LAYER: f32 = 20.0;
const Z_EDGES_FRONT: f32 = Z_FRONT + 2.5;
const Z_EDGES_BACK: f32 = Z_FRONT - 1.0;

pub const PAD: f32 = 84.0;
pub const BG: Color = Color::srgb(0.043, 0.051, 0.067);
pub const TEXT: Color = Color::srgb(0.96, 0.97, 0.99);
pub const MUTED: Color = Color::srgba(0.86, 0.89, 0.95, 0.62);
pub const ACCENT: Color = Color::srgb(0.48, 0.64, 1.0);

#[derive(Resource)]
pub struct Diagram {
    pub model: Model,
    pub warnings: Vec<String>,
    pub path: Option<PathBuf>,
    /// A model (from a path or the built-in example) is open, rather than the welcome screen.
    pub loaded: bool,
    /// Why the last attempt to open something failed.
    pub error: Option<String>,
    pub focus: Option<Id>,
    /// A card or relationship zoomed in to show its details.
    pub detail: Option<Detail>,
    pub layout: ViewLayout,
    /// Bumped whenever the view or selection changes, so the UI knows to rebuild.
    pub version: u64,
}

/// Something opened up to show everything known about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detail {
    Element(Id),
    /// A relationship line between two visible elements (possibly several model relations
    /// lifted onto it).
    Relation(Id, Id),
}

/// The model bundled for `--example` and the welcome screen's "try the example".
pub const EXAMPLE: &str = include_str!("../examples/cloud.c4");

pub enum Source {
    Path(PathBuf),
    Example,
}

pub enum Nav {
    /// Replace the open model.
    Load(Source),
    /// Go to this focus, walking the tree one level at a time.
    Focus(Option<Id>),
    /// A single step of a walk.
    Step(Option<Id>),
    Up,
    Detail(Option<Detail>),
    Fit,
    Reload,
}

#[derive(Resource, Default)]
pub struct NavQueue(pub Vec<Nav>);

/// Time between levels when walking the tree, so each hop reads as its own zoom.
const WALK_STEP: f32 = 0.42;

/// A multi-level jump in progress: the remaining foci and a selection to make on arrival.
#[derive(Resource, Default)]
pub struct Walk {
    steps: std::collections::VecDeque<Option<Id>>,
    detail: Option<Option<Detail>>,
    timer: f32,
}

/// The foci between `from` and `to`: up to their common ancestor, then down to `to`.
fn tree_walk(model: &Model, from: Option<Id>, to: Option<Id>) -> Vec<Option<Id>> {
    let a = model.path(from);
    let b = model.path(to);
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut steps: Vec<Option<Id>> = (common..a.len())
        .rev()
        .map(|i| if i == 0 { None } else { Some(a[i - 1]) })
        .collect();
    steps.extend(b[common..].iter().map(|&id| Some(id)));
    steps
}

#[derive(Resource)]
pub struct Fonts {
    pub ui: Handle<Font>,
}

#[derive(Resource)]
pub struct SceneAssets {
    pub quad: Handle<Mesh>,
    /// Plain vertex-coloured, alpha-blended material (edges, glass slabs).
    pub vertex_colors: Handle<ColorMaterial>,
}

#[derive(Resource, Default)]
pub struct ElemIndex(pub HashMap<String, Entity>);

/// Text is rasterised at `size * k` and drawn at `1/k` scale so it stays crisp when zoomed in.
#[derive(Resource)]
pub struct TextRes {
    pub k: f32,
}

#[derive(Component)]
pub struct ScaledText {
    pub size: f32,
    pub bounds: Option<f32>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Group {
    Body,
    /// Technology and description under the title, hidden on faint context cards.
    Detail,
    Frame,
    Small,
}

pub struct Parts {
    quad: Entity,
    material: Handle<CardMaterial>,
    body: Entity,
    frame_text: Entity,
    kind_text: Entity,
    badge: Option<Entity>,
    texts: Vec<(Entity, Color, Group)>,
}

#[derive(Component)]
pub struct Elem {
    pub fqn: String,
    pub id: Id,
    pub pos: Spring<Vec2>,
    pub size: Spring<Vec2>,
    pub opacity: Spring<f32>,
    pub frame: Spring<f32>,
    pub hover: Spring<f32>,
    pub dim: Spring<f32>,
    pub external: Spring<f32>,
    /// 1 for faint context elements around the focus.
    pub ghost: Spring<f32>,
    /// How far behind the front layer this sits (0 for the open level).
    pub depth: Spring<f32>,
    /// Depth as drawn this frame, including being set back behind an open detail.
    pub render_depth: f32,
    /// How far it has been pulled from its place by hand; springs back with a wobble.
    pub grab: Spring<Vec2>,
    /// Being dragged right now.
    pub held: bool,
    /// The glass slab drawn under a container (made the first time it is one).
    slab: Option<(Entity, Handle<Mesh>)>,
    pub role: Option<Role>,
    pub delay: f32,
    pub z: f32,
    pub dead: bool,
    pub parts: Parts,
}

impl EdgeSet {
    /// Where the label (or, without one, the middle) of the `from` → `to` line is drawn.
    pub fn label_rect(&self, from: &str, to: &str) -> Option<Rect> {
        let e = self.edges.get(&(from.to_string(), to.to_string()))?;
        e.label_rect.or_else(|| {
            let mid = *e.pts.get(e.pts.len() / 2)?;
            Some(Rect::from_center_size(mid, Vec2::splat(24.0)))
        })
    }
}

/// Distance from `p` to the nearest point of a polyline.
fn polyline_distance(pts: &[Vec2], p: Vec2) -> f32 {
    pts.windows(2)
        .map(|w| {
            let (a, b) = (w[0], w[1]);
            let ab = b - a;
            let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
            p.distance(a + ab * t)
        })
        .fold(f32::INFINITY, f32::min)
}

impl Elem {
    /// Where it sits on its own layer.
    pub fn rect(&self) -> Rect {
        Rect::from_center_size(self.pos.value, self.size.value)
    }

    /// Where it appears on screen once its layer's perspective is applied.
    pub fn screen_rect(&self, cam: &Cam) -> Rect {
        let (c, k) = cam.project(self.pos.value + self.grab.value, self.render_depth);
        Rect::from_center_size(c, self.size.value * k)
    }

    fn target_rect(&self) -> Rect {
        Rect::from_center_size(self.pos.target, self.size.target)
    }
}

struct EdgeVis {
    from: String,
    to: String,
    /// The curve and label as last drawn, for hit testing.
    pts: Vec<Vec2>,
    label_rect: Option<Rect>,
    /// Rope particles and where they were last step (Verlet), on the front plane.
    rope: Vec<Vec2>,
    prev: Vec<Vec2>,
    /// The side of each end card the rope leaves from (an outward normal), kept until
    /// another side is clearly better so lines don't flip back and forth.
    side_a: Vec2,
    side_b: Vec2,
    /// Attachment points in each card's normalised local space, eased between positions.
    anchor_a: Spring<Vec2>,
    anchor_b: Spring<Vec2>,
    /// While the label is dragged: where its point on the rope is held.
    grab: Option<Vec2>,
    alpha: Spring<f32>,
    hl: Spring<f32>,
    label: String,
    label_text: Option<Entity>,
    label_bg: Option<(Entity, Handle<CardMaterial>)>,
    dead: bool,
}

#[derive(Resource)]
pub struct EdgeSet {
    edges: HashMap<(String, String), EdgeVis>,
    /// Front edges (between cards on the open level) and edges reaching back.
    meshes: [Handle<Mesh>; 2],
}

#[derive(Component)]
pub struct EdgeLabel;

#[derive(Component)]
pub struct EdgeLabelBg;

/// What a drag is doing, decided when the cursor first moves with the button down.
#[derive(Default, Clone, PartialEq)]
enum Drag {
    #[default]
    None,
    Pan,
    /// Shaking a card about by hand.
    Card(Entity),
    /// Pulling on a relationship by its name tag.
    Label((String, String)),
}

#[derive(Resource, Default)]
pub struct Pointer {
    pub hovered: Option<Entity>,
    /// A relationship line under the cursor, by (from, to) fqn.
    pub hovered_edge: Option<(String, String)>,
    pressed_edge: Option<(String, String)>,
    /// The label under the cursor when the button went down (it can be grabbed).
    pressed_label: Option<(String, String)>,
    drag: Drag,
    over_detail: bool,
    press: Option<Vec2>,
    pressed_on: Option<Entity>,
    last: Vec2,
    velocity: Vec2,
    cursor_icon: u8,
}

#[derive(Resource)]
pub struct Watch {
    timer: f32,
    pub stamp: Option<SystemTime>,
}

fn latest_mtime(path: &PathBuf) -> Option<SystemTime> {
    parser::source_files(path)
        .iter()
        .filter_map(|f| std::fs::metadata(f).and_then(|m| m.modified()).ok())
        .max()
}

pub fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut colors: ResMut<Assets<ColorMaterial>>,
    mut fonts: ResMut<Assets<Font>>,
    diagram: Res<Diagram>,
) {
    let quad = meshes.add(Rectangle::new(1.0, 1.0));

    // Prefer the system UI font; fall back to Bevy's built-in one elsewhere.
    let ui = [
        "/System/Library/Fonts/SFNS.ttf",
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "C:\\Windows\\Fonts\\segoeui.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ]
    .iter()
    .find_map(|p| std::fs::read(p).ok())
    .and_then(|bytes| Font::try_from_bytes(bytes).ok())
    .map(|f| fonts.add(f))
    .unwrap_or_default();
    commands.insert_resource(Fonts { ui });

    let edge_material = colors.add(ColorMaterial {
        color: Color::WHITE,
        alpha_mode: bevy::sprite_render::AlphaMode2d::Blend,
        ..default()
    });
    commands.insert_resource(SceneAssets {
        quad,
        vertex_colors: edge_material.clone(),
    });
    let edge_meshes = [Z_EDGES_FRONT, Z_EDGES_BACK].map(|z| {
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; 3]);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[0.0f32; 4]; 3]);
        mesh.insert_indices(Indices::U32(vec![0, 1, 2]));
        let mesh = meshes.add(mesh);
        commands.spawn((
            Mesh2d(mesh.clone()),
            MeshMaterial2d(edge_material.clone()),
            Transform::from_xyz(0.0, 0.0, z),
            NoFrustumCulling,
        ));
        mesh
    });
    commands.insert_resource(EdgeSet {
        edges: HashMap::new(),
        meshes: edge_meshes,
    });
    commands.insert_resource(Watch {
        timer: 0.0,
        stamp: diagram.path.as_ref().and_then(latest_mtime),
    });
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    let cut = cut.rsplit_once(' ').map(|(a, _)| a).unwrap_or(&cut);
    format!("{}…", cut.trim_end_matches([',', '.', ';', ':']))
}

pub fn text_font(fonts: &Fonts, size: f32, weight: FontWeight, k: f32) -> TextFont {
    TextFont {
        font: fonts.ui.clone(),
        font_size: size * k,
        weight,
        ..default()
    }
}

fn build_parts(
    commands: &mut Commands,
    root: Entity,
    model: &Model,
    id: Id,
    assets: &SceneAssets,
    mats: &mut Assets<CardMaterial>,
    fonts: &Fonts,
    k: f32,
) -> Parts {
    let el = &model.elements[id];
    let material = mats.add(CardMaterial {
        u: CardUniform::default(),
    });
    let quad = commands
        .spawn((
            Mesh2d(assets.quad.clone()),
            MeshMaterial2d(material.clone()),
            Transform::default(),
            ChildOf(root),
        ))
        .id();

    let mut texts = Vec::new();
    let inv = Vec3::splat(1.0 / k);

    let body_w = CARD.x - 44.0;
    let body = commands
        .spawn((
            Text2d::new(truncate(&el.title, 48)),
            text_font(fonts, 21.0, FontWeight::SEMIBOLD, k),
            TextColor(TEXT),
            TextLayout::new(Justify::Center, LineBreak::WordBoundary),
            TextBounds::new_horizontal(body_w * k),
            ScaledText {
                size: 21.0,
                bounds: Some(body_w),
            },
            Anchor::CENTER,
            Transform::from_xyz(0.0, 0.0, 0.2).with_scale(inv),
            ChildOf(root),
        ))
        .id();
    texts.push((body, TEXT, Group::Body));
    if !el.technology.is_empty() {
        let c = Color::srgba(0.92, 0.94, 1.0, 0.72);
        let span = commands
            .spawn((
                TextSpan::new(format!("\n[{}]", truncate(&el.technology, 40))),
                text_font(fonts, 13.5, FontWeight::MEDIUM, k),
                TextColor(c),
                ScaledText {
                    size: 13.5,
                    bounds: None,
                },
                ChildOf(body),
            ))
            .id();
        texts.push((span, c, Group::Detail));
    }
    if !el.description.is_empty() {
        let c = Color::srgba(0.92, 0.94, 1.0, 0.82);
        let span = commands
            .spawn((
                TextSpan::new(format!("\n{}", truncate(&el.description, 96))),
                text_font(fonts, 14.5, FontWeight::NORMAL, k),
                TextColor(c),
                ScaledText {
                    size: 14.5,
                    bounds: None,
                },
                ChildOf(body),
            ))
            .id();
        texts.push((span, c, Group::Detail));
    }

    let frame_text = commands
        .spawn((
            Text2d::new(el.title.clone()),
            text_font(fonts, 24.0, FontWeight::BOLD, k),
            TextColor(TEXT),
            ScaledText {
                size: 24.0,
                bounds: None,
            },
            Anchor::TOP_LEFT,
            Transform::from_xyz(0.0, 0.0, 0.2).with_scale(inv),
            ChildOf(root),
        ))
        .id();
    texts.push((frame_text, TEXT, Group::Frame));
    let sub = [el.kind.as_str(), el.technology.as_str()]
        .iter()
        .filter(|s| !s.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join("  ·  ");
    if !sub.is_empty() {
        let span = commands
            .spawn((
                TextSpan::new(format!("    {sub}")),
                text_font(fonts, 15.0, FontWeight::MEDIUM, k),
                TextColor(MUTED),
                ScaledText {
                    size: 15.0,
                    bounds: None,
                },
                ChildOf(frame_text),
            ))
            .id();
        texts.push((span, MUTED, Group::Frame));
    }

    let small = Color::srgba(0.94, 0.96, 1.0, 0.66);
    let kind_text = commands
        .spawn((
            Text2d::new(el.kind.to_uppercase()),
            text_font(fonts, 10.5, FontWeight::SEMIBOLD, k),
            TextColor(small),
            ScaledText {
                size: 10.5,
                bounds: None,
            },
            Anchor::TOP_LEFT,
            Transform::from_xyz(0.0, 0.0, 0.2).with_scale(inv),
            ChildOf(root),
        ))
        .id();
    texts.push((kind_text, small, Group::Small));

    let badge = (!el.children.is_empty()).then(|| {
        let n = el.children.len();
        let e = commands
            .spawn((
                Text2d::new(format!("{n} inside  ›")),
                text_font(fonts, 11.5, FontWeight::MEDIUM, k),
                TextColor(small),
                ScaledText {
                    size: 11.5,
                    bounds: None,
                },
                Anchor::BOTTOM_RIGHT,
                Transform::from_xyz(0.0, 0.0, 0.2).with_scale(inv),
                ChildOf(root),
            ))
            .id();
        texts.push((e, small, Group::Small));
        e
    });

    Parts {
        quad,
        material,
        body,
        frame_text,
        kind_text,
        badge,
        texts,
    }
}

fn despawn_parts(commands: &mut Commands, parts: &Parts) {
    for e in [parts.quad, parts.body, parts.frame_text, parts.kind_text] {
        commands.entity(e).despawn();
    }
    if let Some(b) = parts.badge {
        commands.entity(b).despawn();
    }
}

/// How strongly each role is drawn: context fades with every level further out.
fn role_alpha(role: Role) -> f32 {
    match role {
        Role::Context(level) => 0.5 * 0.6f32.powi(level as i32 - 1),
        Role::ContextFrame(level) => 0.75 * 0.65f32.powi(level as i32 - 1),
        _ => 1.0,
    }
}

/// Maps `r` from `from`'s coordinate frame into `to`'s, so things can grow out of (or
/// shrink into) a parent that is itself changing size.
fn map_rect(r: Rect, from: Rect, to: Rect) -> Rect {
    let ratio = to.size() / from.size().max(Vec2::splat(1.0));
    let c = to.center() + (r.center() - from.center()) * ratio;
    let s = r.size() * ratio.min_element().clamp(0.08, 1.0);
    Rect::from_center_size(c, s)
}

#[allow(clippy::too_many_arguments)]
pub fn apply_nav(
    mut commands: Commands,
    mut queue: ResMut<NavQueue>,
    mut diagram: ResMut<Diagram>,
    mut cam: ResMut<Cam>,
    mut elems: Query<(Entity, &mut Elem, &mut Visibility)>,
    mut index: ResMut<ElemIndex>,
    mut edge_set: ResMut<EdgeSet>,
    mut mats: ResMut<Assets<CardMaterial>>,
    assets: Res<SceneAssets>,
    fonts: Res<Fonts>,
    text_res: Res<TextRes>,
    time: Res<Time>,
    mut walk: ResMut<Walk>,
    mut watch: ResMut<Watch>,
) {
    if !walk.steps.is_empty() || walk.detail.is_some() {
        walk.timer -= time.delta_secs();
        if walk.timer <= 0.0 {
            if let Some(step) = walk.steps.pop_front() {
                queue.0.push(Nav::Step(step));
                walk.timer = WALK_STEP;
            } else if let Some(sel) = walk.detail.take() {
                queue.0.push(Nav::Detail(sel));
            }
        }
    }
    if queue.0.is_empty() {
        return;
    }
    let mut relayout = false;
    let mut intro = false;
    // Set when the old model's entities were thrown away this frame (still in the queries).
    let mut cleared = false;
    let prev_focus = diagram.focus;
    for nav in std::mem::take(&mut queue.0) {
        match nav {
            Nav::Focus(f) => {
                walk.steps = tree_walk(&diagram.model, diagram.focus, f).into();
                walk.detail = None;
                if let Some(first) = walk.steps.pop_front() {
                    diagram.focus = first;
                    relayout = true;
                    walk.timer = WALK_STEP;
                }
            }
            Nav::Step(f) => {
                diagram.focus = f;
                relayout = true;
            }
            Nav::Up => {
                walk.steps.clear();
                walk.detail = None;
                if let Some(f) = diagram.focus {
                    diagram.focus = diagram.model.elements[f].parent;
                    relayout = true;
                }
            }
            // Mid-walk, the selection waits until the walk arrives.
            Nav::Detail(d) if !walk.steps.is_empty() => walk.detail = Some(d),
            Nav::Detail(d) => {
                // Closing a detail returns the camera to the level it was opened from.
                if d.is_none() && diagram.detail.is_some() {
                    cam.fit(diagram.layout.bounds, false);
                    cam.vanish.target = diagram.layout.anchor - cam.center.target;
                }
                diagram.detail = d;
                diagram.version += 1;
            }
            Nav::Fit => cam.fit(diagram.layout.bounds, false),
            Nav::Load(source) => {
                let loaded = match &source {
                    Source::Path(p) => parser::load(p),
                    Source::Example => Ok(parser::parse_sources(&[("example.c4".into(), EXAMPLE.into())])),
                };
                match loaded {
                    Ok((model, warnings)) => {
                        for (e, _, _) in &elems {
                            commands.entity(e).despawn();
                        }
                        index.0.clear();
                        for (_, edge) in edge_set.edges.drain() {
                            if let Some(t) = edge.label_text {
                                commands.entity(t).despawn();
                            }
                            if let Some((b, _)) = edge.label_bg {
                                commands.entity(b).despawn();
                            }
                        }
                        cleared = true;
                        diagram.model = model;
                        diagram.warnings = warnings;
                        diagram.path = match source {
                            Source::Path(p) => Some(p),
                            Source::Example => None,
                        };
                        watch.stamp = diagram.path.as_ref().and_then(latest_mtime);
                        diagram.loaded = true;
                        diagram.error = None;
                        diagram.focus = None;
                        diagram.detail = None;
                        walk.steps.clear();
                        walk.detail = None;
                        intro = true;
                        relayout = true;
                    }
                    Err(e) => {
                        diagram.error = Some(e);
                        diagram.version += 1;
                    }
                }
            }
            Nav::Reload => {
                let Some(path) = diagram.path.clone() else { continue };
                match parser::load(&path) {
                    Ok((model, warnings)) => {
                        let old = &diagram.model;
                        let fqn_of = |id: Option<Id>| id.map(|i| old.elements[i].fqn.clone());
                        let focus = fqn_of(diagram.focus);
                        let detail = match diagram.detail {
                            Some(Detail::Element(id)) => fqn_of(Some(id)),
                            _ => None,
                        };
                        let remap = |f: Option<String>| f.and_then(|f| model.by_fqn.get(&f).copied());
                        diagram.focus = remap(focus);
                        diagram.detail = remap(detail).map(Detail::Element);
                        for (_, mut elem, _) in &mut elems {
                            // The landscape has no name; it carries over by being the root.
                            let same = match elem.fqn.is_empty() {
                                true => model.root,
                                false => model.by_fqn.get(&elem.fqn).copied(),
                            };
                            match same {
                                Some(id) => {
                                    elem.id = id;
                                    despawn_parts(&mut commands, &elem.parts);
                                    let root = index.0[&elem.fqn];
                                    elem.parts = build_parts(
                                        &mut commands,
                                        root,
                                        &model,
                                        id,
                                        &assets,
                                        &mut mats,
                                        &fonts,
                                        text_res.k,
                                    );
                                }
                                None => {
                                    elem.dead = true;
                                    elem.role = None;
                                    elem.opacity.target = 0.0;
                                }
                            }
                        }
                        diagram.model = model;
                        diagram.warnings = warnings;
                        relayout = true;
                    }
                    Err(e) => diagram.warnings = vec![format!("reload failed: {e}")],
                }
            }
        }
    }
    if !relayout {
        return;
    }
    if prev_focus != diagram.focus {
        diagram.detail = None;
    }
    diagram.version += 1;
    let model = &diagram.model;
    let focus = diagram.focus;
    let mut view = layout::compose(model, focus, cam.avail());

    // What is on screen right now, keyed by model id.
    let visible: HashMap<Id, (Rect, Rect)> = elems
        .iter()
        .filter(|(_, e, _)| !cleared && e.role.is_some() && !e.dead)
        .map(|(_, e, _)| (e.id, (e.rect(), e.target_rect())))
        .collect();

    // Anchor the new layout on an element that is in both views, so the transition reads as
    // one continuous space instead of a cut.
    let pivot = [focus, prev_focus]
        .into_iter()
        .flatten()
        .chain(view.nodes.iter().map(|p| p.id))
        .find(|id| visible.contains_key(id) && view.get(*id).is_some());
    let offset = match pivot {
        Some(p) => visible[&p].1.center() - view.get(p).unwrap().rect.center(),
        None => cam.center.target - view.bounds.center(),
    };
    for n in &mut view.nodes {
        n.rect = Rect::from_center_size(n.rect.center() + offset, n.rect.size());
    }
    view.bounds = Rect::from_center_size(view.bounds.center() + offset, view.bounds.size());
    view.anchor += offset;
    let pivot_pt = pivot
        .and_then(|p| view.get(p))
        .map(|p| p.rect.center())
        .unwrap_or(view.bounds.center());

    let new_ids: HashSet<Id> = view.nodes.iter().map(|p| p.id).collect();

    // Nearest ancestor that is visible now and also placed in the new view.
    let anchor_ancestor = |id: Id| -> Option<Id> {
        let mut cur = model.elements[id].parent;
        while let Some(c) = cur {
            if visible.contains_key(&c) && new_ids.contains(&c) {
                return Some(c);
            }
            cur = model.elements[c].parent;
        }
        None
    };

    // Elements leaving the view.
    for (_, mut elem, _) in &mut elems {
        if cleared || elem.role.is_none() || new_ids.contains(&elem.id) || elem.dead {
            continue;
        }
        let cur = elem.rect();
        let target = match anchor_ancestor(elem.id) {
            Some(a) => map_rect(cur, visible[&a].0, view.get(a).unwrap().rect),
            None => {
                let c = cur.center();
                Rect::from_center_size(c + (c - pivot_pt) * 0.9, cur.size())
            }
        };
        elem.pos.target = target.center();
        elem.size.target = target.size();
        elem.opacity.target = 0.0;
        elem.role = None;
        elem.delay = 0.0;
    }

    // Elements in the new view, appearing ones staggered outward from the pivot.
    let mut appear_order: Vec<(f32, Id)> = view
        .nodes
        .iter()
        .filter(|p| !visible.contains_key(&p.id))
        .map(|p| (p.rect.center().distance(pivot_pt), p.id))
        .collect();
    appear_order.sort_by(|a, b| a.0.total_cmp(&b.0));
    let rank: HashMap<Id, usize> = appear_order.iter().enumerate().map(|(i, (_, id))| (*id, i)).collect();

    for (i, place) in view.nodes.iter().enumerate() {
        let el = &model.elements[place.id];
        // Each layer gets its own band of z, nearer layers on top; frames under cards.
        let layer = Z_FRONT - Z_LAYER * place.level as f32;
        let z = match place.role {
            Role::Frame | Role::ContextFrame(_) => layer + model.depth(place.id) as f32 * 0.05,
            _ => layer + 5.0 + i as f32 * 0.01,
        };
        let depth_t = layout::DEPTH * place.level as f32;
        let frame_t = if place.role.is_frame() { 1.0 } else { 0.0 };
        let ext_t = if place.role == Role::External { 1.0 } else { 0.0 };
        let ghost_t = if matches!(place.role, Role::Context(_) | Role::ContextFrame(_)) { 1.0 } else { 0.0 };
        let alpha = role_alpha(place.role);
        let appearing = !visible.contains_key(&place.id);
        let start = if appearing {
            match anchor_ancestor(place.id) {
                Some(a) => map_rect(place.rect, view.get(a).unwrap().rect, visible[&a].0),
                None => {
                    let c = place.rect.center();
                    let push = if intro { -0.25 } else { 0.45 };
                    Rect::from_center_size(c + (c - pivot_pt) * push, place.rect.size() * 0.92)
                }
            }
        } else {
            place.rect
        };
        let delay = if appearing {
            let base = if intro { 0.05 } else { 0.08 };
            base + (rank[&place.id] as f32 * 0.03).min(0.3)
        } else {
            0.0
        };

        let existing = index.0.get(&el.fqn).copied();
        match existing.and_then(|e| elems.get_mut(e).ok()) {
            Some((_, mut elem, mut vis)) => {
                if appearing {
                    elem.pos.snap(start.center());
                    elem.size.snap(start.size());
                    elem.frame.snap(frame_t);
                    elem.external.snap(ext_t);
                    elem.ghost.snap(ghost_t);
                    elem.depth.snap(depth_t);
                    elem.opacity.snap(0.0);
                    elem.delay = delay;
                }
                elem.dead = false;
                elem.id = place.id;
                elem.pos.target = place.rect.center();
                elem.size.target = place.rect.size();
                elem.opacity.target = alpha;
                elem.frame.target = frame_t;
                elem.external.target = ext_t;
                elem.ghost.target = ghost_t;
                elem.depth.target = depth_t;
                elem.role = Some(place.role);
                elem.z = z;
                *vis = Visibility::Inherited;
            }
            None => {
                let root = commands
                    .spawn((
                        Transform::from_translation(start.center().extend(z)),
                        Visibility::Inherited,
                    ))
                    .id();
                let parts = build_parts(
                    &mut commands,
                    root,
                    model,
                    place.id,
                    &assets,
                    &mut mats,
                    &fonts,
                    text_res.k,
                );
                let mut elem = Elem {
                    fqn: el.fqn.clone(),
                    id: place.id,
                    pos: Spring::new(start.center()),
                    size: Spring::new(start.size()),
                    opacity: Spring::new(0.0),
                    frame: Spring::new(frame_t),
                    hover: Spring::new(0.0),
                    dim: Spring::new(0.0),
                    external: Spring::new(ext_t),
                    ghost: Spring::new(ghost_t),
                    depth: Spring::new(depth_t),
                    render_depth: depth_t,
                    grab: Spring::new(Vec2::ZERO),
                    held: false,
                    slab: None,
                    role: Some(place.role),
                    delay,
                    z,
                    dead: false,
                    parts,
                };
                elem.pos.target = place.rect.center();
                elem.size.target = place.rect.size();
                elem.opacity.target = alpha;
                commands.entity(root).insert(elem);
                index.0.insert(el.fqn.clone(), root);
            }
        }
    }

    // Relationships.
    let wanted: HashMap<(String, String), &layout::EdgeSpec> = view
        .edges
        .iter()
        .map(|e| {
            (
                (model.elements[e.from].fqn.clone(), model.elements[e.to].fqn.clone()),
                e,
            )
        })
        .collect();
    for (key, edge) in edge_set.edges.iter_mut() {
        if !wanted.contains_key(key) {
            edge.alpha.target = 0.0;
            edge.dead = true;
        }
    }
    for (key, spec) in wanted {
        let label = match (spec.label.is_empty() || spec.context, spec.count) {
            _ if spec.context => String::new(),
            (true, 1) => String::new(),
            (true, n) => format!("{n} relationships"),
            (false, 1) => truncate(&spec.label, 40),
            (false, n) => format!("{}  +{}", truncate(&spec.label, 34), n - 1),
        };
        let edge = edge_set.edges.entry(key.clone()).or_insert_with(|| EdgeVis {
            from: key.0.clone(),
            to: key.1.clone(),
            pts: Vec::new(),
            label_rect: None,
            rope: Vec::new(),
            prev: Vec::new(),
            side_a: Vec2::ZERO,
            side_b: Vec2::ZERO,
            anchor_a: Spring::new(Vec2::ZERO),
            anchor_b: Spring::new(Vec2::ZERO),
            grab: None,
            alpha: Spring::new(0.0),
            hl: Spring::new(0.0),
            label: String::new(),
            label_text: None,
            label_bg: None,
            dead: false,
        });
        edge.dead = false;
        edge.alpha.target = 1.0;
        if edge.label != label {
            if let Some(t) = edge.label_text.take() {
                commands.entity(t).despawn();
            }
            if let Some((b, _)) = edge.label_bg.take() {
                commands.entity(b).despawn();
            }
            edge.label = label.clone();
            if !label.is_empty() {
                let k = text_res.k;
                edge.label_text = Some(
                    commands
                        .spawn((
                            Text2d::new(label),
                            text_font(&fonts, 12.5, FontWeight::MEDIUM, k),
                            TextColor(MUTED),
                            ScaledText {
                                size: 12.5,
                                bounds: None,
                            },
                            Transform::from_xyz(0.0, 0.0, 6.1).with_scale(Vec3::splat(1.0 / k)),
                            EdgeLabel,
                        ))
                        .id(),
                );
                let m = mats.add(CardMaterial {
                    u: CardUniform::default(),
                });
                let bg = commands
                    .spawn((
                        Mesh2d(assets.quad.clone()),
                        MeshMaterial2d(m.clone()),
                        Transform::from_xyz(0.0, 0.0, 6.0),
                        EdgeLabelBg,
                    ))
                    .id();
                edge.label_bg = Some((bg, m));
            }
        }
    }

    cam.fit(view.bounds, intro);
    let vanish = view.anchor - cam.center.target;
    if intro {
        cam.vanish.snap(vanish);
    } else {
        cam.vanish.target = vanish;
    }
    diagram.layout = view;
}

#[allow(clippy::too_many_arguments)]
pub fn pointer(
    mut commands: Commands,
    time: Res<Time>,
    mut cam: ResMut<Cam>,
    mut ptr: ResMut<Pointer>,
    mut queue: ResMut<NavQueue>,
    diagram: Res<Diagram>,
    mut elems: Query<(Entity, &mut Elem)>,
    mouse: Res<ButtonInput<MouseButton>>,
    ui: Query<&Interaction>,
    window: Single<Entity, With<PrimaryWindow>>,
    mut edge_set: ResMut<EdgeSet>,
    detail_view: Res<crate::detail::DetailView>,
) {
    cam.over_ui = ui.iter().any(|i| *i != Interaction::None);
    let world = if cam.over_ui { None } else { cam.cursor_world() };
    // While a detail is open the rest of the diagram is set back and not interactive.
    let detail_open = diagram.detail.is_some();

    // Hit test: the top-most card under the cursor, or a frame's title strip.
    let mut best: Option<(f32, Entity)> = None;
    let dragging = ptr.drag != Drag::None;
    if let Some(w) = world.filter(|_| !dragging && !detail_open) {
        for (e, elem) in &elems {
            if elem.dead {
                continue;
            }
            let r = elem.screen_rect(&cam);
            let k = r.height() / elem.size.value.y.max(1.0);
            let hit = match elem.role {
                Some(role) if role.is_frame() => r.contains(w) && w.y > r.max.y - 84.0 * k,
                Some(_) => r.contains(w),
                None => false,
            };
            if hit && best.is_none_or(|(z, _)| elem.z > z) {
                best = Some((elem.z, e));
            }
        }
    }
    ptr.hovered = match &ptr.drag {
        // A card in hand stays lit.
        Drag::Card(e) => Some(*e),
        _ => best.map(|(_, e)| e),
    };
    ptr.over_detail = world.zip(detail_view.rect).is_some_and(|(w, r)| r.contains(w));

    // Relationship lines (and their labels) are clickable where no card is.
    ptr.hovered_edge = None;
    if let (Some(w), None, false) = (world.filter(|_| !dragging), ptr.hovered, detail_open) {
        let reach = 7.0 * cam.scale();
        let mut nearest = reach;
        for (key, edge) in &edge_set.edges {
            if edge.dead || edge.alpha.target < 0.5 {
                continue;
            }
            let on_label = edge.label_rect.is_some_and(|r| r.contains(w));
            let d = if on_label { 0.0 } else { polyline_distance(&edge.pts, w) };
            if d < nearest {
                nearest = d;
                ptr.hovered_edge = Some(key.clone());
            }
        }
    }

    // Hover emphasis: lift the hovered card and dim what it is not connected to.
    let hovered_card = ptr.hovered.and_then(|e| elems.get(e).ok()).and_then(|(_, el)| {
        (!el.role.is_some_and(Role::is_frame)).then_some(el.id)
    });
    let connected: HashSet<Id> = match hovered_card {
        Some(h) => diagram
            .layout
            .edges
            .iter()
            .filter_map(|e| {
                if e.from == h {
                    Some(e.to)
                } else if e.to == h {
                    Some(e.from)
                } else {
                    None
                }
            })
            .chain([h])
            .collect(),
        None => HashSet::new(),
    };
    for (e, mut elem) in &mut elems {
        elem.hover.target = if Some(e) == ptr.hovered { 1.0 } else { 0.0 };
        let dimmed = hovered_card.is_some()
            && !elem.role.is_some_and(Role::is_frame)
            && !connected.contains(&elem.id);
        elem.dim.target = if dimmed { 1.0 } else { 0.0 };
    }

    // Press / drag / click.
    let cursor = cam.cursor;
    let pan_button = mouse.pressed(MouseButton::Left) || mouse.pressed(MouseButton::Middle);
    if (mouse.just_pressed(MouseButton::Left) || mouse.just_pressed(MouseButton::Middle))
        && !cam.over_ui
    {
        if let Some(c) = cursor {
            ptr.press = Some(c);
            ptr.pressed_on = if mouse.just_pressed(MouseButton::Left) {
                ptr.hovered
            } else {
                None
            };
            ptr.pressed_edge = ptr.hovered_edge.clone().filter(|_| mouse.just_pressed(MouseButton::Left));
            ptr.pressed_label = ptr.pressed_edge.clone().filter(|k| {
                let label = edge_set.edges.get(k).and_then(|e| e.label_rect);
                world.zip(label).is_some_and(|(w, r)| r.contains(w))
            });
            ptr.drag = Drag::None;
            ptr.last = c;
            ptr.velocity = Vec2::ZERO;
            cam.fling = Vec2::ZERO;
        }
    }
    if let (Some(start), Some(c), true) = (ptr.press, cursor, pan_button) {
        if ptr.drag == Drag::None && c.distance(start) > 4.0 {
            // Dragging a card or a name tag picks it up; dragging anything else pans.
            let card = ptr.pressed_on.filter(|e| {
                !detail_open
                    && elems
                        .get(*e)
                        .is_ok_and(|(_, el)| !el.role.is_some_and(Role::is_frame))
            });
            ptr.drag = match (card, ptr.pressed_label.clone()) {
                (Some(e), _) => Drag::Card(e),
                (None, Some(key)) if !detail_open => Drag::Label(key),
                _ => Drag::Pan,
            };
            if let Drag::Card(e) = ptr.drag {
                if let Ok((_, mut el)) = elems.get_mut(e) {
                    el.held = true;
                }
            }
            ptr.pressed_on = None;
            ptr.pressed_edge = None;
        }
        let delta = c - ptr.last;
        match ptr.drag.clone() {
            Drag::Pan => {
                cam.pan_screen(delta);
                let dt = time.delta_secs().max(1e-4);
                let v = delta * Vec2::new(-1.0, 1.0) * cam.scale() / dt;
                ptr.velocity = ptr.velocity.lerp(v, 0.35);
            }
            Drag::Card(e) => {
                if let Ok((_, mut el)) = elems.get_mut(e) {
                    // Move it with the cursor, allowing for its layer's perspective.
                    let (_, k) = cam.project(el.pos.value, el.render_depth);
                    el.grab.target += delta * Vec2::new(1.0, -1.0) * cam.scale() / k.max(0.05);
                }
            }
            Drag::Label(key) => {
                if let (Some(edge), Some(w)) = (edge_set.edges.get_mut(&key), cam.cursor_world()) {
                    edge.grab = Some(w);
                }
            }
            Drag::None => {}
        }
        ptr.last = c;
    }
    let released =
        mouse.just_released(MouseButton::Left) || mouse.just_released(MouseButton::Middle);
    if released && ptr.press.is_some() {
        let drag = std::mem::take(&mut ptr.drag);
        if drag != Drag::None {
            match drag {
                Drag::Pan if ptr.velocity.length() * time.delta_secs() > 0.5 => {
                    cam.fling = ptr.velocity * 0.9;
                }
                // Let go: it swings home (the spring in `animate` does the motion).
                Drag::Card(e) => {
                    if let Ok((_, mut el)) = elems.get_mut(e) {
                        el.held = false;
                        el.grab.target = Vec2::ZERO;
                    }
                }
                Drag::Label(key) => {
                    if let Some(edge) = edge_set.edges.get_mut(&key) {
                        edge.grab = None;
                    }
                }
                _ => {}
            }
        } else if mouse.just_released(MouseButton::Left) {
            let world = cam.cursor_world();
            if detail_open {
                if !ptr.over_detail {
                    queue.0.push(Nav::Detail(None));
                }
            } else if let Some((a, b)) = ptr.pressed_edge.clone().filter(|k| Some(k) == ptr.hovered_edge.as_ref()) {
                let model = &diagram.model;
                if let (Some(&a), Some(&b)) = (model.by_fqn.get(&a), model.by_fqn.get(&b)) {
                    queue.0.push(Nav::Detail(Some(Detail::Relation(a, b))));
                }
            } else {
                click(&ptr, &mut queue, &diagram, &elems, world);
            }
        }
        ptr.press = None;
    }
    if mouse.just_pressed(MouseButton::Right) && !cam.over_ui {
        queue.0.push(if detail_open { Nav::Detail(None) } else { Nav::Up });
    }

    let icon = if ptr.drag != Drag::None {
        2
    } else if ptr.hovered.is_some() || ptr.hovered_edge.is_some() {
        1
    } else {
        0
    };
    if icon != ptr.cursor_icon {
        ptr.cursor_icon = icon;
        let i = match icon {
            1 => SystemCursorIcon::Pointer,
            2 => SystemCursorIcon::Grabbing,
            _ => SystemCursorIcon::Default,
        };
        commands.entity(*window).insert(CursorIcon::from(i));
    }
}

fn click(
    ptr: &Pointer,
    queue: &mut NavQueue,
    diagram: &Diagram,
    elems: &Query<(Entity, &mut Elem)>,
    world: Option<Vec2>,
) {
    let target = ptr.pressed_on.filter(|e| Some(*e) == ptr.hovered);
    let Some((_, elem)) = target.and_then(|e| elems.get(e).ok()) else {
        // Empty space outside the open frame steps back out; inside it just deselects.
        let inside_frame = diagram
            .focus
            .and_then(|f| diagram.layout.get(f))
            .zip(world)
            .is_some_and(|(p, w)| p.rect.contains(w));
        if diagram.focus.is_some() && !inside_frame {
            queue.0.push(Nav::Up);
        }
        return;
    };
    let model = &diagram.model;
    let id = elem.id;
    match elem.role {
        Some(Role::Frame) => queue.0.push(Nav::Up),
        // A faint ancestor's title: walk back out to that level.
        // The landscape stands for the top level itself.
        Some(Role::ContextFrame(_)) if Some(id) == model.root => queue.0.push(Nav::Focus(None)),
        Some(Role::ContextFrame(_)) => queue.0.push(Nav::Focus(Some(id))),
        // Something outside the open frame: walk over to it, opening it if it has insides,
        // otherwise opening its parent and selecting it there.
        Some(Role::External | Role::Context(_)) if !model.elements[id].children.is_empty() => {
            queue.0.push(Nav::Focus(Some(id)));
        }
        Some(Role::External | Role::Context(_)) => {
            queue.0.push(Nav::Focus(model.elements[id].parent));
            queue.0.push(Nav::Detail(Some(Detail::Element(id))));
        }
        Some(Role::Card) if !model.elements[id].children.is_empty() => {
            queue.0.push(Nav::Focus(Some(id)));
        }
        // Nothing inside: zoom into the card itself to show its details.
        Some(Role::Card) => queue.0.push(Nav::Detail(Some(Detail::Element(id)))),
        None => {}
    }
}

pub fn keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    diagram: Res<Diagram>,
    mut menu: ResMut<crate::ui::Menu>,
    mut queue: ResMut<NavQueue>,
) {
    if !diagram.loaded {
        return;
    }
    if keys.just_pressed(KeyCode::Escape) {
        menu.open = !menu.open;
        return;
    }
    if menu.open {
        return;
    }
    if keys.just_pressed(KeyCode::Backspace) {
        if diagram.detail.is_some() {
            queue.0.push(Nav::Detail(None));
        } else {
            queue.0.push(Nav::Up);
        }
    }
    if keys.any_just_pressed([KeyCode::KeyF, KeyCode::Space, KeyCode::Digit0]) {
        queue.0.push(Nav::Fit);
    }
    if keys.just_pressed(KeyCode::KeyR) {
        queue.0.push(Nav::Reload);
    }
    if keys.just_pressed(KeyCode::Home) {
        queue.0.push(Nav::Focus(None));
    }
}

pub fn watch_files(
    time: Res<Time>,
    mut watch: ResMut<Watch>,
    diagram: Res<Diagram>,
    mut queue: ResMut<NavQueue>,
) {
    let Some(path) = &diagram.path else { return };
    watch.timer += time.delta_secs();
    if watch.timer < 0.5 {
        return;
    }
    watch.timer = 0.0;
    let stamp = latest_mtime(path);
    if stamp != watch.stamp {
        watch.stamp = stamp;
        queue.0.push(Nav::Reload);
    }
}

pub fn text_scale(
    cam: Res<Cam>,
    mut res: ResMut<TextRes>,
    mut texts: Query<(&ScaledText, &mut TextFont, Option<&mut TextBounds>)>,
) {
    let want = (1.0 / cam.scale()).log2();
    let cur = res.k.log2();
    if (want - cur).abs() < 0.75 {
        return;
    }
    let k = 2f32.powf(want.round().clamp(-2.0, 3.0));
    if k == res.k {
        return;
    }
    res.k = k;
    for (st, mut font, bounds) in &mut texts {
        font.font_size = st.size * k;
        if let (Some(mut b), Some(w)) = (bounds, st.bounds) {
            b.width = Some(w * k);
        }
    }
}

fn mix(a: Vec4, b: Vec4, t: f32) -> Vec4 {
    a + (b - a) * t
}

#[allow(clippy::too_many_arguments)]
pub fn animate(
    mut commands: Commands,
    time: Res<Time>,
    cam: Res<Cam>,
    diagram: Res<Diagram>,
    text_res: Res<TextRes>,
    mut index: ResMut<ElemIndex>,
    mut elems: Query<(Entity, &mut Elem, &mut Transform, &mut Visibility)>,
    mut parts_tf: Query<&mut Transform, Without<Elem>>,
    mut colors: Query<&mut TextColor>,
    mut mats: ResMut<Assets<CardMaterial>>,
    detail_view: Res<crate::detail::DetailView>,
    mut meshes: ResMut<Assets<Mesh>>,
    assets: Res<SceneAssets>,
) {
    let dt = time.delta_secs().min(0.05);
    let s = cam.scale();
    let push = detail_view.push.value.clamp(0.0, 1.0);
    let small_fade = 1.0 - ((s - 2.4) / 0.8).clamp(0.0, 1.0);
    let inv = Vec3::splat(1.0 / text_res.k);
    let model = &diagram.model;

    for (entity, mut elem, mut tf, mut vis) in &mut elems {
        if elem.delay > 0.0 {
            elem.delay -= dt;
        } else {
            elem.pos.step(dt, 10.5, 0.8);
            elem.size.step(dt, 10.5, 0.8);
            elem.opacity.step(dt, 9.0, 1.0);
            elem.frame.step(dt, 10.0, 1.0);
            elem.external.step(dt, 8.0, 1.0);
            elem.ghost.step(dt, 8.0, 1.0);
            elem.depth.step(dt, 8.5, 0.95);
        }
        elem.hover.step(dt, 22.0, 0.9);
        elem.dim.step(dt, 14.0, 1.0);

        let op = elem.opacity.value.clamp(0.0, 1.0);
        if elem.role.is_none() && elem.opacity.target == 0.0 && op < 0.004 {
            if elem.dead {
                index.0.remove(&elem.fqn);
                commands.entity(entity).despawn();
            } else if *vis != Visibility::Hidden {
                *vis = Visibility::Hidden;
            }
            continue;
        }

        let frame = elem.frame.value.clamp(0.0, 1.0);
        let hover = elem.hover.value;
        let dim = elem.dim.value.clamp(0.0, 1.0);
        let ext = elem.external.value.clamp(0.0, 1.0);
        let ghost = elem.ghost.value.clamp(0.0, 1.0);
        // Faint context brightens a little under the cursor, to show it is clickable.
        let op = (op + 0.3 * hover * ghost).min(1.0);
        let size = elem.size.value.max(Vec2::splat(1.0)) * (1.0 + 0.018 * hover * (1.0 - frame));
        let half = size / 2.0;
        // With a detail open, everything not part of it is set further back.
        let kept = detail_view.keep.contains(&elem.id);
        let set_back = if kept { 0.0 } else { push };
        elem.render_depth = elem.depth.value + 0.9 * set_back;
        // Held: follow the hand closely. Let go: swing home with a little weight, no bounce.
        let (omega, zeta) = if elem.held { (30.0, 0.9) } else { (13.0, 0.8) };
        elem.grab.step(dt, omega, zeta);
        let (center, k) = cam.project(elem.pos.value + elem.grab.value, elem.render_depth);
        // Set-back elements also drop below the ones kept in front of them.
        let mut z = if set_back > 0.01 { elem.z - Z_LAYER * 0.5 } else { elem.z };
        // In hand (or still bouncing back): lift it over its neighbours.
        if elem.held || elem.grab.value.length() > 2.0 {
            z += Z_LAYER * 0.45;
        }
        tf.translation = center.extend(z);
        tf.scale = Vec3::new(k, k, 1.0);

        let Some(el) = model.elements.get(elem.id) else { continue };
        let selected = kept && push > 0.01;
        let base = Color::srgb(el.color[0], el.color[1], el.color[2]);

        let ghost_fill = base.mix(&Color::srgb(0.3, 0.33, 0.4), 0.6).mix(&BG, 0.25);
        let card_fill = mix(lin(base.mix(&BG, 0.22 * ext)), lin(ghost_fill), ghost);
        // Ancestor frames are big; keep their tint light so they read as "around you".
        let frame_fill = lin(base.with_alpha(0.075 * (1.0 - 0.6 * ghost)));
        let card_border = lin(base.mix(&Color::WHITE, 0.3).with_alpha(0.9 - 0.45 * ghost));
        let frame_border = lin(base.mix(&Color::WHITE, 0.25).with_alpha(0.55));
        let mut border = mix(card_border, frame_border, frame);
        if selected {
            border = lin(ACCENT);
        }
        border = mix(border, lin(Color::WHITE), 0.35 * hover * (1.0 - frame));
        let glow_col = if selected { ACCENT } else { base.mix(&Color::WHITE, 0.4) };
        let glow = lin(glow_col).truncate().extend((0.5 * hover * (1.0 - 0.6 * ghost) + if selected { 0.35 } else { 0.0 }) * (1.0 - frame));
        let eff = op * (1.0 - 0.62 * dim) * (1.0 - 0.5 * set_back);
        let shape = if el.shape == Shape::Rectangle { 0.0 } else { el.shape.index() };
        let stacked = if el.children.is_empty() { 0.0 } else { (1.0 - frame) * (1.0 - 0.6 * ghost) };

        if let Some(mat) = mats.get_mut(&elem.parts.material) {
            mat.u = CardUniform {
                fill: mix(card_fill, frame_fill, frame),
                border,
                glow,
                dims: Vec4::new(size.x, size.y, 14.0 + 8.0 * frame, 1.5),
                p: Vec4::new(eff, shape, (1.0 - frame) * (1.0 - ghost) * (1.0 + 0.6 * hover), stacked),
                q: Vec4::new(PAD, 1.0 - frame, 0.0, 0.0),
            };
        }
        if let Ok(mut q) = parts_tf.get_mut(elem.parts.quad) {
            q.scale = (size + Vec2::splat(2.0 * PAD)).extend(1.0);
        }

        // A container is a slab of glass: its contents sit on the top face and the slab
        // runs back into the scene, so its walls show in perspective (and shift as you pan).
        let slab_a = op * frame * (1.0 - 0.5 * set_back);
        if slab_a > 0.01 && elem.slab.is_none() {
            let mesh = meshes.add(Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default()));
            let e = commands
                .spawn((
                    Mesh2d(mesh.clone()),
                    MeshMaterial2d(assets.vertex_colors.clone()),
                    Transform::from_xyz(0.0, 0.0, -0.5),
                    NoFrustumCulling,
                    ChildOf(entity),
                ))
                .id();
            elem.slab = Some((e, mesh));
        }
        if let Some((_, handle)) = &elem.slab {
            let mut buf = Buf::default();
            if slab_a > 0.01 {
                let pos = elem.pos.value + elem.grab.value;
                let back = elem.render_depth + SLAB_DEPTH * frame;
                let top = rounded_outline(size, 14.0 + 8.0 * frame);
                // The bottom face, seen through perspective, in the root's (top-face) space.
                let bottom: Vec<Vec2> = top
                    .iter()
                    .map(|o| (cam.project(pos + *o, back).0 - center) / k)
                    .collect();
                let wall = base.mix(&Color::WHITE, 0.15);
                let n = top.len();
                for i in 0..n {
                    let j = (i + 1) % n;
                    // Lit from above: the top wall catches the light, the bottom one doesn't.
                    let out = ((top[i] + top[j]) * 0.5).normalize_or_zero();
                    let light = 0.45 + 0.55 * (out.y * 0.5 + 0.5);
                    let near = rgba(wall, slab_a * 0.2 * light);
                    let far = rgba(wall, slab_a * 0.03 * light);
                    buf.quad([top[i], top[j], bottom[j], bottom[i]], [near, near, far, far]);
                }
                let center_b = bottom.iter().copied().sum::<Vec2>() / n as f32;
                let floor = rgba(base, slab_a * 0.06);
                for i in 0..n {
                    buf.tri(center_b, bottom[i], bottom[(i + 1) % n], floor);
                }
                // Crisp edges: the outline of the far face and the four corner edges.
                let line = 1.2 * s / k.max(0.05);
                let edge = rgba(base.mix(&Color::WHITE, 0.35), slab_a * 0.4);
                let mut ring = bottom.clone();
                ring.push(bottom[0]);
                buf.ribbon(&ring, line, edge);
                let per_corner = n / 4;
                for c in 0..4 {
                    let mid = c * per_corner + per_corner / 2;
                    buf.ribbon(&[top[mid], bottom[mid]], line, edge);
                }
            } else {
                buf.tri(Vec2::ZERO, Vec2::ZERO, Vec2::ZERO, [0.0; 4]);
            }
            if let Some(mesh) = meshes.get_mut(handle) {
                mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, buf.pos);
                mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, buf.col);
                mesh.insert_indices(Indices::U32(buf.idx));
            }
        }
        let body_y = if el.shape == Shape::Person { -8.0 } else { 0.0 };
        let placements = [
            (Some(elem.parts.body), Vec2::new(0.0, body_y)),
            (Some(elem.parts.frame_text), Vec2::new(-half.x + 30.0, half.y - 26.0)),
            (Some(elem.parts.kind_text), Vec2::new(-half.x + 16.0, half.y - 13.0)),
            (elem.parts.badge, Vec2::new(half.x - 16.0, -half.y + 11.0)),
        ];
        for (e, p) in placements {
            if let Some(Ok(mut t)) = e.map(|e| parts_tf.get_mut(e)) {
                t.translation = p.extend(0.2);
                t.scale = inv;
            }
        }
        for &(e, c, group) in &elem.parts.texts {
            let a = match group {
                Group::Body => eff * (1.0 - frame).powi(2),
                Group::Detail => eff * (1.0 - frame).powi(2) * (1.0 - ghost),
                Group::Frame => op * frame * frame,
                Group::Small => eff * (1.0 - frame).powi(2) * small_fade * (1.0 - ghost),
            };
            if let Ok(mut tc) = colors.get_mut(e) {
                tc.0 = c.with_alpha(c.alpha() * a);
            }
        }
    }
}

/// Which side of `r` a line toward `toward` should leave from, as an outward normal. The
/// `current` side is kept while it still faces the target and isn't clearly worse, so
/// lines don't flip sides as things move (or as the camera shifts the layers about).
fn pick_side(r: Rect, toward: Vec2, current: Vec2) -> Vec2 {
    let d = (toward - r.center()) / r.half_size().max(Vec2::splat(1.0));
    // Slight preference for top and bottom, which suits the mostly vertical layouts.
    let score = |n: Vec2| n.x * d.x + n.y * d.y * 1.18;
    let best = if score(Vec2::new(0.0, d.y.signum())) >= score(Vec2::new(d.x.signum(), 0.0)) {
        Vec2::new(0.0, d.y.signum())
    } else {
        Vec2::new(d.x.signum(), 0.0)
    };
    if current != Vec2::ZERO && score(current) > 0.0 && score(best) < score(current) * 1.35 {
        current
    } else {
        best
    }
}

/// Where on `side` of `r` to attach, in the card's normalised local space ([-1, 1]²):
/// spread along the side toward the target so several lines fan out.
fn side_anchor(r: Rect, toward: Vec2, side: Vec2) -> Vec2 {
    let d = (toward - r.center()) / r.half_size().max(Vec2::splat(1.0));
    if side.y != 0.0 {
        Vec2::new((d.x * 0.22).clamp(-0.55, 0.55), side.y)
    } else {
        Vec2::new(side.x, (d.y * 0.22).clamp(-0.45, 0.45))
    }
}

/// How deep a container's glass slab runs behind its top face (in layer-depth units).
const SLAB_DEPTH: f32 = 0.3;

/// Outline of a rounded rectangle centred on the origin, counter-clockwise.
fn rounded_outline(size: Vec2, radius: f32) -> Vec<Vec2> {
    let half = size / 2.0;
    let r = radius.min(half.x).min(half.y).max(0.0);
    let segs = 6;
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

const ROPE_POINTS: usize = 20;
/// Pull of each link toward the ideal curve (per second squared): strong, so the chain
/// answers quickly...
const ROPE_STIFFNESS: f32 = 300.0;
/// ...and heavily damped (a little under critical), so it settles like a heavy chain
/// instead of bouncing.
const ROPE_DAMPING: f32 = 26.0;
/// How far a link may stretch past its rest length: chains barely give.
const ROPE_STRETCH: f32 = 1.02;

/// One frame of rope: Verlet particles pulled toward `rest` (the ideal curve), kept from
/// stretching, with both ends (and optionally one held point) pinned. `shift` drags the
/// interior along, so moving the camera makes the ropes lag and swing.
fn simulate_rope(
    rope: &mut Vec<Vec2>,
    prev: &mut Vec<Vec2>,
    rest: &[Vec2],
    held: Option<(usize, Vec2)>,
    shift: Vec2,
    dt: f32,
) {
    let n = rest.len();
    if rope.len() != n {
        *rope = rest.to_vec();
        *prev = rest.to_vec();
    }
    for i in 1..n - 1 {
        // Heaviest in the middle, like a hanging rope.
        let w = (std::f32::consts::PI * i as f32 / (n - 1) as f32).sin();
        rope[i] += shift * w;
        prev[i] += shift * w;
    }
    let rest_len: f32 = rest.windows(2).map(|w| w[0].distance(w[1])).sum();
    let max_seg = rest_len / (n - 1) as f32 * ROPE_STRETCH;
    let pinned = |i: usize| i == 0 || i == n - 1 || held.is_some_and(|(h, _)| h == i);
    let steps = 4;
    let h = dt / steps as f32;
    let damping = (-ROPE_DAMPING * h).exp();
    for _ in 0..steps {
        for i in 1..n - 1 {
            let v = (rope[i] - prev[i]) * damping;
            let acc = (rest[i] - rope[i]) * ROPE_STIFFNESS;
            prev[i] = rope[i];
            rope[i] += v + acc * h * h;
        }
        rope[0] = rest[0];
        rope[n - 1] = rest[n - 1];
        if let Some((i, p)) = held {
            rope[i] = p;
        }
        for _ in 0..3 {
            for i in 0..n - 1 {
                let d = rope[i + 1] - rope[i];
                let len = d.length();
                if len <= max_seg {
                    continue;
                }
                let fix = d * (1.0 - max_seg / len);
                match (pinned(i), pinned(i + 1)) {
                    (true, true) => {}
                    (true, false) => rope[i + 1] -= fix,
                    (false, true) => rope[i] += fix,
                    (false, false) => {
                        rope[i] += fix * 0.5;
                        rope[i + 1] -= fix * 0.5;
                    }
                }
            }
        }
    }
}

/// A smooth curve through `pts` (Catmull-Rom, `sub` points per segment).
fn smooth(pts: &[Vec2], sub: usize) -> Vec<Vec2> {
    let n = pts.len();
    let mut out = Vec::with_capacity((n - 1) * sub + 1);
    for i in 0..n - 1 {
        let p0 = pts[i.saturating_sub(1)];
        let p1 = pts[i];
        let p2 = pts[i + 1];
        let p3 = pts[(i + 2).min(n - 1)];
        for j in 0..sub {
            let t = j as f32 / sub as f32;
            let t2 = t * t;
            let t3 = t2 * t;
            out.push(
                (p1 * 2.0
                    + (p2 - p0) * t
                    + (p0 * 2.0 - p1 * 5.0 + p2 * 4.0 - p3) * t2
                    + (p1 * 3.0 - p0 - p2 * 3.0 + p3) * t3)
                    * 0.5,
            );
        }
    }
    out.push(pts[n - 1]);
    out
}

fn bezier(p: [Vec2; 4], t: f32) -> Vec2 {
    let u = 1.0 - t;
    p[0] * (u * u * u) + p[1] * (3.0 * u * u * t) + p[2] * (3.0 * u * t * t) + p[3] * (t * t * t)
}

#[derive(Default)]
struct Buf {
    pos: Vec<[f32; 3]>,
    col: Vec<[f32; 4]>,
    idx: Vec<u32>,
}

impl Buf {
    fn ribbon(&mut self, pts: &[Vec2], w: f32, c: [f32; 4]) {
        if pts.len() < 2 {
            return;
        }
        let base = self.pos.len() as u32;
        for i in 0..pts.len() {
            let a = pts[i.saturating_sub(1)];
            let b = pts[(i + 1).min(pts.len() - 1)];
            let t = (b - a).normalize_or_zero();
            let n = Vec2::new(-t.y, t.x) * (w * 0.5);
            self.pos.push((pts[i] + n).extend(0.0).to_array());
            self.pos.push((pts[i] - n).extend(0.0).to_array());
            self.col.push(c);
            self.col.push(c);
        }
        for i in 0..(pts.len() as u32 - 1) {
            let j = base + i * 2;
            self.idx.extend_from_slice(&[j, j + 1, j + 2, j + 1, j + 3, j + 2]);
        }
    }

    /// A ribbon whose width and colour change along it (for lines receding in depth).
    fn ribbon_varying(&mut self, pts: &[Vec2], widths: &[f32], colors: &[[f32; 4]]) {
        if pts.len() < 2 {
            return;
        }
        let base = self.pos.len() as u32;
        for i in 0..pts.len() {
            let a = pts[i.saturating_sub(1)];
            let b = pts[(i + 1).min(pts.len() - 1)];
            let t = (b - a).normalize_or_zero();
            let n = Vec2::new(-t.y, t.x) * (widths[i] * 0.5);
            self.pos.push((pts[i] + n).extend(0.0).to_array());
            self.pos.push((pts[i] - n).extend(0.0).to_array());
            self.col.push(colors[i]);
            self.col.push(colors[i]);
        }
        for i in 0..(pts.len() as u32 - 1) {
            let j = base + i * 2;
            self.idx.extend_from_slice(&[j, j + 1, j + 2, j + 1, j + 3, j + 2]);
        }
    }

    /// A quad with its own colour at each corner (for gradients).
    fn quad(&mut self, p: [Vec2; 4], c: [[f32; 4]; 4]) {
        let base = self.pos.len() as u32;
        for i in 0..4 {
            self.pos.push(p[i].extend(0.0).to_array());
            self.col.push(c[i]);
        }
        self.idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    fn tri(&mut self, a: Vec2, b: Vec2, c: Vec2, col: [f32; 4]) {
        let base = self.pos.len() as u32;
        for p in [a, b, c] {
            self.pos.push(p.extend(0.0).to_array());
            self.col.push(col);
        }
        self.idx.extend_from_slice(&[base, base + 1, base + 2]);
    }
}

fn rgba(c: Color, a: f32) -> [f32; 4] {
    let l = c.to_linear();
    [l.red, l.green, l.blue, l.alpha * a]
}

/// Point at arc length `s` along a polyline with cumulative lengths `acc`.
fn at_len(pts: &[Vec2], acc: &[f32], s: f32) -> Vec2 {
    let i = acc.partition_point(|&a| a < s).clamp(1, pts.len() - 1);
    let seg = (acc[i] - acc[i - 1]).max(1e-4);
    pts[i - 1].lerp(pts[i], ((s - acc[i - 1]) / seg).clamp(0.0, 1.0))
}

#[allow(clippy::too_many_arguments)]
pub fn draw_edges(
    mut commands: Commands,
    time: Res<Time>,
    cam: Res<Cam>,
    ptr: Res<Pointer>,
    text_res: Res<TextRes>,
    index: Res<ElemIndex>,
    mut set: ResMut<EdgeSet>,
    elems: Query<&Elem>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<CardMaterial>>,
    mut labels: Query<(&mut Transform, &mut TextColor, &TextLayoutInfo), With<EdgeLabel>>,
    mut bgs: Query<&mut Transform, (With<EdgeLabelBg>, Without<EdgeLabel>)>,
    diagram: Res<Diagram>,
    mut last_cam: Local<Option<Vec2>>,
) {
    let dt = time.delta_secs().min(0.05);
    // Camera motion this frame, which tugs on the ropes.
    // Capped (in screen pixels) so a fast glide sways the ropes rather than flinging them.
    let cam_shift = last_cam.map_or(Vec2::ZERO, |c| {
        let on_screen = (cam.center.value - c) / cam.scale();
        on_screen.clamp_length_max(10.0) * cam.scale() * 0.25
    });
    *last_cam = Some(cam.center.value);
    let s = cam.scale();
    let px = s.powf(0.8);
    let t_now = time.elapsed_secs();
    let detail_pair = match diagram.detail {
        Some(Detail::Relation(a, b)) => Some((
            diagram.model.elements[a].fqn.clone(),
            diagram.model.elements[b].fqn.clone(),
        )),
        _ => None,
    };
    let hovered_fqn = ptr
        .hovered
        .and_then(|e| elems.get(e).ok())
        .filter(|e| !e.role.is_some_and(Role::is_frame))
        .map(|e| e.fqn.clone());
    let label_zoom = 1.0 - ((s - 2.6) / 0.8).clamp(0.0, 1.0);
    let edge_color = Color::srgb(0.6, 0.65, 0.75);
    let keys: HashSet<(String, String)> = set.edges.keys().cloned().collect();

    let mut bufs = [Buf::default(), Buf::default()];
    let mut remove = Vec::new();
    for (key, edge) in set.edges.iter_mut() {
        edge.alpha.step(dt, 9.0, 1.0);
        let touches = hovered_fqn.as_ref().is_some_and(|h| *h == edge.from || *h == edge.to)
            || ptr.hovered_edge.as_ref() == Some(key)
            || detail_pair.as_ref() == Some(key);
        edge.hl.target = if touches { 1.0 } else { 0.0 };
        edge.hl.step(dt, 18.0, 1.0);

        let ends = index
            .0
            .get(&edge.from)
            .zip(index.0.get(&edge.to))
            .and_then(|(a, b)| elems.get(*a).ok().zip(elems.get(*b).ok()));
        let Some((ea, eb)) = ends else {
            if edge.dead {
                remove.push(key.clone());
            }
            continue;
        };
        if edge.dead && edge.alpha.value < 0.005 {
            remove.push(key.clone());
            continue;
        }
        let vis = edge.alpha.value.clamp(0.0, 1.0)
            * ea.opacity.value.clamp(0.0, 1.0).min(eb.opacity.value.clamp(0.0, 1.0));
        let hl = edge.hl.value.clamp(0.0, 1.0);
        let dimmed = if hovered_fqn.is_some() { 1.0 - hl } else { 0.0 };

        let ra = ea.screen_rect(&cam);
        let rb = eb.screen_rect(&cam);
        // Edges that reach back to a deeper layer draw behind the front frame.
        let deep = ea.depth.value.max(eb.depth.value) > 0.05;
        let layer = deep as usize;
        let z_label = if deep { Z_EDGES_BACK } else { Z_EDGES_FRONT } + 0.6;
        // Attach to a side of each card that stays put unless clearly wrong, and ease the
        // attachment point along the card when it does change.
        edge.side_a = pick_side(ra, rb.center(), edge.side_a);
        edge.side_b = pick_side(rb, ra.center(), edge.side_b);
        let (n0, n3) = (edge.side_a, edge.side_b);
        edge.anchor_a.target = side_anchor(ra, rb.center(), n0);
        edge.anchor_b.target = side_anchor(rb, ra.center(), n3);
        if edge.rope.is_empty() {
            edge.anchor_a.snap(edge.anchor_a.target);
            edge.anchor_b.snap(edge.anchor_b.target);
        }
        edge.anchor_a.step(dt, 12.0, 1.0);
        edge.anchor_b.step(dt, 12.0, 1.0);
        let mut p0 = ra.center() + edge.anchor_a.value * ra.half_size();
        let mut p3 = rb.center() + edge.anchor_b.value * rb.half_size();
        // Two-way traffic: pull the pair apart, and later push each label off to its own side.
        let mut label_side = None;
        if keys.contains(&(key.1.clone(), key.0.clone())) {
            let dir = (p3 - p0).normalize_or_zero();
            // Same perpendicular for both directions, so the two curves split cleanly.
            let canon = if key.0 < key.1 { dir } else { -dir };
            let perp = Vec2::new(-canon.y, canon.x) * if key.0 < key.1 { 1.0 } else { -1.0 };
            p0 += perp * 22.0;
            p3 += perp * 22.0;
            label_side = Some(perp);
        }
        let k = ((p3 - p0).length() * 0.42).clamp(30.0, 360.0);
        // Perspective along the chain: each point lies at a depth between its two ends, so a
        // chain reaching back to an outer layer narrows (and fades a little) as it recedes.
        let (da, db) = (ea.render_depth, eb.render_depth);
        let persp = |frac: f32| layout::depth_scale(s, da + (db - da) * frac.clamp(0.0, 1.0));
        let arrow_len = 13.0 * px * persp(1.0);
        let ctrl = [p0, p0 + n0 * k, p3 + n3 * k, p3];

        // The rope: particles that settle onto the ideal curve but carry momentum, so
        // anything that moves an end (or the camera, or a hand on the label) sets it swinging.
        // Edges from outside cross the frame's title strip around their midpoint, so their
        // labels hang further along, inside the frame next to the inner card.
        let label_t = match (ea.role, eb.role) {
            (Some(Role::External), Some(Role::Card)) => 0.7,
            (Some(Role::Card), Some(Role::External)) => 0.3,
            _ => 0.5,
        };
        let label_i = (label_t * (ROPE_POINTS - 1) as f32).round() as usize;
        let rest: Vec<Vec2> = (0..ROPE_POINTS)
            .map(|i| bezier(ctrl, i as f32 / (ROPE_POINTS - 1) as f32))
            .collect();
        simulate_rope(
            &mut edge.rope,
            &mut edge.prev,
            &rest,
            edge.grab.map(|g| (label_i, g)),
            cam_shift,
            dt,
        );
        let pts = smooth(&edge.rope, 3);
        let mut acc = vec![0.0];
        for i in 1..pts.len() {
            acc.push(acc[i - 1] + pts[i].distance(pts[i - 1]));
        }
        let total = *acc.last().unwrap();
        edge.pts.clone_from(&pts);
        edge.label_rect = None;

        if vis > 0.003 {
            let color = edge_color.mix(&ACCENT, hl);
            let a = vis * (0.55 + 0.4 * hl) * (1.0 - 0.7 * dimmed);
            // Stop the line short of the tip so it doesn't poke out around the arrowhead.
            let cut = (total - arrow_len * 0.8).max(0.0);
            let mut line: Vec<(Vec2, f32)> = pts
                .iter()
                .zip(&acc)
                .filter(|(_, d)| **d < cut)
                .map(|(p, d)| (*p, *d))
                .collect();
            line.push((at_len(&pts, &acc, cut), cut));
            let frac = |d: f32| d / total.max(1e-3);
            let width = (1.6 + 1.0 * hl) * px;
            let fog = |kk: f32| 0.55 + 0.45 * kk;
            let line_pts: Vec<Vec2> = line.iter().map(|(p, _)| *p).collect();
            let widths: Vec<f32> = line.iter().map(|(_, d)| width * persp(frac(*d))).collect();
            let colors: Vec<[f32; 4]> = line
                .iter()
                .map(|(_, d)| rgba(color, a * fog(persp(frac(*d)))))
                .collect();
            bufs[layer].ribbon_varying(&line_pts, &widths, &colors);

            // Flowing dashes show direction at a glance.
            let period = 64.0 * px * persp(0.5);
            let dash = 14.0 * px * persp(0.5);
            let speed = (48.0 + 40.0 * hl) * px;
            let mut start = (t_now * speed) % period - period;
            let dash_col = rgba(edge_color.mix(&Color::WHITE, 0.5).mix(&ACCENT, hl), a * (0.55 + 0.4 * hl));
            while start < cut {
                let a0 = start.max(0.0);
                let a1 = (start + dash).min(cut);
                if a1 > a0 {
                    let seg: Vec<Vec2> = (0..=4)
                        .map(|i| at_len(&pts, &acc, a0 + (a1 - a0) * i as f32 / 4.0))
                        .collect();
                    let kk = persp(frac((a0 + a1) * 0.5));
                    let mut c = dash_col;
                    c[3] *= fog(kk);
                    bufs[layer].ribbon(&seg, (2.4 + 1.0 * hl) * px * kk, c);
                }
                start += period;
            }

            let tip = pts[pts.len() - 1];
            let dir = (tip - at_len(&pts, &acc, total - arrow_len)).normalize_or(-n3);
            let nrm = Vec2::new(-dir.y, dir.x);
            let back = tip - dir * arrow_len;
            let wing = (5.5 + 1.0 * hl) * px * persp(1.0);
            bufs[layer].tri(tip, back + nrm * wing, back - nrm * wing, rgba(color, a.max(vis * 0.7)));
        }

        // Label at the middle of the curve.
        let la = vis * label_zoom * (1.0 - 0.75 * dimmed);
        if let Some(t) = edge.label_text {
            if let Ok((mut tf, mut tc, info)) = labels.get_mut(t) {
                // Layout size is already in logical pixels at the rasterised (×k) font size;
                // the tag then shrinks with the depth of its point on the chain.
                let kl = persp(label_i as f32 / (ROPE_POINTS - 1) as f32);
                let size = (info.size / text_res.k + Vec2::new(20.0, 10.0)) * kl;
                // The tag hangs on its point of the rope.
                let mut mid = edge.rope[label_i];
                if let (Some(side), None) = (label_side, edge.grab) {
                    // Slide the label off the line far enough that its twin can't overlap it.
                    let reach = side.x.abs() * (size.x * 0.5 + 6.0) + side.y.abs() * (size.y * 0.5 + 6.0);
                    mid += side * reach;
                }
                tf.translation = mid.extend(z_label);
                if la > 0.2 {
                    edge.label_rect = Some(Rect::from_center_size(mid, size));
                }
                tf.scale = Vec3::splat(kl / text_res.k);
                tc.0 = MUTED.mix(&TEXT, hl).with_alpha(0.85 * la);
                if let Some((bg, m)) = &edge.label_bg {
                    if let Ok(mut bt) = bgs.get_mut(*bg) {
                        bt.translation = mid.extend(z_label - 0.1);
                        bt.scale = (size + Vec2::splat(8.0 * kl)).extend(1.0);
                    }
                    if let Some(mat) = mats.get_mut(m) {
                        mat.u = CardUniform {
                            fill: lin(Color::srgba(0.07, 0.08, 0.105, 0.94)),
                            border: lin(Color::WHITE.mix(&ACCENT, hl).with_alpha(0.07 + 0.3 * hl)),
                            glow: Vec4::ZERO,
                            dims: Vec4::new(size.x, size.y, size.y * 0.5, 1.0),
                            p: Vec4::new(la, 0.0, 0.0, 0.0),
                            q: Vec4::new(4.0 * kl, 0.0, 0.0, 0.0),
                        };
                    }
                }
            }
        }
    }
    for key in remove {
        if let Some(edge) = set.edges.remove(&key) {
            if let Some(t) = edge.label_text {
                commands.entity(t).despawn();
            }
            if let Some((b, _)) = edge.label_bg {
                commands.entity(b).despawn();
            }
        }
    }

    for (mut buf, handle) in bufs.into_iter().zip(set.meshes.clone()) {
        if buf.pos.is_empty() {
            buf.tri(Vec2::ZERO, Vec2::ZERO, Vec2::ZERO, [0.0; 4]);
        }
        if let Some(mesh) = meshes.get_mut(&handle) {
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, buf.pos);
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, buf.col);
            mesh.insert_indices(Indices::U32(buf.idx));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walks_up_to_the_common_ancestor_then_down() {
        let src = "model { a = s { b = s { c = s } d = s } e = s { f = s } }";
        let (m, _) = parser::parse_sources(&[("t.c4".into(), src.into())]);
        let id = |f: &str| Some(m.by_fqn[f]);
        assert_eq!(tree_walk(&m, id("a.b.c"), id("a.d")), vec![id("a.b"), id("a"), id("a.d")]);
        assert_eq!(tree_walk(&m, id("a.b"), id("e.f")), vec![id("a"), None, id("e"), id("e.f")]);
        assert_eq!(tree_walk(&m, id("a.b.c"), None), vec![id("a.b"), id("a"), None]);
        assert_eq!(tree_walk(&m, None, id("a.b")), vec![id("a"), id("a.b")]);
        assert!(tree_walk(&m, id("a"), id("a")).is_empty());
    }
}
