//! Visual styles: the neon Tron look, a pen-on-paper sketch, a blocky voxel world of islands
//! in the sea, an architect's blueprint, a plain dark theme like a code editor's, and a toy
//! city of buildings joined by roads with cars on them. Switch with T, from the toolbar or
//! View menu, or start with `--style sketch|blocky|blueprint|dark|city`.

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
    Blueprint,
    Dark,
    City,
}

impl Style {
    pub const ALL: [Style; 6] = [Style::Tron, Style::Sketch, Style::Blocky, Style::Blueprint, Style::Dark, Style::City];

    /// Passed to the shaders.
    pub fn index(self) -> f32 {
        match self {
            Style::Tron => 0.0,
            Style::Sketch => 1.0,
            Style::Blocky => 2.0,
            Style::Blueprint => 3.0,
            Style::Dark => 4.0,
            Style::City => 5.0,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Style::Tron => "Tron",
            Style::Sketch => "Sketch",
            Style::Blocky => "Blocky",
            Style::Blueprint => "Blueprint",
            Style::Dark => "Dark",
            Style::City => "City",
        }
    }

    pub fn parse(s: &str) -> Option<Style> {
        Style::ALL.into_iter().find(|st| st.name().eq_ignore_ascii_case(s))
    }

    pub fn next(self) -> Style {
        match self {
            Style::Tron => Style::Sketch,
            Style::Sketch => Style::Blocky,
            Style::Blocky => Style::Blueprint,
            Style::Blueprint => Style::Dark,
            Style::Dark => Style::City,
            Style::City => Style::Tron,
        }
    }

    pub fn background(self) -> Color {
        match self {
            Style::Tron => Color::srgb(0.004, 0.009, 0.018),
            Style::Sketch => Color::srgb(0.95, 0.93, 0.88),
            Style::Blocky => Color::srgb(0.56, 0.74, 1.0),
            Style::Blueprint => Color::srgb(0.22, 0.44, 0.72),
            Style::Dark => Color::srgb(0.118, 0.118, 0.118),
            Style::City => Color::srgb(0.62, 0.78, 0.93),
        }
    }

    /// The blueprint style's blocks: a deep navy, so they stand clear of the lighter paper.
    pub const BLUEPRINT_BLOCK: Color = Color::srgb(0.07, 0.2, 0.42);

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
            Style::Blueprint => (
                Color::srgb(0.96, 0.98, 1.0),
                Color::srgba(0.72, 0.86, 1.0, 0.95),
                Color::srgba(0.85, 0.92, 1.0, 0.85),
            ),
            // Dark lettering on pale rooftops.
            Style::City => (
                Color::srgb(0.12, 0.13, 0.15),
                Color::srgb(0.72, 0.3, 0.06),
                Color::srgb(0.28, 0.29, 0.32),
            ),
            // Editor text, keyword blue, and comment grey.
            Style::Dark => (
                Color::srgb(0.9, 0.9, 0.9),
                Color::srgb(0.34, 0.61, 0.84),
                Color::srgb(0.62, 0.62, 0.62),
            ),
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
            // Like a floating name tag: white on translucent black.
            Style::Blocky => (
                Color::WHITE,
                Vec4::new(0.0, 0.0, 0.0, 0.55),
                Vec4::new(0.0, 0.0, 0.0, 0.55),
            ),
            Style::Blueprint => (
                Color::srgb(0.96, 0.98, 1.0),
                Vec4::new(0.012, 0.06, 0.23, 0.96),
                Vec4::new(0.85, 0.92, 1.0, 1.0),
            ),
            // Like a green road sign.
            Style::City => (
                Color::WHITE,
                Vec4::new(0.0, 0.2, 0.08, 0.97),
                Vec4::new(0.9, 0.95, 0.9, 1.0),
            ),
            Style::Dark => (
                Color::srgb(0.8, 0.8, 0.8),
                Vec4::new(0.0185, 0.0185, 0.0194, 0.97),
                Vec4::new(0.045, 0.045, 0.045, 1.0),
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
                    ..f.u
                },
                Style::Sketch => FloorUniform {
                    color: Vec4::new(0.25, 0.45, 0.85, 1.0),
                    p: Vec4::new(40.0, 5.0, 12000.0, 1.0),
                    base: lin(style.background()),
                    ..f.u
                },
                Style::Blocky => FloorUniform {
                    color: lin(style.background()),
                    p: Vec4::new(4.0, 16.0, 26000.0, 2.0),
                    base: lin(Color::srgb(0.16, 0.3, 0.74)),
                    ..f.u
                },
                Style::Blueprint => FloorUniform {
                    color: Vec4::new(0.85, 0.92, 1.0, 1.0),
                    p: Vec4::new(20.0, 5.0, 14000.0, 3.0),
                    base: lin(style.background()),
                    ..f.u
                },
                Style::City => FloorUniform {
                    color: lin(style.background()),
                    p: Vec4::new(6.0, 8.0, 26000.0, 5.0),
                    base: lin(Color::srgb(0.42, 0.62, 0.3)),
                    ..f.u
                },
                Style::Dark => FloorUniform {
                    color: lin(Color::srgb(0.2, 0.2, 0.2)),
                    p: Vec4::new(40.0, 5.0, 14000.0, 4.0),
                    base: lin(style.background()),
                    ..f.u
                },
            };
        }
    }
    painter.style = style;
    if !first && diagram.loaded {
        diagram.keep_view = true;
        diagram.resets += 1;
    }
}

/// T cycles the style.
pub fn keys(
    keys: Res<ButtonInput<KeyCode>>,
    code: Res<crate::editor::Code>,
    mut theme: ResMut<Theme>,
) {
    if keys.just_pressed(KeyCode::KeyT) && !code.captures_keys() {
        let next = theme.style.next();
        theme.set(next);
    }
}
