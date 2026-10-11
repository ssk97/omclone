use crate::render_library::*;
use miniquad::*;
use crate::sim::*;

//The mspaintart images are drawn at 110 pixels per world unit (1 hex = 220 pixels wide)
const PX_PER_UNIT: f32 = 110.;
const QUAD_INDEX_BUF: [u16;6] = [
    0, 1, 2,
    1, 2, 3];

//Non-power-of-two images must use TextureWrap::Clamp (and no mipmaps) or WebGL1 renders them black
pub fn load_texture(ctx: &mut dyn RenderingBackend, byte_data: &[u8], wrap: TextureWrap) -> (TextureId, u32, u32){
    use image::ImageReader;
    use image::ImageFormat::Png;
    use std::io::Cursor;
    let img = ImageReader::with_format(Cursor::new(byte_data),Png).decode().unwrap().into_rgba8();
    let (width, height) = (img.width(), img.height());
    let params = TextureParams {
        format: TextureFormat::RGBA8,
        wrap,
        width, height,
        ..Default::default()
    };
    (ctx.new_texture_from_data_and_format(&img, params), width, height)
}

//Makes a textured quad where origin_px (in image pixels, from the top-left) lands on (0,0)
fn image_quad(ctx: &mut dyn RenderingBackend, byte_data: &[u8], origin_px: [f32;2]) -> Bindings{
    let (texture, width, height) = load_texture(ctx, byte_data, TextureWrap::Clamp);
    let [ox, oy] = origin_px;
    let left = -ox/PX_PER_UNIT;
    let right = (width as f32-ox)/PX_PER_UNIT;
    let bottom = -(height as f32-oy)/PX_PER_UNIT;
    let top = oy/PX_PER_UNIT;
    let verts: [UvVert;4] = [
        [left,  bottom, 0., 1.],
        [left,  top,    0., 0.],
        [right, bottom, 1., 1.],
        [right, top,    1., 0.]];
    let vb = ctx.new_buffer(BufferType::VertexBuffer,BufferUsage::Immutable, BufferSource::slice(&verts));
    let index_buffer = ctx.new_buffer(BufferType::IndexBuffer,BufferUsage::Immutable, BufferSource::slice(&QUAD_INDEX_BUF));
    Bindings {
        vertex_buffers: vec![vb],
        index_buffer,
        images: vec![texture],
    }
}

pub struct GlyphImageData {
    pub tex: Bindings,
    pub rot_offset: Rot,
    pub aux: Vec<(Bindings, Pos)>,
}

impl GlyphImageData {
    //WARNING: Assumes the textured pipeline has been set already
    pub fn draw(&self, ctx: &mut Context, glyph: &Glyph, world_offset: GFXPos, scale: (f32, f32)) {
        let offset = pos_to_xy(glyph.pos).into();
        let angle = rot_to_angle(glyph.rot+self.rot_offset);
        ctx.apply_bindings(&self.tex);
        ctx.apply_uniforms(UniformsSource::table(&UvUniforms {
            offset, world_offset, angle, scale
        }));
        ctx.draw(0, 6, 1);
        //Dot positions rotate with the glyph, but the dots themselves stay upright
        for (binding, pos) in self.aux.iter(){
            let offset = pos_to_xy(glyph.reposition(*pos)).into();
            ctx.apply_bindings(binding);
            ctx.apply_uniforms(UniformsSource::table(&UvUniforms {
                offset, world_offset, angle: 0., scale
            }));
            ctx.draw(0, 6, 1);
        }
    }
}

pub struct GlyphImages {
    glyphs: Vec<GlyphImageData>,
}

impl GlyphImages {
    pub fn new(ctx: &mut dyn RenderingBackend) -> GlyphImages {
        //Dots share one hex-sized quad, centered on the hex
        let (dot_w, dot_h) = (220./PX_PER_UNIT/2., 251./PX_PER_UNIT/2.);
        let dot_verts: [UvVert;4] = [
            [-dot_w, -dot_h, 0., 1.],
            [-dot_w,  dot_h, 0., 0.],
            [ dot_w, -dot_h, 1., 1.],
            [ dot_w,  dot_h, 1., 0.]];
        let dot_vb = ctx.new_buffer(BufferType::VertexBuffer,BufferUsage::Immutable, BufferSource::slice(&dot_verts));
        let dot_ib = ctx.new_buffer(BufferType::IndexBuffer,BufferUsage::Immutable, BufferSource::slice(&QUAD_INDEX_BUF));
        let mut dot = |byte_data: &[u8]| -> Bindings {
            let (texture, _, _) = load_texture(ctx, byte_data, TextureWrap::Clamp);
            Bindings {
                vertex_buffers: vec![dot_vb],
                index_buffer: dot_ib,
                images: vec![texture],
            }
        };
        let anim1 = dot(include_bytes!("../mspaintart/dots/anim1.png"));
        let anim2 = dot(include_bytes!("../mspaintart/dots/anim2.png"));
        let anim3 = dot(include_bytes!("../mspaintart/dots/anim3.png"));
        let card1 = dot(include_bytes!("../mspaintart/dots/card1.png"));
        let disp1 = dot(include_bytes!("../mspaintart/dots/disp1.png"));
        let disp2 = dot(include_bytes!("../mspaintart/dots/disp2.png"));
        let disp3 = dot(include_bytes!("../mspaintart/dots/disp3.png"));
        let disp4 = dot(include_bytes!("../mspaintart/dots/disp4.png"));
        let disp5 = dot(include_bytes!("../mspaintart/dots/disp5.png"));
        let div1 = dot(include_bytes!("../mspaintart/dots/div1.png"));
        let div2 = dot(include_bytes!("../mspaintart/dots/div2.png"));
        let metal1 = dot(include_bytes!("../mspaintart/dots/metal1.png"));
        let metal2 = dot(include_bytes!("../mspaintart/dots/metal2.png"));
        let metal3 = dot(include_bytes!("../mspaintart/dots/metal3.png"));
        let quint1 = dot(include_bytes!("../mspaintart/dots/quint1.png"));
        let quint2 = dot(include_bytes!("../mspaintart/dots/quint2.png"));
        let salt1 = dot(include_bytes!("../mspaintart/dots/salt1.png"));
        let triplex1 = dot(include_bytes!("../mspaintart/dots/triplex1.png"));
        let unif1 = dot(include_bytes!("../mspaintart/dots/unif1.png"));
        let unif2 = dot(include_bytes!("../mspaintart/dots/unif2.png"));

        //Pixel location of the glyph's primary hex within each image
        const ORIGIN: [f32;2] = [220., 316.];
        const ORIGIN_CENTER: [f32;2] = [440., 316.];
        const ORIGIN_TOP: [f32;2] = [330., 125.5];
        let p = |x, y| Pos::new(x, y);
        let mut glyph = |byte_data: &[u8], origin_px: [f32;2], rot_offset: Rot, aux: Vec<(&Bindings, Pos)>| -> GlyphImageData {
            GlyphImageData {
                tex: image_quad(ctx, byte_data, origin_px),
                rot_offset,
                aux: aux.into_iter().map(|(b, pos)| (b.clone(), pos)).collect(),
            }
        };
        //Order must match GlyphImages::get
        let glyphs = vec![
            glyph(include_bytes!("../mspaintart/animismus.png"), ORIGIN, 0,
                vec![(&anim3, p(0,0)), (&anim3, p(1,0)), (&anim1, p(0,1)), (&anim2, p(1,-1))]),
            glyph(include_bytes!("../mspaintart/bonding.png"), ORIGIN, 0, vec![]),
            glyph(include_bytes!("../mspaintart/calcification.png"), ORIGIN, 0,
                vec![(&card1, p(0,0))]),
            glyph(include_bytes!("../mspaintart/dispersion.png"), ORIGIN_TOP, 0,
                vec![(&disp5, p(0,0)), (&disp4, p(1,0)), (&disp3, p(1,-1)), (&disp2, p(0,-1)), (&disp1, p(-1,0))]),
            glyph(include_bytes!("../mspaintart/disposal.png"), ORIGIN_CENTER, 0, vec![]),
            glyph(include_bytes!("../mspaintart/duplication.png"), ORIGIN, 0,
                vec![(&card1, p(0,0)), (&salt1, p(1,0))]),
            glyph(include_bytes!("../mspaintart/equilibrium.png"), ORIGIN, 0, vec![]),
            //art is drawn mirrored from the sim's layout, rotating by 60 degrees lines it up
            glyph(include_bytes!("../mspaintart/multibonding.png"), ORIGIN_CENTER, 1, vec![]),
            glyph(include_bytes!("../mspaintart/projection.png"), ORIGIN, 0,
                vec![(&quint1, p(0,0)), (&metal1, p(1,0))]),
            glyph(include_bytes!("../mspaintart/purification.png"), ORIGIN, 0,
                vec![(&metal2, p(0,0)), (&metal2, p(1,0)), (&metal3, p(0,1))]),
            glyph(include_bytes!("../mspaintart/triplex.png"), ORIGIN, 0,
                vec![(&triplex1, p(0,0)), (&triplex1, p(1,0)), (&triplex1, p(0,1))]),
            glyph(include_bytes!("../mspaintart/unbonding.png"), ORIGIN, 0, vec![]),
            glyph(include_bytes!("../mspaintart/unification.png"), ORIGIN, 0,
                vec![(&unif2, p(0,0)), (&unif1, p(0,1)), (&unif1, p(-1,1)), (&unif1, p(0,-1)), (&unif1, p(1,-1))]),
            glyph(include_bytes!("../mspaintart/rejection.png"), ORIGIN, 0,
                vec![(&metal1, p(0,0)), (&quint2, p(1,0))]),
            glyph(include_bytes!("../mspaintart/division.png"), ORIGIN_CENTER, 0,
                vec![(&metal2, p(0,0)), (&div1, p(1,0)), (&div2, p(-1,0))]),
            glyph(include_bytes!("../mspaintart/proliferation.png"), ORIGIN, 0,
                vec![(&metal1, p(-1,1)), (&quint1, p(1,1)), (&metal3, p(1,-1))]),
        ];
        GlyphImages { glyphs }
    }
    pub fn get(&self, glyph_type: &GlyphType) -> Option<&GlyphImageData> {
        use GlyphType::*;
        let i = match glyph_type{
            Animismus       => 0,
            Bonding         => 1,
            Calcification   => 2,
            Dispersion      => 3,
            Disposal        => 4,
            Duplication     => 5,
            Equilibrium     => 6,
            MultiBond       => 7,
            Projection      => 8,
            Purification    => 9,
            TriplexBond     => 10,
            Unbonding       => 11,
            Unification     => 12,
            Rejection       => 13,
            Division        => 14,
            Proliferation   => 15,
            Input(..) | Output(..) | OutputRepeating(..) | Conduit(..) | Track(..) => return None,
        };
        Some(&self.glyphs[i])
    }
}
