//! Everything the toolbar, the menus and the keyboard shortcuts can do, in one place, so
//! they all stay in step: what each action is called, its shortcut, and running it.

use bevy::prelude::*;

use crate::camera::Cam;
use crate::palette::{Appearance, Ui};
use crate::editor::Code;
use crate::scene::{Diagram, Nav, NavQueue, Source};
use crate::story::Playback;
use crate::theme::{Style, Theme};
use crate::ui::{Dialog, PendingDialog};
use crate::workspace::Example;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum AppAction {
    OpenFolder,
    OpenFile,
    OpenExample(Example),
    Save,
    Reload,
    ToggleSource,
    ZoomIn,
    ZoomOut,
    Fit,
    Style(Style),
    NextStyle,
    /// Light or dark interface, or follow the system.
    Appearance(Appearance),
    /// Show a dynamic view (by index), or none.
    Sequence(Option<usize>),
    PrevStep,
    NextStep,
    Quit,
}

impl AppAction {
    pub fn label(self, diagram: &Diagram) -> String {
        match self {
            AppAction::OpenFolder => "Open Folder…".into(),
            AppAction::OpenFile => "Open File…".into(),
            AppAction::OpenExample(Example::Cloud) => "Open C4 Example".into(),
            AppAction::OpenExample(Example::Mermaid) => "Open Mermaid Example".into(),
            AppAction::Save => "Save".into(),
            AppAction::Reload => "Reload from Disk".into(),
            AppAction::ToggleSource => "Toggle Source Panel".into(),
            AppAction::ZoomIn => "Zoom In".into(),
            AppAction::ZoomOut => "Zoom Out".into(),
            AppAction::Fit => "Fit View".into(),
            AppAction::Style(s) => s.name().into(),
            AppAction::NextStyle => "Next Style".into(),
            AppAction::Appearance(a) => a.name().into(),
            AppAction::Sequence(None) => "No Sequence".into(),
            AppAction::Sequence(Some(i)) => diagram.model.views.get(i).map_or_else(|| "Sequence".into(), |v| v.title.clone()),
            AppAction::PrevStep => "Previous Step".into(),
            AppAction::NextStep => "Next Step".into(),
            AppAction::Quit => "Quit".into(),
        }
    }

    /// The shortcut, as shown in tooltips and menus.
    pub fn shortcut(self) -> Option<&'static str> {
        let mac = cfg!(target_os = "macos");
        Some(match self {
            AppAction::OpenFolder => if mac { "⌘O" } else { "Ctrl+O" },
            AppAction::OpenFile => if mac { "⇧⌘O" } else { "Ctrl+Shift+O" },
            AppAction::Save => if mac { "⌘S" } else { "Ctrl+S" },
            AppAction::Reload => if mac { "⌘R" } else { "Ctrl+R" },
            AppAction::ToggleSource => if mac { "⌘E" } else { "Ctrl+E" },
            AppAction::ZoomIn => "+",
            AppAction::ZoomOut => "−",
            AppAction::Fit => "F",
            AppAction::NextStyle => "T",
            AppAction::PrevStep => "←",
            AppAction::NextStep => "→",
            AppAction::Quit => if mac { "⌘Q" } else { "Ctrl+Q" },
            _ => return None,
        })
    }
}

/// The menus: the same entries in the native macOS menu bar and the in-window one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MenuKind {
    File,
    View,
    Sequences,
    Style,
}

impl MenuKind {
    pub fn title(self) -> &'static str {
        match self {
            MenuKind::File => "File",
            MenuKind::View => "View",
            MenuKind::Sequences => "Sequences",
            MenuKind::Style => "Style",
        }
    }
}

pub enum Entry {
    Item { action: AppAction, checked: Option<bool>, enabled: bool },
    Separator,
    Heading(&'static str),
}

fn item(action: AppAction) -> Entry {
    Entry::Item {
        action,
        checked: None,
        enabled: true,
    }
}

pub fn entries(kind: MenuKind, diagram: &Diagram, theme: &Theme, ui: &Ui, playback: &Playback, native: bool) -> Vec<Entry> {
    let loaded = diagram.loaded;
    let styles = || {
        Style::ALL.into_iter().map(|s| Entry::Item {
            action: AppAction::Style(s),
            checked: Some(theme.style == s),
            enabled: true,
        })
    };
    match kind {
        MenuKind::File => {
            let mut out = vec![
                item(AppAction::OpenFolder),
                item(AppAction::OpenFile),
                Entry::Separator,
                item(AppAction::OpenExample(Example::Cloud)),
                item(AppAction::OpenExample(Example::Mermaid)),
                Entry::Separator,
                Entry::Item {
                    action: AppAction::Save,
                    checked: None,
                    enabled: loaded,
                },
                Entry::Item {
                    action: AppAction::Reload,
                    checked: None,
                    enabled: diagram.path.is_some(),
                },
            ];
            // On macOS, Quit lives in the app menu.
            if !native {
                out.push(Entry::Separator);
                out.push(item(AppAction::Quit));
            }
            out
        }
        MenuKind::View => {
            let mut out = vec![
                item(AppAction::ToggleSource),
                Entry::Separator,
                item(AppAction::ZoomIn),
                item(AppAction::ZoomOut),
                item(AppAction::Fit),
                Entry::Separator,
                Entry::Heading("Style"),
            ];
            out.extend(styles());
            out.push(item(AppAction::NextStyle));
            out.push(Entry::Separator);
            out.push(Entry::Heading("Appearance"));
            out.extend(Appearance::ALL.into_iter().map(|a| Entry::Item {
                action: AppAction::Appearance(a),
                checked: Some(ui.choice == a),
                enabled: true,
            }));
            out
        }
        MenuKind::Style => styles().collect(),
        MenuKind::Sequences => {
            let mut out = vec![Entry::Item {
                action: AppAction::Sequence(None),
                checked: Some(playback.active.is_none()),
                enabled: true,
            }];
            if !diagram.model.views.is_empty() {
                out.push(Entry::Separator);
            }
            for i in 0..diagram.model.views.len() {
                out.push(Entry::Item {
                    action: AppAction::Sequence(Some(i)),
                    checked: Some(playback.active == Some(i)),
                    enabled: true,
                });
            }
            out.push(Entry::Separator);
            let on = playback.active.is_some();
            out.push(Entry::Item {
                action: AppAction::PrevStep,
                checked: None,
                enabled: on,
            });
            out.push(Entry::Item {
                action: AppAction::NextStep,
                checked: None,
                enabled: on,
            });
            out
        }
    }
}

/// Actions waiting to run this frame.
#[derive(Resource, Default)]
pub struct Actions(pub Vec<AppAction>);

#[allow(clippy::too_many_arguments)]
pub fn run(
    mut actions: ResMut<Actions>,
    mut pending: ResMut<PendingDialog>,
    mut queue: ResMut<NavQueue>,
    mut code: ResMut<Code>,
    mut theme: ResMut<Theme>,
    mut playback: ResMut<Playback>,
    mut cam: ResMut<Cam>,
    mut ui: ResMut<Ui>,
    diagram: Res<Diagram>,
    mut exit: MessageWriter<AppExit>,
) {
    for a in std::mem::take(&mut actions.0) {
        match a {
            AppAction::OpenFolder => pending.set(Dialog::Folder),
            AppAction::OpenFile => pending.set(Dialog::File),
            AppAction::OpenExample(e) => queue.0.push(Nav::Load(Source::Example(e))),
            AppAction::Save => code.request_save(),
            AppAction::Reload => queue.0.push(Nav::Reload),
            AppAction::ToggleSource => {
                if diagram.loaded {
                    code.toggle();
                }
            }
            AppAction::ZoomIn | AppAction::ZoomOut => {
                let c = cam.viewport / 2.0;
                cam.zoom_at(c, if a == AppAction::ZoomIn { 0.75 } else { 1.33 });
            }
            AppAction::Fit => queue.0.push(Nav::Fit),
            AppAction::Style(s) => theme.set(s),
            AppAction::NextStyle => {
                let next = theme.style.next();
                theme.set(next);
            }
            AppAction::Appearance(a) => ui.set(a),
            AppAction::Sequence(Some(i)) => playback.start(i),
            AppAction::Sequence(None) => playback.stop(),
            AppAction::PrevStep => playback.prev(),
            AppAction::NextStep => {
                let stages = playback.active.and_then(|i| diagram.model.views.get(i)).map_or(0, |v| v.stages().len());
                playback.next(stages);
            }
            AppAction::Quit => {
                exit.write(AppExit::Success);
            }
        }
    }
}

/// Keyboard shortcuts with ⌘ / Ctrl. (On macOS the native menu handles these itself.)
pub fn shortcuts(keys: Res<ButtonInput<KeyCode>>, mut actions: ResMut<Actions>, native: Res<crate::chrome::Chrome>) {
    if native.native_menu {
        return;
    }
    let cmd = keys.any_pressed([KeyCode::SuperLeft, KeyCode::SuperRight, KeyCode::ControlLeft, KeyCode::ControlRight]);
    if !cmd {
        return;
    }
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let pressed = |k| keys.just_pressed(k);
    let a = if pressed(KeyCode::KeyO) && shift {
        AppAction::OpenFile
    } else if pressed(KeyCode::KeyO) {
        AppAction::OpenFolder
    } else if pressed(KeyCode::KeyS) {
        AppAction::Save
    } else if pressed(KeyCode::KeyR) {
        AppAction::Reload
    } else if pressed(KeyCode::KeyE) {
        AppAction::ToggleSource
    } else if pressed(KeyCode::KeyQ) {
        AppAction::Quit
    } else {
        return;
    };
    actions.0.push(a);
}
