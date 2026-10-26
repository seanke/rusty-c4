//! Painting text into textures, so titles can be printed on the blocks themselves and lie
//! in the 3D scene like any other surface.

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use cosmic_text::{fontdb, Align, Attrs, Buffer, Color as Ink, Family, FontSystem, Metrics, Shaping, SwashCache, Weight};

/// Texels per layout unit: cards are 320 units wide, so about 1000 texels across.
const TEXELS: f32 = 3.2;

pub struct Span {
    pub text: String,
    pub size: f32,
    pub weight: u16,
    pub color: Color,
}

pub fn span(text: impl Into<String>, size: f32, weight: u16, color: Color) -> Span {
    Span {
        text: text.into(),
        size,
        weight,
        color,
    }
}

/// A painted block of text: its texture and its size in layout units.
pub struct Painted {
    pub image: Image,
    pub size: Vec2,
}

#[derive(Resource)]
pub struct Painter {
    fonts: FontSystem,
    cache: SwashCache,
    family: Option<String>,
}

impl Painter {
    pub fn new() -> Painter {
        // A family with real bold faces: the system's variable SF font renders its heavier
        // weights slanted through this rasteriser.
        let mut db = fontdb::Database::new();
        let families: [&[&str]; 4] = [
            &["/System/Library/Fonts/HelveticaNeue.ttc"],
            &["/System/Library/Fonts/Supplemental/Arial.ttf", "/System/Library/Fonts/Supplemental/Arial Bold.ttf"],
            &["C:\\Windows\\Fonts\\segoeui.ttf", "C:\\Windows\\Fonts\\segoeuib.ttf"],
            &["/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"],
        ];
        for files in families {
            let found: Vec<Vec<u8>> = files.iter().filter_map(|p| std::fs::read(p).ok()).collect();
            if !found.is_empty() {
                for bytes in found {
                    db.load_font_data(bytes);
                }
                break;
            }
        }
        let family = db.faces().next().and_then(|f| f.families.first()).map(|f| f.0.clone());
        let fonts = if family.is_some() {
            FontSystem::new_with_locale_and_db("en-US".into(), db)
        } else {
            FontSystem::new()
        };
        Painter {
            fonts,
            cache: SwashCache::new(),
            family,
        }
    }

    /// Lay out `spans` within `width` layout units and paint them. Centred text keeps the
    /// full width; left-aligned text is cropped to what it uses.
    pub fn paint(&mut self, spans: &[Span], width: f32, centered: bool) -> Option<Painted> {
        let family = match &self.family {
            Some(f) => Family::Name(f.as_str()),
            None => Family::SansSerif,
        };
        let base = Attrs::new().family(family);
        let first = spans.first()?;
        let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(first.size * TEXELS, first.size * TEXELS * 1.25));
        buffer.set_size(&mut self.fonts, Some(width * TEXELS), None);
        let attrs: Vec<(&str, Attrs)> = spans
            .iter()
            .map(|s| {
                let c = s.color.to_srgba();
                let ink = Ink::rgba(
                    (c.red * 255.0) as u8,
                    (c.green * 255.0) as u8,
                    (c.blue * 255.0) as u8,
                    (c.alpha * 255.0) as u8,
                );
                let a = base
                    .clone()
                    .weight(Weight(s.weight))
                    .color(ink)
                    .metrics(Metrics::new(s.size * TEXELS, s.size * TEXELS * 1.25));
                (s.text.as_str(), a)
            })
            .collect();
        let align = if centered { Align::Center } else { Align::Left };
        buffer.set_rich_text(&mut self.fonts, attrs, &base, Shaping::Advanced, Some(align));
        buffer.shape_until_scroll(&mut self.fonts, false);

        let (mut used_w, mut used_h) = (0.0f32, 0.0f32);
        for run in buffer.layout_runs() {
            used_w = used_w.max(run.line_w);
            used_h = used_h.max(run.line_top + run.line_height);
        }
        if used_h <= 0.0 {
            return None;
        }
        let pad = 6;
        let w = if centered { (width * TEXELS).ceil() as i32 } else { used_w.ceil() as i32 } + pad * 2;
        let h = used_h.ceil() as i32 + pad * 2;
        // Premultiplied RGBA.
        let mut px = vec![0f32; (w * h * 4) as usize];
        buffer.draw(&mut self.fonts, &mut self.cache, Ink::rgb(255, 255, 255), |x, y, gw, gh, ink| {
            let a = ink.a() as f32 / 255.0;
            if a <= 0.0 {
                return;
            }
            let rgb = [ink.r() as f32 / 255.0, ink.g() as f32 / 255.0, ink.b() as f32 / 255.0];
            for yy in y..y + gh as i32 {
                for xx in x..x + gw as i32 {
                    let (px_x, px_y) = (xx + pad, yy + pad);
                    if px_x < 0 || px_y < 0 || px_x >= w || px_y >= h {
                        continue;
                    }
                    let i = ((px_y * w + px_x) * 4) as usize;
                    for k in 0..3 {
                        px[i + k] = rgb[k] * a + px[i + k] * (1.0 - a);
                    }
                    px[i + 3] = a + px[i + 3] * (1.0 - a);
                }
            }
        });
        let size = Vec2::new(w as f32, h as f32) / TEXELS;
        Some(Painted {
            image: mipmapped(px, w as u32, h as u32),
            size,
        })
    }
}

/// An sRGB texture with a full chain of box-filtered mip levels, so text stays smooth when
/// seen from afar.
fn mipmapped(level0: Vec<f32>, w: u32, h: u32) -> Image {
    let mut data = Vec::new();
    let mut levels = 0;
    let (mut cw, mut ch, mut cur) = (w, h, level0);
    loop {
        data.extend(cur.iter().map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8));
        levels += 1;
        if (cw == 1 && ch == 1) || levels >= 8 {
            break;
        }
        let (nw, nh) = ((cw / 2).max(1), (ch / 2).max(1));
        let mut next = vec![0f32; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                for k in 0..4 {
                    let mut sum = 0.0;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let sx = (x * 2 + dx).min(cw - 1);
                        let sy = (y * 2 + dy).min(ch - 1);
                        sum += cur[((sy * cw + sx) * 4 + k) as usize];
                    }
                    next[((y * nw + x) * 4 + k) as usize] = sum / 4.0;
                }
            }
        }
        cw = nw;
        ch = nh;
        cur = next;
    }
    // Stored in sRGB bytes; the alpha is linear, so this is close enough for text.
    let mut image = Image::new_uninit(
        Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = levels;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..default()
    });
    image
}
