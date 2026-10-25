//! Tiny scripted driver for headless checks and recording demos, enabled with
//! `RUSTYC4_SCRIPT="1.0 shot a.png; 1.2 focus shop; 2.5 shot b.png; 3 exit"`.
//! Each step is `<seconds> <action> [arg]`; actions: shot, focus, up, select, relation
//! (two fqns), hover, unhover, pan (dx dy), orbit (dx dy), menu, exit.

use bevy::{
    prelude::*,
    render::view::screenshot::{save_to_disk, Screenshot},
};

use crate::camera::Cam;
use crate::scene::{Diagram, Nav, NavQueue};
use crate::world::{BlockIndex, Geometry, Heights};

#[derive(Resource)]
pub struct Script {
    steps: Vec<(f32, String, String)>,
    next: usize,
}

impl Script {
    pub fn from_env() -> Option<Script> {
        let raw = std::env::var("RUSTYC4_SCRIPT").ok()?;
        let steps = raw
            .split(';')
            .filter_map(|s| {
                let mut it = s.split_whitespace();
                let t = it.next()?.parse().ok()?;
                let action = it.next()?.to_string();
                let arg = it.collect::<Vec<_>>().join(" ");
                Some((t, action, arg))
            })
            .collect();
        Some(Script { steps, next: 0 })
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    mut commands: Commands,
    time: Res<Time>,
    script: Option<ResMut<Script>>,
    diagram: Res<Diagram>,
    index: Res<BlockIndex>,
    geometry: Res<Geometry>,
    heights: Res<Heights>,
    mut cam: ResMut<Cam>,
    mut queue: ResMut<NavQueue>,
    mut menu: ResMut<crate::ui::Menu>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(mut script) = script else { return };
    let now = time.elapsed_secs();
    while let Some((t, action, arg)) = script.steps.get(script.next).cloned() {
        if now < t {
            break;
        }
        script.next += 1;
        let id = diagram.model.by_fqn.get(&arg).copied();
        let words: Vec<&str> = arg.split_whitespace().collect();
        let num = |i: usize| words.get(i).and_then(|w| w.parse::<f32>().ok()).unwrap_or(0.0);
        match action.as_str() {
            "shot" => {
                commands
                    .spawn(Screenshot::primary_window())
                    .observe(save_to_disk(arg));
            }
            "focus" => queue.0.push(Nav::Focus(id)),
            "up" => queue.0.push(Nav::Up),
            "select" => queue.0.push(Nav::Detail(id.map(crate::scene::Detail::Element))),
            "relation" => {
                let ids: Vec<_> = arg.split_whitespace().filter_map(|f| diagram.model.by_fqn.get(f)).collect();
                if let [a, b] = ids[..] {
                    queue.0.push(Nav::Detail(Some(crate::scene::Detail::Relation(*a, *b))));
                }
            }
            "hover" => {
                // Move the virtual cursor over the middle of an element's top.
                let at = id.filter(|i| index.0.contains_key(i)).and_then(|i| {
                    let g = geometry.geos.get(&i)?;
                    let top = heights.base.get(&i).copied().unwrap_or(0.0) + heights.height.get(&i).copied().unwrap_or(0.0);
                    cam.world_to_screen(Vec3::new(g.center.x, top, g.center.y))
                });
                cam.fake_cursor = at;
            }
            "unhover" => cam.fake_cursor = None,
            "pan" => {
                let c = cam.viewport / 2.0;
                cam.drag(c, c + Vec2::new(num(0), num(1)));
            }
            "orbit" => cam.orbit(Vec2::new(num(0), num(1))),
            "menu" => menu.open = !menu.open,
            "exit" => {
                exit.write(AppExit::Success);
            }
            _ => {}
        }
    }
}
