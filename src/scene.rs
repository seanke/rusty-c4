//! App state and interaction: what is open, navigation (walking the tree a level at a
//! time), and the mouse and keyboard.

use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::time::SystemTime;

use bevy::{
    camera::visibility::RenderLayers,
    core_pipeline::tonemapping::Tonemapping,
    post_process::bloom::Bloom,
    prelude::*,
    ui::IsDefaultUiCamera,
    window::{CursorIcon, PrimaryWindow, SystemCursorIcon},
};

use crate::camera::Cam;
use crate::model::{Id, Model};
use crate::parser;
use crate::pipes::{self, Links, Pipe};
use crate::render::{FloorMaterial, FloorUniform};
use crate::world::{self, Block, Geometry, Heights, View};

pub const BG: Color = Color::srgb(0.004, 0.009, 0.018);
pub const TEXT: Color = Color::srgb(0.94, 0.98, 1.0);
pub const MUTED: Color = Color::srgba(0.72, 0.9, 1.0, 0.7);
pub const ACCENT: Color = Color::srgb(0.2, 0.9, 1.0);
/// Render layer of the flat overlay (labels, tags, detail cards).
pub const OVERLAY: usize = 1;

#[derive(Resource)]
pub struct Diagram {
    pub model: Model,
    pub warnings: Vec<String>,
    pub path: Option<PathBuf>,
    /// A model (from a path or the built-in example) is open, rather than the welcome screen.
    pub loaded: bool,
    /// Why the last attempt to open something failed.
    pub error: Option<String>,
    /// The deepest opened element (None: the top level).
    pub focus: Option<Id>,
    /// A card or relationship zoomed in to show its details.
    pub detail: Option<Detail>,
    /// Bumped whenever the view or selection changes, so the UI knows to rebuild.
    pub version: u64,
    /// Bumped when the 3D view must follow a new focus, and when it must be rebuilt.
    pub view_version: u64,
    pub resets: u64,
    /// The next rebuild should play the opening animation.
    pub intro: bool,
}

/// Something opened up to show everything known about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detail {
    Element(Id),
    /// A relationship pipe between two visible blocks (possibly several model relations).
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

/// Time between levels when walking the tree, so each hop reads as its own move.
const WALK_STEP: f32 = 0.55;

/// A multi-level jump in progress: the remaining foci and a detail to open on arrival.
#[derive(Resource, Default)]
pub struct Walk {
    steps: VecDeque<Option<Id>>,
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

/// Shared overlay assets.
#[derive(Resource)]
pub struct SceneAssets {
    pub quad: Handle<Mesh>,
}

#[derive(Resource, Default)]
pub struct Pointer {
    pub hovered: Option<Id>,
    pub hovered_pipe: Option<(Id, Id)>,
    press: Option<(Vec2, MouseButton)>,
    pressed_on: Option<Hit>,
    dragging: bool,
    last: Vec2,
    cursor_icon: u8,
}

/// What a click landed on.
#[derive(Clone, Copy, PartialEq)]
enum Hit {
    Block(Id),
    Pipe(Id, Id),
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
    mut fonts: ResMut<Assets<Font>>,
    mut floors: ResMut<Assets<FloorMaterial>>,
) {
    let quad = meshes.add(Rectangle::new(1.0, 1.0));
    commands.insert_resource(SceneAssets { quad });

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
    commands.insert_resource(Watch {
        timer: 0.0,
        stamp: None,
    });

    // The 3D scene, with bloom for the neon.
    commands.spawn((
        Camera3d::default(),
        Camera {
            order: 0,
            clear_color: ClearColorConfig::Custom(BG),
            ..default()
        },
        Tonemapping::TonyMcMapface,
        Bloom {
            intensity: 0.22,
            ..Bloom::NATURAL
        },
        Projection::Perspective(PerspectiveProjection {
            fov: 0.62,
            near: 1.0,
            far: 1e6,
            ..default()
        }),
        Msaa::Sample4,
        Transform::from_xyz(0.0, 1500.0, 1200.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    // A flat overlay on top for text and UI. It shares the 3D camera's HDR target (so it
    // draws over the finished scene) but adds no tonemapping of its own.
    commands.spawn((
        Camera2d,
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        bevy::render::view::Hdr,
        Tonemapping::None,
        Msaa::Sample4,
        RenderLayers::layer(OVERLAY),
        IsDefaultUiCamera,
    ));
    // The floor: a neon grid fading into the dark.
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(600_000.0, 600_000.0))),
        MeshMaterial3d(floors.add(FloorMaterial {
            u: FloorUniform {
                color: Vec4::new(0.0, 0.55, 0.85, 1.0),
                p: Vec4::new(40.0, 5.0, 9000.0, 0.0),
                base: crate::render::lin(BG),
            },
        })),
        Transform::from_xyz(0.0, -0.05, 0.0),
    ));
}

#[allow(clippy::too_many_arguments)]
pub fn apply_nav(
    mut queue: ResMut<NavQueue>,
    mut diagram: ResMut<Diagram>,
    mut cam: ResMut<Cam>,
    view: Res<View>,
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
            } else if let Some(d) = walk.detail.take() {
                queue.0.push(Nav::Detail(d));
            }
        }
    }
    for nav in std::mem::take(&mut queue.0) {
        match nav {
            Nav::Load(source) => {
                let loaded = match &source {
                    Source::Path(p) => parser::load(p),
                    Source::Example => Ok(parser::parse_sources(&[("example.c4".into(), EXAMPLE.into())])),
                };
                match loaded {
                    Ok((model, warnings)) => {
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
                        diagram.intro = true;
                        diagram.resets += 1;
                        diagram.view_version += 1;
                        diagram.version += 1;
                    }
                    Err(e) => {
                        diagram.error = Some(e);
                        diagram.version += 1;
                    }
                }
            }
            Nav::Focus(f) => {
                walk.steps = tree_walk(&diagram.model, diagram.focus, f).into();
                walk.detail = None;
                if let Some(first) = walk.steps.pop_front() {
                    queue_step(&mut diagram, first);
                    walk.timer = WALK_STEP;
                }
            }
            Nav::Step(f) => queue_step(&mut diagram, f),
            Nav::Up => {
                walk.steps.clear();
                walk.detail = None;
                if let Some(f) = diagram.focus {
                    let up = diagram.model.elements[f].parent;
                    queue_step(&mut diagram, up);
                }
            }
            // Mid-walk, the detail waits until the walk arrives.
            Nav::Detail(d) if !walk.steps.is_empty() => walk.detail = Some(d),
            Nav::Detail(d) => {
                if d.is_none() && diagram.detail.is_some() {
                    cam.fit(view.fit.0, view.fit.1, false);
                }
                diagram.detail = d;
                diagram.version += 1;
            }
            Nav::Fit => cam.fit(view.fit.0, view.fit.1, false),
            Nav::Reload => {
                let Some(path) = diagram.path.clone() else { continue };
                match parser::load(&path) {
                    Ok((model, warnings)) => {
                        let old = &diagram.model;
                        let focus = diagram.focus.map(|i| old.elements[i].fqn.clone());
                        diagram.focus = focus.and_then(|f| model.by_fqn.get(&f).copied());
                        diagram.detail = None;
                        diagram.model = model;
                        diagram.warnings = warnings;
                        diagram.resets += 1;
                        diagram.view_version += 1;
                        diagram.version += 1;
                    }
                    Err(e) => {
                        diagram.warnings = vec![format!("reload failed: {e}")];
                        diagram.version += 1;
                    }
                }
            }
        }
    }
}

fn queue_step(diagram: &mut Diagram, focus: Option<Id>) {
    if diagram.focus != focus {
        diagram.focus = focus;
        diagram.detail = None;
        diagram.view_version += 1;
        diagram.version += 1;
    }
}

#[allow(clippy::too_many_arguments)]
pub fn pointer(
    mut commands: Commands,
    mut cam: ResMut<Cam>,
    mut ptr: ResMut<Pointer>,
    mut queue: ResMut<NavQueue>,
    diagram: Res<Diagram>,
    view: Res<View>,
    geometry: Res<Geometry>,
    heights: Res<Heights>,
    links: Res<Links>,
    mut blocks: Query<(Entity, &mut Block)>,
    mut pipes_q: Query<(Entity, &mut Pipe)>,
    mouse: Res<ButtonInput<MouseButton>>,
    ui: Query<&Interaction>,
    window: Single<Entity, With<PrimaryWindow>>,
    detail_view: Res<crate::detail::DetailView>,
) {
    cam.over_ui = ui.iter().any(|i| *i != Interaction::None);
    let cursor = cam.cursor.filter(|_| !cam.over_ui);
    let detail_open = diagram.detail.is_some();
    let over_detail = cursor
        .map(|c| cam.screen_to_overlay(c))
        .zip(detail_view.rect)
        .is_some_and(|(o, r)| r.contains(o));

    // What is under the cursor: a block, else a pipe.
    let mut hit = None;
    if let (Some(c), false, false) = (cursor, ptr.dragging, detail_open) {
        let (o, d) = cam.ray(c);
        if let Some((_, id, _)) = world::pick(&heights, &geometry, blocks.iter().map(|(e, b)| (e, &*b)), o, d) {
            hit = Some(Hit::Block(id));
        }
        let on_platform = matches!(hit, Some(Hit::Block(id)) if view.opened.contains(&id));
        if hit.is_none() || on_platform {
            if let Some((a, b)) = pipes::pick(&cam, &pipes_q, c) {
                hit = Some(Hit::Pipe(a, b));
            }
        }
    }
    // The open platform under a hovered child is just the ground; don't light it up.
    ptr.hovered = match hit {
        Some(Hit::Block(id)) if view.focus != Some(id) => Some(id),
        _ => None,
    };
    ptr.hovered_pipe = match hit {
        Some(Hit::Pipe(a, b)) => Some((a, b)),
        _ => None,
    };

    // Hover emphasis: light the hovered block and its pipes, dim what isn't connected.
    let hovered_closed = ptr.hovered.filter(|id| !view.opened.contains(id));
    let connected: HashSet<Id> = match (hovered_closed, ptr.hovered_pipe) {
        (Some(h), _) => links
            .0
            .iter()
            .filter_map(|l| {
                if l.from == h {
                    Some(l.to)
                } else if l.to == h {
                    Some(l.from)
                } else {
                    None
                }
            })
            .chain([h])
            .collect(),
        (None, Some((a, b))) => [a, b].into_iter().collect(),
        _ => HashSet::new(),
    };
    let emphasis = !connected.is_empty();
    for (_, mut b) in &mut blocks {
        b.hover.target = if ptr.hovered == Some(b.id) || (ptr.hovered_pipe.is_some() && connected.contains(&b.id)) {
            1.0
        } else {
            0.0
        };
        let dimmed = emphasis && !connected.contains(&b.id) && !view.opened.contains(&b.id);
        b.dim.target = if dimmed { 1.0 } else { 0.0 };
    }
    for (_, mut p) in &mut pipes_q {
        let lit = ptr.hovered_pipe == Some(p.key) || hovered_closed.is_some_and(|h| p.from == h || p.to == h);
        p.hl.target = if lit { 1.0 } else { 0.0 };
        p.dim.target = if emphasis && !lit { 1.0 } else { 0.0 };
    }

    // Press, drag (left: pan, right: orbit), click.
    for button in [MouseButton::Left, MouseButton::Middle, MouseButton::Right] {
        if mouse.just_pressed(button) && ptr.press.is_none() && !cam.over_ui {
            if let Some(c) = cam.cursor {
                ptr.press = Some((c, button));
                ptr.pressed_on = if button == MouseButton::Left { hit } else { None };
                ptr.dragging = false;
                ptr.last = c;
            }
        }
    }
    if let (Some((start, button)), Some(c)) = (ptr.press, cam.cursor) {
        if mouse.pressed(button) {
            if !ptr.dragging && c.distance(start) > 4.0 {
                ptr.dragging = true;
            }
            if ptr.dragging {
                let last = ptr.last;
                match button {
                    MouseButton::Right => cam.orbit(c - last),
                    _ => cam.drag(last, c),
                }
            }
            ptr.last = c;
        }
        if mouse.just_released(button) {
            if !ptr.dragging {
                match button {
                    MouseButton::Left => {
                        if detail_open {
                            if !over_detail {
                                queue.0.push(Nav::Detail(None));
                            }
                        } else if ptr.pressed_on == hit {
                            click(&mut queue, &diagram, &view, hit);
                        }
                    }
                    MouseButton::Right => {
                        queue.0.push(if detail_open { Nav::Detail(None) } else { Nav::Up });
                    }
                    _ => {}
                }
            }
            ptr.press = None;
            ptr.dragging = false;
        }
    }

    let icon = if ptr.dragging {
        2
    } else if ptr.hovered.is_some() || ptr.hovered_pipe.is_some() {
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

fn click(queue: &mut NavQueue, diagram: &Diagram, view: &View, hit: Option<Hit>) {
    let model = &diagram.model;
    match hit {
        Some(Hit::Pipe(a, b)) => queue.0.push(Nav::Detail(Some(Detail::Relation(a, b)))),
        // An opened platform: the open one is just ground; an outer one walks back out.
        Some(Hit::Block(id)) if view.opened.contains(&id) => {
            if view.focus != Some(id) {
                queue.0.push(Nav::Focus(Some(id)));
            }
        }
        Some(Hit::Block(id)) => {
            let el = &model.elements[id];
            let on_open_level = el.parent == view.focus;
            match (on_open_level, el.children.is_empty()) {
                (_, false) => queue.0.push(Nav::Focus(Some(id))),
                (true, true) => queue.0.push(Nav::Detail(Some(Detail::Element(id)))),
                // Somewhere else: walk over to its level, then show it.
                (false, true) => {
                    queue.0.push(Nav::Focus(el.parent));
                    queue.0.push(Nav::Detail(Some(Detail::Element(id))));
                }
            }
        }
        // The floor, or a lower level: step back out.
        None => {
            if diagram.focus.is_some() {
                queue.0.push(Nav::Up);
            }
        }
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

pub fn watch_files(time: Res<Time>, mut watch: ResMut<Watch>, diagram: Res<Diagram>, mut queue: ResMut<NavQueue>) {
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
