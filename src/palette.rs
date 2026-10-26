//! The interface's colours, in a light and a dark variant: the toolbar, menus, tooltips, the
//! steps panel, the source editor and the welcome screen. By default it follows the system's
//! appearance (and changes with it); View ▸ Appearance can pin it to light or dark.
//!
//! UI nodes say which role each of their colours plays with the `UiFill` / `UiInk` /
//! `UiEdge` / `UiTint` / `UiShadow` components, and `restyle` paints them from the current
//! palette, again whenever it changes. Hover and pressed states read `Ui::p` directly.

use bevy::{prelude::*, window::WindowThemeChanged, window::WindowTheme};

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

impl Appearance {
    pub const ALL: [Appearance; 3] = [Appearance::System, Appearance::Light, Appearance::Dark];

    pub fn name(self) -> &'static str {
        match self {
            Appearance::System => "Match System",
            Appearance::Light => "Light",
            Appearance::Dark => "Dark",
        }
    }
}

/// What a UI colour is for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Bar,
    MenuBar,
    Hairline,
    Icon,
    Text,
    Muted,
    Faint,
    Accent,
    Tooltip,
    TooltipEdge,
    Shadow,
    Panel,
    PanelEdge,
    Well,
    Card,
    Warn,
    Error,
    Fix,
    TreeDir,
    TreeFile,
    TreeActive,
    /// Text on a primary (accent-filled) button.
    OnPrimary,
}

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub bar: Color,
    pub menubar: Color,
    pub hairline: Color,
    pub hover: Color,
    pub pressed: Color,
    pub active: Color,
    pub icon: Color,
    pub text: Color,
    pub muted: Color,
    pub faint: Color,
    pub accent: Color,
    pub popup: Color,
    pub popup_edge: Color,
    pub item_hover: Color,
    pub tooltip: Color,
    pub tooltip_edge: Color,
    pub shadow: Color,
    pub panel: Color,
    pub panel_edge: Color,
    pub row_current: Color,
    pub well: Color,
    pub card: Color,
    pub button: Color,
    pub button_hover: Color,
    pub primary: Color,
    pub primary_hover: Color,
    pub link_hover: Color,
    pub warn: Color,
    pub error: Color,
    pub fix: Color,
    pub tree_dir: Color,
    pub tree_file: Color,
    pub tree_active: Color,
    pub code: Code,
}

/// The source editor's colours, as bytes for its software-painted image.
#[derive(Clone, Copy, Debug)]
pub struct Code {
    pub bg: [u8; 4],
    pub line: [u8; 4],
    pub selection: [u8; 4],
    pub caret: [u8; 4],
    pub gutter: [u8; 4],
    pub error: [u8; 4],
    pub warning: [u8; 4],
    /// The completion popup: its border, background and selected row.
    pub popup_edge: [u8; 4],
    pub popup_bg: [u8; 4],
    pub popup_sel: [u8; 4],
    /// Plain, keyword, string, comment, arrow, tag, number, brace.
    pub tokens: [[u8; 3]; 8],
}

const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Color {
    Color::srgba(r, g, b, a)
}

pub const DARK: Palette = Palette {
    bar: rgba(0.055, 0.063, 0.082, 1.0),
    menubar: rgba(0.045, 0.052, 0.068, 1.0),
    hairline: rgba(1.0, 1.0, 1.0, 0.08),
    hover: rgba(1.0, 1.0, 1.0, 0.08),
    pressed: rgba(1.0, 1.0, 1.0, 0.14),
    active: rgba(0.35, 0.6, 1.0, 0.22),
    icon: rgba(0.86, 0.9, 0.97, 0.9),
    text: rgba(0.94, 0.98, 1.0, 1.0),
    muted: rgba(0.72, 0.9, 1.0, 0.7),
    faint: rgba(0.85, 0.88, 0.95, 0.4),
    accent: rgba(0.2, 0.9, 1.0, 1.0),
    popup: rgba(0.085, 0.095, 0.12, 1.0),
    popup_edge: rgba(1.0, 1.0, 1.0, 0.12),
    item_hover: rgba(0.35, 0.6, 1.0, 0.35),
    tooltip: rgba(0.13, 0.14, 0.18, 1.0),
    tooltip_edge: rgba(1.0, 1.0, 1.0, 0.16),
    shadow: rgba(0.0, 0.0, 0.0, 0.45),
    panel: rgba(0.045, 0.052, 0.07, 1.0),
    panel_edge: rgba(1.0, 1.0, 1.0, 0.09),
    row_current: rgba(0.35, 0.6, 1.0, 0.25),
    well: rgba(0.0, 0.0, 0.0, 0.18),
    card: rgba(0.07, 0.08, 0.105, 1.0),
    button: rgba(1.0, 1.0, 1.0, 0.06),
    button_hover: rgba(1.0, 1.0, 1.0, 0.12),
    primary: rgba(0.27, 0.42, 0.85, 1.0),
    primary_hover: rgba(0.33, 0.49, 0.93, 1.0),
    link_hover: rgba(1.0, 1.0, 1.0, 0.05),
    warn: rgba(0.95, 0.72, 0.35, 1.0),
    error: rgba(0.95, 0.55, 0.45, 1.0),
    fix: rgba(0.25, 0.5, 0.9, 0.35),
    tree_dir: rgba(0.72, 0.8, 0.92, 0.6),
    tree_file: rgba(0.85, 0.9, 1.0, 0.75),
    tree_active: rgba(0.3, 0.55, 0.95, 0.28),
    code: Code {
        bg: [14, 17, 23, 255],
        line: [255, 255, 255, 10],
        selection: [70, 130, 210, 90],
        caret: [120, 200, 255, 255],
        gutter: [110, 120, 140, 255],
        error: [240, 90, 90, 255],
        warning: [235, 185, 70, 255],
        popup_edge: [90, 120, 170, 255],
        popup_bg: [24, 29, 40, 255],
        popup_sel: [60, 100, 170, 200],
        tokens: [
            [214, 222, 235],
            [199, 146, 234],
            [236, 196, 141],
            [105, 120, 120],
            [127, 219, 202],
            [173, 219, 103],
            [247, 140, 108],
            [140, 150, 170],
        ],
    },
};

pub const LIGHT: Palette = Palette {
    bar: rgba(0.953, 0.953, 0.957, 1.0),
    menubar: rgba(0.975, 0.975, 0.978, 1.0),
    hairline: rgba(0.0, 0.0, 0.0, 0.1),
    hover: rgba(0.0, 0.0, 0.0, 0.06),
    pressed: rgba(0.0, 0.0, 0.0, 0.11),
    active: rgba(0.0, 0.47, 0.83, 0.15),
    icon: rgba(0.2, 0.21, 0.24, 0.9),
    text: rgba(0.12, 0.12, 0.13, 1.0),
    muted: rgba(0.3, 0.33, 0.38, 0.85),
    faint: rgba(0.2, 0.22, 0.26, 0.5),
    accent: rgba(0.0, 0.37, 0.72, 1.0),
    popup: rgba(1.0, 1.0, 1.0, 1.0),
    popup_edge: rgba(0.0, 0.0, 0.0, 0.14),
    item_hover: rgba(0.0, 0.47, 0.83, 0.16),
    tooltip: rgba(1.0, 1.0, 1.0, 1.0),
    tooltip_edge: rgba(0.0, 0.0, 0.0, 0.15),
    shadow: rgba(0.0, 0.0, 0.0, 0.16),
    panel: rgba(0.972, 0.972, 0.976, 1.0),
    panel_edge: rgba(0.0, 0.0, 0.0, 0.1),
    row_current: rgba(0.0, 0.47, 0.83, 0.13),
    well: rgba(0.0, 0.0, 0.0, 0.035),
    card: rgba(1.0, 1.0, 1.0, 1.0),
    button: rgba(0.0, 0.0, 0.0, 0.05),
    button_hover: rgba(0.0, 0.0, 0.0, 0.09),
    primary: rgba(0.0, 0.47, 0.83, 1.0),
    primary_hover: rgba(0.0, 0.41, 0.74, 1.0),
    link_hover: rgba(0.0, 0.0, 0.0, 0.05),
    warn: rgba(0.66, 0.42, 0.0, 1.0),
    error: rgba(0.78, 0.2, 0.15, 1.0),
    fix: rgba(0.0, 0.47, 0.83, 0.16),
    tree_dir: rgba(0.25, 0.28, 0.33, 0.7),
    tree_file: rgba(0.15, 0.16, 0.19, 0.85),
    tree_active: rgba(0.0, 0.47, 0.83, 0.16),
    code: Code {
        bg: [255, 255, 255, 255],
        line: [0, 0, 0, 10],
        selection: [173, 214, 255, 170],
        caret: [0, 0, 0, 255],
        gutter: [110, 118, 129, 255],
        error: [205, 49, 49, 255],
        warning: [191, 136, 3, 255],
        popup_edge: [200, 200, 205, 255],
        popup_bg: [252, 252, 252, 255],
        popup_sel: [0, 120, 212, 60],
        tokens: [
            [31, 31, 31],
            [175, 0, 219],
            [163, 21, 21],
            [0, 128, 0],
            [0, 112, 193],
            [38, 127, 153],
            [9, 134, 88],
            [100, 100, 100],
        ],
    },
};

impl Palette {
    pub fn get(&self, role: Role) -> Color {
        match role {
            Role::Bar => self.bar,
            Role::MenuBar => self.menubar,
            Role::Hairline => self.hairline,
            Role::Icon => self.icon,
            Role::Text => self.text,
            Role::Muted => self.muted,
            Role::Faint => self.faint,
            Role::Accent => self.accent,
            Role::Tooltip => self.tooltip,
            Role::TooltipEdge => self.tooltip_edge,
            Role::Shadow => self.shadow,
            Role::Panel => self.panel,
            Role::PanelEdge => self.panel_edge,
            Role::Well => self.well,
            Role::Card => self.card,
            Role::Warn => self.warn,
            Role::Error => self.error,
            Role::Fix => self.fix,
            Role::TreeDir => self.tree_dir,
            Role::TreeFile => self.tree_file,
            Role::TreeActive => self.tree_active,
            Role::OnPrimary => Color::WHITE,
        }
    }
}

/// The interface's appearance: what was chosen, what the system is using, and the palette
/// that results.
#[derive(Resource)]
pub struct Ui {
    pub choice: Appearance,
    system_dark: bool,
    pub dark: bool,
    pub p: Palette,
}

impl Ui {
    pub fn new(choice: Appearance) -> Ui {
        let system_dark = system_is_dark().unwrap_or(true);
        let mut ui = Ui {
            choice,
            system_dark,
            dark: true,
            p: DARK,
        };
        ui.resolve();
        ui
    }

    pub fn set(&mut self, choice: Appearance) {
        if choice == Appearance::System {
            if let Some(d) = system_is_dark() {
                self.system_dark = d;
            }
        }
        self.choice = choice;
        self.resolve();
    }

    fn resolve(&mut self) {
        self.dark = match self.choice {
            Appearance::System => self.system_dark,
            Appearance::Light => false,
            Appearance::Dark => true,
        };
        self.p = if self.dark { DARK } else { LIGHT };
    }
}

/// Ask the system directly (macOS keeps it in the global defaults; absent means light).
fn system_is_dark() -> Option<bool> {
    if cfg!(target_os = "macos") {
        let out = std::process::Command::new("defaults").args(["read", "-g", "AppleInterfaceStyle"]).output().ok()?;
        Some(String::from_utf8_lossy(&out.stdout).trim().eq_ignore_ascii_case("dark"))
    } else {
        None
    }
}

/// A node's background colour, by role.
#[derive(Component, Clone, Copy)]
pub struct UiFill(pub Role);

/// A text's colour, by role, with an alpha multiplier.
#[derive(Component, Clone, Copy)]
pub struct UiInk(pub Role, pub f32);

/// A node's border colour, by role.
#[derive(Component, Clone, Copy)]
pub struct UiEdge(pub Role);

/// An image's tint, by role, with an alpha multiplier.
#[derive(Component, Clone, Copy)]
pub struct UiTint(pub Role, pub f32);

/// A node's drop shadow colour, by role.
#[derive(Component, Clone, Copy)]
pub struct UiShadow(pub Role);

/// Follow the system's appearance while "Match System" is chosen, and keep the window's
/// own title bar in step with the choice.
pub fn follow_system(mut ui: ResMut<Ui>, mut changes: MessageReader<WindowThemeChanged>, mut window: Single<&mut Window>, mut seen: Local<Option<Appearance>>) {
    for c in changes.read() {
        if ui.choice == Appearance::System {
            let dark = c.theme == WindowTheme::Dark;
            if ui.system_dark != dark {
                ui.system_dark = dark;
                ui.resolve();
            }
        }
    }
    if *seen != Some(ui.choice) {
        *seen = Some(ui.choice);
        window.window_theme = match ui.choice {
            Appearance::System => None,
            Appearance::Light => Some(WindowTheme::Light),
            Appearance::Dark => Some(WindowTheme::Dark),
        };
    }
}

/// Paint the role-tagged nodes: all of them when the palette changes, else just new ones.
#[allow(clippy::type_complexity)]
pub fn restyle(
    ui: Res<Ui>,
    mut fills: Query<(Ref<UiFill>, &mut BackgroundColor)>,
    mut inks: Query<(Ref<UiInk>, &mut TextColor)>,
    mut edges: Query<(Ref<UiEdge>, &mut BorderColor)>,
    mut tints: Query<(Ref<UiTint>, &mut ImageNode)>,
    mut shadows: Query<(Ref<UiShadow>, &mut BoxShadow)>,
) {
    let all = ui.is_changed();
    let p = &ui.p;
    for (f, mut bg) in &mut fills {
        if all || f.is_added() {
            bg.0 = p.get(f.0);
        }
    }
    for (i, mut tc) in &mut inks {
        if all || i.is_added() {
            let c = p.get(i.0);
            tc.0 = c.with_alpha(c.alpha() * i.1);
        }
    }
    for (e, mut bc) in &mut edges {
        if all || e.is_added() {
            *bc = BorderColor::all(p.get(e.0));
        }
    }
    for (t, mut img) in &mut tints {
        if all || t.is_added() {
            let c = p.get(t.0);
            img.color = c.with_alpha(c.alpha() * t.1);
        }
    }
    for (s, mut sh) in &mut shadows {
        if all || s.is_added() {
            for style in sh.0.iter_mut() {
                style.color = p.get(s.0);
            }
        }
    }
}
