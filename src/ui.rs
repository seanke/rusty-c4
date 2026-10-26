//! Screen-space overlay: hints, warnings, the welcome screen and pause menu.

use bevy::{ecs::system::NonSendMarker, prelude::*, window::FileDragAndDrop};

use crate::scene::{Diagram, Fonts, Nav, NavQueue, Source, ACCENT, MUTED, TEXT};

const HAIRLINE: Color = Color::srgba(1.0, 1.0, 1.0, 0.07);

#[derive(Component)]
pub struct WarnText;

/// Shown only while a model is open (hints).
#[derive(Component)]
pub struct WhenLoaded;

#[derive(Component)]
pub struct Welcome;

#[derive(Component)]
pub struct PauseMenu;

#[derive(Component)]
pub struct MenuTitle;

#[derive(Component)]
pub struct ErrorText;

/// The pause menu, toggled with Esc while a model is open.
#[derive(Resource, Default)]
pub struct Menu {
    pub open: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Dialog {
    Folder,
    File,
}

/// A native open dialog to show next frame, from the main thread.
#[derive(Resource, Default)]
pub struct PendingDialog(Option<Dialog>);

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Resume,
    Style(crate::theme::Style),
    OpenFolder,
    OpenFile,
    Example,
    Reload,
    Quit,
}

const BUTTON: Color = Color::srgba(1.0, 1.0, 1.0, 0.06);
const BUTTON_HOVER: Color = Color::srgba(1.0, 1.0, 1.0, 0.12);
const BUTTON_PRIMARY: Color = Color::srgb(0.27, 0.42, 0.85);
const BUTTON_PRIMARY_HOVER: Color = Color::srgb(0.33, 0.49, 0.93);

/// A button's resting and hovered background.
#[derive(Component)]
pub struct Look {
    base: Color,
    hover: Color,
}

#[derive(Resource)]
pub struct UiState {
    version: u64,
}

impl Default for UiState {
    fn default() -> Self {
        Self { version: u64::MAX }
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
            bottom: px(14),
            left: px(0),
            right: px(0),
            justify_content: JustifyContent::Center,
            ..default()
        },
        WhenLoaded,
        children![(
            Node {
                padding: UiRect::axes(px(14), px(5)),
                border_radius: BorderRadius::all(px(10)),
                ..default()
            },
            // A backing, so the hints read on light styles too.
            BackgroundColor(Color::srgba(0.02, 0.03, 0.05, 0.55)),
            Text::new(
                "scroll / pinch  zoom      drag  pan      right-drag  orbit      click  fly to      backspace / right-click  zoom out      f  fit      t  style      esc  menu"
            ),
            font(&fonts, 12.0, FontWeight::MEDIUM),
            TextColor(Color::srgba(0.85, 0.88, 0.95, 0.6)),
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

    // Welcome screen, shown until something is opened.
    commands
        .spawn((
            overlay(Color::NONE),
            Welcome,
        ))
        .with_children(|o| {
            o.spawn(card()).with_children(|c| {
                c.spawn((
                    Text::new("rustyc4"),
                    font(&fonts, 40.0, FontWeight::BOLD),
                    TextColor(TEXT),
                ));
                c.spawn((
                    Text::new("Open a LikeC4 project to explore it."),
                    font(&fonts, 15.0, FontWeight::NORMAL),
                    TextColor(MUTED),
                ));
                c.spawn(error_line(&fonts));
                c.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(10),
                    margin: UiRect::top(px(14)),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn(button(&fonts, "Open folder…", Action::OpenFolder, true));
                    row.spawn(button(&fonts, "Open file…", Action::OpenFile, false));
                });
                c.spawn((
                    Node {
                        margin: UiRect::top(px(14)),
                        ..default()
                    },
                    Text::new("or drop a .c4 file or a folder onto this window"),
                    font(&fonts, 12.5, FontWeight::NORMAL),
                    TextColor(Color::srgba(0.85, 0.88, 0.95, 0.4)),
                ));
                c.spawn(link(&fonts, "Try the example", Action::Example));
            });
        });

    // Pause menu.
    commands
        .spawn((
            overlay(Color::srgba(0.02, 0.025, 0.035, 0.62)),
            Visibility::Hidden,
            PauseMenu,
        ))
        .with_children(|o| {
            o.spawn(card()).with_children(|c| {
                c.spawn((
                    Text::new(""),
                    font(&fonts, 22.0, FontWeight::BOLD),
                    TextColor(TEXT),
                    MenuTitle,
                ));
                c.spawn(error_line(&fonts));
                c.spawn(Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(8),
                    margin: UiRect::top(px(12)),
                    width: percent(100),
                    ..default()
                })
                .with_children(|col| {
                    col.spawn(button(&fonts, "Resume", Action::Resume, true));
                    col.spawn(button(&fonts, "Open folder…", Action::OpenFolder, false));
                    col.spawn(button(&fonts, "Open file…", Action::OpenFile, false));
                    col.spawn(button(&fonts, "Reload", Action::Reload, false));
                    // Style: one button per look, side by side.
                    col.spawn(Node {
                        flex_direction: FlexDirection::Row,
                        column_gap: px(6),
                        margin: UiRect::vertical(px(4)),
                        ..default()
                    })
                    .with_children(|row| {
                        for s in crate::theme::Style::ALL {
                            row.spawn(button(&fonts, s.name(), Action::Style(s), false)).insert(StyleButton(s));
                        }
                    });
                    col.spawn(button(&fonts, "Quit", Action::Quit, false));
                });
            });
        });
}

#[allow(clippy::too_many_arguments)]
pub fn update(diagram: Res<Diagram>, mut state: ResMut<UiState>, mut warn: Single<&mut Text, With<WarnText>>) {
    if state.version != diagram.version {
        state.version = diagram.version;

        warn.0 = match diagram.warnings.as_slice() {
            [] => String::new(),
            [one] => format!("⚠  {one}"),
            [first, rest @ ..] => format!("⚠  {first}   (+{} more)", rest.len()),
        };
    }
}

fn overlay(bg: Color) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(bg),
        // Catches the pointer so the diagram underneath doesn't react.
        Interaction::default(),
        GlobalZIndex(10),
    )
}

fn card() -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(6),
            min_width: px(340),
            padding: UiRect::all(px(32)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(18)),
            ..default()
        },
        BackgroundColor(Color::srgb(0.07, 0.08, 0.105)),
        BorderColor::all(HAIRLINE),
        BoxShadow::new(
            Color::srgba(0.0, 0.0, 0.0, 0.5),
            px(0),
            px(12),
            px(0),
            px(40),
        ),
    )
}

fn error_line(fonts: &Fonts) -> impl Bundle {
    (
        Node {
            max_width: px(520),
            ..default()
        },
        Text::new(""),
        font(fonts, 13.0, FontWeight::MEDIUM),
        TextColor(Color::srgb(0.95, 0.55, 0.45)),
        ErrorText,
    )
}

fn button(fonts: &Fonts, label: &str, action: Action, primary: bool) -> impl Bundle {
    (
        Button,
        Node {
            padding: UiRect::axes(px(18), px(10)),
            justify_content: JustifyContent::Center,
            min_width: px(150),
            border_radius: BorderRadius::all(px(10)),
            ..default()
        },
        BackgroundColor(if primary { BUTTON_PRIMARY } else { BUTTON }),
        if primary {
            Look {
                base: BUTTON_PRIMARY,
                hover: BUTTON_PRIMARY_HOVER,
            }
        } else {
            Look {
                base: BUTTON,
                hover: BUTTON_HOVER,
            }
        },
        action,
        children![(
            Text::new(label),
            font(fonts, 14.5, FontWeight::SEMIBOLD),
            TextColor(TEXT),
        )],
    )
}

fn link(fonts: &Fonts, label: &str, action: Action) -> impl Bundle {
    (
        Button,
        Node {
            padding: UiRect::axes(px(8), px(4)),
            margin: UiRect::top(px(2)),
            border_radius: BorderRadius::all(px(6)),
            ..default()
        },
        BackgroundColor(Color::NONE),
        Look {
            base: Color::NONE,
            hover: Color::srgba(1.0, 1.0, 1.0, 0.05),
        },
        action,
        children![(
            Text::new(label),
            font(fonts, 13.0, FontWeight::MEDIUM),
            TextColor(ACCENT),
        )],
    )
}

/// One of the style buttons in the menu.
#[derive(Component)]
pub struct StyleButton(crate::theme::Style);

/// Show which style is in use.
pub fn style_buttons(
    theme: Res<crate::theme::Theme>,
    mut buttons: Query<(&StyleButton, &mut Look, &mut BackgroundColor)>,
    mut last: Local<Option<u64>>,
    added: Query<(), Added<StyleButton>>,
) {
    if *last == Some(theme.version) && added.is_empty() {
        return;
    }
    *last = Some(theme.version);
    for (b, mut look, mut bg) in &mut buttons {
        let on = b.0 == theme.style;
        look.base = if on { BUTTON_PRIMARY } else { BUTTON };
        look.hover = if on { BUTTON_PRIMARY_HOVER } else { BUTTON_HOVER };
        bg.0 = look.base;
    }
}

pub fn menu_buttons(
    mut buttons: Query<(&Interaction, &Action, &Look, &mut BackgroundColor), Changed<Interaction>>,
    mut menu: ResMut<Menu>,
    mut pending: ResMut<PendingDialog>,
    mut queue: ResMut<NavQueue>,
    mut theme: ResMut<crate::theme::Theme>,
    mut exit: MessageWriter<AppExit>,
) {
    for (interaction, action, look, mut bg) in &mut buttons {
        bg.0 = match interaction {
            Interaction::None => look.base,
            _ => look.hover,
        };
        if *interaction != Interaction::Pressed {
            continue;
        }
        match action {
            Action::Resume => menu.open = false,
            Action::Style(s) => theme.set(*s),
            Action::OpenFolder => pending.0 = Some(Dialog::Folder),
            Action::OpenFile => pending.0 = Some(Dialog::File),
            Action::Example => queue.0.push(Nav::Load(Source::Example)),
            Action::Reload => {
                queue.0.push(Nav::Reload);
                menu.open = false;
            }
            Action::Quit => {
                exit.write(AppExit::Success);
            }
        }
    }
}

/// Native dialogs block, and macOS wants them on the main thread.
pub fn dialogs(
    _main_thread: NonSendMarker,
    mut pending: ResMut<PendingDialog>,
    mut queue: ResMut<NavQueue>,
    mut menu: ResMut<Menu>,
) {
    let Some(kind) = pending.0.take() else { return };
    let picked = match kind {
        Dialog::Folder => rfd::FileDialog::new()
            .set_title("Open a LikeC4 project folder")
            .pick_folder(),
        Dialog::File => rfd::FileDialog::new()
            .set_title("Open a LikeC4 file")
            .add_filter("LikeC4", &["c4", "likec4"])
            .pick_file(),
    };
    if let Some(path) = picked {
        queue.0.push(Nav::Load(Source::Path(path)));
        menu.open = false;
    }
}

pub fn file_drop(
    mut drops: MessageReader<FileDragAndDrop>,
    mut queue: ResMut<NavQueue>,
    mut menu: ResMut<Menu>,
) {
    for drop in drops.read() {
        if let FileDragAndDrop::DroppedFile { path_buf, .. } = drop {
            queue.0.push(Nav::Load(Source::Path(path_buf.clone())));
            menu.open = false;
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn overlays(
    diagram: Res<Diagram>,
    menu: Res<Menu>,
    mut welcome: Single<&mut Visibility, (With<Welcome>, Without<PauseMenu>)>,
    mut pause: Single<&mut Visibility, (With<PauseMenu>, Without<Welcome>)>,
    mut loaded_only: Query<&mut Visibility, (With<WhenLoaded>, Without<Welcome>, Without<PauseMenu>)>,
    mut title: Single<&mut Text, With<MenuTitle>>,
    mut errors: Query<&mut Text, (With<ErrorText>, Without<MenuTitle>)>,
    mut window: Single<&mut Window>,
    mut last: Local<Option<(bool, bool, u64)>>,
) {
    let state = (diagram.loaded, menu.open, diagram.version);
    if *last == Some(state) {
        return;
    }
    *last = Some(state);
    let show = |v: bool| if v { Visibility::Inherited } else { Visibility::Hidden };
    **welcome = show(!diagram.loaded);
    **pause = show(diagram.loaded && menu.open);
    for mut v in &mut loaded_only {
        *v = show(diagram.loaded);
    }
    let name = match &diagram.path {
        Some(p) => p
            .canonicalize()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_else(|| p.display().to_string()),
        None if diagram.loaded => "example".to_string(),
        None => String::new(),
    };
    title.0 = name.clone();
    for mut t in &mut errors {
        t.0 = diagram.error.clone().unwrap_or_default();
    }
    window.title = if name.is_empty() {
        "rustyc4".to_string()
    } else {
        format!("rustyc4 · {name}")
    };
}
