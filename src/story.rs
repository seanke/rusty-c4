//! Playing dynamic views. A view is a scenario told step by step: each step sends a bright
//! pulse along the pipe between its two blocks, in order (steps in a `parallel` block go
//! together). The camera walks to whichever level a step happens on and frames it, and
//! everything not taking part is dimmed.

use std::collections::HashSet;

use bevy::prelude::*;

use crate::camera::Cam;
use crate::model::{Id, Step};
use crate::pipes::{visible_rep, Pipe};
use crate::scene::{Diagram, Fonts, ACCENT, MUTED, TEXT};
use crate::world::{Block, Geometry, Heights, View};

/// Seconds for a pulse to travel its pipe, and to linger before the next step.
const TRAVEL: f32 = 1.3;
const HOLD: f32 = 0.9;

#[derive(Resource, Default)]
pub struct Playback {
    /// The dynamic view playing (index into the model's views).
    pub active: Option<usize>,
    pub stage: usize,
    pub playing: bool,
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
        self.playing = true;
        self.version += 1;
        self.reset_stage();
    }

    pub fn stop(&mut self) {
        if self.active.take().is_some() {
            self.playing = false;
            self.version += 1;
            self.current.clear();
        }
    }

    pub fn toggle(&mut self, stages: usize) {
        if !self.playing && self.stage + 1 >= stages && self.t >= 1.0 {
            // At the end: play again from the top.
            self.stage = 0;
            self.reset_stage();
        }
        self.playing = !self.playing;
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
    // Opening a detail card pauses the story.
    if diagram.detail.is_some() {
        pb.playing = false;
    }
    let steps: Vec<&Step> = stages[pb.stage].iter().map(|&i| &dv.steps[i]).collect();
    // Fly to frame both ends of the step(s), as they will stand once their containers open.
    if !pb.framed {
        pb.framed = true;
        let mut rect: Option<Rect> = None;
        let mut y = 0.0f32;
        for s in &steps {
            for id in [s.from, s.to] {
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
        .all(|k| pipes.iter().any(|p| p.key == *k && p.ready()));
    if settled && pipes_ready {
        let dt = time.delta_secs().min(0.05);
        if pb.t < 1.0 {
            pb.t = (pb.t + dt / TRAVEL).min(1.0);
        } else {
            pb.hold += dt;
            if pb.playing && pb.hold > HOLD {
                if pb.stage + 1 < stages.len() {
                    pb.stage += 1;
                    pb.reset_stage();
                } else {
                    pb.playing = false;
                }
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
        p.dim.target = if all.contains(&p.key) { 0.0 } else { 1.0 };
        p.hl.target = if pb.current.contains(&p.key) { 1.0 } else { 0.0 };
    }
}

/// The pulse's look in each style: a glowing ball, a dot of ink, a block of redstone, a
/// yellow marker dot.
#[derive(Resource)]
pub struct PacketAssets {
    mesh: [Handle<Mesh>; 4],
    material: [Handle<StandardMaterial>; 4],
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
        mesh: [ball.clone(), ball.clone(), cube, ball],
        material: [
            mats.add(StandardMaterial {
                base_color: Color::BLACK,
                emissive: LinearRgba::rgb(14.0, 34.0, 40.0),
                ..default()
            }),
            mats.add(flat(Color::srgb(0.8, 0.16, 0.1))),
            mats.add(flat(Color::srgb(1.0, 0.2, 0.1))),
            mats.add(flat(Color::srgb(1.0, 0.85, 0.3))),
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
        let Some(pipe) = pipes.iter().find(|p| p.key == key && p.ready()) else {
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

const PANEL_BG: Color = Color::srgba(0.01, 0.03, 0.05, 0.9);
const EDGE: Color = Color::srgba(0.2, 0.9, 1.0, 0.25);

#[derive(Component)]
pub struct ViewsPanel;

#[derive(Component)]
pub struct PlayerBar;

#[derive(Component)]
pub struct Caption;

#[derive(Component, Clone, Copy, PartialEq)]
pub enum StoryButton {
    View(usize),
    Prev,
    Toggle,
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

fn button(fonts: &Fonts, label: &str, action: StoryButton, wide: bool) -> impl Bundle {
    (
        Button,
        Node {
            padding: UiRect::axes(px(if wide { 14 } else { 12 }), px(7)),
            justify_content: if wide { JustifyContent::FlexStart } else { JustifyContent::Center },
            min_width: px(if wide { 0 } else { 44 }),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(8)),
            ..default()
        },
        BackgroundColor(Color::NONE),
        BorderColor::all(Color::NONE),
        action,
        children![(Text::new(label), font(fonts, 14.0, FontWeight::MEDIUM), TextColor(TEXT))],
    )
}

pub fn spawn_ui(mut commands: Commands, fonts: Res<Fonts>) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: px(16),
            right: px(16),
            flex_direction: FlexDirection::Column,
            row_gap: px(2),
            padding: UiRect::all(px(8)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(12)),
            ..default()
        },
        BackgroundColor(PANEL_BG),
        BorderColor::all(EDGE),
        Interaction::default(),
        Visibility::Hidden,
        ViewsPanel,
    ));
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: px(42),
                left: px(0),
                right: px(0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Visibility::Hidden,
            PlayerBar,
        ))
        .with_children(|outer| {
            outer
                .spawn((
                    Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: px(6),
                        padding: UiRect::axes(px(10), px(8)),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(14)),
                        max_width: px(900),
                        ..default()
                    },
                    BackgroundColor(PANEL_BG),
                    BorderColor::all(EDGE),
                    Interaction::default(),
                ))
                .with_children(|bar| {
                    bar.spawn(button(&fonts, "‹", StoryButton::Prev, false));
                    bar.spawn(button(&fonts, "Pause", StoryButton::Toggle, false));
                    bar.spawn(button(&fonts, "›", StoryButton::Next, false));
                    bar.spawn((
                        Node {
                            margin: UiRect::horizontal(px(10)),
                            min_width: px(320),
                            ..default()
                        },
                        Text::new(""),
                        font(&fonts, 13.0, FontWeight::MEDIUM),
                        TextColor(MUTED),
                        Caption,
                    ));
                    bar.spawn(button(&fonts, "×", StoryButton::Close, false));
                });
        });
}

#[allow(clippy::too_many_arguments)]
pub fn ui(
    mut commands: Commands,
    diagram: Res<Diagram>,
    fonts: Res<Fonts>,
    menu: Res<crate::ui::Menu>,
    mut pb: ResMut<Playback>,
    panel: Single<(Entity, &mut Visibility), (With<ViewsPanel>, Without<PlayerBar>)>,
    mut bar: Single<&mut Visibility, (With<PlayerBar>, Without<ViewsPanel>)>,
    mut caption: Single<&mut Text, With<Caption>>,
    mut buttons: Query<(&Interaction, &StoryButton, &mut BackgroundColor), Changed<Interaction>>,
    all_buttons: Query<(&StoryButton, &Children)>,
    mut labels: Query<&mut Text, Without<Caption>>,
    mut colors: Query<&mut TextColor>,
    mut built: Local<Option<(u64, usize)>>,
) {
    let model = &diagram.model;
    let (panel_e, mut panel_vis) = panel.into_inner();
    let show = diagram.loaded && !menu.open;

    // The list of dynamic views (rebuilt for each model).
    if *built != Some((diagram.resets, model.views.len())) {
        *built = Some((diagram.resets, model.views.len()));
        pb.stop();
        commands.entity(panel_e).despawn_related::<Children>();
        commands.spawn((
            Node {
                padding: UiRect::axes(px(14), px(6)),
                ..default()
            },
            Text::new("DYNAMIC VIEWS"),
            font(&fonts, 11.0, FontWeight::SEMIBOLD),
            TextColor(MUTED),
            ChildOf(panel_e),
        ));
        for (i, v) in model.views.iter().enumerate() {
            commands.spawn((button(&fonts, &v.title, StoryButton::View(i), true), ChildOf(panel_e)));
        }
    }
    let want = if show && !model.views.is_empty() { Visibility::Inherited } else { Visibility::Hidden };
    if *panel_vis != want {
        *panel_vis = want;
    }

    let stages = pb.active.and_then(|i| model.views.get(i)).map_or(0, |v| v.stages().len());
    for (interaction, action, mut bg) in &mut buttons {
        bg.0 = match interaction {
            Interaction::None => Color::NONE,
            _ => Color::srgba(0.2, 0.9, 1.0, 0.12),
        };
        if *interaction != Interaction::Pressed {
            continue;
        }
        match action {
            StoryButton::View(i) => {
                if pb.active == Some(*i) {
                    pb.stop();
                } else {
                    pb.start(*i);
                }
            }
            StoryButton::Prev => pb.prev(),
            StoryButton::Next => pb.next(stages),
            StoryButton::Toggle => pb.toggle(stages),
            StoryButton::Close => pb.stop(),
        }
    }

    // Mark the playing view in the list, and the play / pause label.
    for (action, children) in &all_buttons {
        let Some(&child) = children.first() else { continue };
        match action {
            StoryButton::View(i) => {
                if let Ok(mut c) = colors.get_mut(child) {
                    c.0 = if pb.active == Some(*i) { ACCENT } else { TEXT };
                }
            }
            StoryButton::Toggle => {
                if let Ok(mut t) = labels.get_mut(child) {
                    let at_end = pb.stage + 1 >= stages && pb.t >= 1.0;
                    let label = if pb.playing { "Pause" } else if at_end { "Replay" } else { "Play" };
                    if t.0 != label {
                        t.0 = label.to_string();
                    }
                }
            }
            _ => {}
        }
    }

    let Some(dv) = pb.active.and_then(|i| model.views.get(i)) else {
        **bar = Visibility::Hidden;
        return;
    };
    **bar = if show { Visibility::Inherited } else { Visibility::Hidden };
    let stage_steps = dv.stages().get(pb.stage).cloned().unwrap_or_default();
    let mut text = format!("{}   ·   step {} of {}", dv.title, pb.stage + 1, stages);
    if !dv.description.is_empty() {
        text.push_str(&format!("   ·   {}", dv.description));
    }
    for i in stage_steps {
        let s = &dv.steps[i];
        let (a, b) = (&model.elements[s.from].title, &model.elements[s.to].title);
        text.push_str(&format!("\n{a}  →  {b}"));
        if !s.title.is_empty() {
            text.push_str(&format!("     {}", s.title));
        }
        if !s.technology.is_empty() {
            text.push_str(&format!("  [{}]", s.technology));
        }
        if s.backward {
            text.push_str("  (reply)");
        }
    }
    if caption.0 != text {
        caption.0 = text;
    }
}
