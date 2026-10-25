//! Tiny scripted driver for headless checks and recording demos, enabled with
//! `RUSTYC4_SCRIPT="1.0 shot a.png; 1.2 focus shop; 2.5 shot b.png; 3 exit"`.
//! Each step is `<seconds> <action> [arg]`; actions: shot, focus, up, select, relation
//! (two fqns), hover, grab (fqn dx dy), release (fqn), pan (dx dy), menu, exit.

use bevy::{
    prelude::*,
    render::view::screenshot::{save_to_disk, Screenshot},
};

use crate::camera::Cam;
use crate::scene::{Diagram, ElemIndex, Elem, Nav, NavQueue};

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

pub fn run(
    mut commands: Commands,
    time: Res<Time>,
    script: Option<ResMut<Script>>,
    diagram: Res<Diagram>,
    index: Res<ElemIndex>,
    mut elems: Query<&mut Elem>,
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
                // Move the virtual cursor over an element's card.
                if let Some(e) = index.0.get(&arg).and_then(|e| elems.get(*e).ok()) {
                    let (world, _) = cam.project(e.pos.value, e.depth.value);
                    let s = cam.log_scale.target.exp();
                    let off = (world - cam.center.target) / s;
                    cam.fake_cursor = Some(cam.viewport / 2.0 + off * Vec2::new(1.0, -1.0));
                }
            }
            "grab" => {
                if let Some(mut e) = words.first().and_then(|f| index.0.get(*f)).and_then(|e| elems.get_mut(*e).ok()) {
                    e.held = true;
                    e.grab.target = Vec2::new(num(1), num(2));
                }
            }
            "release" => {
                if let Some(mut e) = index.0.get(&arg).and_then(|e| elems.get_mut(*e).ok()) {
                    e.held = false;
                    e.grab.target = Vec2::ZERO;
                }
            }
            "pan" => cam.pan_screen(Vec2::new(num(0), num(1))),
            "menu" => menu.open = !menu.open,
            "exit" => {
                exit.write(AppExit::Success);
            }
            _ => {}
        }
    }
}
