//! Materials and small animation helpers.

use std::ops::{Add, Mul, Sub};

use bevy::{
    prelude::*,
    render::render_resource::{AsBindGroup, ShaderType},
    shader::ShaderRef,
    sprite_render::{AlphaMode2d, Material2d},
};

#[derive(Clone, Copy, Default, Debug, ShaderType)]
pub struct CardUniform {
    pub fill: Vec4,
    pub border: Vec4,
    pub glow: Vec4,
    /// w, h, corner radius, border width
    pub dims: Vec4,
    /// opacity, shape index, shadow alpha, stacked alpha
    pub p: Vec4,
    /// quad padding, shape mix
    pub q: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct CardMaterial {
    #[uniform(0)]
    pub u: CardUniform,
}

impl Material2d for CardMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://rustyc4/card.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}

#[derive(Clone, Copy, Default, Debug, ShaderType)]
pub struct GridUniform {
    pub base: Vec4,
    pub center: Vec4,
    pub dot: Vec4,
    pub params: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct GridMaterial {
    #[uniform(0)]
    pub u: GridUniform,
}

impl Material2d for GridMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://rustyc4/grid.wgsl".into()
    }
}

pub fn lin(c: Color) -> Vec4 {
    let l = c.to_linear();
    Vec4::new(l.red, l.green, l.blue, l.alpha)
}

/// A damped spring, integrated with small fixed substeps so it stays stable on slow frames.
#[derive(Clone, Copy, Debug, Default)]
pub struct Spring<T> {
    pub value: T,
    pub vel: T,
    pub target: T,
}

impl<T> Spring<T>
where
    T: Copy + Default + Add<Output = T> + Sub<Output = T> + Mul<f32, Output = T>,
{
    pub fn new(v: T) -> Self {
        Self {
            value: v,
            vel: T::default(),
            target: v,
        }
    }

    pub fn snap(&mut self, v: T) {
        self.value = v;
        self.target = v;
        self.vel = T::default();
    }

    pub fn step(&mut self, dt: f32, omega: f32, zeta: f32) {
        let n = (dt * 240.0).ceil().clamp(1.0, 32.0) as usize;
        let h = dt / n as f32;
        for _ in 0..n {
            let acc = (self.target - self.value) * (omega * omega) - self.vel * (2.0 * zeta * omega);
            self.vel = self.vel + acc * h;
            self.value = self.value + self.vel * h;
        }
    }
}
