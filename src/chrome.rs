//! The window's chrome: a slim toolbar of icons along the top (with tooltips naming each
//! one and its shortcut), dropdown menus, and the menu bar: the native one on macOS, or a
//! row of menus in the window elsewhere.

use bevy::prelude::*;

use crate::actions::{entries, Actions, AppAction, Entry, MenuKind};
use crate::editor::Code;
use crate::icons::{Icon, Icons};
use crate::scene::{Diagram, Fonts, TEXT};
use crate::story::Playback;
use crate::theme::Theme;

pub const MENUBAR_H: f32 = 28.0;
pub const TOOLBAR_H: f32 = 40.0;
const BAR_BG: Color = Color::srgb(0.055, 0.063, 0.082);
const HAIRLINE: Color = Color::srgba(1.0, 1.0, 1.0, 0.08);
const HOVER: Color = Color::srgba(1.0, 1.0, 1.0, 0.08);
const PRESSED: Color = Color::srgba(1.0, 1.0, 1.0, 0.14);
const ACTIVE: Color = Color::srgba(0.35, 0.6, 1.0, 0.22);
const ICON: Color = Color::srgba(0.86, 0.9, 0.97, 0.9);

/// How the window is laid out around the 3D view.
#[derive(Resource)]
pub struct Chrome {
    /// Menus are in the system menu bar (macOS), not in the window.
    pub native_menu: bool,
    /// Height taken at the top by the menu bar (if in the window) and the toolbar.
    pub top: f32,
}

impl Chrome {
    pub fn new() -> Chrome {
        let forced = std::env::var("RUSTYC4_MENUBAR").is_ok_and(|v| v == "window");
        let native_menu = cfg!(target_os = "macos") && !forced;
        Chrome {
            native_menu,
            top: TOOLBAR_H + if native_menu { 0.0 } else { MENUBAR_H },
        }
    }
}

#[derive(Component)]
pub struct ToolButton {
    action: Option<AppAction>,
    menu: Option<MenuKind>,
}

/// A menu title in the in-window menu bar.
#[derive(Component)]
pub struct MenuTitle(MenuKind);

#[derive(Component)]
pub struct ToolIcon;

#[derive(Component)]
pub struct SequenceName;

#[derive(Component)]
pub struct Tooltip;

#[derive(Component)]
pub struct PopupItem(Option<AppAction>);

#[derive(Resource, Default)]
pub struct Popup {
    /// The open menu, the button it hangs from, and whether that's in the menu bar.
    open: Option<(MenuKind, Entity, bool)>,
    root: Option<Entity>,
    hover: Option<(Entity, f32)>,
}

/// A menu to open as if its button were clicked (for scripted checks).
#[derive(Resource, Default)]
pub struct MenuRequest(pub Option<String>);

fn ui_font(fonts: &Fonts, size: f32, weight: FontWeight) -> TextFont {
    TextFont {
        font: fonts.ui.clone(),
        font_size: size,
        weight,
        ..default()
    }
}

fn tool(icons: &Icons, icon: Icon, action: Option<AppAction>, menu: Option<MenuKind>) -> impl Bundle {
    (
        Button,
        Node {
            width: px(30),
            height: px(28),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border_radius: BorderRadius::all(px(6)),
            ..default()
        },
        BackgroundColor(Color::NONE),
        ToolButton { action, menu },
        children![(
            ImageNode::new(icons.get(icon)).with_color(ICON),
            Node {
                width: px(16),
                height: px(16),
                ..default()
            },
            ToolIcon,
        )],
    )
}

fn separator() -> impl Bundle {
    (
        Node {
            width: px(1),
            height: px(18),
            margin: UiRect::horizontal(px(6)),
            ..default()
        },
        BackgroundColor(HAIRLINE),
    )
}

pub fn spawn(mut commands: Commands, chrome: Res<Chrome>, icons: Res<Icons>, fonts: Res<Fonts>) {
    if !chrome.native_menu {
        commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    top: px(0),
                    left: px(0),
                    right: px(0),
                    height: px(MENUBAR_H),
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(px(6)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.045, 0.052, 0.068)),
                Interaction::default(),
            ))
            .with_children(|bar| {
                for kind in [MenuKind::File, MenuKind::View, MenuKind::Sequences] {
                    bar.spawn((
                        Button,
                        Node {
                            padding: UiRect::axes(px(10), px(4)),
                            border_radius: BorderRadius::all(px(5)),
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                        MenuTitle(kind),
                        children![(Text::new(kind.title()), ui_font(&fonts, 13.0, FontWeight::NORMAL), TextColor(TEXT.with_alpha(0.85)))],
                    ));
                }
            });
    }

    let top = if chrome.native_menu { 0.0 } else { MENUBAR_H };
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(top),
                left: px(0),
                right: px(0),
                height: px(TOOLBAR_H),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                padding: UiRect::horizontal(px(8)),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BackgroundColor(BAR_BG),
            BorderColor::all(HAIRLINE),
            Interaction::default(),
        ))
        .with_children(|bar| {
            bar.spawn(Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(2),
                ..default()
            })
            .with_children(|left| {
                left.spawn(tool(&icons, Icon::Folder, Some(AppAction::OpenFolder), None));
                left.spawn(tool(&icons, Icon::File, Some(AppAction::OpenFile), None));
                left.spawn(tool(&icons, Icon::Save, Some(AppAction::Save), None));
                left.spawn(tool(&icons, Icon::Refresh, Some(AppAction::Reload), None));
                left.spawn(separator());
                // The sequence picker: an icon, the current sequence and a chevron.
                left.spawn((
                    Button,
                    Node {
                        height: px(28),
                        padding: UiRect::axes(px(8), px(0)),
                        column_gap: px(7),
                        align_items: AlignItems::Center,
                        border_radius: BorderRadius::all(px(6)),
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                    ToolButton {
                        action: None,
                        menu: Some(MenuKind::Sequences),
                    },
                ))
                .with_children(|b| {
                    b.spawn((
                        ImageNode::new(icons.get(Icon::List)).with_color(ICON),
                        Node {
                            width: px(16),
                            height: px(16),
                            ..default()
                        },
                    ));
                    b.spawn((
                        Text::new("Sequences"),
                        ui_font(&fonts, 12.5, FontWeight::MEDIUM),
                        TextColor(TEXT.with_alpha(0.85)),
                        SequenceName,
                    ));
                    b.spawn((
                        ImageNode::new(icons.get(Icon::ChevronDown)).with_color(ICON.with_alpha(0.6)),
                        Node {
                            width: px(12),
                            height: px(12),
                            ..default()
                        },
                    ));
                });
                left.spawn(tool(&icons, Icon::ChevronLeft, Some(AppAction::PrevStep), None));
                left.spawn(tool(&icons, Icon::ChevronRight, Some(AppAction::NextStep), None));
            });
            bar.spawn(Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(2),
                ..default()
            })
            .with_children(|right| {
                right.spawn(tool(&icons, Icon::ZoomOut, Some(AppAction::ZoomOut), None));
                right.spawn(tool(&icons, Icon::ZoomIn, Some(AppAction::ZoomIn), None));
                right.spawn(tool(&icons, Icon::Fit, Some(AppAction::Fit), None));
                right.spawn(separator());
                right.spawn(tool(&icons, Icon::Palette, None, Some(MenuKind::Style)));
                right.spawn(tool(&icons, Icon::Sidebar, Some(AppAction::ToggleSource), None));
            });
        });

    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            padding: UiRect::axes(px(8), px(4)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(5)),
            ..default()
        },
        BackgroundColor(Color::srgb(0.13, 0.14, 0.18)),
        BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.16)),
        BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.45), px(0), px(4), px(0), px(12)),
        GlobalZIndex(120),
        Visibility::Hidden,
        Text::new(""),
        ui_font(&fonts, 12.0, FontWeight::MEDIUM),
        TextColor(TEXT.with_alpha(0.9)),
        Tooltip,
    ));
}

/// The rectangle a UI node covers, in logical pixels.
fn node_rect(node: &ComputedNode, tf: &UiGlobalTransform) -> Rect {
    let k = node.inverse_scale_factor;
    Rect::from_center_size(tf.translation * k, node.size * k)
}

fn tip_text(tb: &ToolButton, diagram: &Diagram) -> String {
    match (tb.action, tb.menu) {
        (Some(a), _) => match a.shortcut() {
            Some(s) => format!("{}   {s}", a.label(diagram)),
            None => a.label(diagram),
        },
        (None, Some(MenuKind::Style)) => "Style   T".into(),
        (None, Some(MenuKind::Sequences)) => "Choose a sequence".into(),
        (None, Some(k)) => k.title().into(),
        _ => String::new(),
    }
}

/// Clicks, hover states, tooltips and the dropdown menus.
#[allow(clippy::too_many_arguments)]
pub fn interact(
    mut commands: Commands,
    (time, diagram, theme, playback, code, fonts): (Res<Time>, Res<Diagram>, Res<Theme>, Res<Playback>, Res<Code>, Res<Fonts>),
    window: Single<&Window>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut actions: ResMut<Actions>,
    mut popup: ResMut<Popup>,
    mut request: ResMut<MenuRequest>,
    mut buttons: Query<(Entity, &Interaction, &ToolButton, &mut BackgroundColor, &Children, &ComputedNode, &UiGlobalTransform), Without<MenuTitle>>,
    mut titles: Query<(Entity, &Interaction, &MenuTitle, &mut BackgroundColor, &ComputedNode, &UiGlobalTransform), Without<ToolButton>>,
    mut items: Query<(&Interaction, &PopupItem, &mut BackgroundColor), (Without<ToolButton>, Without<MenuTitle>)>,
    mut icons: Query<&mut ImageNode, With<ToolIcon>>,
    mut seq_name: Single<&mut Text, (With<SequenceName>, Without<Tooltip>)>,
    tip: Single<(&mut Node, &mut Text, &mut Visibility, &ComputedNode), With<Tooltip>>,
) {
    // Dropdown items.
    let mut picked = false;
    for (i, item, mut bg) in &mut items {
        bg.0 = match (i, item.0.is_some()) {
            (Interaction::None, _) | (_, false) => Color::NONE,
            _ => Color::srgba(0.35, 0.6, 1.0, 0.35),
        };
        if *i == Interaction::Pressed {
            if let Some(a) = item.0 {
                actions.0.push(a);
                picked = true;
            }
        }
    }
    let close = |commands: &mut Commands, popup: &mut Popup| {
        if let Some(r) = popup.root.take() {
            commands.entity(r).despawn();
        }
        popup.open = None;
    };
    if picked {
        close(&mut commands, &mut popup);
    }

    // Which menu to open, if a button for one was pressed.
    let mut open: Option<(MenuKind, Entity, Rect, bool)> = None;
    let wanted = request.0.take().and_then(|n| {
        [MenuKind::File, MenuKind::View, MenuKind::Sequences, MenuKind::Style]
            .into_iter()
            .find(|k| k.title().eq_ignore_ascii_case(&n))
    });
    let mut hovered: Option<(Entity, String, Rect)> = None;
    let mut any_pressed = false;
    for (e, i, tb, mut bg, children, node, tf) in &mut buttons {
        let active = match tb.action {
            Some(AppAction::ToggleSource) => code.open,
            _ => tb.menu.is_some_and(|m| popup.open.is_some_and(|(k, _, _)| k == m)),
        };
        if wanted.is_some() && tb.menu == wanted {
            open = Some((tb.menu.unwrap(), e, node_rect(node, tf), false));
        }
        bg.0 = match i {
            Interaction::Pressed => PRESSED,
            Interaction::Hovered => HOVER,
            Interaction::None if active => ACTIVE,
            Interaction::None => Color::NONE,
        };
        let enabled = match tb.action {
            Some(AppAction::PrevStep | AppAction::NextStep) => playback.active.is_some(),
            Some(AppAction::Save | AppAction::ToggleSource | AppAction::Fit | AppAction::ZoomIn | AppAction::ZoomOut) => diagram.loaded,
            Some(AppAction::Reload) => diagram.path.is_some(),
            _ => true,
        };
        for c in children.iter() {
            if let Ok(mut img) = icons.get_mut(c) {
                img.color = if enabled { ICON } else { ICON.with_alpha(0.3) };
            }
        }
        if *i == Interaction::Hovered {
            hovered = Some((e, tip_text(tb, &diagram), node_rect(node, tf)));
        }
        if *i == Interaction::Pressed && mouse.just_pressed(MouseButton::Left) {
            any_pressed = true;
            match (tb.action, tb.menu) {
                (Some(a), _) if enabled => actions.0.push(a),
                (None, Some(m)) => open = Some((m, e, node_rect(node, tf), false)),
                _ => {}
            }
        }
    }
    for (e, i, t, mut bg, node, tf) in &mut titles {
        let active = popup.open.is_some_and(|(k, _, _)| k == t.0);
        bg.0 = match i {
            Interaction::None if !active => Color::NONE,
            _ => HOVER,
        };
        if wanted == Some(t.0) {
            open = Some((t.0, e, node_rect(node, tf), true));
        }
        if *i == Interaction::Pressed && mouse.just_pressed(MouseButton::Left) {
            any_pressed = true;
            open = Some((t.0, e, node_rect(node, tf), true));
        }
        // Moving across the menu bar with one of its menus open switches menus.
        if *i == Interaction::Hovered && popup.open.is_some_and(|(k, _, bar)| bar && k != t.0) {
            open = Some((t.0, e, node_rect(node, tf), true));
        }
    }

    // Open (or toggle) a menu.
    if let Some((kind, anchor, rect, bar)) = open {
        let same = popup.open.is_some_and(|(k, _, _)| k == kind);
        close(&mut commands, &mut popup);
        if !same {
            let list = entries(kind, &diagram, &theme, &playback, false);
            let root = spawn_menu(&mut commands, &fonts, &diagram, &list, rect, window.width());
            popup.open = Some((kind, anchor, bar));
            popup.root = Some(root);
        }
    } else if mouse.just_pressed(MouseButton::Left) && popup.open.is_some() && !any_pressed {
        // A click anywhere else closes the menu (items handle their own clicks above).
        let on_item = items.iter().any(|(i, _, _)| *i != Interaction::None);
        if !on_item {
            close(&mut commands, &mut popup);
        }
    }

    // The sequence picker shows the current sequence.
    let name = match playback.active.and_then(|i| diagram.model.views.get(i)) {
        Some(v) => v.title.clone(),
        None => "Sequences".to_string(),
    };
    if seq_name.0 != name {
        seq_name.0 = name;
    }

    // Tooltip, after a short hover, below the button.
    let (mut tip_node, mut tip_textc, mut tip_vis, tip_size) = tip.into_inner();
    match hovered.filter(|_| popup.open.is_none()) {
        Some((e, text, rect)) => {
            let t = match popup.hover {
                Some((he, t)) if he == e => t + time.delta_secs(),
                _ => 0.0,
            };
            popup.hover = Some((e, t));
            if t > 0.45 && !text.is_empty() {
                if tip_textc.0 != text {
                    tip_textc.0 = text;
                }
                // Centred under the button, using its laid-out width once it has one.
                let measured = tip_size.size().x * tip_size.inverse_scale_factor();
                let w = if measured > 0.0 { measured } else { tip_textc.0.chars().count() as f32 * 7.0 + 18.0 };
                let left = (rect.center().x - w / 2.0).clamp(6.0, (window.width() - w - 6.0).max(6.0));
                tip_node.left = px(left);
                tip_node.top = px(rect.max.y + 11.0);
                *tip_vis = Visibility::Inherited;
            } else {
                *tip_vis = Visibility::Hidden;
            }
        }
        None => {
            popup.hover = None;
            *tip_vis = Visibility::Hidden;
        }
    }
}

fn spawn_menu(commands: &mut Commands, fonts: &Fonts, diagram: &Diagram, list: &[Entry], anchor: Rect, window_w: f32) -> Entity {
    let width = 250.0;
    let left = anchor.min.x.min(window_w - width - 6.0).max(4.0);
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(left),
                top: px(anchor.max.y + 4.0),
                width: px(width),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(5)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.085, 0.095, 0.12)),
            BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.12)),
            BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.45), px(0), px(6), px(0), px(18)),
            GlobalZIndex(110),
            Interaction::default(),
        ))
        .with_children(|m| {
            for e in list {
                match e {
                    Entry::Separator => {
                        m.spawn((
                            Node {
                                height: px(1),
                                margin: UiRect::axes(px(6), px(4)),
                                ..default()
                            },
                            BackgroundColor(HAIRLINE),
                        ));
                    }
                    Entry::Heading(h) => {
                        m.spawn((
                            Node {
                                padding: UiRect::new(px(10), px(10), px(4), px(2)),
                                ..default()
                            },
                            Text::new(h.to_uppercase()),
                            ui_font(fonts, 10.5, FontWeight::SEMIBOLD),
                            TextColor(TEXT.with_alpha(0.45)),
                        ));
                    }
                    Entry::Item { action, checked, enabled } => {
                        let alpha = if *enabled { 0.9 } else { 0.35 };
                        m.spawn((
                            Button,
                            Node {
                                flex_direction: FlexDirection::Row,
                                align_items: AlignItems::Center,
                                padding: UiRect::axes(px(8), px(5)),
                                border_radius: BorderRadius::all(px(5)),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                            PopupItem(enabled.then_some(*action)),
                        ))
                        .with_children(|row| {
                            row.spawn((
                                Node {
                                    width: px(18),
                                    ..default()
                                },
                                Text::new(if *checked == Some(true) { "✓" } else { "" }),
                                ui_font(fonts, 12.5, FontWeight::SEMIBOLD),
                                TextColor(TEXT.with_alpha(alpha)),
                            ));
                            row.spawn((
                                Node {
                                    flex_grow: 1.0,
                                    ..default()
                                },
                                Text::new(action.label(diagram)),
                                ui_font(fonts, 13.0, FontWeight::NORMAL),
                                TextColor(TEXT.with_alpha(alpha)),
                            ));
                            if let Some(s) = action.shortcut() {
                                row.spawn((Text::new(s), ui_font(fonts, 12.0, FontWeight::NORMAL), TextColor(TEXT.with_alpha(alpha * 0.55))));
                            }
                        });
                    }
                }
            }
        })
        .id()
}

#[cfg(target_os = "macos")]
pub mod native {
    //! The macOS menu bar, built with `muda` from the same entries as the in-window menus.

    use std::cell::RefCell;
    use std::collections::HashMap;

    use bevy::{ecs::system::NonSendMarker, prelude::*};
    use muda::{
        accelerator::{Accelerator, Code as Key, Modifiers},
        CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu,
    };

    use super::Chrome;
    use crate::actions::{entries, Actions, AppAction, Entry, MenuKind};
    use crate::scene::Diagram;
    use crate::story::Playback;
    use crate::theme::Theme;

    struct NativeMenu {
        menu: Menu,
        /// The rebuilt menus, after the app menu.
        menus: Vec<Submenu>,
        ids: HashMap<String, AppAction>,
        state: Option<(u64, u64, Option<usize>, bool, bool)>,
    }

    thread_local! {
        static MENU: RefCell<Option<NativeMenu>> = const { RefCell::new(None) };
    }

    fn accelerator(a: AppAction) -> Option<Accelerator> {
        let cmd = Some(Modifiers::SUPER);
        Some(match a {
            AppAction::OpenFolder => Accelerator::new(cmd, Key::KeyO),
            AppAction::OpenFile => Accelerator::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Key::KeyO),
            AppAction::Save => Accelerator::new(cmd, Key::KeyS),
            AppAction::Reload => Accelerator::new(cmd, Key::KeyR),
            AppAction::ToggleSource => Accelerator::new(cmd, Key::KeyE),
            AppAction::ZoomIn => Accelerator::new(cmd, Key::Equal),
            AppAction::ZoomOut => Accelerator::new(cmd, Key::Minus),
            _ => return None,
        })
    }

    pub fn setup(_main_thread: NonSendMarker, chrome: Res<Chrome>) {
        if !chrome.native_menu {
            return;
        }
        let menu = Menu::new();
        let app = Submenu::new("rustyc4", true);
        let _ = app.append_items(&[
            &PredefinedMenuItem::about(Some("About rustyc4"), None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::services(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::hide(None),
            &PredefinedMenuItem::hide_others(None),
            &PredefinedMenuItem::show_all(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::quit(None),
        ]);
        let _ = menu.append(&app);
        menu.init_for_nsapp();
        MENU.with(|m| {
            *m.borrow_mut() = Some(NativeMenu {
                menu,
                menus: Vec::new(),
                ids: HashMap::new(),
                state: None,
            })
        });
    }

    /// Rebuild the menus when what they show changes, and turn menu clicks into actions.
    pub fn update(_main_thread: NonSendMarker, diagram: Res<Diagram>, theme: Res<Theme>, playback: Res<Playback>, mut actions: ResMut<Actions>) {
        MENU.with(|m| {
            let mut m = m.borrow_mut();
            let Some(nm) = m.as_mut() else { return };
            let state = (diagram.version, theme.version, playback.active, diagram.loaded, diagram.path.is_some());
            if nm.state != Some(state) {
                nm.state = Some(state);
                for s in nm.menus.drain(..) {
                    let _ = nm.menu.remove(&s);
                }
                nm.ids.clear();
                let window = Submenu::new("Window", true);
                let _ = window.append_items(&[&PredefinedMenuItem::minimize(None), &PredefinedMenuItem::fullscreen(None)]);
                for kind in [MenuKind::File, MenuKind::View, MenuKind::Sequences] {
                    let sub = Submenu::new(kind.title(), true);
                    for (n, e) in entries(kind, &diagram, &theme, &playback, true).into_iter().enumerate() {
                        match e {
                            Entry::Separator => {
                                let _ = sub.append(&PredefinedMenuItem::separator());
                            }
                            Entry::Heading(h) => {
                                let _ = sub.append(&MenuItem::new(h, false, None));
                            }
                            Entry::Item { action, checked, enabled } => {
                                let id = format!("{}-{n}", kind.title());
                                let label = action.label(&diagram);
                                let acc = accelerator(action);
                                let _ = match checked {
                                    Some(c) => sub.append(&CheckMenuItem::with_id(id.clone(), label, enabled, c, acc)),
                                    None => sub.append(&MenuItem::with_id(id.clone(), label, enabled, acc)),
                                };
                                nm.ids.insert(id, action);
                            }
                        }
                    }
                    let _ = nm.menu.append(&sub);
                    nm.menus.push(sub);
                }
                let _ = nm.menu.append(&window);
                nm.menus.push(window);
            }
            while let Ok(ev) = MenuEvent::receiver().try_recv() {
                if let Some(a) = nm.ids.get(&ev.id.0) {
                    actions.0.push(*a);
                }
            }
        });
    }
}
