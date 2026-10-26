//! App state and interaction: loading, flying the camera about, and the mouse and keyboard.

use std::collections::HashSet;
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
use crate::parser::{self, Diagnostic};
use crate::workspace::{Example, SourceFile, Workspace};
use crate::pipes::{self, Links, Pipe};
use crate::render::{FloorMaterial, FloorUniform};
use crate::world::{self, Block, Geometry, Heights, View};

pub const BG: Color = Color::srgb(0.004, 0.009, 0.018);
pub const TEXT: Color = Color::srgb(0.94, 0.98, 1.0);
pub const MUTED: Color = Color::srgba(0.72, 0.9, 1.0, 0.7);
pub const ACCENT: Color = Color::srgb(0.2, 0.9, 1.0);
/// Render layer of the flat overlay (detail cards), and an empty one for the UI camera.
pub const OVERLAY: usize = 1;
const UI_LAYER: usize = 2;

/// The camera drawing the flat overlay over the 3D view.
#[derive(Component)]
pub struct OverlayCamera;

#[derive(Resource)]
pub struct Diagram {
    pub model: Model,
    pub warnings: Vec<Diagnostic>,
    pub path: Option<PathBuf>,
    /// A model (from a path or the built-in example) is open, rather than the welcome screen.
    pub loaded: bool,
    /// Why the last attempt to open something failed.
    pub error: Option<String>,
    /// A card or relationship zoomed in to show its details.
    pub detail: Option<Detail>,
    /// Bumped whenever the selection or model changes, so the UI knows to rebuild.
    pub version: u64,
    /// Bumped when the 3D world must be rebuilt.
    pub resets: u64,
    /// The next rebuild should play the opening animation.
    pub intro: bool,
    /// The next rebuild should keep the camera and the open containers where they are (a
    /// change of style, or of the source).
    pub keep_view: bool,
}

/// Something opened up to show everything known about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detail {
    Element(Id),
    /// A relationship pipe between two visible blocks (possibly several model relations).
    Relation(Id, Id),
}

pub enum Source {
    Path(PathBuf),
    Example(Example),
}

pub enum Nav {
    /// Replace the open model.
    Load(Source),
    /// Fly the camera to an element, close enough that it opens.
    Frame(Id),
    /// Fly to an element picked in the code and make it glow: containers open, and a block
    /// with nothing inside is shown among its neighbours.
    Reveal(Id),
    /// Zoom out one level.
    Out,
    Detail(Option<Detail>),
    Fit,
    /// Re-read the files from disk (those not being edited) and rebuild.
    Reload,
    /// Rebuild from the files as edited.
    Reparse,
}

#[derive(Resource, Default)]
pub struct NavQueue(pub Vec<Nav>);

/// Linking the 3D view and the source: a block clicked in 3D to show in the code, and a
/// block picked in the code to glow for a moment in 3D.
#[derive(Resource, Default)]
pub struct Jump {
    pub to_code: Option<Id>,
    pub glow: Option<(Id, f32)>,
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
            fov: 0.42,
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
        OverlayCamera,
    ));
    // The interface draws over the whole window (the panels as well as the 3D view), so it
    // has a camera of its own that renders nothing else.
    commands.spawn((
        Camera2d,
        Camera {
            order: 2,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        bevy::render::view::Hdr,
        Tonemapping::None,
        Msaa::Sample4,
        RenderLayers::layer(UI_LAYER),
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

/// Where an element's top will be once everything above it has opened.
fn expected_top(model: &Model, geometry: &Geometry, id: Id) -> f32 {
    model
        .path(Some(id))
        .iter()
        .rev()
        .skip(1)
        .filter_map(|a| geometry.geos.get(a))
        .map(|g| (world::BLOCK_H + world::RISE) * g.scale)
        .sum()
}

fn fit_root(cam: &mut Cam, geometry: &Geometry) {
    let r = geometry.root_bounds;
    cam.fit(Vec3::new(r.center().x, 0.0, r.center().y), r.size() + Vec2::splat(120.0), false);
}

#[allow(clippy::too_many_arguments)]
pub fn apply_nav(
    mut queue: ResMut<NavQueue>,
    mut diagram: ResMut<Diagram>,
    mut cam: ResMut<Cam>,
    view: Res<View>,
    mut geometry: ResMut<Geometry>,
    mut watch: ResMut<Watch>,
    mut ws: ResMut<Workspace>,
    mut jump: ResMut<Jump>,
) {
    for nav in std::mem::take(&mut queue.0) {
        match nav {
            Nav::Load(source) => {
                let files = match &source {
                    Source::Path(p) => parser::read_sources(p).map(|files| {
                        files
                            .into_iter()
                            .map(|(rel, path, text)| SourceFile {
                                rel,
                                path: Some(path),
                                saved: text.clone(),
                                text,
                                rev: 0,
                            })
                            .collect::<Vec<_>>()
                    }),
                    Source::Example(e) => Ok(e
                        .files()
                        .into_iter()
                        .map(|(rel, text)| SourceFile {
                            rel: rel.into(),
                            path: None,
                            text: text.into(),
                            saved: text.into(),
                            rev: 0,
                        })
                        .collect()),
                };
                match files {
                    Ok(files) => {
                        ws.set(files);
                        let (model, warnings) = parser::parse_sources(&ws.sources());
                        diagram.model = model;
                        diagram.warnings = warnings;
                        diagram.path = match source {
                            Source::Path(p) => Some(p),
                            Source::Example(_) => None,
                        };
                        watch.stamp = diagram.path.as_ref().and_then(latest_mtime);
                        diagram.loaded = true;
                        diagram.error = None;
                        diagram.detail = None;
                        diagram.intro = true;
                        diagram.resets += 1;
                        diagram.version += 1;
                        cam.saved = None;
                    }
                    Err(e) => {
                        diagram.error = Some(e);
                        diagram.version += 1;
                    }
                }
            }
            Nav::Frame(id) => {
                let model = &diagram.model;
                for a in model.path(Some(id)).iter().rev().skip(1).rev() {
                    geometry.ensure_children(model, *a);
                }
                if let Some(g) = geometry.geos.get(&id).copied() {
                    let y = expected_top(model, &geometry, id);
                    cam.fit(Vec3::new(g.center.x, y, g.center.y), g.size * 1.05, false);
                }
            }
            Nav::Reveal(id) => {
                let model = &diagram.model;
                if !model.elements[id].children.is_empty() {
                    queue.0.push(Nav::Frame(id));
                } else {
                    for a in model.path(Some(id)).iter().rev().skip(1).rev() {
                        geometry.ensure_children(model, *a);
                    }
                    if let Some(g) = geometry.geos.get(&id).copied() {
                        let y = expected_top(model, &geometry, id);
                        cam.fit(Vec3::new(g.center.x, y, g.center.y), g.size * 2.6, false);
                    }
                }
                jump.glow = Some((id, 2.5));
            }
            Nav::Out => {
                if diagram.detail.is_some() {
                    queue.0.push(Nav::Detail(None));
                    continue;
                }
                // Frame whatever holds the level you're looking at.
                let t = cam.target.target;
                let under = world::open_under(&diagram.model, &view, &geometry, Vec2::new(t.x, t.z));
                match under.and_then(|u| diagram.model.elements[u].parent) {
                    Some(p) => queue.0.push(Nav::Frame(p)),
                    None if under.is_some() => fit_root(&mut cam, &geometry),
                    None => fit_root(&mut cam, &geometry),
                }
            }
            Nav::Detail(d) => {
                if d.is_none() && diagram.detail.is_some() {
                    if let Some((t, l)) = cam.saved.take() {
                        cam.target.target = t;
                        cam.log_dist.target = l;
                    }
                }
                diagram.detail = d;
                diagram.version += 1;
            }
            Nav::Fit => {
                // Back to the starting angle, too: undo any orbiting.
                let turns = (cam.yaw.target / std::f32::consts::TAU).round();
                cam.yaw.target = turns * std::f32::consts::TAU;
                cam.pitch.target = crate::camera::DEFAULT_PITCH;
                fit_root(&mut cam, &geometry);
            }
            Nav::Reload | Nav::Reparse => {
                if matches!(nav, Nav::Reload) {
                    let Some(path) = diagram.path.clone() else { continue };
                    // Pick up changes on disk, except in files with unsaved edits.
                    match parser::read_sources(&path) {
                        Ok(found) => {
                            let mut files = Vec::new();
                            for (rel, path, text) in found {
                                match ws.files.iter_mut().position(|f| f.rel == rel) {
                                    Some(i) => {
                                        let mut f = std::mem::replace(
                                            &mut ws.files[i],
                                            SourceFile {
                                                rel: String::new(),
                                                path: None,
                                                text: String::new(),
                                                saved: String::new(),
                                                rev: 0,
                                            },
                                        );
                                        if !f.dirty() && f.text != text {
                                            f.text = text.clone();
                                            f.rev += 1;
                                        }
                                        f.saved = text;
                                        files.push(f);
                                    }
                                    None => files.push(SourceFile {
                                        rel,
                                        path: Some(path),
                                        saved: text.clone(),
                                        text,
                                        rev: 1,
                                    }),
                                }
                            }
                            let active = ws.files.get(ws.active).map(|f| f.rel.clone());
                            ws.files = files;
                            ws.active = active.and_then(|a| ws.files.iter().position(|f| f.rel == a)).unwrap_or(0);
                        }
                        Err(e) => {
                            diagram.error = Some(format!("reload failed: {e}"));
                            diagram.version += 1;
                            continue;
                        }
                    }
                }
                let (model, warnings) = parser::parse_sources(&ws.sources());
                diagram.detail = None;
                diagram.model = model;
                diagram.warnings = warnings;
                diagram.keep_view = true;
                diagram.resets += 1;
                diagram.version += 1;
            }
        }
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
    mut jump: ResMut<Jump>,
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
    // An open platform is ground to stand on; don't light it up.
    ptr.hovered = match hit {
        Some(Hit::Block(id)) if !view.opened.contains(&id) => Some(id),
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
    // A hovered pipe lights everything carrying the same relationships (its ports too); a
    // hovered block lights every pipe carrying one of its relationships.
    let hovered_links: HashSet<(Id, Id)> = pipes_q
        .iter()
        .filter(|(_, p)| Some(p.key) == ptr.hovered_pipe)
        .flat_map(|(_, p)| p.links.iter().copied())
        .collect();
    for (_, mut p) in &mut pipes_q {
        let lit = ptr.hovered_pipe == Some(p.key)
            || p.links.iter().any(|l| hovered_links.contains(l))
            || hovered_closed.is_some_and(|h| p.from == h || p.to == h || p.links.iter().any(|(a, b)| *a == h || *b == h));
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
                            click(&mut queue, &diagram, hit);
                            // Show a clicked block in the source, too.
                            if let Some(Hit::Block(id)) = hit {
                                jump.to_code = Some(id);
                            }
                        }
                    }
                    MouseButton::Right => queue.0.push(Nav::Out),
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

fn click(queue: &mut NavQueue, diagram: &Diagram, hit: Option<Hit>) {
    let model = &diagram.model;
    match hit {
        Some(Hit::Pipe(a, b)) => queue.0.push(Nav::Detail(Some(Detail::Relation(a, b)))),
        // Something with insides: fly to it (it opens as it fills the view).
        Some(Hit::Block(id)) if !model.elements[id].children.is_empty() => queue.0.push(Nav::Frame(id)),
        Some(Hit::Block(id)) => queue.0.push(Nav::Detail(Some(Detail::Element(id)))),
        // The floor: step back out.
        None => queue.0.push(Nav::Out),
    }
}

pub fn keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    code: Res<crate::editor::Code>,
    diagram: Res<Diagram>,
    mut queue: ResMut<NavQueue>,
    mut playback: ResMut<crate::story::Playback>,
) {
    if !diagram.loaded || code.captures_keys() {
        return;
    }
    // Esc backs out of whatever is open: a detail card, then a sequence.
    if keys.just_pressed(KeyCode::Escape) {
        if diagram.detail.is_some() {
            queue.0.push(Nav::Detail(None));
        } else {
            playback.stop();
        }
        return;
    }
    // With a sequence open: arrows (or space) step through it, backspace closes it.
    if let Some(stages) = playback.active.and_then(|i| diagram.model.views.get(i)).map(|v| v.stages().len()) {
        if keys.any_just_pressed([KeyCode::ArrowRight, KeyCode::Space]) {
            playback.next(stages);
        }
        if keys.just_pressed(KeyCode::ArrowLeft) {
            playback.prev();
        }
        if keys.just_pressed(KeyCode::Backspace) && diagram.detail.is_none() {
            playback.stop();
            return;
        }
    }
    if keys.just_pressed(KeyCode::Backspace) {
        queue.0.push(Nav::Out);
    }
    let space_fits = playback.active.is_none() && keys.just_pressed(KeyCode::Space);
    if space_fits || keys.any_just_pressed([KeyCode::KeyF, KeyCode::Digit0]) {
        queue.0.push(Nav::Fit);
    }
    if keys.just_pressed(KeyCode::KeyR) {
        queue.0.push(Nav::Reload);
    }
    if keys.just_pressed(KeyCode::Home) {
        queue.0.push(Nav::Fit);
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
