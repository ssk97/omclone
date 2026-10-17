use miniquad::*;
use crate::sim::*;

use super::{glyphs::*, library::*};
use rustc_hash::{FxHashMap, FxHashSet};

use std::f32::consts::PI;

//note: 1 hex has inner radius of 1 (width of 2), see sim/coords.rs
//Appends the arm's triangles in world space: one from the base to each grabber's tip, and a grab marker at the tip
//that grows from nothing as the grip closes
fn arm_triangles(f_arm: &FloatArm, verts: &mut Vec<GFXPos>){
    let tip = 2.*f_arm.len;
    let g = f_arm.grip;
    let shape: [Vert; 6] = [
        [0., -0.4], [0., 0.4], [tip, 0.],
        [tip-0.2*g, 0.], [tip+0.2*g, -0.4*g], [tip+0.2*g, 0.4*g],
    ];
    let shown = if g > 0. {&shape[..]} else {&shape[..3]};
    for r in (0..6).step_by(Arm::angles_between_arm(f_arm.arm_type) as usize) {
        let rotation = nalgebra::Rotation2::new(f_arm.rot+rot_to_angle(r));
        for &[x, y] in shown{
            verts.push((f_arm.pos + rotation*XYVec::new(x, y)).into());
        }
    }
}
//The triangles of an arm drawn on its own, at rest on its hex (wheels hold their atoms, other arms are open)
fn lone_arm_triangles(arm: &Arm) -> Vec<GFXPos>{
    let f_arm = FloatArm{
        pos: pos_to_xy(arm.pos),
        rot: rot_to_angle(arm.rot),
        len: arm.len as f32,
        arm_type: arm.arm_type,
        grip: steady_grip(arm.arm_type.is_wheel()),
    };
    let mut verts = Vec::new();
    arm_triangles(&f_arm, &mut verts);
    verts
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
//Vertices that can change every frame (line pairs or triangles, drawn in order), from a buffer that grows when they
//need more room
struct VertexStream{
    bindings: Bindings,
    capacity: usize,
    count: usize,
}
impl VertexStream{
    //As many vertices as 16-bit indices can reach (WebGL1 may not have 32-bit ones)
    const MAX_VERTS: usize = u16::MAX as usize + 1;
    fn new(ctx: &mut dyn RenderingBackend, capacity: usize) -> Self{
        let vb = ctx.new_buffer(BufferType::VertexBuffer, BufferUsage::Stream, BufferSource::empty::<GFXPos>(capacity));
        let indices: Vec<u16> = (0..capacity).map(|i| i as u16).collect();
        let index_buffer = ctx.new_buffer(BufferType::IndexBuffer, BufferUsage::Immutable, BufferSource::slice(&indices));
        VertexStream{bindings: Bindings{vertex_buffers: vec![vb], index_buffer, images: vec![]}, capacity, count: 0}
    }
    //Replaces the vertices. Any past MAX_VERTS aren't drawn.
    fn set(&mut self, ctx: &mut dyn RenderingBackend, verts: &[GFXPos]){
        let verts = &verts[..verts.len().min(Self::MAX_VERTS)];
        if verts.len() > self.capacity{
            ctx.delete_buffer(self.bindings.vertex_buffers[0]);
            ctx.delete_buffer(self.bindings.index_buffer);
            *self = VertexStream::new(ctx, verts.len().next_power_of_two().min(Self::MAX_VERTS));
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
const ARM_COLOR: Color = [0., 0., 0., 1.];
const WALL_COLOR: Color = [0.42, 0.38, 0.33, 1.];
//How opaque the copy of a dragged arm in its starting pose is
const ARM_START_OPACITY: f32 = 0.35;
//How opaque the board's atoms are while faded, so what's under them shows
const FADED_ATOM_OPACITY: f32 = 0.2;
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
        include_bytes!("../../images/HexGridTiling.png"),
        include_bytes!("../../images/ShadeAtomInOut.png"),
        include_bytes!("../../images/ShadeAreaFill.png"),
        include_bytes!("../../images/BondNormal.png"),
        include_bytes!("../../images/BondRed.png"),
        include_bytes!("../../images/BondWhite.png"),
        include_bytes!("../../images/BondYellow.png"),
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
//Points around a circle of this radius
fn circle_corners(radius: f32) -> [Vert; CIRCLE_VERT_COUNT]{
    std::array::from_fn(|i| {
        let angle = (i as f32)*PI*2./(CIRCLE_VERT_COUNT as f32);
        [angle.cos()*radius, angle.sin()*radius]
    })
}
//An atom's circle (triangles from its center)
fn setup_circle(ctx: &mut dyn RenderingBackend) -> Bindings{
    //The last vertex, the center, stays at (0,0)
    let mut verts = [[0.;2];CIRCLE_VERT_COUNT+1];
    verts[..CIRCLE_VERT_COUNT].copy_from_slice(&circle_corners(0.8));
    let mut indices: [u16; CIRCLE_VERT_COUNT*3] = [CIRCLE_VERT_COUNT as u16;CIRCLE_VERT_COUNT*3];
    for i in 0..CIRCLE_VERT_COUNT{
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
//The band between two polygons whose corners are at the same angles (two triangles a side, so 6 indices a corner)
fn setup_ring(ctx: &mut dyn RenderingBackend, outer: &[Vert], inner: &[Vert]) -> Bindings{
    let n = outer.len();
    let verts: Vec<Vert> = outer.iter().chain(inner).copied().collect();
    let indices: Vec<u16> = (0..n).flat_map(|i| {
        let next = (i+1)%n;
        let (outer, outer_next, inner, inner_next) = (i as u16, next as u16, (i+n) as u16, (next+n) as u16);
        [outer, outer_next, inner, inner, outer_next, inner_next]
    }).collect();
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
    circle_bindings: Bindings,
    //Just around an atom
    circle_ring_bindings: Bindings,
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
    //Puts a world position in the middle of the screen, without zooming
    pub fn center_on(&mut self, at: XYPos) {
        self.offset = -at.coords;
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
        let walls = world.chambers.walls.iter().copied().map(pos_to_xy);
        let mut points: Vec<XYPos> = glyph_hexes.chain(arm_reach).chain(walls).collect();
        //With no parts, the bounds below would be infinite
        if points.is_empty(){
            points.push(XYPos::origin());
        }
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
    scene_tracks: VertexStream,
    dragged_track: VertexStream,
    //The board's arms, an arm being dragged, and how that arm starts
    scene_arms: VertexStream,
    dragged_arm: VertexStream,
    arm_start: VertexStream,
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
                    UniformDesc::new("opacity", UniformType::Float1),
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
            //Just inside a hex's edges
            hex_ring_bindings: setup_ring(ctx, &hex_corners(0.95), &hex_corners(0.78)),
            hex_fill_bindings: setup_hex_fill(ctx),
            circle_bindings: setup_circle(ctx),
            circle_ring_bindings: setup_ring(ctx, &circle_corners(1.15), &circle_corners(0.9)),
            texture_bindings: setup_textures(ctx),
            glyph_images: GlyphImages::new(ctx),
        };
        
        let font = FontStorage::new(ctx);
        let scene_tracks = VertexStream::new(ctx, 1024);
        let dragged_track = VertexStream::new(ctx, 256);
        let scene_arms = VertexStream::new(ctx, 1024);
        let dragged_arm = VertexStream::new(ctx, 64);
        let arm_start = VertexStream::new(ctx, 64);

        Self {
            pipeline,pipeline_textured,pipeline_tracks,shapes,font,scene_tracks,dragged_track,scene_arms,dragged_arm,
            arm_start
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
    fn draw_atoms(&self, ctx: &mut Context, atoms:&[FloatAtom], world_offset: GFXPos, scale: (f32, f32), opacity: f32){
        let fade = |[r, g, b, a]: Color| [r, g, b, a*opacity];
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
                            offset, world_offset, angle, scale, opacity
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
            let color = fade(atom_color(atom.atom_type));
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
            let color = fade(symbol_color(atom_color(atom.atom_type)));
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
                        offset, world_offset, angle, scale, opacity: 1.
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
    //`pipeline` is pipeline_tracks for line pairs or pipeline for triangles
    fn draw_stream(&self, ctx: &mut Context, pipeline: &Pipeline, stream: &VertexStream, color: Color,
        world_offset: GFXPos, scale: (f32, f32))
    {
        if stream.count == 0{
            return;
        }
        ctx.apply_pipeline(pipeline);
        ctx.apply_bindings(&stream.bindings);
        ctx.apply_uniforms(UniformsSource::table(&BasicUniforms {
            color, offset:[0.,0.], world_offset, angle:0., scale
        }));
        ctx.draw(0, stream.count as i32, 1);
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
                self.draw_atoms(ctx, &atoms, world_offset, scale, 1.);
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
                offset:[0.,0.], world_offset:[0.,0.], angle:0., scale:(1.,1.), opacity: 1.
            }));
            ctx.draw(0, 6, 1);
        }
        self.draw_hex_fills(ctx, camera, scene.walls, WALL_COLOR);
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
                    offset, world_offset, angle:0., scale, opacity: 1.
                }));
                ctx.draw(0, 6, 1);
            }
        }

        self.scene_tracks.set(ctx, &track_lines(scene.tracks));
        self.draw_stream(ctx, &self.pipeline_tracks, &self.scene_tracks, WHITE, world_offset, scale);

        //Draw atoms
        let atom_opacity = if scene.faded_atoms {FADED_ATOM_OPACITY} else {1.};
        self.draw_atoms(ctx, &scene.float_world.atoms_xy, world_offset, scale, atom_opacity);

        //Draw arms
        let shown_arms = || scene.float_world.arms_xy.iter().enumerate()
            .filter(|&(num, _)| scene.hidden != Some(PartId::Arm(num)));
        let mut arm_verts = Vec::new();
        for (_, f_arm) in shown_arms() {
            arm_triangles(f_arm, &mut arm_verts);
        }
        self.scene_arms.set(ctx, &arm_verts);
        self.draw_stream(ctx, &self.pipeline, &self.scene_arms, ARM_COLOR, world_offset, scale);
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
            self.draw_atoms(ctx, &atoms, world_offset, scale, 1.);
        }
        ctx.apply_pipeline(&self.pipeline_textured);
        self.draw_glyph_cover(ctx, glyph, polymer, world_offset, scale);
        self.font.set_pipeline(ctx);
        self.draw_conduit_number(ctx, glyph, world_offset, scale);
        if let Some(track) = track{
            self.dragged_track.set(ctx, &track_lines(track));
            self.draw_stream(ctx, &self.pipeline_tracks, &self.dragged_track, WHITE, world_offset, scale);
        }
    }
    //Draws an arm being dragged (in its own state, not the board's), shifted by `extra` (world units) from its hex
    pub fn draw_dragged_arm(&mut self, ctx: &mut Context, camera: &CameraSetup, arm: &Arm, num: usize, extra: XYVec){
        self.dragged_arm.set(ctx, &lone_arm_triangles(arm));
        self.draw_lone_arm(ctx, camera, &self.dragged_arm, arm, num, extra, 1.);
    }
    //Draws faintly, on its hex, how an arm being dragged starts
    pub fn draw_arm_start(&mut self, ctx: &mut Context, camera: &CameraSetup, arm: &Arm, num: usize){
        self.arm_start.set(ctx, &lone_arm_triangles(arm));
        self.draw_lone_arm(ctx, camera, &self.arm_start, arm, num, XYVec::zeros(), ARM_START_OPACITY);
    }
    //Draws an arm from `stream` (holding its lone_arm_triangles) with its number, shifted by `extra` (world units)
    fn draw_lone_arm(&self, ctx: &mut Context, camera: &CameraSetup, stream: &VertexStream, arm: &Arm, num: usize,
        extra: XYVec, opacity: f32)
    {
        let scale = camera.scale(logical_screen_size());
        let world_offset: GFXPos = (camera.offset + extra).into();
        let fade = |[r, g, b, a]: Color| [r, g, b, a*opacity];
        self.draw_stream(ctx, &self.pipeline, stream, fade(ARM_COLOR), world_offset, scale);
        self.font.set_pipeline(ctx);
        self.font.render_text_centered(ctx, &(num+1).to_string(), fade(WHITE), pos_to_xy(arm.pos), world_offset, scale);
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
        self.draw_shapes(ctx, camera, hexes.iter().map(|&p| pos_to_xy(p)), color, shape, index_count);
    }
    //Circles a point, a little wider than an atom
    pub fn draw_ring(&self, ctx: &mut Context, camera: &CameraSetup, at: XYPos, color: Color){
        self.draw_shapes(ctx, camera, [at], color, &self.shapes.circle_ring_bindings, (CIRCLE_VERT_COUNT*6) as i32);
    }
    fn draw_shapes(&self, ctx: &mut Context, camera: &CameraSetup, at: impl IntoIterator<Item = XYPos>, color: Color,
        shape: &Bindings, index_count: i32)
    {
        let scale = camera.scale(logical_screen_size());
        let world_offset: GFXPos = camera.offset.into();
        ctx.apply_pipeline(&self.pipeline);
        ctx.apply_bindings(shape);
        for xy in at{
            ctx.apply_uniforms(UniformsSource::table(&BasicUniforms {
                color, offset: xy.into(), world_offset, angle: 0., scale
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
    //Chamber walls, in a production puzzle
    pub walls: &'a [Pos],
    pub area: &'a FxHashSet<Pos>,
    pub float_world: &'a FloatWorld,
    pub hidden: Option<PartId>,
    //Draws the moving atoms faintly (not the ones shown on inputs and outputs)
    pub faded_atoms: bool,
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
    fn pistons_reach_partway_between_hexes() {
        let piston = FloatArm{pos: XYPos::new(1., 2.), rot: 0.7, len: 1.5, arm_type: ArmType::Piston, grip: 0.};
        let mut verts = Vec::new();
        arm_triangles(&piston, &mut verts);
        let reach = verts.iter().map(|&[x, y]| (XYPos::new(x, y) - piston.pos).norm()).fold(0., f32::max);
        //Hexes are 2 world units apart
        assert!((reach - 3.).abs() < 1e-4, "reaches {reach}");
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
    fn center_on_puts_the_point_in_the_middle() {
        let mut camera = CameraSetup { scale_base: 0.03, offset: XYVec::new(1.5, -2.) };
        let screen_size = (1280., 720.);
        let point = XYPos::new(-7.25, 3.5);
        camera.center_on(point);
        let middle = camera.screen_to_world((640., 360.), screen_size);
        assert!((middle - point).norm() < 1e-3, "{middle:?} != {point:?}");
        assert_eq!(camera.scale_base, 0.03, "the zoom stays");
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
