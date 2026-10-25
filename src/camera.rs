//! Smoothed 2D camera: spring-driven pan/zoom, zoom-to-cursor, trackpad pan and pinch,
//! drag panning with momentum.

use bevy::{
    input::{
        gestures::PinchGesture,
        mouse::{AccumulatedMouseScroll, MouseScrollUnit},
    },
    prelude::*,
    window::PrimaryWindow,
};

use crate::render::{GridMaterial, Spring};

pub const MIN_SCALE: f32 = 0.06;
pub const MAX_SCALE: f32 = 12.0;

#[derive(Resource)]
pub struct Cam {
    pub center: Spring<Vec2>,
    pub log_scale: Spring<f32>,
    /// Momentum after a drag, in world units per second.
    pub fling: Vec2,
    /// Logical window size.
    pub viewport: Vec2,
    pub cursor: Option<Vec2>,
    /// Cursor position injected by the script driver; overrides the real one.
    pub fake_cursor: Option<Vec2>,
    pub over_ui: bool,
    /// Where deeper layers recede towards, relative to the view centre. It travels with the
    /// camera, so panning gives parallax: far layers slide less than near ones.
    pub vanish: Spring<Vec2>,
}

impl Default for Cam {
    fn default() -> Self {
        Self {
            center: Spring::new(Vec2::ZERO),
            log_scale: Spring::new(0.0),
            fling: Vec2::ZERO,
            viewport: Vec2::new(1440.0, 900.0),
            cursor: None,
            fake_cursor: None,
            over_ui: false,
            vanish: Spring::new(Vec2::ZERO),
        }
    }
}

impl Cam {
    pub fn scale(&self) -> f32 {
        self.log_scale.value.exp()
    }

    fn offset(&self, screen: Vec2) -> Vec2 {
        (screen - self.viewport / 2.0) * Vec2::new(1.0, -1.0)
    }

    pub fn screen_to_world(&self, screen: Vec2) -> Vec2 {
        self.center.value + self.offset(screen) * self.scale()
    }

    /// A point on a layer `depth` behind the front, as it appears on the front layer, and
    /// how much that layer is shrunk by perspective.
    pub fn project(&self, pos: Vec2, depth: f32) -> (Vec2, f32) {
        let k = crate::layout::depth_scale(self.scale(), depth);
        let vp = self.center.value + self.vanish.value;
        (vp + (pos - vp) * k, k)
    }

    pub fn cursor_world(&self) -> Option<Vec2> {
        self.cursor.map(|c| self.screen_to_world(c))
    }

    /// Zooms by `factor` keeping the point under `screen` fixed once the motion settles.
    pub fn zoom_at(&mut self, screen: Vec2, factor: f32) {
        let off = self.offset(screen);
        let ts = self.log_scale.target.exp();
        let world = self.center.target + off * ts;
        let ns = (ts * factor).clamp(MIN_SCALE, MAX_SCALE);
        self.log_scale.target = ns.ln();
        self.center.target = world - off * ns;
        self.fling = Vec2::ZERO;
    }

    /// Moves the view by a screen-space delta, immediately (no smoothing).
    pub fn pan_screen(&mut self, delta: Vec2) {
        let d = delta * Vec2::new(-1.0, 1.0) * self.scale();
        self.center.value += d;
        self.center.target += d;
    }

    /// The part of the window the diagram is framed in, leaving room for the overlay UI.
    pub fn avail(&self) -> Vec2 {
        (self.viewport - Vec2::new(140.0, 190.0)).max(Vec2::splat(100.0))
    }

    pub fn fit(&mut self, r: Rect, snap: bool) {
        let s = crate::layout::fit_scale(r, self.avail());
        // Leave a little extra room at the top for the breadcrumb bar.
        let c = r.center() + Vec2::new(0.0, 18.0 * s);
        self.fling = Vec2::ZERO;
        if snap {
            self.center.snap(c);
            self.log_scale.snap(s.ln());
        } else {
            self.center.target = c;
            self.log_scale.target = s.ln();
        }
    }
}

pub fn input(
    mut cam: ResMut<Cam>,
    window: Single<&Window, With<PrimaryWindow>>,
    scroll: Res<AccumulatedMouseScroll>,
    mut pinch: MessageReader<PinchGesture>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    cam.viewport = window.size();
    cam.cursor = cam.fake_cursor.or(window.cursor_position());
    let anchor = cam.cursor.unwrap_or(cam.viewport / 2.0);

    let zoom_mod = keys.any_pressed([
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ]);
    if scroll.delta != Vec2::ZERO && !cam.over_ui {
        match scroll.unit {
            MouseScrollUnit::Line => cam.zoom_at(anchor, (-scroll.delta.y * 0.16).exp()),
            MouseScrollUnit::Pixel if zoom_mod => {
                cam.zoom_at(anchor, (-scroll.delta.y * 0.006).exp())
            }
            MouseScrollUnit::Pixel => {
                let d = scroll.delta * Vec2::new(1.0, 1.0);
                // Trackpad two-finger scroll pans, with a touch of smoothing.
                let s = cam.scale();
                cam.center.target += Vec2::new(-d.x, d.y) * s;
                cam.fling = Vec2::ZERO;
            }
        }
    }
    for g in pinch.read() {
        if !cam.over_ui {
            cam.zoom_at(anchor, (-g.0 * 1.6).exp());
        }
    }
    let step = if keys.pressed(KeyCode::Equal) {
        Some(0.97)
    } else if keys.pressed(KeyCode::Minus) {
        Some(1.03)
    } else {
        None
    };
    if let Some(f) = step {
        let c = cam.viewport / 2.0;
        cam.zoom_at(c, f);
    }
}

#[derive(Component)]
pub struct Background;

pub fn apply(
    time: Res<Time>,
    mut cam: ResMut<Cam>,
    camera: Single<(&mut Transform, &mut Projection), With<Camera2d>>,
    bg: Single<(&mut Transform, &MeshMaterial2d<GridMaterial>), (With<Background>, Without<Camera2d>)>,
) {
    let dt = time.delta_secs().min(0.05);
    if cam.fling.length_squared() > 1.0 {
        let f = cam.fling;
        cam.center.value += f * dt;
        cam.center.target += f * dt;
        cam.fling *= (-dt * 5.0).exp();
    } else {
        cam.fling = Vec2::ZERO;
    }
    cam.center.step(dt, 11.0, 1.0);
    cam.log_scale.step(dt, 11.0, 1.0);
    cam.vanish.step(dt, 9.0, 1.0);
    let s = cam.scale();

    let (mut tf, mut proj) = camera.into_inner();
    tf.translation = cam.center.value.extend(tf.translation.z);
    if let Projection::Orthographic(o) = &mut *proj {
        o.scale = s;
    }
    let (mut bg_tf, _) = bg.into_inner();
    bg_tf.translation = cam.center.value.extend(-100.0);
    bg_tf.scale = (cam.viewport * s * 1.02).extend(1.0);
}
