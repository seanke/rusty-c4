//! Materials and small animation helpers.

use std::ops::{Add, Mul, Sub};

use bevy::{
    mesh::MeshVertexBufferLayoutRef,
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    render::render_resource::{
        AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
    },
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
pub struct BlockUniform {
    /// neon colour (linear), w: glow intensity
    pub neon: Vec4,
    pub body: Vec4,
    /// half width, half depth, corner radius, height
    pub dims: Vec4,
    /// hover, selected, shape index, opened
    pub p: Vec4,
    /// line width, grid spacing, dim, style
    pub q: Vec4,
}

/// An extruded element block (see `block.wgsl`).
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct BlockMaterial {
    #[uniform(0)]
    pub u: BlockUniform,
}

impl Material for BlockMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://rustyc4/block.wgsl".into()
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

#[derive(Clone, Copy, Default, Debug, ShaderType)]
pub struct PipeUniform {
    /// neon colour (linear), w: glow intensity
    pub color: Vec4,
    /// highlight, dim, pulse spacing (radii), pulse speed (radii per second)
    pub p: Vec4,
    /// style
    pub q: Vec4,
}

/// A relationship pipe (see `pipe.wgsl`).
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct PipeMaterial {
    #[uniform(0)]
    pub u: PipeUniform,
}

impl Material for PipeMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://rustyc4/pipe.wgsl".into()
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

#[derive(Clone, Copy, Default, Debug, ShaderType)]
pub struct FloorUniform {
    pub color: Vec4,
    /// minor spacing, major every n minor, fade distance
    pub p: Vec4,
    pub base: Vec4,
}

/// The neon grid floor (see `floor.wgsl`).
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct FloorMaterial {
    #[uniform(0)]
    pub u: FloorUniform,
}

impl Material for FloorMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://rustyc4/floor.wgsl".into()
    }
}

#[derive(Clone, Copy, Default, Debug, ShaderType)]
pub struct DecalUniform {
    /// rgb: tint (HDR), a: opacity
    pub tint: Vec4,
    /// pill backing colour; a = 0 for none
    pub bg: Vec4,
    /// width / height, pill border alpha
    pub p: Vec4,
    /// pill border colour
    pub edge: Vec4,
}

/// Painted text lying on a surface (see `decal.wgsl`).
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct DecalMaterial {
    #[uniform(0)]
    pub u: DecalUniform,
    #[texture(1)]
    #[sampler(2)]
    pub texture: Handle<Image>,
}

impl Material for DecalMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://rustyc4/decal.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Premultiplied
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// A saturated, bright version of an element's colour for neon edges, in linear space.
pub fn neon(c: [f32; 3]) -> Vec3 {
    let hsv: Hsva = Color::srgb(c[0], c[1], c[2]).into();
    let sat = if hsv.saturation < 0.25 { hsv.saturation * 0.6 } else { (hsv.saturation * 1.25).min(1.0) };
    let l = Color::from(Hsva::new(hsv.hue, sat, 1.0, 1.0)).to_linear();
    Vec3::new(l.red, l.green, l.blue)
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
