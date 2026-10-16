use miniquad::*;
use num_traits::FromPrimitive;
use rustc_hash::FxHashMap;
use crate::sim::{AtomType, XYPos, XYVec, STEP_LETTERS};

pub type GFXPos = [f32;2];

pub type Vert = [f32;2];
//Vertex format: (x, y)
pub type UvVert = [f32;4];
//(x, y), (u, v)
//RGBA, with alpha blended over what's drawn below
pub type Color = [f32;4];

//Blends by the fragment's alpha. Opaque colors draw as if unblended.
pub fn alpha_blend() -> PipelineParams{
    PipelineParams{
        color_blend: Some(BlendState::new(
            Equation::Add,
            BlendFactor::Value(BlendValue::SourceAlpha),
            BlendFactor::OneMinusValue(BlendValue::SourceAlpha))
        ),
        ..Default::default()
    }
}

#[repr(C)]
pub struct BasicUniforms{
    pub color: Color,
    pub offset: GFXPos,
    pub world_offset: GFXPos,
    pub angle: f32,
    pub scale: (f32,f32),
}
#[repr(C)]
pub struct UvUniforms{
    pub offset: GFXPos,
    pub world_offset: GFXPos,
    pub angle: f32,
    pub scale: (f32,f32),
    //Scales the texture's alpha
    pub opacity: f32,
}
#[repr(C)]
pub struct TextUniforms{
    pub color: Color,
    pub offset: GFXPos,
    pub world_offset: GFXPos,
    pub angle: f32,
    pub size: f32,//world units per raster pixel
    pub scale: (f32,f32),
}


//A rasterized character. Its quad is in raster pixels, relative to the pen position
//(on the baseline, at the left of the character's advance), y up.
struct CharStorage{
    metrics: fontdue::Metrics,
    binding: Bindings,
}
impl CharStorage{
    //Center of the drawn bitmap, in raster pixels relative to the pen position
    fn bitmap_center(&self) -> XYVec{
        let m = &self.metrics;
        XYVec::new(m.xmin as f32 + m.width as f32/2., m.ymin as f32 + m.height as f32/2.)
    }
}
pub struct FontStorage{
    pipeline: Pipeline,
    chars: FxHashMap<char, CharStorage>,
}
const RASTER_PX:f32 = 128.;
const TEXT_SIZE:f32 = 0.96/RASTER_PX;//world units per raster pixel for text
const TEXT_EXTRA_SPACING:f32 = 0.1;//world units between characters
const SYMBOL_SIZE:f32 = 1.;//world units, for the larger of the symbol's width/height
const TEXTURE_INDEX_BUF: [u16;6] = [
    0, 1, 2,
    1, 2, 3];

//The character drawn on each atom type, and its rotation
pub fn atom_symbol(t: AtomType) -> Option<(char, f32)>{
    use AtomType::*;
    use std::f32::consts::PI;
    match t{
        Salt => Some(('\u{1F714}', 0.)),
        Air => Some(('\u{1F701}', 0.)),
        Fire => Some(('\u{1F702}', 0.)),
        Earth => Some(('\u{1F703}', 0.)),
        Water => Some(('\u{1F704}', 0.)),
        Vitae => Some(('\u{1F70D}', 0.)),
        Mors => Some(('\u{1F70D}', PI)),
        Quicksilver => Some(('\u{263F}', 0.)),
        Gold => Some(('\u{2609}', 0.)),
        Silver => Some(('\u{263D}', 0.)),
        Copper => Some(('\u{2640}', 0.)),
        Iron => Some(('\u{2642}', 0.)),
        Tin => Some(('\u{2643}', 0.)),
        Lead => Some(('\u{2644}', 0.)),
        Quintessence => Some(('\u{2721}', 0.)),
        RepeatingOutputMarker => Some(('\u{2026}', 0.)),
        ConduitSpace => None,
    }
}
//Every character the font needs to provide
fn rendered_chars() -> impl Iterator<Item = char>{
    let atom_chars = (0..=AtomType::Quintessence as u8)
        .filter_map(|id| atom_symbol(AtomType::from_u8(id)?))
        .map(|(c, _)| c);
    ('0'..='9').chain(atom_chars).chain(STEP_LETTERS).chain([ORIGIN_MARK])
}
//Drawn on hex (0,0)
pub const ORIGIN_MARK: char = '@';

/* Symbola v9.00 by George Douros ("Fonts in this site are free for any use"),
 * subsetted to only the characters we render. If a new character is needed,
 * add it to --unicodes and regenerate with fontTools:
 * pyftsubset path/to/Symbola.ttf --output-file=src/Symbola-subset.ttf
 *   --unicodes=U+0030-0039,U+2026,U+2609,U+263D,U+263F,U+2640,U+2642,U+2643,U+2644,U+2721,U+1F701-1F704,U+1F70D,U+1F714,
 *   U+0061,U+0064,U+0065,U+0077,U+0078,U+007A,U+0040
 */
pub fn load_font() -> fontdue::Font{
    fontdue::Font::from_bytes(&include_bytes!("Symbola-subset.ttf")[..],fontdue::FontSettings::default()).unwrap()
}

//Pads the character into a power-of-two square texture (mipmaps need this on WebGL1)
fn build_char(ctx: &mut dyn RenderingBackend, font: &fontdue::Font, c: char) -> CharStorage{
    let (metrics, bitmap) = font.rasterize(c, RASTER_PX);
    let (w, h) = (metrics.width, metrics.height);
    let side = w.max(h).next_power_of_two();
    let mut padded = vec![0u8; side*side];
    for y in 0..h{
        padded[y*side..y*side+w].copy_from_slice(&bitmap[y*w..(y+1)*w]);
    }
    let datatype = TextureParams{
        width: side as _,
        height: side as _,
        format: TextureFormat::Alpha,
        mipmap_filter: MipmapFilterMode::Linear,
        allocate_mipmaps: true,
        ..Default::default()
    };
    let texture = ctx.new_texture_from_data_and_format(&padded, datatype);
    ctx.texture_generate_mipmaps(texture);

    let (left, bottom) = (metrics.xmin as f32, metrics.ymin as f32);
    let (right, top) = (left + w as f32, bottom + h as f32);
    let (u, v) = (w as f32/side as f32, h as f32/side as f32);
    let texture_vert_buf: [UvVert;4] = [
        [left,  bottom, 0., v],
        [left,  top,    0., 0.],
        [right, bottom, u,  v],
        [right, top,    u,  0.]];
    let vb = ctx.new_buffer(BufferType::VertexBuffer,BufferUsage::Immutable, BufferSource::slice(&texture_vert_buf));
    let index_buffer = ctx.new_buffer(BufferType::IndexBuffer,BufferUsage::Immutable, BufferSource::slice(&TEXTURE_INDEX_BUF));
    let binding = Bindings {
        vertex_buffers: vec![vb],
        index_buffer,
        images:vec![texture],
    };
    CharStorage{metrics, binding}
}
impl FontStorage{
    pub fn new(ctx: &mut dyn RenderingBackend) -> FontStorage{
        let shader_meta = ShaderMeta {
            images: vec!["tex".to_string()],
            uniforms: UniformBlockLayout {
                uniforms: vec![
                    UniformDesc::new("color", UniformType::Float4),
                    UniformDesc::new("offset", UniformType::Float2),
                    UniformDesc::new("world_offset", UniformType::Float2),
                    UniformDesc::new("angle", UniformType::Float1),
                    UniformDesc::new("size", UniformType::Float1),
                    UniformDesc::new("scale", UniformType::Float2),
                    ],
            },
        };
        const V_SHADE: &str = include_str!("text_vert.vs");
        const F_SHADE: &str = include_str!("text_frag.fs");
        let shader_uv = ctx.new_shader(ShaderSource::Glsl { vertex: V_SHADE, fragment: F_SHADE } , shader_meta).unwrap();
        let pipeline = ctx.new_pipeline(
            &[BufferLayout::default()],
            &[
                VertexAttribute::new("local_pos", VertexFormat::Float2),
                VertexAttribute::new("uv", VertexFormat::Float2),
            ],
            shader_uv,
            alpha_blend()
        );

        let font = load_font();
        let mut chars = FxHashMap::default();
        for c in rendered_chars(){
            chars.entry(c).or_insert_with(|| build_char(ctx, &font, c));
        }
        FontStorage { pipeline, chars }
    }

    pub fn set_pipeline(&self, ctx: &mut dyn RenderingBackend){
        ctx.apply_pipeline(&self.pipeline);
    }
    //WARNING: Assumes pipeline has been set already
    fn draw_char(&self, ctx: &mut dyn RenderingBackend, char_data: &CharStorage, color: Color, pen: XYPos,
        size: f32, angle: f32, world_offset: GFXPos, scale: (f32, f32))
    {
        ctx.apply_bindings(&char_data.binding);
        ctx.apply_uniforms(UniformsSource::table(&TextUniforms {
            color, offset: pen.into(), world_offset, angle, size, scale
        }));
        ctx.draw(0, 6, 1);
    }
    //WARNING: Assumes pipeline has been set already
    //Centers the text's bounding box on pos. Characters the font wasn't built with are skipped
    pub fn render_text_centered(&self, ctx: &mut dyn RenderingBackend, text: &str, color: Color, pos: XYPos, world_offset: GFXPos, scale: (f32, f32)){
        let text_chars = || text.chars().filter_map(|c| self.chars.get(&c));
        let mut width_total = 0.;
        let (mut bottom, mut top) = (f32::INFINITY, f32::NEG_INFINITY);
        for char_data in text_chars(){
            let m = &char_data.metrics;
            width_total += m.advance_width*TEXT_SIZE + TEXT_EXTRA_SPACING;
            bottom = bottom.min(m.ymin as f32);
            top = top.max(m.ymin as f32 + m.height as f32);
        }
        if width_total == 0. {return}
        width_total -= TEXT_EXTRA_SPACING;
        let mut pen = XYPos::new(pos.x - width_total/2., pos.y - (bottom+top)/2.*TEXT_SIZE);
        for char_data in text_chars(){
            self.draw_char(ctx, char_data, color, pen, TEXT_SIZE, 0., world_offset, scale);
            pen.x += char_data.metrics.advance_width*TEXT_SIZE + TEXT_EXTRA_SPACING;
        }
    }
    //WARNING: Assumes pipeline has been set already
    //Draws the atom's symbol centered on pos, scaled so its larger side is SYMBOL_SIZE
    pub fn render_atom_symbol(&self, ctx: &mut dyn RenderingBackend, t: AtomType, color: Color, pos: XYPos, world_offset: GFXPos, scale: (f32, f32)){
        let Some((c, angle)) = atom_symbol(t) else {return};
        let Some(char_data) = self.chars.get(&c) else {return};
        let m = &char_data.metrics;
        let size = SYMBOL_SIZE/(m.width.max(m.height).max(1) as f32);
        //The shader rotates around the pen, so offset it to keep the bitmap's center on pos
        let pen = pos - nalgebra::Rotation2::new(angle)*(char_data.bitmap_center()*size);
        self.draw_char(ctx, char_data, color, pen, size, angle, world_offset, scale);
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn font_has_all_rendered_chars(){
        let font = load_font();
        for c in rendered_chars(){
            assert_ne!(font.lookup_glyph_index(c), 0, "font is missing {:?} (U+{:04X})", c, c as u32);
        }
    }
}
