use crate::render_library::*;
use miniquad::*;
use crate::sim::*;
use crate::render_sim::*;


pub struct GlyphImageData {
    pub tex: Bindings,
    pub aux: Vec<(Bindings, Pos)>,
}

impl GlyphImageData {
    fn setup() -> GlyphImageData {
        return GlyphImageData { tex: (), aux: () };
    }
    pub fn draw(&self, ctx: &mut Context, glyph: &Glyph) {
        let offset = pos_to_xy(&glyph.pos);
        let angle = rot_to_angle(glyph.rot);
        for (binding, pos) in self.aux.iter(){
            let rot_pos = rotate(*pos, glyph.rot);
        }
    }
}