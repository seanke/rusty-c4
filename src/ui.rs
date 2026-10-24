//! Screen-space overlay: breadcrumbs, the details panel, hints and warnings.

use bevy::prelude::*;

use crate::model::Id;
use crate::render::Spring;
use crate::scene::{Diagram, Fonts, Nav, NavQueue, ACCENT, MUTED, TEXT};

const PANEL_BG: Color = Color::srgba(0.07, 0.08, 0.105, 0.92);
const HAIRLINE: Color = Color::srgba(1.0, 1.0, 1.0, 0.07);

#[derive(Component)]
pub struct CrumbBar;

#[derive(Component)]
pub struct Crumb(Option<Id>);

#[derive(Component)]
pub struct Panel;

#[derive(Component)]
pub struct WarnText;

#[derive(Resource)]
pub struct UiState {
    version: u64,
    panel: Spring<f32>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            version: u64::MAX,
            panel: Spring::new(0.0),
        }
    }
}

fn font(fonts: &Fonts, size: f32, weight: FontWeight) -> TextFont {
    TextFont {
        font: fonts.ui.clone(),
        font_size: size,
        weight,
        ..default()
    }
}

pub fn spawn(mut commands: Commands, fonts: Res<Fonts>) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: px(16),
            left: px(16),
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(2),
            padding: UiRect::all(px(5)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(12)),
            ..default()
        },
        BackgroundColor(PANEL_BG),
        BorderColor::all(HAIRLINE),
        CrumbBar,
    ));

    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            bottom: px(14),
            left: px(0),
            right: px(0),
            justify_content: JustifyContent::Center,
            ..default()
        },
        children![(
            Text::new(
                "scroll / pinch  zoom      drag  pan      click  open      click outside / esc  back      f  fit      r  reload"
            ),
            font(&fonts, 12.0, FontWeight::MEDIUM),
            TextColor(Color::srgba(0.85, 0.88, 0.95, 0.38)),
        )],
    ));

    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            bottom: px(40),
            left: px(18),
            max_width: px(560),
            ..default()
        },
        Text::new(""),
        font(&fonts, 12.5, FontWeight::MEDIUM),
        TextColor(Color::srgb(0.95, 0.72, 0.35)),
        WarnText,
    ));

    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: px(16),
            right: px(16),
            width: px(360),
            max_height: percent(86),
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            padding: UiRect::all(px(20)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(14)),
            overflow: Overflow::clip_y(),
            ..default()
        },
        BackgroundColor(PANEL_BG),
        BorderColor::all(HAIRLINE),
        Interaction::default(),
        Visibility::Hidden,
        Panel,
    ));
}

pub fn crumb_clicks(
    mut crumbs: Query<(&Interaction, &Crumb, &mut BackgroundColor), Changed<Interaction>>,
    mut queue: ResMut<NavQueue>,
) {
    for (interaction, crumb, mut bg) in &mut crumbs {
        bg.0 = match interaction {
            Interaction::Pressed => {
                queue.0.push(Nav::Select(None));
                queue.0.push(Nav::Focus(crumb.0));
                Color::srgba(1.0, 1.0, 1.0, 0.12)
            }
            Interaction::Hovered => Color::srgba(1.0, 1.0, 1.0, 0.07),
            Interaction::None => Color::NONE,
        };
    }
}

#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    time: Res<Time>,
    diagram: Res<Diagram>,
    fonts: Res<Fonts>,
    mut state: ResMut<UiState>,
    bar: Single<Entity, With<CrumbBar>>,
    panel: Single<(Entity, &mut Node, &mut Visibility), With<Panel>>,
    mut warn: Single<&mut Text, With<WarnText>>,
) {
    let (panel_e, mut panel_node, mut panel_vis) = panel.into_inner();
    let model = &diagram.model;

    if state.version != diagram.version {
        state.version = diagram.version;

        // Breadcrumbs.
        commands.entity(*bar).despawn_related::<Children>();
        let mut items: Vec<Option<Id>> = vec![None];
        items.extend(model.path(diagram.focus).into_iter().map(Some));
        let last = items.len() - 1;
        for (i, item) in items.into_iter().enumerate() {
            if i > 0 {
                commands.spawn((
                    Text::new("›"),
                    font(&fonts, 15.0, FontWeight::MEDIUM),
                    TextColor(Color::srgba(0.85, 0.88, 0.95, 0.3)),
                    ChildOf(*bar),
                ));
            }
            let label = match item {
                Some(id) => model.elements[id].title.clone(),
                None => "Landscape".to_string(),
            };
            let current = i == last;
            commands.spawn((
                Button,
                Node {
                    padding: UiRect::axes(px(10), px(6)),
                    border_radius: BorderRadius::all(px(8)),
                    ..default()
                },
                BackgroundColor(Color::NONE),
                Crumb(item),
                ChildOf(*bar),
                children![(
                    Text::new(label),
                    font(
                        &fonts,
                        14.0,
                        if current { FontWeight::SEMIBOLD } else { FontWeight::MEDIUM }
                    ),
                    TextColor(if current { TEXT } else { MUTED }),
                )],
            ));
        }

        // Details panel.
        commands.entity(panel_e).despawn_related::<Children>();
        if let Some(id) = diagram.selected {
            let el = &model.elements[id];
            let header = [el.kind.to_uppercase(), el.technology.clone()]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("  ·  ");
            let c = Color::srgb(el.color[0], el.color[1], el.color[2]);
            let mut line = |text: String, size: f32, weight: FontWeight, color: Color, top: f32| {
                commands.spawn((
                    Node {
                        margin: UiRect::top(px(top)),
                        ..default()
                    },
                    Text::new(text),
                    font(&fonts, size, weight, ),
                    TextColor(color),
                    ChildOf(panel_e),
                ));
            };
            line(header, 11.0, FontWeight::SEMIBOLD, c.mix(&Color::WHITE, 0.45), 0.0);
            line(el.title.clone(), 21.0, FontWeight::BOLD, TEXT, 0.0);
            if !el.description.is_empty() {
                line(
                    el.description.clone(),
                    14.0,
                    FontWeight::NORMAL,
                    Color::srgba(0.92, 0.94, 1.0, 0.8),
                    4.0,
                );
            }
            if !el.children.is_empty() {
                line(
                    format!("{} elements inside", el.children.len()),
                    12.5,
                    FontWeight::MEDIUM,
                    ACCENT,
                    4.0,
                );
            }
            let rels = |outgoing: bool| -> Vec<String> {
                model
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
                            s.push_str(&format!("   {}", r.title));
                        }
                        if !r.technology.is_empty() {
                            s.push_str(&format!("  [{}]", r.technology));
                        }
                        Some(s)
                    })
                    .collect()
            };
            for (name, list) in [("OUTGOING", rels(true)), ("INCOMING", rels(false))] {
                if list.is_empty() {
                    continue;
                }
                line(name.to_string(), 10.5, FontWeight::SEMIBOLD, MUTED, 12.0);
                let n = list.len();
                for s in list.into_iter().take(10) {
                    line(s, 13.0, FontWeight::NORMAL, Color::srgba(0.92, 0.94, 1.0, 0.85), 0.0);
                }
                if n > 10 {
                    line(format!("and {} more", n - 10), 12.0, FontWeight::NORMAL, MUTED, 0.0);
                }
            }
            line(el.fqn.clone(), 11.0, FontWeight::NORMAL, Color::srgba(0.85, 0.88, 0.95, 0.35), 12.0);
        }

        warn.0 = match diagram.warnings.as_slice() {
            [] => String::new(),
            [one] => format!("⚠  {one}"),
            [first, rest @ ..] => format!("⚠  {first}   (+{} more)", rest.len()),
        };
    }

    // Slide the panel in and out.
    state.panel.target = if diagram.selected.is_some() { 1.0 } else { 0.0 };
    state.panel.step(time.delta_secs().min(0.05), 16.0, 0.9);
    let t = state.panel.value;
    panel_node.right = px(16.0 - 400.0 * (1.0 - t));
    let want = if t < 0.01 && state.panel.target == 0.0 {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };
    if *panel_vis != want {
        *panel_vis = want;
    }
}
