use miniquad::*;
use crate::sim::*;

use crate::render_library::*;
use crate::render_glyphs::*;
use rustc_hash::{FxHashMap, FxHashSet};

use std::f32::consts::PI;

//note: 1 hex has inner radius of 1 (width of 2), see sim/coords.rs
fn setup_arms(ctx: &mut dyn RenderingBackend) -> Bindings{
    const ARM_VERT_BUF: [Vert;14] = [
        //Arm Base
        [ 0.,-0.4,],
        [ 0., 0.4,],
        //Arm edge
        [ 2., 0.,],
        [ 4., 0.,],
        [ 6., 0.,],
        //Grab markers
        [ 1.8, 0.,],
        [ 2.2, -0.4,],
        [ 2.2, 0.4,],
        [ 3.8, 0.,],
        [ 4.2, -0.4,],
        [ 4.2, 0.4,],
        [ 5.8, 0.,],
        [ 6.2, -0.4,],
        [ 6.2, 0.4,],
        ];
    //First triangle is arm, 2nd triangle is optional grab marker
    const ARM_INDEX_BUF: [u16;18] = [
        0, 1, 2,    5, 6, 7,
        0, 1, 3,    8, 9, 10,
        0, 1, 4,    11, 12, 13,];
    let vb = ctx.new_buffer(BufferType::VertexBuffer,BufferUsage::Immutable, BufferSource::slice(&ARM_VERT_BUF));
    let index_buffer = ctx.new_buffer(BufferType::IndexBuffer,BufferUsage::Immutable, BufferSource::slice(&ARM_INDEX_BUF));
    Bindings {
        vertex_buffers: vec![vb],
        index_buffer,
        images: vec![],
    }
}

//The tracks as line segments (pairs of vertices): from each hex halfway to where Forward and Back move an arm.
//A hex no track leads to or from (like a one-hex track) gets a small star, so it can be seen.
pub fn track_lines(tracks: &TrackMaps) -> Vec<GFXPos>{
    let mut verts = Vec::new();
    for map in [&tracks.plus, &tracks.minus]{
        for (&from, &offset) in map{
            let xy = pos_to_xy(from);
            verts.push(xy.into());
            verts.push((xy + pos_to_xy_vec(offset)*0.5).into());
        }
    }
    for &hex in tracks.paths.iter().flat_map(|p| &p.hexes){
        if tracks.plus.contains_key(&hex) || tracks.minus.contains_key(&hex){
            continue;
        }
        let xy = pos_to_xy(hex);
        for r in 0..3{
            let arm = pos_to_xy_vec(rot_to_pos(r))*0.2;
            verts.push((xy + arm).into());
            verts.push((xy - arm).into());
        }
    }
    verts
}
//Lines that can change every frame, drawn from a buffer that grows when they need more room
struct LineBuffer{
    bindings: Bindings,
    capacity: usize,
    count: usize,
}
impl LineBuffer{
    //As many vertices as 16-bit indices can reach (WebGL1 may not have 32-bit ones)
    const MAX_VERTS: usize = u16::MAX as usize + 1;
    fn new(ctx: &mut dyn RenderingBackend, capacity: usize) -> Self{
        let vb = ctx.new_buffer(BufferType::VertexBuffer, BufferUsage::Stream, BufferSource::empty::<GFXPos>(capacity));
        let indices: Vec<u16> = (0..capacity).map(|i| i as u16).collect();
        let index_buffer = ctx.new_buffer(BufferType::IndexBuffer, BufferUsage::Immutable, BufferSource::slice(&indices));
        LineBuffer{bindings: Bindings{vertex_buffers: vec![vb], index_buffer, images: vec![]}, capacity, count: 0}
    }
    //Replaces the lines. Any past MAX_VERTS aren't drawn.
    fn set(&mut self, ctx: &mut dyn RenderingBackend, verts: &[GFXPos]){
        let verts = &verts[..verts.len().min(Self::MAX_VERTS)];
        if verts.len() > self.capacity{
            ctx.delete_buffer(self.bindings.vertex_buffers[0]);
            ctx.delete_buffer(self.bindings.index_buffer);
            *self = LineBuffer::new(ctx, verts.len().next_power_of_two().min(Self::MAX_VERTS));
        }
        if !verts.is_empty(){
            ctx.buffer_update(self.bindings.vertex_buffers[0], BufferSource::slice(verts));
        }
        self.count = verts.len();
    }
}

const HEX_GRID_ID:usize = 0;
const SHADE_INOUT_ID:usize = 1;
const SHADE_AREA_ID:usize = 2;
const BOND_NORMAL_ID:usize = 3;
const BOND_RED_ID:usize = 4;
const BOND_WHITE_ID:usize = 5;
const BOND_YELLOW_ID:usize = 6;
const ORIGIN_COLOR: Color = [0.3, 0.3, 0.3, 1.];
const WHITE: Color = [1., 1., 1., 1.];
fn setup_textures(ctx: &mut dyn RenderingBackend) -> Vec<Bindings>{
    const HL:f32 = SQRT3*5./3.;//top edge of the 6x6 quad, so the image's hex lands on (0,0)
    const TEXTURE_VERT_BUF: [UvVert;4] = [
        [-3.,HL-6.,  0., 1.],
        [-3., HL,    0., 0.],
        [ 3.,HL-6.,  1., 1.],
        [ 3., HL,    1., 0.]];
    const TEXTURE_INDEX_BUF: [u16;6] = [
        0, 1, 2,
        1, 2, 3];
    let vb = ctx.new_buffer(BufferType::VertexBuffer,BufferUsage::Immutable, BufferSource::slice(&TEXTURE_VERT_BUF));
    let index_buffer = ctx.new_buffer(BufferType::IndexBuffer,BufferUsage::Immutable, BufferSource::slice(&TEXTURE_INDEX_BUF));
    let texture_list:Vec<&[u8]> = vec![
        include_bytes!("../images/HexGridTiling.png"),
        include_bytes!("../images/ShadeAtomInOut.png"),
        include_bytes!("../images/ShadeAreaFill.png"),
        include_bytes!("../images/BondNormal.png"),
        include_bytes!("../images/BondRed.png"),
        include_bytes!("../images/BondWhite.png"),
        include_bytes!("../images/BondYellow.png"),
    ];
    texture_list.iter().enumerate().map(|(id, byte_data)| -> Bindings{
        let wrap = if id == HEX_GRID_ID {TextureWrap::Repeat} else {TextureWrap::Clamp};
        let (texture, _, _) = load_texture(ctx, byte_data, wrap);
        if id == HEX_GRID_ID{
            let tmp_vb = ctx.new_buffer(BufferType::VertexBuffer, BufferUsage::Stream, 
                BufferSource::empty::<UvVert>(4));
            Bindings {
                vertex_buffers: vec![tmp_vb],
                index_buffer,
                images:vec![texture],
            }
        } else {
            Bindings {
                vertex_buffers: vec![vb],
                index_buffer,
                images:vec![texture],
            }
        }
    }).collect()
}
const CIRCLE_VERT_COUNT:usize = 20;
fn setup_circle(ctx: &mut dyn RenderingBackend) -> Bindings{
    let mut verts = [[0.;2];CIRCLE_VERT_COUNT+1];
    let angle_per = PI*2./(CIRCLE_VERT_COUNT as f32);
    //verts[CIRCLE_VERT_COUNT] = [0.,0.];
    let mut indices: [u16; CIRCLE_VERT_COUNT*3] = [CIRCLE_VERT_COUNT as u16;CIRCLE_VERT_COUNT*3];
    for i in 0..CIRCLE_VERT_COUNT{
        let angle = (i as f32)*angle_per;
        verts[i] = [angle.cos()*0.8,angle.sin()*0.8];
        indices[i*3+1] = (i) as u16;
        indices[i*3+2] = ((i+1)%CIRCLE_VERT_COUNT) as u16;
    }
    let vertex_buffer = ctx.new_buffer(BufferType::VertexBuffer,BufferUsage::Immutable, BufferSource::slice(&verts));
    let index_buffer = ctx.new_buffer(BufferType::IndexBuffer,BufferUsage::Immutable, BufferSource::slice(&indices));
    Bindings {
        vertex_buffers: vec![vertex_buffer],
        index_buffer,
        images: vec![],
    }
}
//A hex's corners, scaled by `size` (1 reaches the edges of the hex grid)
fn hex_corners(size: f32) -> [Vert; 6]{
    const CORNER: f32 = 2./SQRT3;//distance to a corner, for an inner radius of 1
    std::array::from_fn(|i| {
        let angle = PI/6. + (i as f32)*PI/3.;
        let (s, c) = angle.sin_cos();
        [c*CORNER*size, s*CORNER*size]
    })
}
//The hex, out to the edge of a ring (6 triangles from its center)
fn setup_hex_fill(ctx: &mut dyn RenderingBackend) -> Bindings{
    let mut verts = [[0.;2];7];
    verts[..6].copy_from_slice(&hex_corners(0.95));
    let mut indices = [0u16;18];
    for i in 0..6{
        indices[i*3..i*3+3].copy_from_slice(&[6, i as u16, ((i+1)%6) as u16]);
    }
    let vertex_buffer = ctx.new_buffer(BufferType::VertexBuffer,BufferUsage::Immutable, BufferSource::slice(&verts));
    let index_buffer = ctx.new_buffer(BufferType::IndexBuffer,BufferUsage::Immutable, BufferSource::slice(&indices));
    Bindings {
        vertex_buffers: vec![vertex_buffer],
        index_buffer,
        images: vec![],
    }
}
//An outline just inside a hex's edges (12 triangles)
fn setup_hex_ring(ctx: &mut dyn RenderingBackend) -> Bindings{
    let mut verts = [[0.;2];12];
    verts[..6].copy_from_slice(&hex_corners(0.95));
    verts[6..].copy_from_slice(&hex_corners(0.78));
    let mut indices = [0u16;36];
    for i in 0..6{
        let next = (i+1)%6;
        let (outer, outer_next, inner, inner_next) = (i as u16, next as u16, (i+6) as u16, (next+6) as u16);
        indices[i*6..i*6+6].copy_from_slice(&[outer, outer_next, inner, inner, outer_next, inner_next]);
    }
    let vertex_buffer = ctx.new_buffer(BufferType::VertexBuffer,BufferUsage::Immutable, BufferSource::slice(&verts));
    let index_buffer = ctx.new_buffer(BufferType::IndexBuffer,BufferUsage::Immutable, BufferSource::slice(&indices));
    Bindings {
        vertex_buffers: vec![vertex_buffer],
        index_buffer,
        images: vec![],
    }
}
struct ShapeStore{
    hex_ring_bindings: Bindings,
    hex_fill_bindings: Bindings,
    arm_bindings: Bindings,
    circle_bindings: Bindings,
    texture_bindings: Vec<Bindings>,
    glyph_images: GlyphImages,
}
//scale_base is in units of (SCALE_REF_PX/2) pixels per world unit, independent of window size
const SCALE_REF_PX: f32 = 1000.;
//How far the camera zooms out and in (scale_base)
pub const ZOOM_RANGE: std::ops::RangeInclusive<f32> = 0.002..=0.2;
//The window size in logical pixels. The camera and the board's mouse positions use these, so the board looks
//the same at any display scale.
pub fn logical_screen_size() -> (f32, f32){
    let (w, h) = window::screen_size();
    let dpi = window::dpi_scale();
    (w/dpi, h/dpi)
}
pub struct CameraSetup{
    pub scale_base: f32,
    pub offset: XYVec,
}
impl CameraSetup{
    //Clip-space units per world unit, as passed to the shaders
    pub fn scale(&self, screen_size:(f32,f32)) -> (f32, f32) {
        (SCALE_REF_PX/screen_size.0*self.scale_base,SCALE_REF_PX/screen_size.1*self.scale_base)
    }
    pub fn px_per_world(&self) -> f32 {
        self.scale_base*SCALE_REF_PX/2.
    }
    //Moves the view by a mouse movement in screen pixels (y down)
    pub fn pan_pixels(&mut self, dx: f32, dy: f32) {
        self.offset += XYVec::new(dx, -dy)/self.px_per_world();
    }
    //The world position under a screen pixel (y down, origin top-left)
    pub fn screen_to_world(&self, px: (f32, f32), screen_size: (f32, f32)) -> XYPos {
        let scale = self.scale(screen_size);
        let ndc_x = px.0/screen_size.0*2. - 1.;
        let ndc_y = 1. - px.1/screen_size.1*2.;
        XYPos::new(ndc_x/scale.0, ndc_y/scale.1) - self.offset
    }
    //Zooms in by `notches` of the mouse wheel (out if negative), keeping the world position under the pixel `px`
    //where it is. A view already outside ZOOM_RANGE can still move back into it.
    pub fn zoom_at(&mut self, px: (f32, f32), screen_size: (f32, f32), notches: f32) {
        let before = self.screen_to_world(px, screen_size);
        let (min, max) = (ZOOM_RANGE.start().min(self.scale_base), ZOOM_RANGE.end().max(self.scale_base));
        self.scale_base = (self.scale_base*1.1f32.powf(notches)).clamp(min, max);
        self.offset += self.screen_to_world(px, screen_size) - before;
    }
    //Fits every glyph's hexes (tracks included, but only a repeating output's own hex) and everywhere the arms can
    //reach, with a border around them
    pub fn frame_center(world: &World, screen_size:(f32,f32)) -> Self{
        //Without its polymer, a repeating output's footprint is empty: its display runs far past the rest
        let glyph_hexes = world.glyphs.iter().flat_map(|g| g.footprint(None).into_iter().chain([g.pos])).map(pos_to_xy);
        //Grabbers can turn to any angle, so an arm reaches anywhere in a square around its base (2 world units a hex)
        let arm_reach = world.arms.iter().flat_map(|arm| {
            let (base, reach) = (pos_to_xy(arm.pos), XYVec::repeat(arm.len as f32*2.));
            [base - reach, base + reach]
        });
        let points: Vec<XYPos> = glyph_hexes.chain(arm_reach).collect();
        let border = XYVec::repeat(6.);
        let low = points.iter().fold(XYPos::new(f32::INFINITY, f32::INFINITY), |a, b| a.inf(b)) - border;
        let high = points.iter().fold(XYPos::new(f32::NEG_INFINITY, f32::NEG_INFINITY), |a, b| a.sup(b)) + border;
        let size = high - low;
        let offset = -(low.coords + high.coords)/2.;
        let scale_base = (screen_size.0/size.x).min(screen_size.1/size.y)*2./SCALE_REF_PX;
        CameraSetup {scale_base, offset}
    }
}
pub struct RenderDataBase {
    pipeline: Pipeline,
    pipeline_textured: Pipeline,
    pipeline_tracks: Pipeline,
    shapes: ShapeStore,
    font: FontStorage,
    //The board's tracks, and a track being dragged or reshaped
    scene_tracks: LineBuffer,
    dragged_track: LineBuffer,
}

impl RenderDataBase {
    pub fn new(ctx: &mut dyn RenderingBackend) -> Self {
        let shader_meta = ShaderMeta {
            images: vec![],
            uniforms: UniformBlockLayout {
                uniforms: vec![
                    UniformDesc::new("color", UniformType::Float4),
                    UniformDesc::new("offset", UniformType::Float2),
                    UniformDesc::new("world_offset", UniformType::Float2),
                    UniformDesc::new("angle", UniformType::Float1),
                    UniformDesc::new("scale", UniformType::Float2),
                    ],
            },
        };
        const V_SHADE: &str = include_str!("basic_vert.vs");
        const F_SHADE: &str = include_str!("basic_frag.fs");
        
        let shader = ctx.new_shader(ShaderSource::Glsl { vertex: V_SHADE, fragment: F_SHADE } , shader_meta).unwrap();

        let pipeline = ctx.new_pipeline(
            &[BufferLayout::default()],
            &[VertexAttribute::new("local_pos", VertexFormat::Float2)],
            shader, alpha_blend()
        );
        
        let shader_meta_uv = ShaderMeta {
            images: vec!["tex".to_string()],
            uniforms: UniformBlockLayout {
                uniforms: vec![
                    UniformDesc::new("offset", UniformType::Float2),
                    UniformDesc::new("world_offset", UniformType::Float2),
                    UniformDesc::new("angle", UniformType::Float1),
                    UniformDesc::new("scale", UniformType::Float2),
                    ],
            },
        };
        const V_UV_SHADE: &str = include_str!("uv_vert.vs");
        const F_UV_SHADE: &str = include_str!("uv_frag.fs");
        let shader_uv = ctx.new_shader(ShaderSource::Glsl { vertex: V_UV_SHADE, fragment: F_UV_SHADE } , shader_meta_uv).unwrap();
        use miniquad::graphics::*;
        let pipeline_textured = ctx.new_pipeline(
            &[BufferLayout::default()],
            &[
                VertexAttribute::new("local_pos", VertexFormat::Float2),
                VertexAttribute::new("uv", VertexFormat::Float2),
            ],
            shader_uv,
            alpha_blend()
        );
        let pipeline_tracks = ctx.new_pipeline(
            &[BufferLayout::default()],
            &[VertexAttribute::new("local_pos", VertexFormat::Float2)],
            shader,
            PipelineParams{
                primitive_type:PrimitiveType::Lines,
                ..alpha_blend()
            }
        );
        let shapes = ShapeStore{
            hex_ring_bindings: setup_hex_ring(ctx),
            hex_fill_bindings: setup_hex_fill(ctx),
            arm_bindings: setup_arms(ctx),
            circle_bindings: setup_circle(ctx),
            texture_bindings: setup_textures(ctx),
            glyph_images: GlyphImages::new(ctx),
        };
        
        let font = FontStorage::new(ctx);
        let scene_tracks = LineBuffer::new(ctx, 1024);
        let dragged_track = LineBuffer::new(ctx, 256);

        Self {
            pipeline,pipeline_textured,pipeline_tracks,shapes,font,scene_tracks,dragged_track
        }
    }
}

fn atom_color(t: AtomType) -> Color{
    use AtomType::*;
    match t{
        Salt  => [0.8, 0.8, 0.8, 1.],
        Air   => [0., 1., 1., 1.],
        Earth => [0., 1., 0., 1.],
        Fire  => [1., 0., 0., 1.],
        Water => [0., 0., 1., 1.],
        Vitae => [1., 0.6, 0.6, 1.],
        Mors  => [0.4, 0., 0., 1.],
        Quicksilver => [1.,1.,1., 1.],
        Gold => [1., 0.85, 0.2, 1.],
        Silver => [0.35, 0.3, 0.4, 1.],
        Copper => [0.8, 0.4, 0.1, 1.],
        Iron => [0.35, 0.25, 0.20, 1.],
        Tin => [0.4, 0.4, 0.2, 1.],
        Lead => [0.3, 0.3, 0.3, 1.],
        Quintessence => {
            let t = ((miniquad::date::now()/2.).fract() as f32)*PI*2.;
            let colorize = |o:f32|->f32 {
                (t+o).sin().max(0.)
            };
            [colorize(0.),colorize(PI*2./3.),colorize(-PI*2./3.), 1.]
        },
        RepeatingOutputMarker | ConduitSpace => [0., 0., 0., 1.],
    }
}
//Dark symbols on light atoms, light symbols on dark atoms
fn symbol_color(fill: Color) -> Color{
    let luminance = 0.299*fill[0] + 0.587*fill[1] + 0.114*fill[2];
    if luminance > 0.7 {[0.1, 0.1, 0.1, 1.]} else {[0.95, 0.95, 0.95, 1.]}
}

impl RenderDataBase {
	fn draw_atoms(&self, ctx: &mut Context, atoms:&[FloatAtom], world_offset: GFXPos, scale: (f32, f32)){
        ctx.apply_pipeline(&self.pipeline_textured);

        //Draw atom bonds
		for atom in atoms {
			let offset = atom.pos.into();
			for r in 0..3 { //4-6 are done by the other atom
				let matches = [
					(Bonds::NORMAL, &self.shapes.texture_bindings[BOND_NORMAL_ID]),
					(Bonds::TRIPLEX_R, &self.shapes.texture_bindings[BOND_RED_ID]),
					(Bonds::TRIPLEX_K, &self.shapes.texture_bindings[BOND_WHITE_ID]),
					(Bonds::TRIPLEX_Y, &self.shapes.texture_bindings[BOND_YELLOW_ID])];
				let bond = atom.connections[r];
				for (bondtype, bindtype) in matches{
					if bond.intersects(bondtype){
						let angle = rot_to_angle(r as Rot)+atom.rot;
						ctx.apply_bindings(bindtype);
						ctx.apply_uniforms(UniformsSource::table(&UvUniforms {
							offset, world_offset, angle, scale
						}));
						ctx.draw(0, 6, 1);
					}
				}
			}
		}
		//Draw atom circles
		ctx.apply_pipeline(&self.pipeline);
		ctx.apply_bindings(&self.shapes.circle_bindings);
		for atom in atoms {
			let color = atom_color(atom.atom_type);
			let offset = atom.pos.into();
			let angle = 0.;
			ctx.apply_uniforms(UniformsSource::table(&BasicUniforms {
				color, offset, world_offset, angle, scale
			}));
			ctx.draw(0, (CIRCLE_VERT_COUNT*3) as i32, 1);
		}
		//Draw atom symbols
		self.font.set_pipeline(ctx);
		for atom in atoms {
			let color = symbol_color(atom_color(atom.atom_type));
			self.font.render_atom_symbol(ctx, atom.atom_type, color, atom.pos, world_offset, scale);
		}
	}
    //The atoms shown on an input or output
    fn glyph_atoms(glyph: &Glyph, polymer: Option<&Polymer>) -> Vec<FloatAtom> {
        use GlyphType::*;
        match &glyph.glyph_type{
            Input(atoms_meta,_) | Output(atoms_meta,_,_) => {
                atoms_meta[0].iter().map(|atom| (&glyph.place_atom(atom)).into()).collect()
            },
            OutputRepeating(..) => polymer.map_or(Vec::new(), |p| p.display.iter().map(|atom| atom.into()).collect()),
            _ => Vec::new(),
        }
    }
    //The glyph's image, or the half-transparent cover for inputs/outputs/conduits (textured pipeline)
    fn draw_glyph_cover(&self, ctx: &mut Context, glyph: &Glyph, polymer: Option<&Polymer>, world_offset: GFXPos, scale: (f32, f32)){
        let angle = rot_to_angle(glyph.rot);
        use GlyphType::*;
        match &glyph.glyph_type{
            Input(..) | Output(..) | OutputRepeating(..) | Conduit(..) => {
                ctx.apply_bindings(&self.shapes.texture_bindings[SHADE_INOUT_ID]);
                for p in glyph.footprint(polymer){
                    let offset = pos_to_xy(p).into();
                    ctx.apply_uniforms(UniformsSource::table(&UvUniforms {
                        offset, world_offset, angle, scale
                    }));
                    ctx.draw(0, 6, 1);
                }
            },
            Track(_) => {},
            _ => if let Some(glyph_image) = self.shapes.glyph_images.get(&glyph.glyph_type){
                glyph_image.draw(ctx, glyph, world_offset, scale);
            },
        };
    }
    //Font pipeline
    fn draw_conduit_number(&self, ctx: &mut Context, glyph: &Glyph, world_offset: GFXPos, scale: (f32, f32)){
        if let GlyphType::Conduit(_, id) = glyph.glyph_type{
            let offset = pos_to_xy(glyph.pos);
            self.font.render_text_centered(ctx, &id.to_string(), WHITE, offset, world_offset, scale);
        }
    }
    fn draw_lines(&self, ctx: &mut Context, lines: &LineBuffer, world_offset: GFXPos, scale: (f32, f32)){
        if lines.count == 0{
            return;
        }
        ctx.apply_pipeline(&self.pipeline_tracks);
        ctx.apply_bindings(&lines.bindings);
        ctx.apply_uniforms(UniformsSource::table(&BasicUniforms {
            color: WHITE, offset:[0.,0.], world_offset, angle:0., scale
        }));
        ctx.draw(0, lines.count as i32, 1);
    }
    //Basic pipeline with the arm bindings
    fn draw_arm(&self, ctx: &mut Context, f_arm: &FloatArm, world_offset: GFXPos, scale: (f32, f32)){
        let color = [0., 0., 0., 1.];
        let offset = f_arm.pos.into();
        let triangles_drawn = if f_arm.grabbing {6} else {3};
        for r in (0..6).step_by(Arm::angles_between_arm(f_arm.arm_type) as usize) {
            let angle = f_arm.rot+rot_to_angle(r);
            ctx.apply_uniforms(UniformsSource::table(&BasicUniforms {
                color, offset, world_offset, angle, scale
            }));
            //Only whole lengths 1-3 have geometry, so a piston snaps halfway through its motion
            let shown_len = f_arm.len.round().clamp(1., 3.) as i32;
            ctx.draw((shown_len-1)*6, triangles_drawn, 1);
        }
    }
    //note: assumes ctx is in the middle of a render pass
    pub fn draw(&mut self, ctx: &mut Context, camera: &CameraSetup, show_area: bool, scene: &Scene){
        let scale = camera.scale(logical_screen_size());
        let world_offset: GFXPos = camera.offset.into();
        let shown_glyphs = || scene.glyphs.iter().enumerate()
            .filter(|&(id, _)| scene.hidden != Some(PartId::Glyph(id)));

        //Draw input/output atoms
        for (id, glyph) in shown_glyphs(){
            let atoms = Self::glyph_atoms(glyph, scene.polymers.get(&id));
            if !atoms.is_empty(){
                self.draw_atoms(ctx, &atoms, world_offset, scale);
            }
        }
        //Draw the Hex grid
        if camera.scale_base > 0.01{
            let (inv_scale_x, inv_scale_y)= (1./scale.0,1./scale.1);
            let xc = (-world_offset[0]/2.-0.26).fract();
            let yc = ((-world_offset[1]/2.)/SQRT3+0.09).fract();
            let xdf = inv_scale_x/2.;
            let ydf = inv_scale_y/(2.*SQRT3);

            let hex_grid_data: [UvVert;4] = [
                [-1.,-1.,    xc-xdf, yc-ydf],
                [-1., 1.,    xc-xdf, yc+ydf],
                [ 1.,-1.,    xc+xdf, yc-ydf],
                [ 1., 1.,    xc+xdf, yc+ydf]];
            ctx.buffer_update(self.shapes.texture_bindings[HEX_GRID_ID].vertex_buffers[0], BufferSource::slice(&hex_grid_data));
            ctx.apply_pipeline(&self.pipeline_textured);
            ctx.apply_bindings(&self.shapes.texture_bindings[HEX_GRID_ID]);
            ctx.apply_uniforms(UniformsSource::table(&UvUniforms {
                offset:[0.,0.], world_offset:[0.,0.], angle:0., scale:(1.,1.)
            }));
            ctx.draw(0, 6, 1);
        }
        //Mark the origin, under any part there
        self.font.set_pipeline(ctx);
        self.font.render_text_centered(ctx, &ORIGIN_MARK.to_string(), ORIGIN_COLOR, pos_to_xy(Pos::new(0, 0)),
            world_offset, scale);
        //Draw glyphs (including half-transparent cover for input/outputs)
        ctx.apply_pipeline(&self.pipeline_textured);
        for (id, glyph) in shown_glyphs(){
            self.draw_glyph_cover(ctx, glyph, scene.polymers.get(&id), world_offset, scale);
        }
        //Draw conduit numbers
        self.font.set_pipeline(ctx);
        for (_, glyph) in shown_glyphs(){
            self.draw_conduit_number(ctx, glyph, world_offset, scale);
        }
        //draw area cover
        if show_area{
            ctx.apply_pipeline(&self.pipeline_textured);
            ctx.apply_bindings(&self.shapes.texture_bindings[SHADE_AREA_ID]);
            for p in scene.area{
                let offset = pos_to_xy(*p).into();
                ctx.apply_uniforms(UniformsSource::table(&UvUniforms {
                    offset, world_offset, angle:0., scale
                }));
                ctx.draw(0, 6, 1);
            }
        }

        self.scene_tracks.set(ctx, &track_lines(scene.tracks));
        self.draw_lines(ctx, &self.scene_tracks, world_offset, scale);

		//Draw atoms
		self.draw_atoms(ctx, &scene.float_world.atoms_xy, world_offset, scale);

        //Draw arms
        let shown_arms = || scene.float_world.arms_xy.iter().enumerate()
            .filter(|&(num, _)| scene.hidden != Some(PartId::Arm(num)));
        ctx.apply_pipeline(&self.pipeline);
        ctx.apply_bindings(&self.shapes.arm_bindings);
        for (_, f_arm) in shown_arms() {
            self.draw_arm(ctx, f_arm, world_offset, scale);
        }
        self.font.set_pipeline(ctx);
        for (num, f_arm) in shown_arms() {
            let string = (num+1).to_string();
            self.font.render_text_centered(ctx, &string, WHITE, f_arm.pos, world_offset, scale);
        }
    }
    //Draws a glyph being dragged, shifted by `extra` (world units) from its hex. `track` is its own track data.
    pub fn draw_dragged_glyph(&mut self, ctx: &mut Context, camera: &CameraSetup, glyph: &Glyph,
        polymer: Option<&Polymer>, track: Option<&TrackMaps>, extra: XYVec)
    {
        let scale = camera.scale(logical_screen_size());
        let world_offset: GFXPos = (camera.offset + extra).into();
        let atoms = Self::glyph_atoms(glyph, polymer);
        if !atoms.is_empty(){
            self.draw_atoms(ctx, &atoms, world_offset, scale);
        }
        ctx.apply_pipeline(&self.pipeline_textured);
        self.draw_glyph_cover(ctx, glyph, polymer, world_offset, scale);
        self.font.set_pipeline(ctx);
        self.draw_conduit_number(ctx, glyph, world_offset, scale);
        if let Some(track) = track{
            self.dragged_track.set(ctx, &track_lines(track));
            self.draw_lines(ctx, &self.dragged_track, world_offset, scale);
        }
    }
    //Draws an arm being dragged (in its starting state), shifted by `extra` (world units) from its hex
    pub fn draw_dragged_arm(&self, ctx: &mut Context, camera: &CameraSetup, arm: &Arm, num: usize, extra: XYVec){
        let scale = camera.scale(logical_screen_size());
        let world_offset: GFXPos = (camera.offset + extra).into();
        let f_arm = FloatArm{
            pos: pos_to_xy(arm.pos),
            rot: rot_to_angle(arm.rot),
            len: arm.len as f32,
            arm_type: arm.arm_type,
            grabbing: arm.arm_type.is_wheel(),
        };
        ctx.apply_pipeline(&self.pipeline);
        ctx.apply_bindings(&self.shapes.arm_bindings);
        self.draw_arm(ctx, &f_arm, world_offset, scale);
        self.font.set_pipeline(ctx);
        self.font.render_text_centered(ctx, &(num+1).to_string(), WHITE, f_arm.pos, world_offset, scale);
    }
    //Outlines each hex
    pub fn draw_hex_rings(&self, ctx: &mut Context, camera: &CameraSetup, hexes: &[Pos], color: Color){
        self.draw_hexes(ctx, camera, hexes, color, &self.shapes.hex_ring_bindings, 36);
    }
    //Fills each hex
    pub fn draw_hex_fills(&self, ctx: &mut Context, camera: &CameraSetup, hexes: &[Pos], color: Color){
        self.draw_hexes(ctx, camera, hexes, color, &self.shapes.hex_fill_bindings, 18);
    }
    fn draw_hexes(&self, ctx: &mut Context, camera: &CameraSetup, hexes: &[Pos], color: Color, shape: &Bindings,
        index_count: i32)
    {
        let scale = camera.scale(logical_screen_size());
        let world_offset: GFXPos = camera.offset.into();
        ctx.apply_pipeline(&self.pipeline);
        ctx.apply_bindings(shape);
        for &p in hexes{
            ctx.apply_uniforms(UniformsSource::table(&BasicUniforms {
                color, offset: pos_to_xy(p).into(), world_offset, angle: 0., scale
            }));
            ctx.draw(0, index_count, 1);
        }
    }
    //Labels the hexes around `hex` with the path text letters that step to them
    pub fn draw_step_letters(&self, ctx: &mut Context, camera: &CameraSetup, hex: Pos, color: Color){
        let scale = camera.scale(logical_screen_size());
        let world_offset: GFXPos = camera.offset.into();
        self.font.set_pipeline(ctx);
        for (r, letter) in STEP_LETTERS.iter().enumerate(){
            let at = pos_to_xy(hex + rot_to_pos(r as Rot));
            self.font.render_text_centered(ctx, &letter.to_string(), color, at, world_offset, scale);
        }
    }
}
//What to draw for the world: glyph data, plus the moving atoms and arms. `hidden` isn't drawn (it's being dragged).
pub struct Scene<'a>{
    pub glyphs: &'a [Glyph],
    pub tracks: &'a TrackMaps,
    pub polymers: &'a FxHashMap<usize, Polymer>,
    pub area: &'a FxHashSet<Pos>,
    pub float_world: &'a FloatWorld,
    pub hidden: Option<PartId>,
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn screen_to_world_inverts_shader() {
        let camera = CameraSetup { scale_base: 0.03, offset: XYVec::new(-4.5, 2.25) };
        let screen_size = (1280., 720.);
        let scale = camera.scale(screen_size);
        for world in [XYPos::new(0., 0.), XYPos::new(3., -7.5), XYPos::new(-12., 4.)] {
            //Same math as the vertex shaders, then clip space to pixels
            let ndc = (world + camera.offset).coords.component_mul(&XYVec::new(scale.0, scale.1));
            let px = ((ndc.x + 1.)/2.*screen_size.0, (1. - ndc.y)/2.*screen_size.1);
            let back = camera.screen_to_world(px, screen_size);
            assert!((back - world).norm() < 1e-3, "{world:?} -> {px:?} -> {back:?}");        }
    }
    #[test]
    fn track_lines_cover_each_move_and_lone_hexes() {
        let track = |path: &[(i32, i32)]| Glyph{pos: Pos::zeros(), rot: 0,
            glyph_type: GlyphType::Track(path.iter().map(|&(x, y)| Pos::new(x, y)).collect())};
        let lines = |path: &[(i32, i32)]| track_lines(&TrackMaps::build([&track(path)]).unwrap()).len();
        //A segment for each move either way: a loop moves on from every hex, a line not past either end
        assert_eq!(lines(&[(0, 0), (1, 0), (0, 1)]), 2*2*3);
        assert_eq!(lines(&[(0, 0), (1, 0), (2, 0)]), 2*2*2);
        //A lone hex gets a three-line star
        assert_eq!(lines(&[(5, 5)]), 2*3);
    }
    #[test]
    fn zoom_keeps_the_point_under_the_cursor() {
        let mut camera = CameraSetup { scale_base: 0.03, offset: XYVec::new(1.5, -2.) };
        let screen_size = (1280., 720.);
        let px = (900., 150.);
        let under = camera.screen_to_world(px, screen_size);
        camera.zoom_at(px, screen_size, 2.5);
        assert!(camera.scale_base > 0.03);
        let moved = camera.screen_to_world(px, screen_size);
        assert!((moved - under).norm() < 1e-3, "{under:?} != {moved:?}");
        camera.zoom_at(px, screen_size, 1000.);
        assert_eq!(camera.scale_base, *ZOOM_RANGE.end());
        camera.scale_base = 1.;
        camera.zoom_at(px, screen_size, 1.);
        assert_eq!(camera.scale_base, 1., "zooming in further from outside the range does nothing");
    }
    #[test]
    fn pan_follows_cursor() {
        let mut camera = CameraSetup { scale_base: 0.03, offset: XYVec::zeros() };
        let screen_size = (1280., 720.);
        let grabbed = camera.screen_to_world((100., 200.), screen_size);
        camera.pan_pixels(37., -12.);
        let moved = camera.screen_to_world((137., 188.), screen_size);
        assert!((moved - grabbed).norm() < 1e-3, "{grabbed:?} != {moved:?}");
    }
}
