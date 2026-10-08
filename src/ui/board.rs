//Picking, dragging and previewing parts on the board
use super::*;

//Moving the mouse this far (in pixels) with the button held starts a drag
const DRAG_THRESHOLD_PX: f32 = 4.;
//The left mouse button is held on the board
pub(super) struct Press{
    press_px: (f32, f32),
    press_xy: XYPos,
    press_hex: Pos,
    //The parts under the pressed hex, top first, and which one is picked
    pub(super) stack: Vec<PartId>,
    pub(super) cand: usize,
    wheel_accum: f32,
    pub(super) drag: Option<Drag>,
}
pub(super) struct Drag{
    cursor_xy: XYPos,
    //From the rotate/length hotkeys
    rot: Rot,
    len: i32,
    pub(super) preview: Option<Preview>,
    //Every track except the dragged one, when a track is dragged
    scene_tracks: Option<(PartId, TrackBindings)>,
}
//The dragged part placed at its landing spot
pub(super) struct Preview{
    edit: PartEdit,
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
impl Loaded{
    fn part_editable(&self, part: PartId) -> bool{
        self.part_lock(part).is_none()
    }
    //Why the part can't be edited right now, if it can't
    pub(super) fn part_lock(&self, part: PartId) -> Option<&'static str>{
        if !EDITOR_ENABLED{
            Some("Editing is disabled")
        } else if matches!(self.run_state, RunState::FreeRun){
            Some("Editing is disabled while running")
        } else if matches!(part, PartId::Arm(_)) && !self.timeline.at_start(){
            Some("Arms can only be edited at the initial timestep")
        } else {
            None
        }
    }
    //The part to outline: the candidate while the mouse is held, otherwise the selection
    pub(super) fn highlighted_part(&self) -> Option<PartId>{
        self.press.as_ref().map(|p| p.part()).or(self.selection)
    }
    fn check_overlap(&self, overlaps: &[Pos]) -> std::result::Result<(), String>{
        match overlaps.first(){
            Some(hex) if !self.allow_overlap =>
                Err(format!("That would overlap another part at {:?} (see \"Allow overlap\")", (hex.x, hex.y))),
            _ => Ok(()),
        }
    }
    pub(super) fn apply_edit(&mut self, edit: &PartEdit) -> std::result::Result<(), String>{
        if edit.is_noop(){
            return Ok(());
        }
        if let Some(reason) = self.part_lock(edit.part){
            return Err(reason.to_owned());
        }
        let world = World::setup_sim(&edited_layout(self.timeline.base(), edit)).map_err(|e| e.to_string())?;
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
    pub(super) fn cancel_press(&mut self){
        if let Some(drag) = self.press.take().and_then(|p| p.drag){
            self.retire_drag(drag);
        }
    }
    pub(super) fn press(&mut self, px: (f32, f32), screen_size: (f32, f32)){
        self.cancel_press();
        let press_xy = self.camera.screen_to_world(px, screen_size);
        let press_hex = xy_to_pos(press_xy);
        //Arms are picked where they are now (glyphs don't move)
        let stack = self.timeline.curr().parts_at(press_hex);
        if stack.is_empty(){
            self.selection = None;
            self.stack_view = None;
            return;
        }
        let cand = self.selection.and_then(|s| stack.iter().position(|&p| p == s)).unwrap_or(0);
        self.stack_view = Some(stack.clone());
        self.press = Some(Press{press_px: px, press_xy, press_hex, stack, cand, wheel_accum: 0., drag: None});
    }
    pub(super) fn pointer_moved(&mut self, px: (f32, f32), screen_size: (f32, f32)){
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
    //Picks another part from the stack. A drag carries over to it (keeping the offset) if it can be edited;
    //otherwise the drag ends, and starts again on the next mouse movement once an editable part is picked.
    fn cycle(&mut self, step: i32){
        let Some(press) = &mut self.press else {return};
        press.cand = (press.cand as i32 + step).rem_euclid(press.stack.len() as i32) as usize;
        let part = press.part();
        if !self.part_editable(part){
            if let Some(drag) = self.press.as_mut().and_then(|p| p.drag.take()){
                self.retire_drag(drag);
            }
        } else if let Some(drag) = self.press.as_mut().and_then(|p| p.drag.as_mut()){
            drag.rot = 0;
            drag.len = 0;
        }
    }
    pub(super) fn release(&mut self){
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
    //Returns true if the key was used for the held part
    pub(super) fn handle_key(&mut self, keycode: KeyCode, keymods: KeyMods) -> bool{
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
                        let arm = &self.timeline.base().arms[i];
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
        let layout = edited_layout(self.timeline.base(), &edit);
        let (polymer, track, landing) = match edit.part{
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
                self.timeline.base().overlaps_with_others(edit.part, &landing)
            }
            Err(_) => landing.clone(),
        };
        Preview{edit, layout, world, float_world, polymer, track, landing, overlaps}
    }
    //Updates rendering data for part edits (needs the rendering context)
    pub(super) fn refresh_part_gfx(&mut self, ctx: &mut dyn RenderingBackend){
        if matches!(self.run_state, RunState::FreeRun) && self.press.as_ref().is_some_and(|p| p.drag.is_some()){
            self.cancel_press();
        }
        for tracks in self.dead_tracks.drain(..){
            tracks.delete(ctx);
        }
        if self.tracks_dirty{
            let old = std::mem::replace(&mut self.tracks, setup_tracks(ctx, &self.timeline.base().track_maps));
            old.delete(ctx);
            self.tracks_dirty = false;
        }
        let Some(press) = &self.press else {return};
        let Some(drag) = &press.drag else {return};
        let edit = press.edit(drag);
        let part = edit.part;
        let dragging_track = matches!(part, PartId::Glyph(i) if self.timeline.base().glyphs[i].is_track());
        let scene_tracks_part = drag.scene_tracks.as_ref().map(|(p, _)| *p);
        let new_scene_tracks = if !dragging_track{
            None
        } else if scene_tracks_part == Some(part){
            return self.refresh_preview(ctx, edit);
        } else {
            let others = self.timeline.base().glyphs.iter().enumerate()
                .filter(|&(i, _)| PartId::Glyph(i) != part).map(|(_, g)| g);
            Some((part, setup_tracks(ctx, &TrackMaps::build(others).unwrap_or_default())))
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
    pub(super) fn draw_world(&self, render: &RenderDataBase, ctx: &mut Context){
        const SELECT_COLOR: [f32; 3] = [0., 0.8, 1.];
        const OVERLAP_COLOR: [f32; 3] = [1., 0.1, 0.1];
        let dragging = self.press.as_ref().and_then(|press| {
            let drag = press.drag.as_ref()?;
            Some((press, drag, drag.preview.as_ref()?))
        });
        let Some((press, drag, preview)) = dragging else {
            let world = self.timeline.curr();
            let scene = Scene{glyphs: &world.glyphs, polymers: &world.polymers, area: &world.area_touched,
                float_world: &self.float_world, hidden: None};
            render.draw(ctx, &self.camera, &self.tracks, self.show_area, &scene);
            if let Some(part) = self.highlighted_part(){
                render.draw_hex_rings(ctx, &self.camera, &world.part_footprint(part), SELECT_COLOR);
            }
            return;
        };
        let part = preview.edit.part;
        //Before anything moves, show the world as it would start; otherwise keep showing the current step
        let (world, float_world) = match &preview.world{
            Ok(world) if self.timeline.at_start() => (world, &preview.float_world),
            _ => (self.timeline.curr(), &self.float_world),
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
}
