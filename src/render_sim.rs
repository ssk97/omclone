use miniquad::*;
use crate::sim::*;

use crate::render_library::*;
use crate::render_glyphs::*;

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

pub struct TrackBindings{
    bindings: Bindings,
    vert_count: usize,
}
pub fn setup_tracks(ctx: &mut dyn RenderingBackend, tracks: &TrackMaps) -> TrackBindings{
    let mut verts_vec:Vec<GFXPos> = Vec::new();
    let mut index_vec:Vec<u16> = Vec::new();
    let mut curr_index = 0;
    let mut add_from = |track: &TrackMap| {
        for (&from, &offset) in track{
            let xy = pos_to_xy(from);
            verts_vec.push(xy.into());
            index_vec.push(curr_index);
            curr_index += 1;
            let outer_pos = xy + pos_to_xy_vec(offset)*0.5;
            verts_vec.push(outer_pos.into());
            index_vec.push(curr_index);
            curr_index += 1;
        }
    };
    add_from(&tracks.plus);
    add_from(&tracks.minus);

    let vb = ctx.new_buffer(BufferType::VertexBuffer,BufferUsage::Immutable, BufferSource::slice(&verts_vec));
    let index_buffer = ctx.new_buffer(BufferType::IndexBuffer,BufferUsage::Immutable, BufferSource::slice(&index_vec));
    TrackBindings{
        bindings: Bindings {
            vertex_buffers: vec![vb],
            index_buffer,
            images: vec![],
        },
        vert_count: index_vec.len()
    }
}

const HEX_GRID_ID:usize = 0;
const SHADE_INOUT_ID:usize = 1;
const SHADE_AREA_ID:usize = 2;
const BOND_NORMAL_ID:usize = 3;
const BOND_RED_ID:usize = 4;
const BOND_WHITE_ID:usize = 5;
const BOND_YELLOW_ID:usize = 6;
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
struct ShapeStore{
    arm_bindings: Bindings,
    circle_bindings: Bindings,
    texture_bindings: Vec<Bindings>,
    glyph_images: GlyphImages,
}
//scale_base is in units of (SCALE_REF_PX/2) pixels per world unit, independent of window size
const SCALE_REF_PX: f32 = 1000.;
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
    #[allow(dead_code)]//not used for mouse picking yet
    pub fn screen_to_world(&self, px: (f32, f32), screen_size: (f32, f32)) -> XYPos {
        let scale = self.scale(screen_size);
        let ndc_x = px.0/screen_size.0*2. - 1.;
        let ndc_y = 1. - px.1/screen_size.1*2.;
        XYPos::new(ndc_x/scale.0, ndc_y/scale.1) - self.offset
    }
    pub fn frame_center(world: &World, screen_size:(f32,f32)) -> Self{
        let pos_list = world.glyphs.iter().map(|x| pos_to_xy(x.pos));
        let (mut lowx, mut lowy, mut highx, mut highy) = pos_list.fold(
            (f32::INFINITY,f32::INFINITY,f32::NEG_INFINITY,f32::NEG_INFINITY),
            |(lowx, lowy, highx, highy), this| {
                let (thisx, thisy) = (this.x, this.y);
                let new_lowx = if thisx < lowx {thisx} else {lowx};
                let new_highx = if thisx > highx {thisx} else {highx};
                let new_lowy = if thisy < lowy {thisy} else {lowy};
                let new_highy = if thisy > highy {thisy} else {highy};
                (new_lowx, new_lowy, new_highx, new_highy)
            }
        );
        const BORDER: f32 = 6.;
        lowx -= BORDER;
        lowy -= BORDER;
        highx+= BORDER;
        highy+= BORDER;
        let offset = XYVec::new(-(lowx+highx)/2., -(lowy+highy)/2.);
        let world_width_scale = screen_size.0/SCALE_REF_PX*2./(highx-lowx);
        let world_height_scale = screen_size.1/SCALE_REF_PX*2./(highy-lowy);
        let scale_base = if world_width_scale < world_height_scale {world_width_scale} else {world_height_scale};
        CameraSetup {scale_base, offset}
    }
}
pub struct RenderDataBase {
    pipeline: Pipeline,
    pipeline_textured: Pipeline,
    pipeline_tracks: Pipeline,
    shapes: ShapeStore,
    font: FontStorage
}

impl RenderDataBase {
    pub fn new(ctx: &mut dyn RenderingBackend) -> Self {
        let shader_meta = ShaderMeta {
            images: vec![],
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
        const V_SHADE: &str = include_str!("basic_vert.vs");
        const F_SHADE: &str = include_str!("basic_frag.fs");
        
        let shader = ctx.new_shader(ShaderSource::Glsl { vertex: V_SHADE, fragment: F_SHADE } , shader_meta).unwrap();

        let pipeline = ctx.new_pipeline(
            &[BufferLayout::default()],
            &[VertexAttribute::new("local_pos", VertexFormat::Float2)],
            shader, Default::default()
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
            PipelineParams{
                color_blend: Some(BlendState::new(
                    Equation::Add,
                    BlendFactor::Value(BlendValue::SourceAlpha),
                    BlendFactor::OneMinusValue(BlendValue::SourceAlpha))
                ),
                ..Default::default()
            }
        );
        let pipeline_tracks = ctx.new_pipeline(
            &[BufferLayout::default()],
            &[VertexAttribute::new("local_pos", VertexFormat::Float2)],
            shader,
            PipelineParams{
                primitive_type:PrimitiveType::Lines,
                ..Default::default()
            }
        );
        let shapes = ShapeStore{
            arm_bindings: setup_arms(ctx),
            circle_bindings: setup_circle(ctx),
            texture_bindings: setup_textures(ctx),
            glyph_images: GlyphImages::new(ctx),
        };
        
        let font = FontStorage::new(ctx);

        Self {
            pipeline,pipeline_textured,pipeline_tracks,shapes,font
        }
    }
}

fn atom_color(t: AtomType) -> [f32;3]{
    use AtomType::*;
    match t{
        Salt  => [0.8, 0.8, 0.8],
        Air   => [0., 1., 1.],
        Earth => [0., 1., 0.],
        Fire  => [1., 0., 0.],
        Water => [0., 0., 1.],
        Vitae => [1., 0.6, 0.6],
        Mors  => [0.4, 0., 0.],
        Quicksilver => [1.,1.,1.],
        Gold => [1., 0.85, 0.2],
        Silver => [0.35, 0.3, 0.4],
        Copper => [0.8, 0.4, 0.1],
        Iron => [0.35, 0.25, 0.20],
        Tin => [0.4, 0.4, 0.2],
        Lead => [0.3, 0.3, 0.3],
        Quintessence => {
            let t = ((miniquad::date::now()/2.).fract() as f32)*PI*2.;
            let colorize = |o:f32|->f32 {
                (t+o).sin().max(0.)
            };
            [colorize(0.),colorize(PI*2./3.),colorize(-PI*2./3.)]
        },
        RepeatingOutputMarker | ConduitSpace => [0., 0., 0.],
    }
}
//Dark symbols on light atoms, light symbols on dark atoms
fn symbol_color(fill: [f32;3]) -> [f32;3]{
    let luminance = 0.299*fill[0] + 0.587*fill[1] + 0.114*fill[2];
    if luminance > 0.7 {[0.1, 0.1, 0.1]} else {[0.95, 0.95, 0.95]}
}

impl RenderDataBase {
	fn draw_atoms(&self, ctx: &mut Context, atoms:&[FloatAtom], camera: &CameraSetup){
        let scale = camera.scale(window::screen_size());
        let world_offset: GFXPos = camera.offset.into();
        ctx.apply_pipeline(&self.pipeline_textured);

        //Draw atom bonds
        let atoms_copy = atoms;
		for atom in atoms_copy {
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
    //note: assumes ctx is in the middle of a render pass
    pub fn draw(&self, ctx: &mut Context, camera: &CameraSetup, tracks: &TrackBindings,
         show_area: bool, world: &World, float_world: &FloatWorld)
    {
        let scale = camera.scale(window::screen_size());
        let world_offset: GFXPos = camera.offset.into();


        //Draw input/output atoms
        let mut temp_atoms_vec:Vec<FloatAtom> = Vec::new();
        for glyph in world.glyphs.iter(){
            use GlyphType::*;
            temp_atoms_vec.clear();
            match &glyph.glyph_type{
                Input(atoms_meta,_)
                | Output(atoms_meta,_,_) 
                | OutputRepeating(atoms_meta,_,_)=> {
                    for atom in &atoms_meta[0]{
                        temp_atoms_vec.push(atom.into());
                    }
					self.draw_atoms(ctx, &temp_atoms_vec, camera);
                },
                _ => continue,
            };
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
        //Draw glyphs (including half-transparent cover for input/outputs)
        ctx.apply_pipeline(&self.pipeline_textured);
        for glyph in world.glyphs.iter(){
            let angle = rot_to_angle(glyph.rot);
            use GlyphType::*;
            match &glyph.glyph_type{
                Input(atoms_meta,_) | Output(atoms_meta,_,_) | OutputRepeating(atoms_meta,_,_)=> {
                    let atoms = &atoms_meta[0];
                    ctx.apply_bindings(&self.shapes.texture_bindings[SHADE_INOUT_ID]);
                    for atom in atoms{ //transparent cover
                        let offset = pos_to_xy(atom.pos).into();
                        ctx.apply_uniforms(UniformsSource::table(&UvUniforms {
                            offset, world_offset, angle, scale
                        }));
                        ctx.draw(0, 6, 1);
                    }
                    continue
                },
                Conduit(pos_vec,_) => {
                    ctx.apply_bindings(&self.shapes.texture_bindings[SHADE_INOUT_ID]);
                    for p in pos_vec{
                        let offset = pos_to_xy(*p).into();
                        ctx.apply_uniforms(UniformsSource::table(&UvUniforms {
                            offset, world_offset, angle, scale
                        }));
                        ctx.draw(0, 6, 1);
                    }
                    continue
                },
                Track(_) => {},
                _ => if let Some(glyph_image) = self.shapes.glyph_images.get(&glyph.glyph_type){
                    glyph_image.draw(ctx, glyph, world_offset, scale);
                },
            };
        }
        //Draw conduit numbers
        self.font.set_pipeline(ctx);
        for glyph in world.glyphs.iter(){
            if let GlyphType::Conduit(_, id) = glyph.glyph_type{
                let offset = pos_to_xy(glyph.pos);
                let string = id.to_string();
                self.font.render_text_centered(ctx, &string, [1., 1., 1.], offset, world_offset, scale);
            }
        }
        //draw area cover
        if show_area{
            ctx.apply_pipeline(&self.pipeline_textured);
            ctx.apply_bindings(&self.shapes.texture_bindings[SHADE_AREA_ID]);                
            for p in &world.area_touched{
                let offset = pos_to_xy(*p).into();
                ctx.apply_uniforms(UniformsSource::table(&UvUniforms {
                    offset, world_offset, angle:0., scale
                }));
                ctx.draw(0, 6, 1);
            }
        }
        
        //Draw tracks
        ctx.apply_pipeline(&self.pipeline_tracks);
        ctx.apply_bindings(&tracks.bindings);
        ctx.apply_uniforms(UniformsSource::table(&BasicUniforms {
            color:[1., 1., 1.], offset:[0.,0.], world_offset, angle:0., scale
        }));
        ctx.draw(0, tracks.vert_count as i32, 1);
		
		//Draw atoms
        let atoms_slice = &float_world.atoms_xy[..];
		self.draw_atoms(ctx, atoms_slice, camera);
		
        //Draw arms
        ctx.apply_pipeline(&self.pipeline);
        ctx.apply_bindings(&self.shapes.arm_bindings);
        for f_arm in float_world.arms_xy.iter() {
            let color = [0., 0., 0.];
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
        self.font.set_pipeline(ctx);
        for (num, f_arm) in float_world.arms_xy.iter().enumerate() {
            let string = (num+1).to_string();
            self.font.render_text_centered(ctx, &string, [1., 1., 1.], f_arm.pos, world_offset, scale);
        }
    }
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
            assert!((back - world).norm() < 1e-3, "{world:?} -> {px:?} -> {back:?}");
        }
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
