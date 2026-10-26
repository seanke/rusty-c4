//! rustyc4: a smooth, zoomable viewer for LikeC4 architecture models.
//!
//! Usage: `rustyc4 [path]` where `path` is a `.c4` / `.likec4` file or a directory of them,
//! or `rustyc4 --example` for the built-in example. With neither it opens to a welcome
//! screen. Files are watched and reloaded on save.

mod camera;
mod detail;
mod editor;
mod labels;
mod layout;
mod mermaid;
mod model;
mod paint;
mod parser;
mod pipes;
mod render;
mod scene;
mod script;
mod story;
mod theme;
mod ui;
mod workspace;
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
use render::{BlockMaterial, CardMaterial, DecalMaterial, FloorMaterial, PipeMaterial};
use scene::{Diagram, Nav, NavQueue, Pointer, Source, BG};

fn main() {
    let mut source = None;
    let mut style = theme::Style::Tron;
    let mut args = std::env::args().skip(1).peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--example" => {
                // `--example mermaid` for the Mermaid one.
                let mermaid = args.peek().is_some_and(|a| a == "mermaid");
                if mermaid {
                    args.next();
                }
                source = Some(Source::Example(if mermaid { workspace::Example::Mermaid } else { workspace::Example::Cloud }));
            }
            "--style" => match args.next().as_deref().and_then(theme::Style::parse) {
                Some(s) => style = s,
                None => {
                    eprintln!("rustyc4: --style takes tron, sketch, blocky or blueprint");
                    std::process::exit(2);
                }
            },
            "-h" | "--help" => {
                println!("usage: rustyc4 [path | --example] [--style tron|sketch|blocky|blueprint]");
                println!("  path       a .c4 / .likec4 file, or a directory of them");
                println!("  --example  open the built-in example model (`--example mermaid` for the Mermaid one)");
                println!("  --style    how it looks (T switches while running)");
                return;
            }
            p => source = Some(Source::Path(PathBuf::from(p))),
        }
    }

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
            MaterialPlugin::<DecalMaterial>::default(),
        ));
    embedded_asset!(app, "card.wgsl");
    embedded_asset!(app, "block.wgsl");
    embedded_asset!(app, "pipe.wgsl");
    embedded_asset!(app, "floor.wgsl");
    embedded_asset!(app, "decal.wgsl");

    app.insert_resource(Diagram {
        model: Default::default(),
        warnings: Vec::new(),
        path: None,
        loaded: false,
        error: None,
        detail: None,
        version: 0,
        resets: 0,
        intro: false,
        keep_view: false,
    })
    .insert_resource(NavQueue(source.map(Nav::Load).into_iter().collect()))
    .init_resource::<ui::Menu>()
    .init_resource::<detail::DetailView>()
    .init_resource::<ui::PendingDialog>()
    .init_resource::<Cam>()
    .init_resource::<Pointer>()
    .init_resource::<ui::UiState>()
    .init_resource::<world::Geometry>()
    .init_resource::<world::View>()
    .init_resource::<world::BlockIndex>()
    .init_resource::<world::Heights>()
    .init_resource::<pipes::Links>()
    .init_resource::<pipes::PipeState>()
    .init_resource::<story::Playback>()
    .init_resource::<workspace::Workspace>()
    .insert_resource(theme::Theme { style, version: 0 })
    .add_systems(Startup, (scene::setup, labels::setup, pipes::setup, ui::spawn, story::setup, story::spawn_ui, editor::setup).chain())
    .add_systems(
        Update,
        (
            (
                script::run,
                // The editor takes the keys first when it has focus.
                editor::input,
                camera::input,
                scene::pointer,
                scene::keyboard,
                theme::keys,
                story::ui,
                scene::watch_files,
                workspace::live,
                scene::apply_nav,
                theme::apply,
                world::auto_open,
                world::sync_view,
                // The story overrides hover emphasis while a dynamic view plays.
                story::drive,
                world::animate,
                world::follow_surface,
                detail::update,
            )
                .chain(),
            (
                // The camera moves before anything is projected through it this frame.
                camera::apply,
                pipes::update,
                story::draw,
                labels::update,
                ui::update,
                ui::menu_buttons,
                ui::style_buttons,
                editor::draw,
                ui::file_drop,
                ui::dialogs,
                ui::overlays,
            )
                .chain(),
        )
            .chain(),
    )
    ;
    if let Some(s) = script::Script::from_env() {
        app.insert_resource(s);
    }
    app.run();
}
