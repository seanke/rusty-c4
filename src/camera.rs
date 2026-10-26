//! The 3D camera: it orbits a target point, looking down at the diagram at an angle.
//! Everything is spring-smoothed: zoom toward the cursor, drag to pan, right-drag to orbit,
//! trackpad scroll to pan and pinch to zoom.

use bevy::{
    input::{
        gestures::PinchGesture,
        mouse::{AccumulatedMouseScroll, MouseScrollUnit},
    },
    prelude::*,
    window::PrimaryWindow,
};

use crate::render::Spring;

const FOV: f32 = 0.42;
pub const DEFAULT_PITCH: f32 = 1.02;

#[derive(Resource)]
pub struct Cam {
    pub target: Spring<Vec3>,
    pub log_dist: Spring<f32>,
    pub yaw: Spring<f32>,
    pub pitch: Spring<f32>,
    /// Logical window size.
    pub viewport: Vec2,
    pub cursor: Option<Vec2>,
    /// Cursor position injected by the script driver; overrides the real one.
    pub fake_cursor: Option<Vec2>,
    pub over_ui: bool,
    /// Where the camera was before it flew to a detail card, to return there after.
    pub saved: Option<(Vec3, f32)>,
}

impl Default for Cam {
    fn default() -> Self {
        Self {
            target: Spring::new(Vec3::ZERO),
            log_dist: Spring::new(2000f32.ln()),
            yaw: Spring::new(0.0),
            pitch: Spring::new(DEFAULT_PITCH),
            viewport: Vec2::new(1440.0, 900.0),
            cursor: None,
            fake_cursor: None,
            over_ui: false,
            saved: None,
        }
    }
}

impl Cam {
    pub fn dist(&self) -> f32 {
        self.log_dist.value.exp()
    }

    fn eye_for(target: Vec3, dist: f32, yaw: f32, pitch: f32) -> Vec3 {
        target + Vec3::new(yaw.sin() * pitch.cos(), pitch.sin(), yaw.cos() * pitch.cos()) * dist
    }

    pub fn eye(&self) -> Vec3 {
        Self::eye_for(self.target.value, self.dist(), self.yaw.value, self.pitch.value)
    }

    pub fn transform(&self) -> Transform {
        Transform::from_translation(self.eye()).looking_at(self.target.value, Vec3::Y)
    }

    pub fn near(&self) -> f32 {
        (self.dist() * 0.01).max(0.001)
    }

    fn clip_from_world(&self) -> Mat4 {
        let aspect = self.viewport.x / self.viewport.y.max(1.0);
        let proj = Mat4::perspective_infinite_reverse_rh(FOV, aspect, self.near());
        proj * self.transform().to_matrix().inverse()
    }

    /// Where a world point lands in the window (logical pixels from the top left), if it is
    /// in front of the camera.
    pub fn world_to_screen(&self, p: Vec3) -> Option<Vec2> {
        let clip = self.clip_from_world() * p.extend(1.0);
        if clip.w <= 1e-6 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        Some(Vec2::new((ndc.x + 1.0) * 0.5 * self.viewport.x, (1.0 - ndc.y) * 0.5 * self.viewport.y))
    }

    /// The same, in the overlay's coordinates (pixels from the centre, y up).
    pub fn world_to_overlay(&self, p: Vec3) -> Option<Vec2> {
        self.world_to_screen(p).map(|s| self.screen_to_overlay(s))
    }

    pub fn screen_to_overlay(&self, s: Vec2) -> Vec2 {
        Vec2::new(s.x - self.viewport.x / 2.0, self.viewport.y / 2.0 - s.y)
    }

    /// The ray through a screen point: origin and direction.
    pub fn ray(&self, screen: Vec2) -> (Vec3, Vec3) {
        let inv = self.clip_from_world().inverse();
        let ndc = Vec2::new(screen.x / self.viewport.x * 2.0 - 1.0, 1.0 - screen.y / self.viewport.y * 2.0);
        let near = inv * Vec4::new(ndc.x, ndc.y, 1.0, 1.0);
        let far = inv * Vec4::new(ndc.x, ndc.y, 0.1, 1.0);
        let a = near.truncate() / near.w;
        let b = far.truncate() / far.w;
        (a, (b - a).normalize_or(Vec3::NEG_Y))
    }

    /// Where the ray through `screen` meets the horizontal plane at height `y`.
    pub fn ground(&self, screen: Vec2, y: f32) -> Option<Vec3> {
        let (o, d) = self.ray(screen);
        if d.y.abs() < 1e-5 {
            return None;
        }
        let t = (y - o.y) / d.y;
        (t > 0.0).then(|| o + d * t)
    }

    /// The camera's right, flattened onto the ground.
    pub fn right(&self) -> Vec3 {
        let yaw = self.yaw.target;
        Vec3::new(yaw.cos(), 0.0, -yaw.sin())
    }

    /// Frame a horizontal rectangle (`size` along x and z) centred on `center`.
    pub fn fit(&mut self, center: Vec3, size: Vec2, snap: bool) {
        let aspect = self.viewport.x / self.viewport.y.max(1.0);
        let t = (FOV / 2.0).tan();
        let pitch = self.pitch.target;
        // Leave room for the overlay (breadcrumbs above, hints below).
        let margin = 1.12;
        let w = size.x * 0.5 * margin / (t * aspect);
        let d = (size.y * 0.5 * pitch.sin() * margin) / (t * 0.82);
        let dist = w.max(d).max(1e-3) * 1.08;
        if snap {
            self.target.snap(center);
            self.log_dist.snap(dist.ln());
        } else {
            self.target.target = center;
            self.log_dist.target = dist.ln();
        }
    }

    /// Zoom by `factor`, keeping the point under `screen` where it is.
    pub fn zoom_at(&mut self, screen: Vec2, factor: f32) {
        let y = self.target.target.y;
        if let Some(hit) = self.ground(screen, y) {
            self.target.target = hit + (self.target.target - hit) * factor;
        }
        self.log_dist.target += factor.ln();
    }

    /// Move the view so the ground under `from` ends up under `to`, immediately.
    pub fn drag(&mut self, from: Vec2, to: Vec2) {
        let y = self.target.value.y;
        if let (Some(a), Some(b)) = (self.ground(from, y), self.ground(to, y)) {
            let d = Vec3::new(a.x - b.x, 0.0, a.z - b.z);
            self.target.value += d;
            self.target.target += d;
        }
    }

    pub fn orbit(&mut self, delta: Vec2) {
        self.yaw.target -= delta.x * 0.006;
        self.pitch.target = (self.pitch.target + delta.y * 0.004).clamp(0.45, 1.5);
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
            MouseScrollUnit::Pixel if zoom_mod => cam.zoom_at(anchor, (-scroll.delta.y * 0.006).exp()),
            MouseScrollUnit::Pixel => {
                // Trackpad two-finger scroll pans.
                let c = cam.viewport / 2.0;
                cam.drag(c, c + scroll.delta);
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

pub fn apply(
    time: Res<Time>,
    mut cam: ResMut<Cam>,
    camera: Single<(&mut Transform, &mut Projection), With<Camera3d>>,
) {
    let dt = time.delta_secs().min(0.05);
    cam.target.step(dt, 9.0, 1.0);
    cam.log_dist.step(dt, 9.0, 1.0);
    cam.yaw.step(dt, 12.0, 1.0);
    cam.pitch.step(dt, 12.0, 1.0);
    let (mut tf, mut proj) = camera.into_inner();
    *tf = cam.transform();
    if let Projection::Perspective(p) = &mut *proj {
        p.fov = FOV;
        p.near = cam.near();
        p.far = cam.dist() * 400.0;
    }
}
