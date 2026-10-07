use miniquad::*;
use crate::{sim::*, parser, render_sim::*, render_library::GFXPos};
use smallvec::{SmallVec, smallvec};
use std::{io::BufReader, io::prelude::*};

#[cfg(not(target_arch = "wasm32"))]
use std::{fs::File, path::Path};

#[cfg(feature = "color-eyre")]
use color_eyre::eyre::Result;
#[cfg(not(feature = "color-eyre"))]
use simple_eyre::eyre::Result;
use core::ops::Range;
struct TapeBuffer<'a>{
    tape_ref: &'a mut Tape,
    earliest_edit: &'a mut Option<usize>,
    tape_mode: bool,
    held_str: String,
}
impl TapeBuffer<'_>{
    fn record_edit(&mut self, index: usize) {
        *self.earliest_edit = Some(self.earliest_edit.map_or(index, |e| e.min(index)));
    }
}
impl AsRef<str> for TapeBuffer<'_>{
    fn as_ref(&self) -> &str {
        &self.held_str
    }
}
//Note: this implementation assumes all characters in the string are ASCII/1 byte long
//It should be fine with non-ASCII attempted inserts, which will be removed as invalid instructions
impl egui::widgets::text_edit::TextBuffer for TapeBuffer<'_>{
    fn is_mutable(&self) -> bool {
        true
    }
    fn insert_text(&mut self, text: &str, char_index: egui::text::CharIndex) -> usize {
        let char_index = char_index.0;
        let instr_mapped:Vec<Instr> = text.chars().filter_map(|x| Instr::from_char(x.to_ascii_lowercase())).collect();
        if instr_mapped.is_empty() {
            return 0;
        }
        self.record_edit(char_index);
        self.tape_ref.insert(char_index, &instr_mapped, self.tape_mode);
        self.held_str = self.tape_ref.to_string();
        instr_mapped.len()
    }
    fn delete_char_range(&mut self, char_range: Range<egui::text::CharIndex>) {
        let char_range = char_range.start.0..char_range.end.0;
        if char_range.is_empty() {
            return;
        }
        self.record_edit(char_range.start);
        self.tape_ref.delete(char_range, self.tape_mode);
        self.held_str = self.tape_ref.to_string();
    }

    fn clear(&mut self) {
        self.record_edit(0);
        self.tape_ref.clear();
        self.held_str = self.tape_ref.to_string();
    }
    fn take(&mut self) -> String {
        let s = self.as_ref().to_owned();
        self.clear();
        s
    }
    fn as_str(&self) -> &str {
        self.as_ref()
    }
    fn type_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<TapeBuffer>()
    }
}

struct PathInfo{
    base: String,
    puzzle: String,
    solution: String,
}
enum RunState{
    Manual(usize), FreeRun, Crashed(XYPos)
}

//An edit to one part of the layout: it moves by `delta`, then rotates by `rot` around `pivot`.
//Arms can also change length and type.
#[derive(Clone, Copy, Debug, PartialEq)]
struct PartEdit{
    part: PartId,
    delta: Pos,
    rot: Rot,
    pivot: Pos,
    len: i32,
    arm_type: Option<ArmType>,
}
impl PartEdit{
    fn new(part: PartId) -> Self{
        PartEdit{part, delta: Pos::zeros(), rot: 0, pivot: Pos::zeros(), len: 0, arm_type: None}
    }
    fn is_noop(&self) -> bool{
        self.delta == Pos::zeros() && self.rot == 0 && self.len == 0 && self.arm_type.is_none()
    }
}
fn edited_layout(world: &World, edit: &PartEdit) -> InitialWorld{
    let mut layout = world.layout();
    let place = |pos: Pos| rotate_around(pos + edit.delta, edit.rot, edit.pivot);
    match edit.part{
        PartId::Glyph(i) => {
            let glyph = &mut layout.glyphs[i];
            glyph.pos = place(glyph.pos);
            if let GlyphType::Track(path) = &mut glyph.glyph_type{
                //Track rotation is ignored when loading (like omsim), so the path itself rotates
                for p in path{
                    *p = rotate(*p, edit.rot);
                }
            } else if edit.rot != 0{
                glyph.rot = normalize_dir(glyph.rot + edit.rot);
            }
        }
        PartId::Arm(i) => {
            let arm = &mut layout.arms[i];
            arm.pos = place(arm.pos);
            arm.rot = (arm.rot + edit.rot) % 6;
            if let Some(arm_type) = edit.arm_type{
                arm.arm_type = arm_type;
            }
            arm.len = if arm.arm_type.is_wheel() {1} else {(arm.len + edit.len).clamp(1, 3)};
        }
    }
    layout
}
const ALL_ARM_TYPES: [ArmType; 7] = [ArmType::PlainArm, ArmType::DoubleArm, ArmType::TripleArm, ArmType::HexArm,
    ArmType::Piston, ArmType::VanBerlo, ArmType::Ravari];
fn arm_type_name(arm_type: ArmType) -> &'static str{
    match arm_type{
        ArmType::PlainArm => "Arm",
        ArmType::DoubleArm => "Double arm",
        ArmType::TripleArm => "Triple arm",
        ArmType::HexArm => "Hex arm",
        ArmType::Piston => "Piston",
        ArmType::VanBerlo => "Van Berlo's wheel",
        ArmType::Ravari => "Ravari's wheel",
    }
}
fn part_label(world: &World, part: PartId) -> String{
    use GlyphType::*;
    match part{
        PartId::Arm(i) => format!("Arm {} ({})", i+1, arm_type_name(world.arms[i].arm_type)),
        PartId::Glyph(i) => {
            let name = match &world.glyphs[i].glyph_type{
                Calcification => "Calcification".to_owned(),
                Animismus => "Animismus".to_owned(),
                Projection => "Projection".to_owned(),
                Dispersion => "Dispersion".to_owned(),
                Purification => "Purification".to_owned(),
                Duplication => "Duplication".to_owned(),
                Unification => "Unification".to_owned(),
                Bonding => "Bonder".to_owned(),
                Unbonding => "Unbonder".to_owned(),
                TriplexBond => "Triplex bonder".to_owned(),
                MultiBond => "Multi-bonder".to_owned(),
                Disposal => "Disposal".to_owned(),
                Equilibrium => "Equilibrium".to_owned(),
                Rejection => "Rejection".to_owned(),
                Division => "Division".to_owned(),
                Proliferation => "Proliferation".to_owned(),
                Track(_) => "Track".to_owned(),
                Conduit(_, id) => format!("Conduit {id}"),
                Input(_, id) => format!("Input {id}"),
                Output(_, _, id) => format!("Output {id}"),
                OutputRepeating(_, _, id) => format!("Polymer output {id}"),
            };
            format!("{name} (glyph {})", i+1)
        }
    }
}
//The selected part's fields, returning an edit if one was changed
//`locked` explains why the fields can't be edited, if they can't
fn part_fields(ui: &mut egui::Ui, world: &World, part: PartId, locked: Option<&str>) -> Option<PartEdit>{
    ui.label(part_label(world, part));
    if let Some(reason) = locked{
        ui.weak(reason);
    }
    let editable = locked.is_none();
    let (pos, rot) = match part{
        PartId::Glyph(i) => (world.glyphs[i].pos, world.glyphs[i].rot),
        PartId::Arm(i) => (world.arms[i].pos, world.arms[i].rot),
    };
    let is_track = matches!(part, PartId::Glyph(i) if world.glyphs[i].is_track());
    let mut edit = PartEdit::new(part);
    edit.pivot = pos;
    ui.add_enabled_ui(editable, |ui| {
        let (mut x, mut y, mut r) = (pos.x, pos.y, rot);
        ui.horizontal(|ui| {
            ui.label("x:");
            ui.add(egui::DragValue::new(&mut x).speed(0.05));
            ui.label("y:");
            ui.add(egui::DragValue::new(&mut y).speed(0.05));
            //Track rotation isn't stored (the path rotates instead), so it's only changed by dragging
            if !is_track{
                ui.label("rot:");
                ui.add(egui::DragValue::new(&mut r).speed(0.05));
            }
        });
        edit.delta = Pos::new(x, y) - pos;
        edit.rot = r - rot;
        if let PartId::Arm(i) = part{
            let arm = &world.arms[i];
            ui.horizontal(|ui| {
                let mut len = arm.len;
                ui.label("length:");
                ui.add_enabled(!arm.arm_type.is_wheel(), egui::DragValue::new(&mut len).range(1..=3).speed(0.05));
                edit.len = len - arm.len;
                let mut arm_type = arm.arm_type;
                egui::ComboBox::from_id_salt("arm_type").selected_text(arm_type_name(arm_type)).show_ui(ui, |ui| {
                    for t in ALL_ARM_TYPES{
                        ui.selectable_value(&mut arm_type, t, arm_type_name(t));
                    }
                });
                if arm_type != arm.arm_type{
                    edit.arm_type = Some(arm_type);
                }
            });
        }
    });
    (!edit.is_noop()).then_some(edit)
}

//Moving the mouse this far (in pixels) with the button held starts a drag
const DRAG_THRESHOLD_PX: f32 = 4.;
//The left mouse button is held on the board
struct Press{
    press_px: (f32, f32),
    press_xy: XYPos,
    press_hex: Pos,
    //The parts under the pressed hex, top first, and which one is picked
    stack: Vec<PartId>,
    cand: usize,
    wheel_accum: f32,
    drag: Option<Drag>,
}
struct Drag{
    cursor_xy: XYPos,
    //From the rotate/length hotkeys
    rot: Rot,
    len: i32,
    preview: Option<Preview>,
    //Every track except the dragged one, when a track is dragged
    scene_tracks: Option<(PartId, TrackBindings)>,
}
//The dragged part placed at its landing spot
struct Preview{
    edit: PartEdit,
    layout: InitialWorld,
    world: std::result::Result<World, String>,
    float_world: FloatWorld,
    polymer: Option<Polymer>,
    track: Option<TrackBindings>,
    landing: SmallVec<[Pos; 8]>,
    overlaps: SmallVec<[Pos; 8]>,
}
impl Press{
    fn part(&self) -> PartId{
        self.stack[self.cand]
    }
    //Where the grabbed point of the part is now
    fn held_xy(&self, drag: &Drag) -> XYPos{
        pos_to_xy(self.press_hex) + (drag.cursor_xy - self.press_xy)
    }
    //The edit that dropping here would make (snapped to the nearest hex)
    fn edit(&self, drag: &Drag) -> PartEdit{
        let landing = xy_to_pos(self.held_xy(drag));
        PartEdit{part: self.part(), delta: landing - self.press_hex, rot: drag.rot, pivot: landing,
            len: drag.len, arm_type: None}
    }
    //How far the dragged part is drawn from its landing spot
    fn drag_offset(&self, drag: &Drag) -> XYVec{
        let held = self.held_xy(drag);
        held - pos_to_xy(xy_to_pos(held))
    }
}
struct Loaded{
    base_world: World,
    curr_world: World,
    backup_world: World,
    curr_time: f64,
    curr_substep: usize,
    backup_step: u64,
    substep_count: usize,
    float_world: FloatWorld,
    saved_motions: WorldStepInfo,

    max_timestep: usize,
    camera: CameraSetup,
    tracks: TrackBindings,
    solution: parser::FullSolution,
    message: Option<String>,

    tape_mode: bool,
    run_state: RunState,
    show_area: bool,
    run_speed: f64,
    popup_reload: bool,

    //Part editing
    selection: Option<PartId>,
    //The parts at the hex last pressed, top first
    stack_view: Option<Vec<PartId>>,
    allow_overlap: bool,
    press: Option<Press>,
    tracks_dirty: bool,
    //Track buffers to free once the rendering context is available
    dead_tracks: Vec<TrackBindings>,
}
impl Loaded{
    //Arms can only be edited before anything has moved
    fn at_first_step(&self) -> bool{
        self.curr_world.timestep == self.base_world.timestep && self.curr_substep == 0
            && self.curr_time <= self.curr_world.timestep as f64
    }
    fn part_editable(&self, part: PartId) -> bool{
        self.part_lock(part).is_none()
    }
    //Why the part can't be edited right now, if it can't
    fn part_lock(&self, part: PartId) -> Option<&'static str>{
        if !EDITOR_ENABLED{
            Some("Editing is disabled")
        } else if matches!(self.run_state, RunState::FreeRun){
            Some("Editing is disabled while running")
        } else if matches!(part, PartId::Arm(_)) && !self.at_first_step(){
            Some("Arms can only be edited at the initial timestep")
        } else {
            None
        }
    }
    //The part to outline: the candidate while the mouse is held, otherwise the selection
    fn highlighted_part(&self) -> Option<PartId>{
        self.press.as_ref().map(|p| p.part()).or(self.selection)
    }
    //Replaces the starting world after a part edit, then runs back to the shown timestep
    fn commit_world(&mut self, mut world: World){
        world.repeat_length = world.repeat_length.max(self.base_world.repeat_length);
        let target = self.curr_time.floor() as usize;
        self.base_world = world;
        self.tracks_dirty = true;
        self.reset_world();
        self.try_set_target_time(target);
    }
    fn check_overlap(&self, overlaps: &[Pos]) -> std::result::Result<(), String>{
        match overlaps.first(){
            Some(hex) if !self.allow_overlap =>
                Err(format!("That would overlap another part at {:?} (see \"Allow overlap\")", (hex.x, hex.y))),
            _ => Ok(()),
        }
    }
    fn apply_edit(&mut self, edit: &PartEdit) -> std::result::Result<(), String>{
        if edit.is_noop(){
            return Ok(());
        }
        let world = World::setup_sim(&edited_layout(&self.base_world, edit)).map_err(|e| e.to_string())?;
        self.check_overlap(&world.part_overlaps(edit.part))?;
        self.commit_world(world);
        Ok(())
    }
    fn retire_drag(&mut self, drag: Drag){
        if let Some(track) = drag.preview.and_then(|p| p.track){
            self.dead_tracks.push(track);
        }
        if let Some((_, tracks)) = drag.scene_tracks{
            self.dead_tracks.push(tracks);
        }
    }
    fn cancel_press(&mut self){
        if let Some(drag) = self.press.take().and_then(|p| p.drag){
            self.retire_drag(drag);
        }
    }
    fn press(&mut self, px: (f32, f32), screen_size: (f32, f32)){
        self.cancel_press();
        let press_xy = self.camera.screen_to_world(px, screen_size);
        let press_hex = xy_to_pos(press_xy);
        //Arms are picked where they are now (glyphs don't move)
        let stack = self.curr_world.parts_at(press_hex);
        if stack.is_empty(){
            self.selection = None;
            self.stack_view = None;
            return;
        }
        let cand = self.selection.and_then(|s| stack.iter().position(|&p| p == s)).unwrap_or(0);
        self.stack_view = Some(stack.clone());
        self.press = Some(Press{press_px: px, press_xy, press_hex, stack, cand, wheel_accum: 0., drag: None});
    }
    fn pointer_moved(&mut self, px: (f32, f32), screen_size: (f32, f32)){
        let cursor_xy = self.camera.screen_to_world(px, screen_size);
        let editable = self.press.as_ref().is_some_and(|p| self.part_editable(p.part()));
        let Some(press) = &mut self.press else {return};
        match &mut press.drag{
            Some(drag) => drag.cursor_xy = cursor_xy,
            None => {
                let (dx, dy) = (px.0 - press.press_px.0, px.1 - press.press_px.1);
                if editable && dx*dx + dy*dy > DRAG_THRESHOLD_PX*DRAG_THRESHOLD_PX{
                    press.drag = Some(Drag{cursor_xy, rot: 0, len: 0, preview: None, scene_tracks: None});
                }
            }
        }
    }
    //Picks another part from the stack. A drag carries over to it (keeping the offset).
    fn cycle(&mut self, step: i32){
        let Some(press) = &mut self.press else {return};
        press.cand = (press.cand as i32 + step).rem_euclid(press.stack.len() as i32) as usize;
        if let Some(drag) = &mut press.drag{
            drag.rot = 0;
            drag.len = 0;
        }
    }
    fn release(&mut self){
        let Some(mut press) = self.press.take() else {return};
        self.selection = Some(press.part());
        if let Some(drag) = press.drag.take(){
            let edit = press.edit(&drag);
            self.retire_drag(drag);
            if let Err(err) = self.apply_edit(&edit){
                self.message = Some(err);
            }
        }
    }
    //Returns true if the wheel was used to pick a part (rather than zoom)
    fn wheel(&mut self, dy: f32) -> bool{
        let Some(press) = &mut self.press else {return false};
        press.wheel_accum += dy;
        //Steps once per notch, but trackpads send many small movements
        if press.wheel_accum.abs() >= 1.{
            let step = if press.wheel_accum > 0. {-1} else {1};
            press.wheel_accum = 0.;
            self.cycle(step);
        }
        true
    }
    //Returns true if the key was used for the held part
    fn handle_key(&mut self, keycode: KeyCode, keymods: KeyMods) -> bool{
        let Some(press) = &mut self.press else {return false};
        match keycode{
            KeyCode::Tab => self.cycle(if keymods.shift {-1} else {1}),
            KeyCode::Escape => self.cancel_press(),
            KeyCode::A | KeyCode::D | KeyCode::W | KeyCode::S => {
                let part = press.part();
                let Some(drag) = &mut press.drag else {return false};
                match (keycode, part){
                    (KeyCode::A, _) => drag.rot += 1,
                    (KeyCode::D, _) => drag.rot -= 1,
                    (_, PartId::Arm(i)) => {
                        let arm = &self.base_world.arms[i];
                        let change = if keycode == KeyCode::W {1} else {-1};
                        if !arm.arm_type.is_wheel(){
                            drag.len = (arm.len + drag.len + change).clamp(1, 3) - arm.len;
                        }
                    }
                    _ => {}
                }
            }
            _ => return false,
        }
        true
    }
    fn build_preview(&self, ctx: &mut dyn RenderingBackend, edit: PartEdit) -> Preview{
        let layout = edited_layout(&self.base_world, &edit);
        let (polymer, track, landing) = match edit.part{
            PartId::Glyph(i) => {
                let glyph = &layout.glyphs[i];
                let polymer = glyph.build_polymer().ok().flatten();
                let track = glyph.is_track().then(||
                    setup_tracks(ctx, &World::build_track_maps([glyph]).unwrap_or_default()));
                let landing = glyph.footprint(polymer.as_ref());
                (polymer, track, landing)
            }
            PartId::Arm(i) => (None, None, smallvec![layout.arms[i].pos]),
        };
        let world = World::setup_sim(&layout).map_err(|e| e.to_string());
        let mut float_world = FloatWorld::new();
        let overlaps = match &world{
            Ok(world) => {
                float_world.generate_static(world);
                self.base_world.overlaps_with_others(edit.part, &landing)
            }
            Err(_) => landing.clone(),
        };
        Preview{edit, layout, world, float_world, polymer, track, landing, overlaps}
    }
    //Updates rendering data for part edits (needs the rendering context)
    fn refresh_part_gfx(&mut self, ctx: &mut dyn RenderingBackend){
        if matches!(self.run_state, RunState::FreeRun) && self.press.as_ref().is_some_and(|p| p.drag.is_some()){
            self.cancel_press();
        }
        for tracks in self.dead_tracks.drain(..){
            tracks.delete(ctx);
        }
        if self.tracks_dirty{
            let old = std::mem::replace(&mut self.tracks, setup_tracks(ctx, &self.base_world.track_maps));
            old.delete(ctx);
            self.tracks_dirty = false;
        }
        let Some(press) = &self.press else {return};
        let Some(drag) = &press.drag else {return};
        let edit = press.edit(drag);
        let part = edit.part;
        let dragging_track = matches!(part, PartId::Glyph(i) if self.base_world.glyphs[i].is_track());
        let scene_tracks_part = drag.scene_tracks.as_ref().map(|(p, _)| *p);
        let new_scene_tracks = if !dragging_track{
            None
        } else if scene_tracks_part == Some(part){
            return self.refresh_preview(ctx, edit);
        } else {
            let others = self.base_world.glyphs.iter().enumerate()
                .filter(|&(i, _)| PartId::Glyph(i) != part).map(|(_, g)| g);
            Some((part, setup_tracks(ctx, &World::build_track_maps(others).unwrap_or_default())))
        };
        if scene_tracks_part.is_some() || new_scene_tracks.is_some(){
            let drag = self.press.as_mut().unwrap().drag.as_mut().unwrap();
            if let Some((_, old)) = std::mem::replace(&mut drag.scene_tracks, new_scene_tracks){
                self.dead_tracks.push(old);
            }
        }
        self.refresh_preview(ctx, edit);
    }
    fn refresh_preview(&mut self, ctx: &mut dyn RenderingBackend, edit: PartEdit){
        let current = self.press.as_ref().and_then(|p| p.drag.as_ref()).and_then(|d| d.preview.as_ref());
        if current.is_some_and(|p| p.edit == edit){
            return;
        }
        let preview = self.build_preview(ctx, edit);
        let drag = self.press.as_mut().unwrap().drag.as_mut().unwrap();
        if let Some(track) = drag.preview.replace(preview).and_then(|p| p.track){
            self.dead_tracks.push(track);
        }
    }
    fn draw_world(&self, render: &RenderDataBase, ctx: &mut Context){
        const SELECT_COLOR: [f32; 3] = [0., 0.8, 1.];
        const OVERLAP_COLOR: [f32; 3] = [1., 0.1, 0.1];
        let dragging = self.press.as_ref().and_then(|press| {
            let drag = press.drag.as_ref()?;
            Some((press, drag, drag.preview.as_ref()?))
        });
        let Some((press, drag, preview)) = dragging else {
            let world = &self.curr_world;
            let scene = Scene{glyphs: &world.glyphs, polymers: &world.polymers, area: &world.area_touched,
                float_world: &self.float_world, hidden: None};
            render.draw(ctx, &self.camera, &self.tracks, self.show_area, &scene);
            if let Some(part) = self.highlighted_part(){
                render.draw_hex_rings(ctx, &self.camera, &self.curr_world.part_footprint(part), SELECT_COLOR);
            }
            return;
        };
        let part = preview.edit.part;
        //Before anything moves, show the world as it would start; otherwise keep showing the current step
        let (world, float_world) = match &preview.world{
            Ok(world) if self.at_first_step() => (world, &preview.float_world),
            _ => (&self.curr_world, &self.float_world),
        };
        let scene = Scene{glyphs: &world.glyphs, polymers: &world.polymers, area: &world.area_touched,
            float_world, hidden: Some(part)};
        let tracks = drag.scene_tracks.as_ref().map_or(&self.tracks, |(_, t)| t);
        render.draw(ctx, &self.camera, tracks, self.show_area, &scene);
        let extra = press.drag_offset(drag);
        match part{
            PartId::Glyph(i) => render.draw_dragged_glyph(ctx, &self.camera, &preview.layout.glyphs[i],
                preview.polymer.as_ref(), preview.track.as_ref(), extra),
            PartId::Arm(i) => render.draw_dragged_arm(ctx, &self.camera, &preview.layout.arms[i], i, extra),
        }
        render.draw_hex_rings(ctx, &self.camera, &preview.landing, SELECT_COLOR);
        render.draw_hex_rings(ctx, &self.camera, &preview.overlaps, OVERLAP_COLOR);
    }
    //Brings the other worlds up to date after the arms (tapes, types) or loop length of base_world
    //changed. `earliest_step` is the first timestep whose behavior may have changed.
    fn apply_base_change(&mut self, earliest_step: usize){
        let old_start = self.base_world.timestep;
        let new_start = self.base_world.get_first_timestep();
        self.base_world.timestep = new_start;
        let earliest_step = (earliest_step as u64).min(old_start).min(new_start);
        fn sync_arms(target: &mut World, base: &World){
            target.repeat_length = base.repeat_length;
            for (arm, base_arm) in target.arms.iter_mut().zip(&base.arms){
                arm.instruction_tape = base_arm.instruction_tape.clone();
                arm.arm_type = base_arm.arm_type;
            }
        }
        //The backup is either still valid (it's from before earliest_step) or gets replaced by reset_world
        sync_arms(&mut self.backup_world, &self.base_world);
        if earliest_step <= self.curr_time as u64 || earliest_step < self.curr_world.timestep{
            self.reset_to(earliest_step);
        } else {
            sync_arms(&mut self.curr_world, &self.base_world);
        }
    }
    //Sets the loop length, returning the first timestep it can affect
    fn change_repeat_length(&mut self, length: usize) -> usize{
        let old = self.base_world.repeat_length;
        self.base_world.repeat_length = length;
        if self.max_timestep < length{
            self.max_timestep = length;
        }
        //Arms start looping at their first instruction plus the loop length
        self.base_world.get_first_timestep() as usize + old.min(length)
    }
    fn reset_to(&mut self, reset_to:u64){
        //The backup hasn't run its own timestep yet, so it's still valid for a change at backup_step
        if self.backup_step <= reset_to{
            self.reset_to_backup();
        } else {
            self.reset_world();
        }
    }
    fn reset_world(&mut self) {
        if let RunState::Crashed(_) = self.run_state{
            self.run_state = RunState::Manual(self.curr_world.timestep as usize);
        }
        self.saved_motions.clear();
        self.curr_world = self.base_world.clone();
        self.curr_time = 0.;
        self.substep_count = 8;
        self.curr_substep = 0;
        self.message = None;

        self.backup_world = self.base_world.clone();
        self.backup_step = 0;
    }
    fn reset_to_backup(&mut self){
        if let RunState::Crashed(_) = self.run_state{
            self.run_state = RunState::Manual(self.curr_world.timestep as usize);
        }
        self.saved_motions.clear();
        self.curr_world = self.backup_world.clone();
        self.curr_time = self.backup_step as f64;
        self.substep_count = 8;
        self.curr_substep = 0;
        self.message = None;
    }
    fn try_set_target_time(&mut self, target: usize){
        if let RunState::Crashed(_) = self.run_state{
            if self.curr_world.timestep as usize > target{
                self.run_state = RunState::Manual(target);
            }
        } else {
            self.run_state = RunState::Manual(target);
        }
    }
    fn advance(&mut self, substep_time: &mut f64) -> SimResult<()>{
        if self.curr_substep == 0{
            let failcheck = self.curr_world.prepare_step(&mut self.saved_motions, self.show_area);
            if failcheck.is_err(){
                self.saved_motions.clear();
                return failcheck;
            }
            self.substep_count = self.curr_world.substep_count(&self.saved_motions);
            *substep_time = 1.0/(self.substep_count as f64);
        }
        self.curr_substep += 1;
        if self.show_area && self.curr_substep < self.substep_count{
            self.curr_world.collide_substep(&self.saved_motions, self.curr_substep, self.substep_count)?;
        }
        if self.curr_substep == self.substep_count{
            self.curr_substep = 0;
            self.curr_world.finalize_step(&mut self.saved_motions, self.show_area)?;
        }
        Ok(())
    }
    fn update(&mut self){
            let target_time = match self.run_state{
                RunState::Manual(target_timestep) => {
                    let target_timestep = target_timestep as f64;
                    if target_timestep > self.curr_time+1.001{
                        target_timestep
                    } else {
                        f64::min(self.curr_time+self.run_speed,target_timestep)
                    }
                },
                RunState::FreeRun => self.curr_time+self.run_speed,
                RunState::Crashed(_) => return,
            };
            if target_time < self.curr_time{
                self.reset_to(target_time as u64);
            } else { //If backup is too out-of-date, catch up
                if self.curr_time as u64 >= self.backup_step+200 {
                    self.reset_to_backup()
                }
            }

            let mut substep_time = 1.0/(self.substep_count as f64);
            let backup_target_timestep = (target_time.floor() as u64).max(100) - 100;
            if self.curr_world.timestep < backup_target_timestep{
                while self.curr_world.timestep < backup_target_timestep{
                    let output = self.advance(&mut substep_time);
                    if let Err(output) = output{
                        self.run_state = RunState::Crashed(output.location);
                        self.message = Some(output.to_string());
                        self.curr_time = self.curr_world.timestep as f64+(substep_time*self.curr_substep as f64);
                        return;
                    }
                }
                self.backup_step = backup_target_timestep;
                self.backup_world = self.curr_world.clone();
                assert_eq!(self.curr_substep, 0);
                assert_eq!(backup_target_timestep, self.curr_world.timestep);
            }

            let target_timestep = target_time.floor() as u64;
            let target_substep = (target_time.fract()*self.substep_count as f64).floor() as usize;
            while self.curr_world.timestep < target_timestep || (self.curr_world.timestep == target_timestep && self.curr_substep < target_substep){
                let output = self.advance(&mut substep_time);
                if let Err(output) = output{
                    self.run_state = RunState::Crashed(output.location);
                    self.message = Some(output.to_string());
                    self.curr_time = self.curr_world.timestep as f64+(substep_time*self.curr_substep as f64);
                    return;
                }
            }
            self.curr_time = target_time;
    }
}
enum AppState{
    NotLoaded,Loaded(Loaded) 
}
enum AppStateUpdate{
    NoChange, Load, ResetHome
}
use AppState::*;

pub struct MyMiniquadApp {
    mq_ctx: Box<dyn RenderingBackend>,
    egui_mq: egui_miniquad::EguiMq,
    render_data: RenderDataBase,
    app_state: AppState,
    unloaded_info: PathInfo,
    extra_message: Option<String>,
    dragging: Option<GFXPos>,
}

impl MyMiniquadApp {
    pub fn new() -> Self {
        let mut mq_ctx = window::new_rendering_backend();
        let render_data = RenderDataBase::new(mq_ctx.as_mut());
        let (base_str, puzzle_str, solution_str) = get_default_path_strs();
        let base = String::from(base_str);
        let puzzle = String::from(puzzle_str);
        let solution = String::from(solution_str);
        let app_state = AppState::NotLoaded;
        let unloaded_info = PathInfo{base, puzzle, solution};
        let egui_mq = egui_miniquad::EguiMq::new(mq_ctx.as_mut());
        Self {
            mq_ctx, egui_mq,
            render_data,app_state,unloaded_info,
            extra_message:None, dragging: None,
        }
    }
    pub fn load(&mut self, f_puzzle: impl Read, f_sol: impl Read) -> Result<()>{
        let puzzle = parser::parse_puzzle(&mut BufReader::new(f_puzzle))?;
        let solution = parser::parse_solution(&mut BufReader::new(f_sol))?;
        println!("Check: {:?}", solution.stats);
        let init = parser::puzzle_prep(&puzzle, &solution)?;
		let mut max_timestep = 0;
        for (a_num, a) in init.arms.iter().enumerate(){
            println!("Arms {:02}: {:?}", a_num, a.instruction_tape.to_string());
			max_timestep = max_timestep.max(a.instruction_tape.first)
        }
        let world = World::setup_sim(&init)?;
		max_timestep += world.repeat_length*2+100;

        let float_world = FloatWorld::new();
        let mut saved_motions = WorldStepInfo::new();
        saved_motions.clear();
        let camera = CameraSetup::frame_center(&world, window::screen_size());
        let tracks = setup_tracks(self.mq_ctx.as_mut(), &world.track_maps);
        //A solution that already overlaps can keep doing so
        let allow_overlap = world.has_overlap();
        let new_loaded = Loaded{
            base_world: world.clone(),
            backup_world: world.clone(),
            curr_world: world,
            curr_time: 0.,
            curr_substep: 0,
            backup_step: 0,
            substep_count: 8,
            float_world,saved_motions,
        
            max_timestep,camera,tracks,solution,
            message: None,
        
            tape_mode: false,
            run_state: RunState::Manual(0),
            //Also enables collision checks, so edits that collide are caught
            show_area: true,
            run_speed: 0.05,
            popup_reload: false,

            selection: None,
            stack_view: None,
            allow_overlap,
            press: None,
            tracks_dirty: false,
            dead_tracks: Vec::new(),
        };
        self.app_state = Loaded(new_loaded);
        Ok(())
    }
}


#[cfg(target_arch = "wasm32")]
static JS_PUZZLE_INPUT: std::sync::Mutex<Option<Vec<u8>>> = std::sync::Mutex::new(None);
#[cfg(target_arch = "wasm32")]
static JS_SOLUTION_INPUT: std::sync::Mutex<Option<Vec<u8>>> = std::sync::Mutex::new(None);
#[cfg(target_arch = "wasm32")]
static JS_SOLUTION_NAME: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn js_load_puzzle(puzzle: sapp_jsutils::JsObject){
    let mut puzzle_vec = Vec::new();
    puzzle.to_byte_buffer(&mut puzzle_vec);
    *JS_PUZZLE_INPUT.lock().unwrap() = Some(puzzle_vec);
}
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn js_load_solution(solution: sapp_jsutils::JsObject){
    let mut solution_vec = Vec::new();
    solution.to_byte_buffer(&mut solution_vec);
    *JS_SOLUTION_INPUT.lock().unwrap() = Some(solution_vec);
}
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn js_load_solution_name(name: sapp_jsutils::JsObject){
    let mut sol_name = String::new();
    name.to_string(&mut sol_name);
    *JS_SOLUTION_NAME.lock().unwrap() = Some(sol_name);
}

#[cfg(all(target_arch = "wasm32", feature="editor_ui"))]
extern "C" {
    fn save_file(filedata: sapp_jsutils::JsObject, filename: sapp_jsutils::JsObject);
}

const EDITOR_ENABLED: bool = cfg!(feature = "editor_ui");

impl EventHandler for MyMiniquadApp {
    fn update(&mut self) {
        if let Loaded(loaded) = &mut self.app_state{
            loaded.update();
            let display_portion = loaded.curr_time.fract() as f32;
            if loaded.saved_motions.arms.is_empty(){
                loaded.float_world.generate_static(&loaded.curr_world);
            } else {
                loaded.float_world.regenerate(&loaded.curr_world, &loaded.saved_motions, display_portion);
            }
        }
    }

    fn draw(&mut self) {
        let ctx = self.mq_ctx.as_mut();
        ctx.clear(Some((1., 1., 1., 1.)), None, None);
        ctx.begin_default_pass(PassAction::clear_color(0.5, 0.5, 0.5, 1.0));
        if let Loaded(loaded) = &mut self.app_state{
            loaded.refresh_part_gfx(ctx);
            loaded.draw_world(&self.render_data, ctx);
        }
        ctx.end_render_pass();
        
        let mut do_loading = AppStateUpdate::NoChange;
        let screen_size = window::screen_size();
        self.egui_mq.run(ctx, |_mq_ctx, egui_ctx|{
            if let Some(msg) = &self.extra_message{
                let mut opened = true;
                egui::Window::new("Message").id(egui::Id::new("app_message"))
                    .open(&mut opened).show(egui_ctx, |ui| {
                    ui.label(msg);
                });
                if !opened {self.extra_message = None;}
            }
            match &mut self.app_state{
                Loaded(loaded) => {
                    egui::Window::new("World loaded").default_pos((0.,0.)).show(egui_ctx, |ui| {
                        ui.style_mut().spacing.slider_width = 500.;
                        let mut target_time = loaded.curr_time.floor() as usize;
                        if loaded.max_timestep < target_time{
                            loaded.max_timestep = target_time;
                        }
                        ui.horizontal(|ui| {
                            ui.add(egui::Slider::new(&mut target_time, 0..=loaded.max_timestep)
                                .show_value(false));
                            ui.add(egui::DragValue::new(&mut target_time)
                                .range(0..=loaded.max_timestep)
                                .speed(0.1));
                            ui.label("Timestep");
                        });
                        ui.horizontal(|ui| {
                            if ui.button("-1").clicked() {
                                if target_time > 0 {
                                    target_time -= 1;
                                }
                            }
                            if ui.button("+1").clicked() {
                                target_time += 1;
                            }
                            ui.label("Max Timestep:");
                            ui.add(egui::DragValue::new(&mut loaded.max_timestep)
                                .speed(0.1));
                            ui.separator();
                            if loaded.max_timestep < target_time{
                                loaded.max_timestep = target_time;
                            }
                            if target_time != loaded.curr_time.floor() as usize{
                                loaded.try_set_target_time(target_time);
                            }

                            let min_size = loaded.base_world.arms.iter()
                                .fold(0, |val, arm| usize::max(val, arm.instruction_tape.instructions.len()));
                            ui.label("Loop length:");

                            let mut repeat_length = loaded.base_world.repeat_length;
                            let repeat_length_response = ui.add_enabled(EDITOR_ENABLED, egui::DragValue::new(&mut repeat_length)
                                .range(min_size..=usize::MAX));
                            if repeat_length_response.changed() && repeat_length != loaded.base_world.repeat_length{
                                let earliest_step = loaded.change_repeat_length(repeat_length);
                                loaded.apply_base_change(earliest_step);
                            }
                            ui.separator();
                            let area_check = ui.checkbox(&mut loaded.show_area, "Collide/Area");
                            if loaded.show_area{
                                if area_check.changed() {
                                    loaded.reset_to(0);
                                }
                                ui.monospace(format!("{:5}",loaded.curr_world.area_touched.len()));
                            }
                            ui.separator();
                            ui.label("Speed:");
                            ui.add(egui::DragValue::new(&mut loaded.run_speed)
                                .range(0.01..=10.0f32).fixed_decimals(2).speed(0.002));
                            let (mut running_free, enabled) = match loaded.run_state{
                                RunState::FreeRun => (true, true),
                                RunState::Manual(_) => (false, true),
                                RunState::Crashed(_) => (false, false),
                            };
                            let was_freerun = running_free;
                            ui.add_enabled_ui(enabled, |ui| ui.checkbox(&mut running_free, "Run"));
                            if running_free{
                                loaded.run_state = RunState::FreeRun;
                            } else if was_freerun{
                                loaded.try_set_target_time(loaded.curr_time.ceil() as usize);
                            }
                        });
                    });
                    if let Some(msg) = &loaded.message{
                        let mut opened = true;
                        egui::Window::new("Message").id(egui::Id::new("world_message"))
                            .open(&mut opened).show(egui_ctx, |ui| {
                            ui.label(msg);
                        });
                        if !opened {loaded.message = None;}
                    }
                    egui::Window::new("Camera controls").show(egui_ctx, |ui| {
						ui.horizontal(|ui|{
							ui.label("x:");
							ui.add(egui::DragValue::new(&mut loaded.camera.offset.x)
								.speed(0.1));
							ui.label("y:");
							ui.add(egui::DragValue::new(&mut loaded.camera.offset.y)
								.speed(0.1));
                            if ui.button("Center").clicked(){
                                loaded.camera = CameraSetup::frame_center(&loaded.base_world, screen_size);
                            }
						});
						ui.horizontal(|ui|{
                            ui.label("zoom:");
                            ui.add(egui::Slider::new(&mut loaded.camera.scale_base,0.002 ..= 0.2)
                                .logarithmic(true));
                        });
                    });
					
                    #[cfg(feature = "editor_ui")]{
                        egui::Window::new("File info").show(egui_ctx, |ui| {
                            ui.label("solution:");
                            ui.text_edit_singleline(&mut loaded.solution.solution_name);
                            ui.label("filename:");
                            ui.text_edit_singleline(&mut self.unloaded_info.solution);
                            ui.horizontal(|ui| {
                                if ui.button("Save").clicked() {
                                    let dat = &self.unloaded_info;
                                    if !loaded.solution.solution_name.contains("omclone"){
                                        loaded.solution.solution_name += "(omclone)";
                                    }
                                    let new_solution = parser::create_solution(&loaded.base_world,
                                        loaded.solution.puzzle_name.clone(),
                                        loaded.solution.solution_name.clone());
                                    //Serialize fully first so a failure doesn't leave a partial file
                                    let mut data = Vec::new();
                                    let saved = parser::write_solution(&mut data, &new_solution).and_then(|()| {
                                        #[cfg(not(target_arch = "wasm32"))]
                                        std::fs::write(Path::new(&dat.base).join(&dat.solution), &data)?;
                                        #[cfg(target_arch = "wasm32")]{
                                            let filedata = sapp_jsutils::JsObject::buffer(&data);
                                            let filename = sapp_jsutils::JsObject::string(&dat.solution);
                                            unsafe {save_file(filedata,filename);}
                                        }
                                        Ok(())
                                    });
                                    loaded.message = Some(match saved {
                                        Ok(()) => String::from("Saved!"),
                                        Err(err) => format!("Save failed: {err}"),
                                    });
                                }
                                ui.separator();
                                if ui.button("Load").clicked() {
                                    loaded.popup_reload = true;
                                }
                            });
                        });
                    }
                    {
                        let mut arm_window = egui::Window::new("Arms");
                        #[cfg(not(feature = "editor_ui"))]{
                            arm_window = arm_window.default_open(false);
                        }
                        arm_window.default_width(600.).show(egui_ctx, |ui| {
                            #[cfg(not(feature = "editor_ui"))]{
                                ui.label("Editing Disabled");
                            }
                            ui.horizontal( |ui| {
                                #[cfg(feature = "editor_ui")]{
                                    ui.checkbox(&mut loaded.tape_mode, "Tape/overwrite mode");
                                    ui.separator();
                                    if ui.button("space ends").clicked() {
                                        for a in &mut loaded.base_world.arms{
                                            let instr = &mut a.instruction_tape;
                                            for _ in instr.instructions.len()..loaded.base_world.repeat_length{
                                                a.instruction_tape.instructions.push(Instr::Empty);
                                            }
                                        }
                                    }
                                    ui.separator();
                                    if ui.button("clear ends").clicked() {
                                        for a in &mut loaded.base_world.arms{
                                            let instr = &mut a.instruction_tape;
                                            while instr.instructions.last() == Some(&Instr::Empty){
                                                instr.instructions.pop();
                                            }
                                        }
                                    }
                                    if ui.button("Optimize Pistons").clicked() {
                                        let mut pistons_added = 0;
                                        let mut pistons_removed = 0;
                                        for a in &mut loaded.base_world.arms{
                                            if a.instruction_tape.instructions.contains(&Instr::Extend) || a.instruction_tape.instructions.contains(&Instr::Retract) {
                                                if a.arm_type == ArmType::PlainArm {
                                                    a.arm_type = ArmType::Piston;
                                                    pistons_added += 1;
                                                }
                                            } else {
                                                if a.arm_type == ArmType::Piston {
                                                    a.arm_type = ArmType::PlainArm;
                                                    pistons_removed += 1;
                                                }
                                            }
                                        }
                                        if pistons_added + pistons_removed > 0 {
                                            match World::setup_sim(&loaded.base_world.layout()) {
                                                Ok(world) => loaded.commit_world(world),
                                                Err(err) => loaded.message = Some(err.to_string()),
                                            }
                                        }
                                        self.extra_message = Some(format!("{pistons_added} pistons added, {pistons_removed} pistons removed"));
                                    }
                                    ui.separator();
                                }
                            });
                            let curr_timestep = loaded.curr_time.floor() as usize;
                            let end_spacing = loaded.max_timestep.saturating_sub(curr_timestep);
                            let marker = " ".repeat(curr_timestep+3)+"V"+&" ".repeat(end_spacing);
                            //+&(" ".repeat(loaded.max_timestep-loaded.curr_timestep));
                            let mut earliest_edit = None;
                            let mut target_time = None;
                            egui::ScrollArea::horizontal().show(ui, |ui| {
                                ui.add(egui::Label::new(egui::RichText::new(marker).monospace()).wrap_mode(egui::TextWrapMode::Extend));
                                egui::ScrollArea::vertical().show(ui, |ui| {
                                    for (a_num, a) in loaded.base_world.arms.iter_mut().enumerate(){
                                        let text = a.instruction_tape.to_string();
                                        let mut text_buf = TapeBuffer{
                                            tape_ref: &mut a.instruction_tape,
                                            earliest_edit: &mut earliest_edit,
                                            tape_mode: loaded.tape_mode,
                                            held_str: text,
                                        };
                                        ui.horizontal(|ui| {
                                            ui.label(format!("{:02}",a_num+1));
                                            let mut line_edit = egui::TextEdit::singleline(&mut text_buf)
                                            .code_editor().desired_width(f32::INFINITY);
                                            #[cfg(not(feature = "editor_ui"))]{
                                                line_edit = line_edit.interactive(false);
                                            }
                                            let text_output = line_edit.show(ui);
                                            if let Some(cursor) = text_output.cursor_range{
                                                target_time = Some(cursor.primary.index.0);
                                            }
                                        });
                                    }
                                    let marker2 = " ".repeat(curr_timestep+3)+"^";
                                    ui.add(egui::Label::new(egui::RichText::new(marker2).monospace()));
                                });
                            });
                            if let Some(target) = target_time {
                                loaded.try_set_target_time(target);
                            }
                            if let Some(mut edit_step) = earliest_edit{
                                let min_time = loaded.base_world.arms.iter()
                                    .fold(0, |val, arm| usize::max(val, arm.instruction_tape.instructions.len()+arm.instruction_tape.first));
                                if loaded.max_timestep < min_time{
                                    loaded.max_timestep = min_time;
                                }
                                let min_loop = loaded.base_world.arms.iter()
                                    .fold(0, |val, arm| usize::max(val, arm.instruction_tape.instructions.len()));
                                if loaded.base_world.repeat_length < min_loop{
                                    edit_step = edit_step.min(loaded.change_repeat_length(min_loop));
                                }
                                loaded.apply_base_change(edit_step);
                            }
                        });
                    }
                    egui::Window::new("Part").show(egui_ctx, |ui| {
                        ui.add_enabled(EDITOR_ENABLED, egui::Checkbox::new(&mut loaded.allow_overlap, "Allow overlap"));
                        ui.separator();
                        ui.label("Parts here:");
                        let highlighted = loaded.highlighted_part();
                        let mut clicked = None;
                        match &loaded.stack_view {
                            Some(stack) => for &part in stack {
                                if ui.selectable_label(highlighted == Some(part), part_label(&loaded.base_world, part)).clicked() {
                                    clicked = Some(part);
                                }
                            },
                            None => {ui.weak("(click the board)");}
                        }
                        if clicked.is_some() {
                            loaded.selection = clicked;
                        }
                        ui.separator();
                        match loaded.selection {
                            Some(part) => {
                                let locked = loaded.part_lock(part);
                                if let Some(edit) = part_fields(ui, &loaded.base_world, part, locked) {
                                    if let Err(err) = loaded.apply_edit(&edit) {
                                        loaded.message = Some(err);
                                    }
                                }
                            }
                            None => {ui.weak("No part selected");}
                        }
                    });
                    if let Some(press) = &loaded.press {
                        let label = part_label(&loaded.base_world, press.part());
                        let error = press.drag.as_ref().and_then(|d| d.preview.as_ref())
                            .and_then(|p| p.world.as_ref().err().cloned());
                        egui::Tooltip::always_open(egui_ctx.clone(), egui::LayerId::background(),
                            egui::Id::new("part_stack_tooltip"), egui::PopupAnchor::Pointer).show(|ui| {
                            ui.label(format!("{label} ({}/{})", press.cand+1, press.stack.len()));
                            if let Some(err) = error {
                                ui.colored_label(egui::Color32::RED, err);
                            }
                        });
                    }
                    /*egui::Window::new("debug").min_width(128.).show(egui_ctx, |ui|{
                        ui.label(format!("curr time: {}",loaded.curr_time as u64));
                        ui.label(format!("backup time: {}",loaded.backup_step));
                    });*/
                    egui::Window::new("Reload?").open(&mut loaded.popup_reload).show(egui_ctx, |ui| {
                        #[cfg(not(target_arch = "wasm32"))]{
                                if ui.button("Reload current").clicked() {
                                    do_loading = AppStateUpdate::Load;
                                }
                                if ui.button("Load New").clicked() {
                                    do_loading = AppStateUpdate::ResetHome;
                                }
                        }
                        #[cfg(target_arch = "wasm32")]{
                            ui.label("Choose files first, then press:");
                            if ui.button("Reset").clicked() {
                                do_loading = AppStateUpdate::ResetHome;
                            }

                        }
                    });
                }
                NotLoaded => {
                    egui::Window::new("World Not Loaded").show(egui_ctx, |ui| {
                        #[cfg(not(target_arch = "wasm32"))]{
                            let dat = &mut self.unloaded_info;
                            ui.horizontal(|ui| {
                                ui.label("Base path: ");
                                ui.text_edit_singleline(&mut dat.base);
                            });
                            ui.horizontal(|ui| {
                                ui.label("Puzzle: ");
                                ui.text_edit_singleline(&mut dat.puzzle);
                            });
                            ui.horizontal(|ui| {
                                ui.label("Solution: ");
                                ui.text_edit_singleline(&mut dat.solution);
                            });
                            if ui.button("Load").clicked() {
                                do_loading = AppStateUpdate::Load;
                            }
                        }
                        #[cfg(target_arch = "wasm32")]
                        {
                            let puzzle_loaded = JS_PUZZLE_INPUT.lock().unwrap().is_some();
                            let solution_loaded = JS_SOLUTION_INPUT.lock().unwrap().is_some();
                            match (puzzle_loaded, solution_loaded) {
                                (false, false) => {ui.label("Upload puzzle and solution files");},
                                (false, true) => {ui.label("Upload puzzle file");},
                                (true, false) => {ui.label("Upload solution file");},
                                (true, true) => {do_loading = AppStateUpdate::Load;}
                            }
                        }
                    });
                }
            };
        });
        self.egui_mq.draw(ctx);
        match do_loading{
            AppStateUpdate::NoChange => {},
            AppStateUpdate::ResetHome => {
                self.app_state = NotLoaded;
            }
            AppStateUpdate::Load => {
                let mut do_load = || -> Result<()>{
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        let dat = &self.unloaded_info;
                        let base_path = Path::new(&dat.base);
                        let f_puzzle = File::open(base_path.join(&dat.puzzle))?;
                        let solution_path = base_path.join(&dat.solution);
                        let f_sol = File::open(&solution_path)?;
                        self.load(f_puzzle, f_sol)?;
                        Ok(())
                    }
                    #[cfg(target_arch = "wasm32")]
                    {
                        let puzzle = JS_PUZZLE_INPUT.lock().unwrap();
                        let f_puzzle = puzzle.as_ref().unwrap();
                        let solution = JS_SOLUTION_INPUT.lock().unwrap();
                        let f_sol = solution.as_ref().unwrap();
                        let name = JS_SOLUTION_NAME.lock().unwrap().take();
                        self.unloaded_info.solution = name.unwrap_or(String::from("omclone.solution"));
                        self.load(f_puzzle.as_slice(), f_sol.as_slice())?;
                        Ok(())
                    }
                };

                if let Err(err)= do_load(){
                    self.extra_message = Some(err.to_string());
                    #[cfg(target_arch = "wasm32")]
                    {
                        *JS_SOLUTION_INPUT.lock().unwrap() = None;
                    }
                } else {
                    self.extra_message = None;
                }
            }
        }

        // Draw things in front of egui here
        self.mq_ctx.commit_frame();
    }

    fn mouse_motion_event(&mut self, x: f32, y: f32) {
        self.egui_mq.mouse_motion_event( x, y);
        if let Loaded(loaded) = &mut self.app_state{
            if let Some(drag_last) = &mut self.dragging{
                loaded.camera.pan_pixels(x-drag_last[0], y-drag_last[1]);
                *drag_last = [x,y];
            }
            loaded.pointer_moved((x, y), window::screen_size());
        }
    }

    fn mouse_wheel_event(&mut self, dx: f32, dy: f32) {
        self.egui_mq.mouse_wheel_event(dx, dy);
        if let Loaded(loaded) = &mut self.app_state{
            //While a part is held, the wheel picks between the parts under it
            if loaded.wheel(dy){
                return;
            }
        }
        if !self.egui_mq.egui_ctx().is_pointer_over_egui(){
            if let Loaded(loaded) = &mut self.app_state{
                let cam = &mut loaded.camera;
                if dy.is_sign_negative() && cam.scale_base > 0.002{
                    cam.scale_base *= 0.9;
                }
                if dy.is_sign_positive() && cam.scale_base < 0.2{
                    cam.scale_base *= 1.1;
                }
            }
        }
    }

    fn mouse_button_down_event(
        &mut self,
        mb: MouseButton,
        x: f32,
        y: f32,
    ) {
        self.egui_mq.mouse_button_down_event(mb, x, y);
        if !self.egui_mq.egui_ctx().is_pointer_over_egui(){
            match mb {
                MouseButton::Right | MouseButton::Middle => self.dragging = Some([x,y]),
                MouseButton::Left => if let Loaded(loaded) = &mut self.app_state{
                    loaded.press((x, y), window::screen_size());
                },
                _ => {}
            }
        }
    }

    fn mouse_button_up_event(
        &mut self,
        mb: MouseButton,
        x: f32,
        y: f32,
    ) {
        self.egui_mq.mouse_button_up_event(mb, x, y);
        match mb {
            MouseButton::Left => if let Loaded(loaded) = &mut self.app_state{
                loaded.release();
            },
            _ => self.dragging = None,
        }
    }

    fn char_event(
        &mut self,
        character: char,
        _keymods: KeyMods,
        _repeat: bool,
    ) {
        //Hotkeys for a held part aren't typed anywhere
        if let Loaded(loaded) = &self.app_state{
            if loaded.press.is_some() && !self.egui_mq.egui_ctx().text_edit_focused(){
                return;
            }
        }
        self.egui_mq.char_event(character);
    }

    fn key_down_event(
        &mut self,
        keycode: KeyCode,
        keymods: KeyMods,
        _repeat: bool,
    ) {
        if let Loaded(loaded) = &mut self.app_state{
            //Typing in a tape takes priority over the part hotkeys
            if !self.egui_mq.egui_ctx().text_edit_focused() && loaded.handle_key(keycode, keymods){
                return;
            }
        }
        self.egui_mq.key_down_event(keycode, keymods);
    }

    fn key_up_event(&mut self, keycode: KeyCode, keymods: KeyMods) {
        self.egui_mq.key_up_event(keycode, keymods);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glyph(x: i32, y: i32, rot: Rot, glyph_type: GlyphType) -> Glyph {
        Glyph { pos: Pos::new(x, y), rot, glyph_type }
    }
    fn arm(x: i32, y: i32, len: i32, arm_type: ArmType) -> Arm {
        Arm::new(Pos::new(x, y), 0, len, arm_type, Tape::new(0, vec![Instr::RotateClockwise]))
    }
    fn world(glyphs: Vec<Glyph>, arms: Vec<Arm>) -> World {
        World::setup_sim(&InitialWorld { glyphs, arms }).unwrap()
    }
    fn edited(w: &World, edit: PartEdit) -> World {
        World::setup_sim(&edited_layout(w, &edit)).unwrap()
    }
    //Saves and loads the world, like the Save button and a reload
    fn round_trip(w: &World) -> World {
        let solution = parser::create_solution(w, String::from("puzzle"), String::from("solution"));
        let mut data = Vec::new();
        parser::write_solution(&mut data, &solution).unwrap();
        let parsed = parser::parse_solution(&mut data.as_slice()).unwrap();
        let puzzle = parser::FullPuzzle {
            puzzle_name: String::new(), creator_id: 0, allowed_bitfield: 0,
            inputs: vec![], outputs: vec![], output_multiplier: 1, production: false,
        };
        World::setup_sim(&parser::puzzle_prep(&puzzle, &parsed).unwrap()).unwrap()
    }

    #[test]
    fn rotating_while_dragging_keeps_the_held_hex() {
        let w = world(vec![glyph(0, 0, 0, GlyphType::Bonding)], vec![]);
        //Held by its second hex at (1,0), moved two hexes right, then rotated
        let held = Pos::new(3, 0);
        let edit = PartEdit { delta: Pos::new(2, 0), rot: 1, pivot: held, ..PartEdit::new(PartId::Glyph(0)) };
        let moved = edited(&w, edit);
        assert_eq!(moved.glyphs[0].rot, 1);
        assert!(moved.glyph_footprint(0).contains(&held));
        assert_eq!(moved.glyphs[0].pos, held - rotate(Pos::new(1, 0), 1));
    }

    #[test]
    fn tracks_rotate_their_path() {
        let path = vec![Pos::new(0, 0), Pos::new(1, 0), Pos::new(2, 0)];
        let w = world(vec![glyph(5, 5, 0, GlyphType::Track(path))], vec![]);
        let edit = PartEdit { rot: 2, pivot: Pos::new(5, 5), ..PartEdit::new(PartId::Glyph(0)) };
        let rotated = edited(&w, edit);
        assert_eq!(rotated.glyphs[0].rot, 0);
        let expected: Vec<Pos> = (0..3).map(|i| Pos::new(5, 5) + rotate(Pos::new(i, 0), 2)).collect();
        assert_eq!(rotated.glyph_footprint(0).to_vec(), expected);
        //The track moves arms along the new path
        assert_eq!(rotated.track_maps.plus.get(&Pos::new(5, 5)), Some(&rotate(Pos::new(1, 0), 2)));
    }

    #[test]
    fn arm_edits_clamp_length() {
        let w = world(vec![], vec![arm(0, 0, 2, ArmType::Piston), arm(4, 0, 1, ArmType::PlainArm)]);
        let longer = edited(&w, PartEdit { len: 5, ..PartEdit::new(PartId::Arm(0)) });
        assert_eq!(longer.arms[0].len, 3);
        let wheel = edited(&w, PartEdit { len: 1, arm_type: Some(ArmType::VanBerlo), ..PartEdit::new(PartId::Arm(1)) });
        assert_eq!((wheel.arms[1].len, wheel.arms[1].arm_type), (1, ArmType::VanBerlo));
        assert_eq!(wheel.cost, w.cost + 10, "a wheel costs more than a plain arm");
        assert_eq!(wheel.arms[0].instruction_tape.instructions, w.arms[0].instruction_tape.instructions);
    }

    #[test]
    fn edits_survive_saving() {
        let conduit_a = glyph(0, 0, 0, GlyphType::Conduit(vec![Pos::new(0, 0), Pos::new(1, 0)], 7));
        let conduit_b = glyph(6, 0, 0, GlyphType::Conduit(vec![Pos::new(0, 0), Pos::new(1, 0)], 7));
        let track = glyph(0, 4, 0, GlyphType::Track(vec![Pos::new(0, 0), Pos::new(1, 0)]));
        let w = world(vec![conduit_a, conduit_b, track, glyph(3, 3, 0, GlyphType::Unification)],
            vec![arm(0, 4, 1, ArmType::PlainArm)]);
        let edits = [
            PartEdit { delta: Pos::new(0, -3), rot: 1, pivot: Pos::new(0, -3), ..PartEdit::new(PartId::Glyph(0)) },
            PartEdit { delta: Pos::new(1, 1), rot: -2, pivot: Pos::new(1, 5), ..PartEdit::new(PartId::Glyph(2)) },
            PartEdit { rot: 3, pivot: Pos::new(4, 3), ..PartEdit::new(PartId::Glyph(3)) },
            PartEdit { delta: Pos::new(2, 2), rot: -1, pivot: Pos::new(2, 6), len: 1, ..PartEdit::new(PartId::Arm(0)) },
        ];
        let mut w = w;
        for edit in edits {
            w = edited(&w, edit);
        }
        let loaded = round_trip(&w);
        for i in 0..w.glyphs.len() {
            assert_eq!(loaded.glyph_footprint(i), w.glyph_footprint(i), "glyph {i}");
        }
        assert_eq!(loaded.conduit_pairs, w.conduit_pairs);
        assert_eq!(loaded.track_maps.plus, w.track_maps.plus);
        let a = (&loaded.arms[0], &w.arms[0]);
        assert_eq!((a.0.pos, a.0.rot, a.0.len), (a.1.pos, a.1.rot, a.1.len));
    }
}
