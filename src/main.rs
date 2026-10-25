//! rustyc4: a smooth, zoomable viewer for LikeC4 architecture models.
//!
//! Usage: `rustyc4 [path]` where `path` is a `.c4` / `.likec4` file or a directory of them,
//! or `rustyc4 --example` for the built-in example. With neither it opens to a welcome
//! screen. Files are watched and reloaded on save.

mod camera;
mod detail;
mod layout;
mod model;
mod parser;
mod render;
mod scene;
mod script;
mod ui;

use std::path::PathBuf;

use bevy::{
    asset::embedded_asset,
    prelude::*,
    sprite_render::Material2dPlugin,
    window::WindowResolution,
};

use camera::{Background, Cam};
use render::{lin, CardMaterial, GridMaterial, GridUniform};
use scene::{Diagram, ElemIndex, Nav, NavQueue, Pointer, Source, TextRes, BG};

fn main() {
    let source = match std::env::args().nth(1).as_deref() {
        Some("--example") => Some(Source::Example),
        Some("-h" | "--help") => {
            println!("usage: rustyc4 [path | --example]");
            println!("  path       a .c4 / .likec4 file, or a directory of them");
            println!("  --example  open the built-in example model");
            return;
        }
        Some(p) => Some(Source::Path(PathBuf::from(p))),
        None => None,
    };

    let mut app = App::new();
    app.insert_resource(ClearColor(BG))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "rustyc4".into(),
                resolution: WindowResolution::new(1440, 900),
                ..default()
            }),
            ..default()
        }))
        .add_plugins((
            Material2dPlugin::<CardMaterial>::default(),
            Material2dPlugin::<GridMaterial>::default(),
        ));
    embedded_asset!(app, "card.wgsl");
    embedded_asset!(app, "grid.wgsl");

    app.insert_resource(Diagram {
        model: Default::default(),
        warnings: Vec::new(),
        path: None,
        loaded: false,
        error: None,
        focus: None,
        detail: None,
        layout: Default::default(),
        version: 0,
    })
    .insert_resource(NavQueue(source.map(Nav::Load).into_iter().collect()))
    .init_resource::<ui::Menu>()
    .init_resource::<detail::DetailView>()
    .init_resource::<ui::PendingDialog>()
    .init_resource::<Cam>()
    .init_resource::<Pointer>()
    .init_resource::<ElemIndex>()
    .init_resource::<scene::Walk>()
    .init_resource::<ui::UiState>()
    .insert_resource(TextRes { k: 1.0 })
    .add_systems(Startup, (setup_camera, scene::setup, ui::spawn).chain())
    .add_systems(
        Update,
        (
            script::run,
            camera::input,
            scene::pointer,
            scene::keyboard,
            ui::crumb_clicks,
            scene::watch_files,
            scene::apply_nav,
            detail::update,
            // The camera moves before anything is projected through it this frame.
            camera::apply,
            scene::text_scale,
            scene::animate,
            scene::draw_edges,
            ui::update,
            ui::menu_buttons,
            ui::file_drop,
            ui::dialogs,
            ui::overlays,
        )
            .chain(),
    )
    ;
    if let Some(s) = script::Script::from_env() {
        app.insert_resource(s);
    }
    app.run();
}

fn setup_camera(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut grids: ResMut<Assets<GridMaterial>>,
) {
    commands.spawn((Camera2d, Msaa::Sample4));
    commands.spawn((
        Mesh2d(meshes.add(Rectangle::new(1.0, 1.0))),
        MeshMaterial2d(grids.add(GridMaterial {
            u: GridUniform {
                base: lin(BG),
                center: lin(Color::srgb(0.075, 0.087, 0.112)),
                dot: lin(Color::srgba(0.75, 0.8, 0.95, 0.16)),
                params: Vec4::new(32.0, 1.1, 0.0, 0.0),
            },
        })),
        Transform::from_xyz(0.0, 0.0, -100.0),
        Background,
    ));
}
