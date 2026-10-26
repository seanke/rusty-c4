//! Visual styles: the neon Tron look, a pen-on-paper sketch, and a blocky voxel world.
//! Switch with T, from the Esc menu, or start with `--style sketch|blocky`.

use bevy::{core_pipeline::tonemapping::Tonemapping, post_process::bloom::Bloom, prelude::*};

use crate::paint::Painter;
use crate::render::{lin, FloorMaterial, FloorUniform};
use crate::scene::Diagram;

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Style {
    #[default]
    Tron,
    Sketch,
    Blocky,
}

impl Style {
    pub const ALL: [Style; 3] = [Style::Tron, Style::Sketch, Style::Blocky];

    /// Passed to the shaders.
    pub fn index(self) -> f32 {
        match self {
            Style::Tron => 0.0,
            Style::Sketch => 1.0,
            Style::Blocky => 2.0,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Style::Tron => "Tron",
            Style::Sketch => "Sketch",
            Style::Blocky => "Blocky",
        }
    }

    pub fn parse(s: &str) -> Option<Style> {
        Style::ALL.into_iter().find(|st| st.name().eq_ignore_ascii_case(s))
    }

    pub fn next(self) -> Style {
        match self {
            Style::Tron => Style::Sketch,
            Style::Sketch => Style::Blocky,
            Style::Blocky => Style::Tron,
        }
    }

    pub fn background(self) -> Color {
        match self {
            Style::Tron => Color::srgb(0.004, 0.009, 0.018),
            Style::Sketch => Color::srgb(0.95, 0.93, 0.88),
            Style::Blocky => Color::srgb(0.56, 0.74, 1.0),
        }
    }

    /// Painted text: title, subtitle and body colours.
    pub fn ink(self) -> (Color, Color, Color) {
        match self {
            Style::Tron => (
                Color::srgb(0.94, 0.98, 1.0),
                Color::srgba(0.55, 0.95, 1.0, 0.9),
                Color::srgba(0.85, 0.95, 1.0, 0.85),
            ),
            Style::Sketch => (
                Color::srgb(0.1, 0.11, 0.18),
                Color::srgb(0.2, 0.32, 0.62),
                Color::srgb(0.24, 0.24, 0.3),
            ),
            Style::Blocky => (Color::WHITE, Color::srgb(1.0, 1.0, 0.33), Color::srgb(0.82, 0.82, 0.82)),
        }
    }

    /// Relationship and port tags: text colour, backing, border.
    pub fn tag(self) -> (Color, Vec4, Vec4) {
        match self {
            Style::Tron => (
                Color::srgba(0.72, 0.9, 1.0, 1.0),
                Vec4::new(0.005, 0.02, 0.035, 0.92),
                Vec4::new(0.2, 0.9, 1.0, 1.0),
            ),
            Style::Sketch => (
                Color::srgb(0.12, 0.13, 0.2),
                Vec4::new(0.97, 0.95, 0.9, 0.96),
                Vec4::new(0.12, 0.13, 0.2, 1.0),
            ),
            Style::Blocky => (
                Color::WHITE,
                Vec4::new(0.06, 0.0, 0.06, 0.94),
                Vec4::new(0.31, 0.0, 1.0, 1.0),
            ),
        }
    }

    /// Brightness of painted text (above 1 glows under bloom).
    pub fn text_tint(self) -> f32 {
        match self {
            Style::Tron => 1.25,
            _ => 1.0,
        }
    }
}

#[derive(Resource, Default)]
pub struct Theme {
    pub style: Style,
    pub version: u64,
}

impl Theme {
    pub fn set(&mut self, style: Style) {
        if style != self.style {
            self.style = style;
            self.version += 1;
        }
    }
}

/// Restyle the scene when the theme changes: camera, floor, text painter; the world is
/// rebuilt (in place, keeping the camera) so blocks, pipes and labels pick up the style.
#[allow(clippy::too_many_arguments)]
pub fn apply(
    mut commands: Commands,
    theme: Res<Theme>,
    mut last: Local<Option<u64>>,
    camera: Single<(Entity, &mut Camera), With<Camera3d>>,
    floor: Query<&MeshMaterial3d<FloorMaterial>>,
    mut floors: ResMut<Assets<FloorMaterial>>,
    mut painter: ResMut<Painter>,
    mut diagram: ResMut<Diagram>,
    mut clear: ResMut<ClearColor>,
) {
    if *last == Some(theme.version) {
        return;
    }
    let first = last.is_none();
    *last = Some(theme.version);
    let style = theme.style;
    let (e, mut cam) = camera.into_inner();
    cam.clear_color = ClearColorConfig::Custom(style.background());
    clear.0 = style.background();
    match style {
        Style::Tron => {
            commands.entity(e).insert((
                Tonemapping::TonyMcMapface,
                Bloom {
                    intensity: 0.22,
                    ..Bloom::NATURAL
                },
            ));
        }
        _ => {
            commands.entity(e).remove::<Bloom>().insert(Tonemapping::None);
        }
    }
    for m in &floor {
        if let Some(f) = floors.get_mut(&m.0) {
            f.u = match style {
                Style::Tron => FloorUniform {
                    color: Vec4::new(0.0, 0.55, 0.85, 1.0),
                    p: Vec4::new(40.0, 5.0, 9000.0, 0.0),
                    base: lin(style.background()),
                },
                Style::Sketch => FloorUniform {
                    color: Vec4::new(0.25, 0.45, 0.85, 1.0),
                    p: Vec4::new(40.0, 5.0, 12000.0, 1.0),
                    base: lin(style.background()),
                },
                Style::Blocky => FloorUniform {
                    color: lin(style.background()),
                    p: Vec4::new(4.0, 16.0, 26000.0, 2.0),
                    base: lin(Color::srgb(0.33, 0.6, 0.2)),
                },
            };
        }
    }
    painter.style = style;
    if !first && diagram.loaded {
        diagram.restyle = true;
        diagram.resets += 1;
    }
}

/// T cycles the style.
pub fn keys(keys: Res<ButtonInput<KeyCode>>, menu: Res<crate::ui::Menu>, mut theme: ResMut<Theme>) {
    if keys.just_pressed(KeyCode::KeyT) && !menu.open {
        let next = theme.style.next();
        theme.set(next);
    }
}
