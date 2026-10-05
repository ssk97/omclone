use miniquad::*;
use num_traits::FromPrimitive;
use crate::sim::AtomType;

pub type GFXPos = [f32;2];

pub type Vert = [f32;2];
//Vertex format: (x, y)
pub type UvVert = [f32;4];
//(x, y), (u, v)

#[repr(C)]
pub struct BasicUniforms{
    pub color: [f32;3],
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
}
#[repr(C)]
pub struct TextUniforms{
    pub color: [f32;3],
    pub offset: GFXPos,
    pub world_offset: GFXPos,
    pub angle: f32,
    pub scale: (f32,f32),
}


struct CharStorage{
    metrics: fontdue::Metrics,
    binding: Bindings,
}
pub struct FontStorage{
    pipeline: Pipeline,
    data: [CharStorage;10],
    //Indexed by AtomType; the f32 is a fixed rotation applied to the glyph
    atom_symbols: Vec<Option<(Bindings, f32)>>,
}
const FONT_SCALE:f32 = 0.03;
const FONT_EXTRA_SPACING:f32 = 0.1;
const SYMBOL_RASTER_PX:f32 = 128.;
const SYMBOL_SIZE:f32 = 1.;//world units, for the larger of the glyph's width/height
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

/* Symbola v9.00 by George Douros ("Fonts in this site are free for any use"),
 * subsetted to only the characters we render. If a new character is needed,
 * add it to --unicodes and regenerate with fontTools:
 * pyftsubset path/to/Symbola.ttf --output-file=src/Symbola-subset.ttf
 *   --unicodes=U+0030-0039,U+2026,U+2609,U+263D,U+263F,U+2640,U+2642,U+2643,U+2644,U+2721,U+1F701-1F704,U+1F70D,U+1F714
 */
pub fn load_font() -> fontdue::Font{
    fontdue::Font::from_bytes(&include_bytes!("Symbola-subset.ttf")[..],fontdue::FontSettings::default()).unwrap()
}

//Pads the glyph into a power-of-two square texture (mipmaps need this on WebGL1)
//and makes a SYMBOL_SIZE quad centered on the glyph's bounding box
fn symbol_binding(ctx: &mut dyn RenderingBackend, font: &fontdue::Font, c: char) -> Bindings{
    let (metrics, bitmap) = font.rasterize(c, SYMBOL_RASTER_PX);
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

    let world_per_px = SYMBOL_SIZE/(w.max(h) as f32);
    let xx = w as f32 * world_per_px * 0.5;
    let yy = h as f32 * world_per_px * 0.5;
    let (u, v) = (w as f32/side as f32, h as f32/side as f32);
    let texture_vert_buf: [UvVert;4] = [
        [-xx,-yy,    0., v],
        [-xx, yy,    0., 0.],
        [ xx,-yy,    u,  v],
        [ xx, yy,    u,  0.]];
    let vb = ctx.new_buffer(BufferType::VertexBuffer,BufferUsage::Immutable, BufferSource::slice(&texture_vert_buf));
    let index_buffer = ctx.new_buffer(BufferType::IndexBuffer,BufferUsage::Immutable, BufferSource::slice(&TEXTURE_INDEX_BUF));
    Bindings {
        vertex_buffers: vec![vb],
        index_buffer,
        images:vec![texture],
    }
}
impl FontStorage{
    pub fn new(ctx: &mut dyn RenderingBackend) -> FontStorage{
        let shader_meta = ShaderMeta {
            images: vec!["tex".to_string()],
            uniforms: UniformBlockLayout {
                uniforms: vec![
                    UniformDesc::new("color", UniformType::Float3),
                    UniformDesc::new("offset", UniformType::Float2),
                    UniformDesc::new("world_offset", UniformType::Float2),
                    UniformDesc::new("angle", UniformType::Float1),
                    UniformDesc::new("scale", UniformType::Float2),
                    ],
            },
        };
        const V_SHADE: &str = include_str!("text_vert.vs");
        const F_SHADE: &str = include_str!("text_frag.fs");
        let shader_uv = ctx.new_shader(ShaderSource::Glsl { vertex: V_SHADE, fragment: F_SHADE } , shader_meta).unwrap();
        use miniquad::graphics::*;
        let pipeline = ctx.new_pipeline(
            &[BufferLayout::default()],
            &[
                VertexAttribute::new("local_pos", VertexFormat::Float2),
                VertexAttribute::new("uv", VertexFormat::Float2),
            ],
            shader_uv,
            PipelineParams{
                color_blend: Some(BlendState::new(
                    Equation::Add,
                    BlendFactor::Value(BlendValue::SourceAlpha),
                    BlendFactor::OneMinusValue(BlendValue::SourceAlpha))
                ),
                ..Default::default()
            }
        );

        let chars = ['0','1','2','3','4','5','6','7','8','9'];
        let font = load_font();
        let data = chars.map(|char| -> CharStorage {
            let (metrics, bitmap) = font.rasterize(char, 32.0);
            let datatype = TextureParams{
                width: metrics.width as _,
                height: metrics.height as _,
                format: TextureFormat::Alpha,
                ..Default::default()
            };
            let texture = ctx.new_texture_from_data_and_format(&bitmap, datatype);

            let xx = metrics.width as f32 * FONT_SCALE * 0.5;
            let yy = metrics.height as f32 * FONT_SCALE * 0.5;
            let texture_vert_buf: [UvVert;4] = [
                [-xx,-yy,    0., 1.],
                [-xx, yy,    0., 0.],
                [ xx,-yy,    1., 1.],
                [ xx, yy,    1., 0.]];
            let vb = ctx.new_buffer(BufferType::VertexBuffer,BufferUsage::Immutable, BufferSource::slice(&texture_vert_buf));
            let index_buffer = ctx.new_buffer(BufferType::IndexBuffer,BufferUsage::Immutable, BufferSource::slice(&TEXTURE_INDEX_BUF));

            let binding = Bindings {
                vertex_buffers: vec![vb],
                index_buffer,
                images:vec![texture],
            };
            CharStorage{metrics, binding}
        });
        let atom_symbols = (0..=AtomType::Quintessence as u8).map(|id| {
            let (c, rot) = atom_symbol(AtomType::from_u8(id)?)?;
            Some((symbol_binding(ctx, &font, c), rot))
        }).collect();
        FontStorage { pipeline, data, atom_symbols }
    }

    pub fn set_pipeline(&self, ctx: &mut dyn RenderingBackend){
        ctx.apply_pipeline(&self.pipeline);
    }
    //WARNING: Assumes pipeline has been set already
    //Only digits are drawn; other characters are skipped
    pub fn render_text_centered(&self, ctx: &mut dyn RenderingBackend, text: &str, color: [f32;3], pos: GFXPos, world_offset: GFXPos, scale: (f32, f32)){
        let digits = || text.chars().filter_map(|c| c.to_digit(10));
        let mut width_total = 0.;
        for digit in digits(){
            let char_data = &self.data[digit as usize];
            width_total += char_data.metrics.advance_width * FONT_SCALE + FONT_EXTRA_SPACING;
        }
        let mut x = pos[0]-width_total/2.;
        for digit in digits(){
            let char_data = &self.data[digit as usize];
            let metrics = char_data.metrics;
            ctx.apply_bindings(&char_data.binding);
            let offset = [x+(metrics.xmin as f32 *FONT_SCALE), pos[1]+(metrics.ymin as f32 *FONT_SCALE)];
            ctx.apply_uniforms(UniformsSource::table(&TextUniforms {
                color, offset, world_offset, angle: 0., scale
            }));
            ctx.draw(0, 6, 1);
            x += metrics.advance_width * FONT_SCALE + FONT_EXTRA_SPACING;
        }
    }
    //WARNING: Assumes pipeline has been set already
    pub fn render_atom_symbol(&self, ctx: &mut dyn RenderingBackend, t: AtomType, color: [f32;3], pos: GFXPos, world_offset: GFXPos, scale: (f32, f32)){
        if let Some(Some((binding, angle))) = self.atom_symbols.get(t as usize){
            ctx.apply_bindings(binding);
            ctx.apply_uniforms(UniformsSource::table(&TextUniforms {
                color, offset: pos, world_offset, angle: *angle, scale
            }));
            ctx.draw(0, 6, 1);
        }
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn font_has_all_rendered_chars(){
        let font = load_font();
        let atom_chars = (0..=AtomType::Quintessence as u8)
            .filter_map(|id| atom_symbol(AtomType::from_u8(id)?))
            .map(|(c, _)| c);
        for c in ('0'..='9').chain(atom_chars){
            assert_ne!(font.lookup_glyph_index(c), 0, "missing glyph for {:?} (U+{:04X})", c, c as u32);
        }
    }
}
