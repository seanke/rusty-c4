//! Screen-space overlay: warnings, the welcome screen, file dialogs and drag-and-drop.

use bevy::{ecs::system::NonSendMarker, prelude::*, window::FileDragAndDrop};

use crate::actions::{Actions, AppAction};
use crate::chrome::Chrome;
use crate::palette::{Role, Ui, UiEdge, UiFill, UiInk, UiShadow};
use crate::scene::{Diagram, Fonts, Nav, NavQueue, Source};
use crate::workspace::Example;

#[derive(Component)]
pub struct WarnText;

/// Shown only while a model is open.
#[derive(Component)]
pub struct WhenLoaded;

#[derive(Component)]
pub struct Welcome;

#[derive(Component)]
pub struct ErrorText;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Dialog {
    Folder,
    File,
}

/// A native open dialog to show next frame, from the main thread.
#[derive(Resource, Default)]
pub struct PendingDialog(Option<Dialog>);

impl PendingDialog {
    pub fn set(&mut self, d: Dialog) {
        self.0 = Some(d);
    }
}

/// What a welcome-screen button does.
#[derive(Component, Clone, Copy, PartialEq)]
pub struct Action(AppAction);

/// What kind of welcome-screen button this is, for its colours.
#[derive(Component, Clone, Copy, PartialEq)]
pub enum Look {
    Primary,
    Plain,
    Link,
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

pub fn spawn(mut commands: Commands, fonts: Res<Fonts>, chrome: Res<Chrome>) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            bottom: px(16),
            left: px(18),
            max_width: px(560),
            ..default()
        },
        Text::new(""),
        font(&fonts, 12.5, FontWeight::MEDIUM),
        TextColor::default(),
        UiInk(Role::Warn, 1.0),
        WarnText,
    ));

    // Welcome screen, shown until something is opened.
    commands
        .spawn((
            overlay(Color::NONE, chrome.top),
            Welcome,
        ))
        .with_children(|o| {
            o.spawn(card()).with_children(|c| {
                c.spawn((
                    Text::new("rustyc4"),
                    font(&fonts, 40.0, FontWeight::BOLD),
                    TextColor::default(),
                    UiInk(Role::Text, 1.0),
                ));
                c.spawn((
                    Text::new("Open a LikeC4 project to explore it."),
                    font(&fonts, 15.0, FontWeight::NORMAL),
                    TextColor::default(),
                    UiInk(Role::Muted, 1.0),
                ));
                c.spawn(error_line(&fonts));
                c.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(10),
                    margin: UiRect::top(px(14)),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn(button(&fonts, "Open folder…", Action(AppAction::OpenFolder), true));
                    row.spawn(button(&fonts, "Open file…", Action(AppAction::OpenFile), false));
                });
                c.spawn((
                    Node {
                        margin: UiRect::top(px(14)),
                        ..default()
                    },
                    Text::new("or drop a .c4 or .mmd file, or a folder, onto this window"),
                    font(&fonts, 12.5, FontWeight::NORMAL),
                    TextColor::default(),
                    UiInk(Role::Faint, 1.0),
                ));
                c.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(4),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn(link(&fonts, "Try the example", Action(AppAction::OpenExample(Example::Cloud))));
                    row.spawn(link(&fonts, "Try the Mermaid example", Action(AppAction::OpenExample(Example::Mermaid))));
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

fn overlay(bg: Color, top: f32) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            top: px(top),
            left: px(0),
            right: px(0),
            bottom: px(0),
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
        BackgroundColor::default(),
        BorderColor::default(),
        UiFill(Role::Card),
        UiEdge(Role::Hairline),
        UiShadow(Role::Shadow),
        BoxShadow::new(
            Color::NONE,
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
        TextColor::default(),
        UiInk(Role::Error, 1.0),
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
        BackgroundColor::default(),
        if primary { Look::Primary } else { Look::Plain },
        action,
        children![(
            Text::new(label),
            font(fonts, 14.5, FontWeight::SEMIBOLD),
            TextColor(Color::WHITE),
            UiInk(if primary { Role::OnPrimary } else { Role::Text }, 1.0),
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
        Look::Link,
        action,
        children![(
            Text::new(label),
            font(fonts, 13.0, FontWeight::MEDIUM),
            TextColor::default(),
            UiInk(Role::Accent, 1.0),
        )],
    )
}

pub fn welcome_buttons(
    mut buttons: Query<(Ref<Interaction>, &Action, &Look, &mut BackgroundColor)>,
    mut actions: ResMut<Actions>,
    ui: Res<Ui>,
) {
    let p = &ui.p;
    for (interaction, action, look, mut bg) in &mut buttons {
        let hot = *interaction != Interaction::None;
        let want = match look {
            Look::Primary => if hot { p.primary_hover } else { p.primary },
            Look::Plain => if hot { p.button_hover } else { p.button },
            Look::Link => if hot { p.link_hover } else { Color::NONE },
        };
        if bg.0 != want {
            bg.0 = want;
        }
        if interaction.is_changed() && *interaction == Interaction::Pressed {
            actions.0.push(action.0);
        }
    }
}

/// Native dialogs block, and macOS wants them on the main thread.
pub fn dialogs(
    _main_thread: NonSendMarker,
    mut pending: ResMut<PendingDialog>,
    mut queue: ResMut<NavQueue>,
) {
    let Some(kind) = pending.0.take() else { return };
    let picked = match kind {
        Dialog::Folder => rfd::FileDialog::new()
            .set_title("Open a LikeC4 project folder")
            .pick_folder(),
        Dialog::File => rfd::FileDialog::new()
            .set_title("Open a LikeC4 file")
            .add_filter("LikeC4 or Mermaid", &["c4", "likec4", "mmd", "mermaid"])
            .pick_file(),
    };
    if let Some(path) = picked {
        queue.0.push(Nav::Load(Source::Path(path)));
    }
}

pub fn file_drop(mut drops: MessageReader<FileDragAndDrop>, mut queue: ResMut<NavQueue>) {
    for drop in drops.read() {
        if let FileDragAndDrop::DroppedFile { path_buf, .. } = drop {
            queue.0.push(Nav::Load(Source::Path(path_buf.clone())));
        }
    }
}

pub fn overlays(
    diagram: Res<Diagram>,
    mut welcome: Single<&mut Visibility, With<Welcome>>,
    mut loaded_only: Query<&mut Visibility, (With<WhenLoaded>, Without<Welcome>)>,
    mut errors: Query<&mut Text, With<ErrorText>>,
    mut window: Single<&mut Window>,
    mut last: Local<Option<(bool, u64)>>,
) {
    let state = (diagram.loaded, diagram.version);
    if *last == Some(state) {
        return;
    }
    *last = Some(state);
    let show = |v: bool| if v { Visibility::Inherited } else { Visibility::Hidden };
    **welcome = show(!diagram.loaded);
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
    for mut t in &mut errors {
        t.0 = diagram.error.clone().unwrap_or_default();
    }
    window.title = if name.is_empty() {
        "rustyc4".to_string()
    } else {
        format!("rustyc4 · {name}")
    };
}
