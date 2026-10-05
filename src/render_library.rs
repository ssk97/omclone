use miniquad::*;

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
    pub offset: GFXPos,
    pub world_offset: GFXPos,
    pub scale: (f32,f32),
}


struct CharStorage{
    metrics: fontdue::Metrics,
    binding: Bindings,
}
pub struct FontStorage{
    pipeline: Pipeline,
    data: [CharStorage;10],
}
const FONT_SCALE:f32 = 0.03;
const FONT_EXTRA_SPACING:f32 = 0.1;
impl FontStorage{
    pub fn new(ctx: &mut dyn RenderingBackend) -> FontStorage{
        let shader_meta = ShaderMeta {
            images: vec!["tex".to_string()],
            uniforms: UniformBlockLayout {
                uniforms: vec![
                    UniformDesc::new("offset", UniformType::Float2),
                    UniformDesc::new("world_offset", UniformType::Float2),
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

        const TEXTURE_INDEX_BUF: [u16;6] = [
            0, 1, 2,
            1, 2, 3];
        let chars = ['0','1','2','3','4','5','6','7','8','9'];
        let font = fontdue::Font::from_bytes(&include_bytes!("Montserrat-Light.ttf")[..],fontdue::FontSettings::default()).unwrap();
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
        FontStorage { pipeline, data }
    }

    pub fn set_pipeline(&self, ctx: &mut dyn RenderingBackend){
        ctx.apply_pipeline(&self.pipeline);
    }
    //WARNING: Assumes pipeline has been set already
    //Only digits are drawn; other characters are skipped
    pub fn render_text_centered(&self, ctx: &mut dyn RenderingBackend, text: &str, pos: GFXPos, world_offset: GFXPos, scale: (f32, f32)){
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
                offset, world_offset, scale
            }));
            ctx.draw(0, 6, 1);
            x += metrics.advance_width * FONT_SCALE + FONT_EXTRA_SPACING;
        }
    }
}