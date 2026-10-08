//Picking, dragging and previewing parts on the board, placing new ones, and reshaping track
use super::*;

//Moving the mouse this far (in pixels) with the button held starts a drag
pub(super) const DRAG_THRESHOLD_PX: f32 = 4.;
//What an edit does to a part, for checking whether it's allowed right now
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum EditKind{
    //Moving, rotating, resizing, or changing an arm's type
    Move, Delete, Duplicate, Reorder,
}
//The left mouse button is held on the board
pub(super) struct Press{
    press_px: (f32, f32),
    press_xy: XYPos,
    press_hex: Pos,
    //The parts under the pressed hex, top first, and which one is picked
    pub(super) stack: Vec<PartId>,
    pub(super) cand: usize,
    wheel_accum: f32,
    //Ctrl was held, so dragging makes a copy
    pub(super) duplicate: bool,
    pub(super) drag: Option<Drag>,
}
pub(super) struct Drag{
    cursor_xy: XYPos,
    //From the rotate/length hotkeys
    rot: Rot,
    len: i32,
}
//A new part on the cursor, waiting to be placed
pub(super) struct Placing{
    pub(super) part: NewPart,
    rot: Rot,
    len: i32,
    //None while the cursor is off the board
    cursor_xy: Option<XYPos>,
}
//Strokes over the board reshape a track
pub(super) struct TrackMode{
    //None for a new track, until its first hex is placed
    pub(super) glyph: Option<usize>,
    pub(super) tool: TrackTool,
}
//The track's path during a stroke (from pressing the mouse button to releasing it)
pub(super) struct TrackStroke{
    path: Vec<Pos>,
    //Hexes this stroke added, which going back over takes off again
    added: usize,
    last_hex: Pos,
    //Why the last hex entered wasn't used
    pub(super) refusal: Option<&'static str>,
}
//The pending edit, made, for drawing
pub(super) struct Preview{
    edit: LayoutEdit,
    //The part it places or changes
    part: PartId,
    layout: InitialWorld,
    pub(super) world: std::result::Result<World, String>,
    float_world: FloatWorld,
    polymer: Option<Polymer>,
    track: Option<TrackBindings>,
    landing: SmallVec<[Pos; 8]>,
    overlaps: SmallVec<[Pos; 8]>,
}
impl Press{
    pub(super) fn part(&self) -> PartId{
        self.stack[self.cand]
    }
    pub(super) fn kind(&self) -> EditKind{
        if self.duplicate {EditKind::Duplicate} else {EditKind::Move}
    }
    //Where the grabbed point of the part is now
    fn held_xy(&self, drag: &Drag) -> XYPos{
        pos_to_xy(self.press_hex) + (drag.cursor_xy - self.press_xy)
    }
    //The edit that dropping here would make (snapped to the nearest hex)
    fn edit(&self, drag: &Drag) -> LayoutEdit{
        let landing = xy_to_pos(self.held_xy(drag));
        let edit = PartEdit{part: self.part(), delta: landing - self.press_hex, rot: drag.rot, pivot: landing,
            len: drag.len, arm_type: None};
        if self.duplicate {LayoutEdit::Duplicate(edit)} else {LayoutEdit::Part(edit)}
    }
    //How far the dragged part is drawn from its landing spot
    fn drag_offset(&self, drag: &Drag) -> XYVec{
        let held = self.held_xy(drag);
        held - pos_to_xy(xy_to_pos(held))
    }
}
impl Loaded{
    //Why the layout can't be changed right now, if it can't
    pub(super) fn edit_lock(&self) -> Option<&'static str>{
        if !EDITOR_ENABLED{
            Some("Editing is disabled")
        } else if matches!(self.run_state, RunState::FreeRun){
            Some("Editing is disabled while running")
        } else {
            None
        }
    }
    //Why the part can't be edited this way right now, if it can't
    pub(super) fn part_lock(&self, part: PartId, kind: EditKind) -> Option<&'static str>{
        use EditKind::*;
        if let Some(reason) = self.edit_lock(){
            return Some(reason);
        }
        match (part, kind){
            //At a later step an arm isn't where it starts, so moving it there would be unclear
            (PartId::Arm(_), Move | Duplicate) if !self.timeline.at_start() =>
                Some("Arms can only be moved or copied at the initial timestep"),
            (PartId::Glyph(i), Delete | Duplicate) if is_puzzle_part(&self.timeline.base().glyphs[i].glyph_type) =>
                Some("Inputs, outputs and conduits come with the puzzle"),
            _ => None,
        }
    }
    fn layout_edit_lock(&self, edit: &LayoutEdit) -> Option<&'static str>{
        match edit{
            LayoutEdit::Part(e) => self.part_lock(e.part, EditKind::Move),
            LayoutEdit::Duplicate(e) => self.part_lock(e.part, EditKind::Duplicate),
            LayoutEdit::Delete(part) => self.part_lock(*part, EditKind::Delete),
            LayoutEdit::MoveArm{from, ..} => self.part_lock(PartId::Arm(*from), EditKind::Reorder),
            LayoutEdit::Add{..} | LayoutEdit::SetTrack{..} => self.edit_lock(),
        }
    }
    //Why the held part can't be dragged, if it can't
    pub(super) fn press_lock(&self) -> Option<&'static str>{
        let press = self.press.as_ref()?;
        self.part_lock(press.part(), press.kind())
    }
    //The part to outline: the candidate while the mouse is held, otherwise the selection
    pub(super) fn highlighted_part(&self) -> Option<PartId>{
        self.press.as_ref().map(|p| p.part()).or(self.selection)
    }
    //A track mode ends when anything else is selected
    pub(super) fn select(&mut self, part: Option<PartId>){
        self.selection = part;
        if self.track_mode.as_ref().is_some_and(|m| m.glyph.map(PartId::Glyph) != part){
            self.end_track_mode();
        }
    }
    fn check_overlap(&self, overlaps: &[Pos]) -> std::result::Result<(), String>{
        match overlaps.first(){
            Some(hex) if !self.allow_overlap =>
                Err(format!("That would overlap another part at {:?} (see \"Allow overlap\")", (hex.x, hex.y))),
            _ => Ok(()),
        }
    }
    //Makes the edit if it's allowed. A part it adds or renumbers is selected.
    pub(super) fn apply_edit(&mut self, edit: &LayoutEdit) -> std::result::Result<(), String>{
        if matches!(edit, LayoutEdit::Part(e) if e.is_noop()){
            return Ok(());
        }
        if let Some(reason) = self.layout_edit_lock(edit){
            return Err(reason.to_owned());
        }
        let (layout, part) = edit.apply(self.timeline.base());
        let world = World::setup_sim(&layout).map_err(|e| e.to_string())?;
        if let Some(part) = part{
            self.check_overlap(&world.part_overlaps(part))?;
        }
        self.commit_world(world);
        if edit.is_structural(){
            self.stack_view = None;
            self.select(part);
        }
        Ok(())
    }
    pub(super) fn cancel_press(&mut self){
        self.press = None;
    }
    pub(super) fn start_placing(&mut self, part: NewPart){
        self.cancel_press();
        self.end_track_mode();
        self.placing = Some(Placing{part, rot: 0, len: 1, cursor_xy: None});
    }
    pub(super) fn start_new_track(&mut self){
        self.cancel_press();
        self.placing = None;
        self.select(None);
        self.track_mode = Some(TrackMode{glyph: None, tool: TrackTool::AddBack});
    }
    //Sets (or with None, ends) the board tool for reshaping the track
    pub(super) fn set_track_tool(&mut self, glyph: usize, tool: Option<TrackTool>){
        self.cancel_press();
        self.placing = None;
        self.end_track_mode();
        self.track_mode = tool.map(|tool| TrackMode{glyph: Some(glyph), tool});
    }
    pub(super) fn end_track_mode(&mut self){
        self.track_mode = None;
        self.stroke = None;
    }
    //A right-click (without panning) puts away the part being placed or ends the track mode
    pub(super) fn right_click(&mut self){
        if self.placing.is_some(){
            self.placing = None;
        } else {
            self.end_track_mode();
        }
    }
    pub(super) fn press(&mut self, px: (f32, f32), screen_size: (f32, f32), keymods: KeyMods){
        self.cancel_press();
        let press_xy = self.camera.screen_to_world(px, screen_size);
        let press_hex = xy_to_pos(press_xy);
        if self.placing.is_some(){
            return self.place(press_hex, keymods.shift);
        }
        if self.track_mode.is_some(){
            return self.start_stroke(press_hex);
        }
        //Arms are picked where they are now (glyphs don't move)
        let stack = self.timeline.curr().parts_at(press_hex);
        if stack.is_empty(){
            self.select(None);
            self.stack_view = None;
            return;
        }
        let cand = self.selection.and_then(|s| stack.iter().position(|&p| p == s)).unwrap_or(0);
        self.stack_view = Some(stack.clone());
        self.press = Some(Press{press_px: px, press_xy, press_hex, stack, cand, wheel_accum: 0.,
            duplicate: keymods.ctrl || keymods.logo, drag: None});
    }
    //Puts the part on the cursor down; with `keep`, another stays on the cursor
    fn place(&mut self, hex: Pos, keep: bool){
        let Some(placing) = &self.placing else {return};
        let edit = LayoutEdit::Add{part: placing.part.clone(), pos: hex, rot: placing.rot, len: placing.len};
        match self.apply_edit(&edit){
            Ok(()) => if !keep {self.placing = None},
            Err(err) => self.message = Some(err),
        }
    }
    fn start_stroke(&mut self, hex: Pos){
        let Some(mode) = &self.track_mode else {return};
        let mut stroke = match mode.glyph{
            Some(i) => TrackStroke{path: track_path(self.timeline.base(), i), added: 0, last_hex: hex, refusal: None},
            //A new track starts where it's pressed
            None => {
                if let Err(err) = self.check_overlap(&self.timeline.base().track_overlaps(None, &[hex])){
                    self.message = Some(err);
                    return;
                }
                TrackStroke{path: vec![hex], added: 0, last_hex: hex, refusal: None}
            }
        };
        self.stroke_hex(&mut stroke, hex);
        self.stroke = Some(stroke);
    }
    //Applies the track tool to a hex the stroke entered
    fn stroke_hex(&self, stroke: &mut TrackStroke, hex: Pos){
        let Some(mode) = &self.track_mode else {return};
        let blocked = mode.tool != TrackTool::Remove && !self.allow_overlap && !stroke.path.contains(&hex)
            && !self.timeline.base().track_overlaps(mode.glyph, &[hex]).is_empty();
        let result = if blocked{
            Err("That would overlap another part (see \"Allow overlap\")")
        } else {
            stroke_hex(&mut stroke.path, &mut stroke.added, mode.tool, hex, self.allow_gaps, self.allow_overlap)
        };
        stroke.refusal = result.err();
    }
    fn finish_stroke(&mut self){
        let Some(stroke) = self.stroke.take() else {return};
        let Some(glyph) = self.track_mode.as_ref().map(|m| m.glyph) else {return};
        if glyph.is_some_and(|i| track_path(self.timeline.base(), i) == stroke.path){
            return;
        }
        match self.apply_edit(&LayoutEdit::SetTrack{glyph, path: stroke.path}){
            //A new track keeps being drawn
            Ok(()) => if glyph.is_none(){
                let new = self.timeline.base().glyphs.len()-1;
                self.track_mode = Some(TrackMode{glyph: Some(new), tool: TrackTool::AddBack});
            }
            Err(err) => self.message = Some(err),
        }
    }
    pub(super) fn pointer_moved(&mut self, px: (f32, f32), screen_size: (f32, f32), over_ui: bool){
        let cursor_xy = self.camera.screen_to_world(px, screen_size);
        self.pointer_over_ui = over_ui;
        if let Some(placing) = &mut self.placing{
            placing.cursor_xy = (!over_ui).then_some(cursor_xy);
        }
        if let Some(mut stroke) = self.stroke.take(){
            //Fast movements skip hexes, so go through each one on the way
            let hex = xy_to_pos(cursor_xy);
            if hex != stroke.last_hex{
                for h in hex_line(stroke.last_hex, hex).into_iter().skip(1){
                    self.stroke_hex(&mut stroke, h);
                    if stroke.refusal.is_some(){
                        break;
                    }
                }
                stroke.last_hex = hex;
            }
            self.stroke = Some(stroke);
        }
        let draggable = self.press.is_some() && self.press_lock().is_none();
        let Some(press) = &mut self.press else {return};
        match &mut press.drag{
            Some(drag) => drag.cursor_xy = cursor_xy,
            None => {
                let (dx, dy) = (px.0 - press.press_px.0, px.1 - press.press_px.1);
                if draggable && dx*dx + dy*dy > DRAG_THRESHOLD_PX*DRAG_THRESHOLD_PX{
                    press.drag = Some(Drag{cursor_xy, rot: 0, len: 0});
                }
            }
        }
    }
    //Picks another part from the stack. A drag carries over to it (keeping the offset) if it can be edited;
    //otherwise the drag ends, and starts again on the next mouse movement once an editable part is picked.
    fn cycle(&mut self, step: i32){
        let Some(press) = &mut self.press else {return};
        press.cand = (press.cand as i32 + step).rem_euclid(press.stack.len() as i32) as usize;
        let locked = self.press_lock().is_some();
        let Some(press) = &mut self.press else {return};
        if locked{
            press.drag = None;
        } else if let Some(drag) = &mut press.drag{
            drag.rot = 0;
            drag.len = 0;
        }
    }
    pub(super) fn release(&mut self){
        if self.stroke.is_some(){
            return self.finish_stroke();
        }
        let Some(press) = self.press.take() else {return};
        self.select(Some(press.part()));
        if let Some(drag) = &press.drag{
            if let Err(err) = self.apply_edit(&press.edit(drag)){
                self.message = Some(err);
            }
        }
    }
    //Returns true if the wheel was used to pick a part (rather than zoom)
    pub(super) fn wheel(&mut self, dy: f32) -> bool{
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
    //Returns true if the key was used for editing parts
    pub(super) fn handle_key(&mut self, keycode: KeyCode, keymods: KeyMods) -> bool{
        let command = keymods.ctrl || keymods.logo;
        //Only picking from the stack works without the editor
        if !EDITOR_ENABLED && !matches!(keycode, KeyCode::Tab | KeyCode::Escape){
            return false;
        }
        match keycode{
            KeyCode::Z if command => if keymods.shift {self.redo()} else {self.undo()},
            KeyCode::Y if command => self.redo(),
            KeyCode::Delete | KeyCode::Backspace => {
                let Some(part) = self.highlighted_part() else {return false};
                self.cancel_press();
                if let Err(err) = self.apply_edit(&LayoutEdit::Delete(part)){
                    self.message = Some(err);
                }
            }
            KeyCode::Escape => {
                if self.press.is_some(){
                    self.cancel_press();
                } else if self.stroke.is_some(){
                    self.stroke = None;
                } else if self.placing.is_some(){
                    self.placing = None;
                } else if self.track_mode.is_some(){
                    self.end_track_mode();
                } else {
                    return false;
                }
            }
            KeyCode::Tab if self.press.is_some() => self.cycle(if keymods.shift {-1} else {1}),
            KeyCode::A | KeyCode::D | KeyCode::W | KeyCode::S => return self.turn_or_resize(keycode),
            _ => return false,
        }
        true
    }
    //A/D turn and W/S lengthen or shorten the part being placed or dragged
    fn turn_or_resize(&mut self, keycode: KeyCode) -> bool{
        let turn = match keycode {KeyCode::A => 1, KeyCode::D => -1, _ => 0};
        let grow = match keycode {KeyCode::W => 1, KeyCode::S => -1, _ => 0};
        if let Some(placing) = &mut self.placing{
            placing.rot = (placing.rot + turn) % 6;
            if let NewPart::Arm(arm_type) = placing.part{
                placing.len = arm_type.clamp_len(placing.len + grow);
            }
            return true;
        }
        let Some(press) = &mut self.press else {return false};
        let part = press.part();
        let Some(drag) = &mut press.drag else {return false};
        drag.rot += turn;
        if let PartId::Arm(i) = part{
            let arm = &self.timeline.base().arms[i];
            drag.len = arm.arm_type.clamp_len(arm.len + drag.len + grow) - arm.len;
        }
        true
    }
    //The edit the mouse would make now, and how far from its hex the moving part is drawn
    fn pending_edit(&self) -> Option<(LayoutEdit, XYVec)>{
        if let Some(stroke) = &self.stroke{
            let glyph = self.track_mode.as_ref()?.glyph;
            return Some((LayoutEdit::SetTrack{glyph, path: stroke.path.clone()}, XYVec::zeros()));
        }
        if let Some(press) = &self.press{
            let drag = press.drag.as_ref()?;
            return Some((press.edit(drag), press.drag_offset(drag)));
        }
        let placing = self.placing.as_ref()?;
        let cursor_xy = placing.cursor_xy?;
        let hex = xy_to_pos(cursor_xy);
        let edit = LayoutEdit::Add{part: placing.part.clone(), pos: hex, rot: placing.rot, len: placing.len};
        Some((edit, cursor_xy - pos_to_xy(hex)))
    }
    //The hexes the track mode can work on (as the stroke has the path so far): the end being added to,
    //or the hexes that can be removed
    fn track_hexes_being_edited(&self) -> Vec<Pos>{
        let Some(mode) = &self.track_mode else {return Vec::new()};
        let path = match (&self.stroke, mode.glyph){
            (Some(stroke), _) => stroke.path.clone(),
            (None, Some(glyph)) => track_path(self.timeline.base(), glyph),
            (None, None) => return Vec::new(),
        };
        let ends = || path.first().into_iter().chain(path.last()).copied().collect();
        match mode.tool{
            TrackTool::AddFront => path.first().copied().into_iter().collect(),
            TrackTool::AddBack => path.last().copied().into_iter().collect(),
            //The last hex can't be removed
            TrackTool::Remove if path.len() < 2 => Vec::new(),
            TrackTool::Remove if self.allow_gaps => path.clone(),
            TrackTool::Remove => ends(),
        }
    }
    //The track an edit moves or reshapes, which the scene draws without
    fn changed_track(&self, edit: &LayoutEdit) -> Option<usize>{
        match edit{
            LayoutEdit::Part(PartEdit{part: PartId::Glyph(i), ..}) if self.timeline.base().glyphs[*i].is_track() => Some(*i),
            LayoutEdit::SetTrack{glyph, ..} => *glyph,
            _ => None,
        }
    }
    fn build_preview(&self, ctx: &mut dyn RenderingBackend, edit: LayoutEdit) -> Preview{
        let (layout, part) = edit.apply(self.timeline.base());
        let part = part.expect("a pending edit places or changes a part");
        let (polymer, track, landing) = match part{
            PartId::Glyph(i) => {
                let glyph = &layout.glyphs[i];
                let polymer = glyph.build_polymer().ok().flatten();
                let track = glyph.is_track().then(||
                    setup_tracks(ctx, &TrackMaps::build([glyph]).unwrap_or_default()));
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
                world.part_overlaps(part)
            }
            Err(_) => landing.clone(),
        };
        Preview{edit, part, layout, world, float_world, polymer, track, landing, overlaps}
    }
    pub(super) fn drop_preview(&mut self){
        if let Some(track) = self.preview.take().and_then(|p| p.track){
            self.dead_tracks.push(track);
        }
        if let Some((_, tracks)) = self.scene_tracks.take(){
            self.dead_tracks.push(tracks);
        }
    }
    //Updates rendering data for part edits (needs the rendering context)
    pub(super) fn refresh_part_gfx(&mut self, ctx: &mut dyn RenderingBackend){
        if matches!(self.run_state, RunState::FreeRun){
            //Nothing can be edited while running
            if self.press.as_ref().is_some_and(|p| p.drag.is_some()){
                self.cancel_press();
            }
            self.placing = None;
            self.end_track_mode();
        }
        for tracks in self.dead_tracks.drain(..){
            tracks.delete(ctx);
        }
        if self.tracks_dirty{
            let old = std::mem::replace(&mut self.tracks, setup_tracks(ctx, &self.timeline.base().track_maps));
            old.delete(ctx);
            self.tracks_dirty = false;
        }
        let Some((edit, _)) = self.pending_edit() else {
            return self.drop_preview();
        };
        //Every track except the changed one, which is drawn with the preview
        let changed = self.changed_track(&edit);
        if self.scene_tracks.as_ref().map(|(i, _)| *i) != changed{
            let new = changed.map(|i| {
                let others = self.timeline.base().glyphs.iter().enumerate().filter(|&(j, _)| j != i).map(|(_, g)| g);
                (i, setup_tracks(ctx, &TrackMaps::build(others).unwrap_or_default()))
            });
            if let Some((_, old)) = std::mem::replace(&mut self.scene_tracks, new){
                self.dead_tracks.push(old);
            }
        }
        if self.preview.as_ref().is_some_and(|p| p.edit == edit){
            return;
        }
        let preview = self.build_preview(ctx, edit);
        if let Some(track) = self.preview.replace(preview).and_then(|p| p.track){
            self.dead_tracks.push(track);
        }
    }
    //Frees all rendering data, once the solution is no longer shown
    pub(super) fn delete_gfx(mut self, ctx: &mut dyn RenderingBackend){
        self.drop_preview();
        self.tracks.delete(ctx);
        for tracks in self.dead_tracks{
            tracks.delete(ctx);
        }
    }
    pub(super) fn draw_world(&self, render: &RenderDataBase, ctx: &mut Context){
        const SELECT_COLOR: [f32; 3] = [0., 0.8, 1.];
        const OVERLAP_COLOR: [f32; 3] = [1., 0.1, 0.1];
        const TRACK_TOOL_COLOR: [f32; 3] = [1., 0.75, 0.];
        let track_hexes = self.track_hexes_being_edited();
        let Some((preview, (_, extra))) = self.preview.as_ref().zip(self.pending_edit()) else {
            let world = self.timeline.curr();
            let scene = Scene{glyphs: &world.glyphs, polymers: &world.polymers, area: &world.area_touched,
                float_world: &self.float_world, hidden: None};
            render.draw(ctx, &self.camera, &self.tracks, self.show_area, &scene);
            if let Some(part) = self.highlighted_part(){
                render.draw_hex_rings(ctx, &self.camera, &world.part_footprint(part), SELECT_COLOR);
            }
            render.draw_hex_rings(ctx, &self.camera, &track_hexes, TRACK_TOOL_COLOR);
            return;
        };
        let part = preview.part;
        //Before anything moves, show the world as it would start; otherwise keep showing the current step.
        //A new part isn't in the current step, so hiding it there hides nothing.
        let (world, float_world) = match &preview.world{
            Ok(world) if self.timeline.at_start() => (world, &preview.float_world),
            _ => (self.timeline.curr(), &self.float_world),
        };
        let scene = Scene{glyphs: &world.glyphs, polymers: &world.polymers, area: &world.area_touched,
            float_world, hidden: Some(part)};
        let tracks = self.scene_tracks.as_ref().map_or(&self.tracks, |(_, t)| t);
        render.draw(ctx, &self.camera, tracks, self.show_area, &scene);
        match part{
            PartId::Glyph(i) => render.draw_dragged_glyph(ctx, &self.camera, &preview.layout.glyphs[i],
                preview.polymer.as_ref(), preview.track.as_ref(), extra),
            PartId::Arm(i) => render.draw_dragged_arm(ctx, &self.camera, &preview.layout.arms[i], i, extra),
        }
        render.draw_hex_rings(ctx, &self.camera, &preview.landing, SELECT_COLOR);
        render.draw_hex_rings(ctx, &self.camera, &preview.overlaps, OVERLAP_COLOR);
        render.draw_hex_rings(ctx, &self.camera, &track_hexes, TRACK_TOOL_COLOR);
    }
}
