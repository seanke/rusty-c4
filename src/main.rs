//! rustyc4: a smooth, zoomable viewer for LikeC4 architecture models.
//!
//! Usage: `rustyc4 [path]` where `path` is a `.c4` / `.likec4` file or a directory of them,
//! or `rustyc4 --example` for the built-in example. With neither it opens to a welcome
//! screen. Files are watched and reloaded on save.

mod camera;
mod detail;
mod labels;
mod layout;
mod model;
mod parser;
mod pipes;
mod render;
mod scene;
mod script;
mod ui;
mod world;

use std::path::PathBuf;

use bevy::{
    asset::embedded_asset,
    pbr::MaterialPlugin,
    prelude::*,
    sprite_render::Material2dPlugin,
    window::WindowResolution,
};

use camera::Cam;
use render::{BlockMaterial, CardMaterial, FloorMaterial, PipeMaterial};
use scene::{Diagram, Nav, NavQueue, Pointer, Source, BG};

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
            MaterialPlugin::<BlockMaterial>::default(),
            MaterialPlugin::<PipeMaterial>::default(),
            MaterialPlugin::<FloorMaterial>::default(),
        ));
    embedded_asset!(app, "card.wgsl");
    embedded_asset!(app, "block.wgsl");
    embedded_asset!(app, "pipe.wgsl");
    embedded_asset!(app, "floor.wgsl");

    app.insert_resource(Diagram {
        model: Default::default(),
        warnings: Vec::new(),
        path: None,
        loaded: false,
        error: None,
        focus: None,
        detail: None,
        version: 0,
        view_version: 0,
        resets: 0,
        intro: false,
    })
    .insert_resource(NavQueue(source.map(Nav::Load).into_iter().collect()))
    .init_resource::<ui::Menu>()
    .init_resource::<detail::DetailView>()
    .init_resource::<ui::PendingDialog>()
    .init_resource::<Cam>()
    .init_resource::<Pointer>()
    .init_resource::<scene::Walk>()
    .init_resource::<ui::UiState>()
    .init_resource::<world::Geometry>()
    .init_resource::<world::View>()
    .init_resource::<world::BlockIndex>()
    .init_resource::<world::Heights>()
    .init_resource::<pipes::Links>()
    .init_resource::<pipes::PipeState>()
    .add_systems(Startup, (scene::setup, ui::spawn).chain())
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
            world::sync_view,
            world::animate,
            detail::update,
            // The camera moves before anything is projected through it this frame.
            camera::apply,
            pipes::update,
            labels::update,
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
