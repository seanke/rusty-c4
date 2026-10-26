//! Stepping through dynamic views ("sequences"). A sequence is a scenario told step by step;
//! you move through it yourself, from the numbered step list or with ← and →. Each step sends
//! a bright pulse (again and again) along the pipe between its two blocks (steps in a
//! `parallel` block go together), the camera flies to where it happens, and everything not
//! taking part is dimmed.

use std::collections::HashSet;

use bevy::prelude::*;

use crate::camera::Cam;
use crate::model::{Id, Step};
use crate::pipes::{visible_rep, Pipe};
use crate::palette::{Role, Ui, UiEdge, UiFill, UiInk, UiTint};
use crate::scene::{Diagram, Fonts};
use crate::world::{Block, Geometry, Heights, View};

/// Seconds for a pulse to travel its pipe, and to rest before it goes again.
const TRAVEL: f32 = 1.3;
const REST: f32 = 0.7;

#[derive(Resource, Default)]
pub struct Playback {
    /// The dynamic view playing (index into the model's views).
    pub active: Option<usize>,
    pub stage: usize,
    /// Bumped when the active view changes, so pipes are re-routed with its steps.
    pub version: u64,
    /// Progress of the current stage's pulses (0..1), then time spent after arriving.
    t: f32,
    hold: f32,
    framed: bool,
    /// The pipes (from, to) carrying the current stage, as seen at the current level.
    current: Vec<(Id, Id)>,
    packets: Vec<Entity>,
}

impl Playback {
    pub fn start(&mut self, view: usize) {
        self.active = Some(view);
        self.stage = 0;
        self.version += 1;
        self.reset_stage();
    }

    pub fn stop(&mut self) {
        if self.active.take().is_some() {
            self.version += 1;
            self.current.clear();
        }
    }

    pub fn go_to(&mut self, stage: usize) {
        if stage != self.stage {
            self.stage = stage;
            self.reset_stage();
        }
    }

    pub fn next(&mut self, stages: usize) {
        if self.stage + 1 < stages {
            self.stage += 1;
            self.reset_stage();
        }
    }

    pub fn prev(&mut self) {
        self.stage = self.stage.saturating_sub(1);
        self.reset_stage();
    }

    fn reset_stage(&mut self) {
        self.t = 0.0;
        self.hold = 0.0;
        self.framed = false;
    }
}

/// Advance the story: fly to the step (its blocks open as they fill the view), run the
/// pulses, and dim everything not taking part.
#[allow(clippy::too_many_arguments)]
pub fn drive(
    time: Res<Time>,
    diagram: Res<Diagram>,
    view: Res<View>,
    mut geometry: ResMut<Geometry>,
    heights: Res<Heights>,
    mut cam: ResMut<Cam>,
    mut pb: ResMut<Playback>,
    mut blocks: Query<&mut Block>,
    mut pipes: Query<&mut Pipe>,
) {
    let model = &diagram.model;
    let Some(dv) = pb.active.and_then(|i| model.views.get(i)) else {
        pb.active = None;
        pb.current.clear();
        return;
    };
    let stages = dv.stages();
    if pb.stage >= stages.len() {
        pb.stage = stages.len().saturating_sub(1);
    }
    let steps: Vec<&Step> = stages[pb.stage].iter().map(|&i| &dv.steps[i]).collect();
    // Fly to frame both ends of the step(s), as they will stand once their containers open.
    if !pb.framed {
        pb.framed = true;
        let mut rect: Option<Rect> = None;
        let mut y = 0.0f32;
        // Frame the deepest end's container (so it opens and shows that end), together with
        // anything on the same level; ends further out still show, via their ports.
        let depth = |id: Id| model.path(Some(id)).len();
        let deepest = steps.iter().flat_map(|s| [s.from, s.to]).map(depth).max().unwrap_or(0);
        let mut targets: Vec<Id> = Vec::new();
        for s in &steps {
            for id in [s.from, s.to] {
                let t = if depth(id) == deepest || deepest <= 1 {
                    id
                } else {
                    // A shallower end: frame the container at the deepest end's level instead.
                    let other = if id == s.from { s.to } else { s.from };
                    model.path(Some(other)).get(deepest.saturating_sub(2)).copied().unwrap_or(other)
                };
                if !targets.contains(&t) {
                    targets.push(t);
                }
            }
        }
        for id in targets {
            {
                for a in model.path(Some(id)).iter().rev().skip(1).rev() {
                    geometry.ensure_children(model, *a);
                }
                if let Some(g) = geometry.geos.get(&id) {
                    rect = Some(rect.map_or(g.rect(), |r| r.union(g.rect())));
                    let top: f32 = model
                        .path(Some(id))
                        .iter()
                        .rev()
                        .skip(1)
                        .filter_map(|a| geometry.geos.get(a))
                        .map(|g| (crate::world::BLOCK_H + crate::world::RISE) * g.scale)
                        .sum();
                    y = y.max(top);
                }
            }
        }
        if let Some(r) = rect {
            cam.fit(Vec3::new(r.center().x, y, r.center().y), r.size() * 1.6, false);
        }
    }

    let rep = |x: Id| visible_rep(model, &view.visible, x);
    let pair = |s: &Step| rep(s.from).zip(rep(s.to)).filter(|(a, b)| a != b);
    pb.current = steps.iter().filter_map(|s| pair(s)).collect();
    let all: HashSet<(Id, Id)> = dv.steps.iter().filter_map(|s| pair(s)).collect();
    let taking_part: HashSet<Id> = all.iter().flat_map(|(a, b)| [*a, *b]).collect();
    let now: HashSet<Id> = pb.current.iter().flat_map(|(a, b)| [*a, *b]).collect();

    let cam_still = (cam.log_dist.target - cam.log_dist.value).abs() < 0.02
        && cam.target.target.distance(cam.target.value) < cam.dist() * 0.01;
    let settled = cam_still && !heights.moving;
    let pipes_ready = pb
        .current
        .iter()
        .all(|k| pipes.iter().any(|p| p.is_trunk() && p.links.contains(k) && p.ready()));
    if settled && pipes_ready {
        let dt = time.delta_secs().min(0.05);
        // The pulse runs, rests, and runs again, until you move on.
        if pb.t < 1.0 {
            pb.t = (pb.t + dt / TRAVEL).min(1.0);
        } else {
            pb.hold += dt;
            if pb.hold > REST {
                pb.t = 0.0;
                pb.hold = 0.0;
            }
        }
    }

    // Emphasis: the current step bright, the rest of the story normal, the others dim.
    for mut b in &mut blocks {
        let part = taking_part.contains(&b.id) || view.opened.contains(&b.id);
        b.dim.target = if part { 0.0 } else { 1.0 };
        b.hover.target = if now.contains(&b.id) { 1.0 } else { 0.0 };
    }
    for mut p in &mut pipes {
        let in_story = p.links.iter().any(|l| all.contains(l));
        let now_playing = p.links.iter().any(|l| pb.current.contains(l));
        p.dim.target = if in_story { 0.0 } else { 1.0 };
        p.hl.target = if now_playing { 1.0 } else { 0.0 };
    }
}

/// The pulse's look in each style: a glowing ball, a dot of ink, a block of redstone, a
/// yellow marker dot, a bright blue dot, a yellow taxi.
#[derive(Resource)]
pub struct PacketAssets {
    mesh: [Handle<Mesh>; 6],
    material: [Handle<StandardMaterial>; 6],
}

pub fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut mats: ResMut<Assets<StandardMaterial>>) {
    let ball = meshes.add(Sphere::new(1.0).mesh().uv(16, 10));
    let cube = meshes.add(Cuboid::new(1.4, 1.4, 1.4));
    let flat = |c: Color| StandardMaterial {
        base_color: c,
        unlit: true,
        ..default()
    };
    commands.insert_resource(PacketAssets {
        mesh: [ball.clone(), ball.clone(), cube.clone(), ball.clone(), ball, cube],
        material: [
            mats.add(StandardMaterial {
                base_color: Color::BLACK,
                emissive: LinearRgba::rgb(14.0, 34.0, 40.0),
                ..default()
            }),
            mats.add(flat(Color::srgb(0.8, 0.16, 0.1))),
            mats.add(flat(Color::srgb(1.0, 0.2, 0.1))),
            mats.add(flat(Color::srgb(1.0, 0.85, 0.3))),
            mats.add(flat(Color::srgb(0.31, 0.76, 1.0))),
            mats.add(flat(Color::srgb(1.0, 0.78, 0.1))),
        ],
    });
}

/// Move the pulses along their pipes.
pub fn draw(
    mut commands: Commands,
    mut pb: ResMut<Playback>,
    diagram: Res<Diagram>,
    geometry: Res<Geometry>,
    assets: Res<PacketAssets>,
    theme: Res<crate::theme::Theme>,
    pipes: Query<&Pipe>,
    mut transforms: Query<(&mut Transform, &mut Visibility)>,
    mut last_style: Local<Option<u64>>,
) {
    // A new style: start the pulses afresh in its look.
    if *last_style != Some(theme.version) {
        *last_style = Some(theme.version);
        for e in pb.packets.drain(..) {
            commands.entity(e).despawn();
        }
    }
    let st = theme.style.index() as usize;
    let want = if pb.active.is_some() { pb.current.len() } else { 0 };
    while pb.packets.len() < want {
        let e = commands
            .spawn((
                Mesh3d(assets.mesh[st].clone()),
                MeshMaterial3d(assets.material[st].clone()),
                Transform::default(),
                Visibility::Hidden,
            ))
            .id();
        pb.packets.push(e);
    }
    while pb.packets.len() > want {
        if let Some(e) = pb.packets.pop() {
            commands.entity(e).despawn();
        }
    }
    // Ease in and out along the pipe; shrink away once arrived.
    let t = pb.t;
    let along = t * t * (3.0 - 2.0 * t);
    let size = if t < 1.0 { 1.0 } else { (1.0 - pb.hold / 0.35).max(0.0) };
    let model = &diagram.model;
    for (i, key) in pb.current.clone().into_iter().enumerate() {
        let Some(&e) = pb.packets.get(i) else { continue };
        let Ok((mut tf, mut vis)) = transforms.get_mut(e) else { continue };
        let Some(pipe) = pipes.iter().find(|p| p.is_trunk() && p.links.contains(&key) && p.ready()) else {
            *vis = Visibility::Hidden;
            continue;
        };
        let pts = &pipe.samples;
        let total: f32 = pts.windows(2).map(|w| w[0].distance(w[1])).sum();
        let mut left = total * along;
        let mut at = pts.first().copied().unwrap_or_default();
        for w in pts.windows(2) {
            let l = w[0].distance(w[1]);
            if left <= l {
                at = w[0].lerp(w[1], left / l.max(1e-6));
                break;
            }
            left -= l;
            at = w[1];
        }
        let s = geometry
            .surface_scale(model.elements[key.0].parent)
            .min(geometry.surface_scale(model.elements[key.1].parent));
        let r = 3.6 * s * 3.4 * size;
        *vis = if r > 1e-4 && t > 0.0 { Visibility::Inherited } else { Visibility::Hidden };
        tf.translation = at;
        tf.scale = Vec3::splat(r.max(1e-4));
    }
}

// --- UI ------------------------------------------------------------------------------------


/// The step list, shown while a sequence is open.
#[derive(Component)]
pub struct StepsPanel;

#[derive(Component)]
pub struct StepsTitle;

#[derive(Component)]
pub struct StepsList;

#[derive(Component, Clone, Copy, PartialEq)]
pub enum StepButton {
    Go(usize),
    Prev,
    Next,
    Close,
}

fn font(fonts: &Fonts, size: f32, weight: FontWeight) -> TextFont {
    TextFont {
        font: fonts.ui.clone(),
        font_size: size,
        weight,
        ..default()
    }
}

fn icon_button(icons: &crate::icons::Icons, icon: crate::icons::Icon, action: StepButton) -> impl Bundle {
    (
        Button,
        Node {
            width: px(26),
            height: px(24),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border_radius: BorderRadius::all(px(5)),
            ..default()
        },
        BackgroundColor(Color::NONE),
        action,
        children![(
            ImageNode::new(icons.get(icon)),
            UiTint(Role::Icon, 0.95),
            Node {
                width: px(14),
                height: px(14),
                ..default()
            },
        )],
    )
}

pub fn spawn_ui(mut commands: Commands, fonts: Res<Fonts>, icons: Res<crate::icons::Icons>, chrome: Res<crate::chrome::Chrome>) {
    use crate::icons::Icon;
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(chrome.top + 12.0),
                left: px(12),
                width: px(360),
                max_height: percent(70),
                flex_direction: FlexDirection::Column,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(10)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::NONE),
            BorderColor::default(),
            UiFill(Role::Panel),
            UiEdge(Role::PanelEdge),
            Interaction::default(),
            Visibility::Hidden,
            StepsPanel,
        ))
        .with_children(|p| {
            p.spawn(Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                padding: UiRect::new(px(12), px(6), px(8), px(8)),
                column_gap: px(2),
                border: UiRect::bottom(px(1)),
                ..default()
            })
            .insert((BorderColor::default(), UiEdge(Role::PanelEdge)))
            .with_children(|h| {
                h.spawn((
                    Node {
                        flex_grow: 1.0,
                        ..default()
                    },
                    Text::new(""),
                    font(&fonts, 13.0, FontWeight::SEMIBOLD),
                    TextColor::default(),
                    UiInk(Role::Text, 1.0),
                    StepsTitle,
                ));
                h.spawn(icon_button(&icons, Icon::ChevronLeft, StepButton::Prev));
                h.spawn(icon_button(&icons, Icon::ChevronRight, StepButton::Next));
                h.spawn(icon_button(&icons, Icon::Close, StepButton::Close));
            });
            p.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(px(6)),
                    row_gap: px(1),
                    overflow: Overflow::scroll_y(),
                    ..default()
                },
                ScrollPosition::default(),
                StepsList,
            ));
        });
}

/// Step numbers: parallel steps share a number, lettered.
fn numbering(dv: &crate::model::DynamicView) -> Vec<String> {
    let stages = dv.stages();
    let mut out = vec![String::new(); dv.steps.len()];
    for (n, stage) in stages.iter().enumerate() {
        for (k, &i) in stage.iter().enumerate() {
            out[i] = if stage.len() > 1 {
                format!("{}{}", n + 1, (b'a' + k as u8) as char)
            } else {
                format!("{}", n + 1)
            };
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
pub fn ui(
    mut commands: Commands,
    diagram: Res<Diagram>,
    fonts: Res<Fonts>,
    mut pb: ResMut<Playback>,
    mut panel: Single<&mut Visibility, With<StepsPanel>>,
    mut title: Single<&mut Text, With<StepsTitle>>,
    list: Single<Entity, With<StepsList>>,
    mut buttons: Query<(&Interaction, &StepButton, &mut BackgroundColor)>,
    mouse: Res<ButtonInput<MouseButton>>,
    ui_theme: Res<Ui>,
    mut built: Local<Option<(u64, u64, Option<usize>)>>,
) {
    let model = &diagram.model;
    if *built != Some((diagram.resets, diagram.version, pb.active)) {
        let new_model = built.is_none_or(|(r, _, _)| r != diagram.resets);
        *built = Some((diagram.resets, diagram.version, pb.active));
        if new_model && pb.active.is_some_and(|i| i >= model.views.len()) {
            pb.stop();
        }
        commands.entity(*list).despawn_related::<Children>();
        if let Some(dv) = pb.active.and_then(|i| model.views.get(i)) {
            let numbers = numbering(dv);
            for (i, s) in dv.steps.iter().enumerate() {
                let stage = dv.stages().iter().position(|st| st.contains(&i)).unwrap_or(0);
                let (a, b) = (&model.elements[s.from].title, &model.elements[s.to].title);
                let mut what = format!("{a} → {b}");
                if s.backward {
                    what.push_str("  (reply)");
                }
                commands.spawn((
                    Button,
                    Node {
                        flex_direction: FlexDirection::Row,
                        column_gap: px(10),
                        padding: UiRect::axes(px(8), px(6)),
                        border_radius: BorderRadius::all(px(6)),
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                    StepButton::Go(stage),
                    ChildOf(*list),
                    children![
                        (
                            Node {
                                width: px(26),
                                ..default()
                            },
                            Text::new(numbers[i].clone()),
                            font(&fonts, 12.5, FontWeight::SEMIBOLD),
                            TextColor::default(),
                            UiInk(Role::Accent, 1.0),
                        ),
                        (
                            Node {
                                flex_direction: FlexDirection::Column,
                                flex_shrink: 1.0,
                                ..default()
                            },
                            children![
                                (
                                    Text::new(if s.title.is_empty() { what.clone() } else { s.title.clone() }),
                                    font(&fonts, 13.0, FontWeight::MEDIUM),
                                    TextColor::default(),
                                    UiInk(Role::Text, 1.0),
                                ),
                                (
                                    Text::new(if s.title.is_empty() { String::new() } else { what }),
                                    font(&fonts, 11.5, FontWeight::NORMAL),
                                    TextColor::default(),
                                    UiInk(Role::Muted, 1.0),
                                ),
                            ],
                        ),
                    ],
                ));
            }
        }
    }

    let Some(dv) = pb.active.and_then(|i| model.views.get(i)) else {
        **panel = Visibility::Hidden;
        return;
    };
    **panel = Visibility::Inherited;
    let stages = dv.stages().len();
    let head = format!("{}   ·   {} / {}", dv.title, pb.stage + 1, stages);
    if title.0 != head {
        title.0 = head;
    }

    for (i, b, mut bg) in &mut buttons {
        let current = matches!(b, StepButton::Go(s) if *s == pb.stage);
        bg.0 = match i {
            Interaction::None if current => ui_theme.p.row_current,
            Interaction::None => Color::NONE,
            _ => ui_theme.p.hover,
        };
        if *i == Interaction::Pressed && mouse.just_pressed(MouseButton::Left) {
            match b {
                StepButton::Go(s) => pb.go_to(*s),
                StepButton::Prev => pb.prev(),
                StepButton::Next => pb.next(stages),
                StepButton::Close => pb.stop(),
            }
        }
    }
}
