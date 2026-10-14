//Picking, dragging and previewing parts on the board, placing new ones, and reshaping track
use super::*;

//Moving the mouse this far (in pixels) with the button held starts a drag
pub(super) const DRAG_THRESHOLD_PX: f32 = 4.;
const TRACK_OUTSIDE_CHAMBERS: &str = "That would put track outside the chambers (see \"Allow overlap\")";
//What an edit does to a part, for checking whether it's allowed right now
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum EditKind{
    //Moving, rotating, resizing, or changing an arm's type
    Move, Delete, Duplicate, Reorder,
}
//What the left mouse button does on the board
pub(super) enum BoardTool{
    //Clicking picks parts, and dragging one moves it. Holds the press while the button is down.
    Select(Option<Press>),
    //Clicking places a new part
    Place(Placing),
    //Clicking and dragging reshape a track. Holds the stroke while the button is down.
    Track(TrackMode, Option<TrackStroke>),
}
impl BoardTool{
    pub(super) fn press(&self) -> Option<&Press>{
        match self {BoardTool::Select(press) => press.as_ref(), _ => None}
    }
    fn press_mut(&mut self) -> Option<&mut Press>{
        match self {BoardTool::Select(press) => press.as_mut(), _ => None}
    }
    pub(super) fn placing(&self) -> Option<&Placing>{
        match self {BoardTool::Place(placing) => Some(placing), _ => None}
    }
    pub(super) fn track_mode(&self) -> Option<&TrackMode>{
        match self {BoardTool::Track(mode, _) => Some(mode), _ => None}
    }
    pub(super) fn stroke(&self) -> Option<&TrackStroke>{
        match self {BoardTool::Track(_, stroke) => stroke.as_ref(), _ => None}
    }
}
//Left mouse button input for the board, in logical pixels on a screen of size `screen`
pub(super) enum BoardInput{
    Press{px: (f32, f32), screen: (f32, f32), keymods: KeyMods},
    Move{px: (f32, f32), screen: (f32, f32), over_ui: bool},
    Release,
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
    track: Option<TrackMaps>,
    //An arm as the edit would have it at `timestep`, the timeline's current one
    arm_now: Option<Arm>,
    timestep: u64,
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
            LayoutEdit::ReorderArm{from, ..} => self.part_lock(PartId::Arm(*from), EditKind::Reorder),
            LayoutEdit::ReorderGlyph{from, ..} => self.part_lock(PartId::Glyph(*from), EditKind::Reorder),
            LayoutEdit::Add{..} | LayoutEdit::SetTrack{..} => self.edit_lock(),
        }
    }
    //Why the held part can't be dragged, if it can't
    pub(super) fn press_lock(&self) -> Option<&'static str>{
        let press = self.tool.press()?;
        self.part_lock(press.part(), press.kind())
    }
    //The part to outline: the candidate while the mouse is held, otherwise the selection
    pub(super) fn highlighted_part(&self) -> Option<PartId>{
        self.tool.press().map(|p| p.part()).or(self.selection)
    }
    //A track tool ends when anything but its track is selected (a new track has none yet, so it keeps on
    //when nothing is selected). So does Path text, which would otherwise be shown again unapplied.
    pub(super) fn select(&mut self, part: Option<PartId>){
        self.selection = part;
        if self.tool.track_mode().is_some_and(|m| m.glyph.map(PartId::Glyph) != part){
            self.end_track_mode();
        }
        if self.path_text.as_ref().is_some_and(|p| Some(PartId::Glyph(p.glyph)) != part){
            self.path_text = None;
        }
    }
    fn check_overlap(&self, overlaps: &[Pos]) -> std::result::Result<(), String>{
        match overlaps.first(){
            Some(hex) if !self.allow_overlap =>
                Err(format!("That would overlap another part at {:?} (see \"Allow overlap\")", (hex.x, hex.y))),
            _ => Ok(()),
        }
    }
    //Refuses a part that breaks a production puzzle's rules about chambers, unless overlap is allowed
    fn check_chambers(&self, world: &World, part: PartId) -> std::result::Result<(), String>{
        if self.allow_overlap{
            return Ok(());
        }
        if let Some(hex) = world.chamber_conflicts(part).first(){
            return Err(format!("That would put a part outside its chamber at {:?} (see \"Allow overlap\")", (hex.x, hex.y)));
        }
        if !world.isolation_conflicts(part).is_empty(){
            return Err("Inputs and outputs can't share a chamber in this puzzle (see \"Allow overlap\")".to_owned());
        }
        Ok(())
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
            self.check_chambers(&world, part)?;
        }
        self.commit_world(world);
        self.follow_renumbering(edit);
        if edit.is_structural(){
            self.select(part);
        }
        Ok(())
    }
    //Keeps the track tool, the Path text and the ends of the tapes' selected arms on the parts they had through
    //`edit`, which may renumber them. The tool and text end if their track is deleted. Clicks and drags in the
    //windows hold numbers this can't reach, so they check numbering_version instead.
    fn follow_renumbering(&mut self, edit: &LayoutEdit){
        if edit.renumbers(){
            self.numbering_version += 1;
        }
        let renumbered_glyph = |i| match edit.renumbered(PartId::Glyph(i)){
            Some(PartId::Glyph(i)) => Some(i),
            _ => None,
        };
        if let BoardTool::Track(TrackMode{glyph: Some(track), ..}, _) = &mut self.tool{
            match renumbered_glyph(*track){
                Some(now) => *track = now,
                None => self.end_track_mode(),
            }
        }
        self.path_text = self.path_text.take().and_then(|mut text| {
            text.glyph = renumbered_glyph(text.glyph)?;
            Some(text)
        });
        //A deleted arm's number is left for TapeSelection::fit to keep in range
        let sel = &mut self.tape_sel;
        for end in [&mut sel.arm, &mut sel.anchor_arm]{
            if let Some(PartId::Arm(now)) = edit.renumbered(PartId::Arm(*end)){
                *end = now;
            }
        }
    }
    pub(super) fn cancel_press(&mut self){
        if let BoardTool::Select(press) = &mut self.tool{
            *press = None;
        }
    }
    pub(super) fn start_placing(&mut self, part: NewPart){
        self.tool = BoardTool::Place(Placing{part, rot: 0, len: 1, cursor_xy: None});
    }
    pub(super) fn start_new_track(&mut self){
        self.select(None);
        self.tool = BoardTool::Track(TrackMode{glyph: None, tool: TrackTool::AddBack}, None);
    }
    //Sets (or with None, ends) the board tool for reshaping the track
    pub(super) fn set_track_tool(&mut self, glyph: usize, tool: Option<TrackTool>){
        self.tool = match tool{
            Some(tool) => BoardTool::Track(TrackMode{glyph: Some(glyph), tool}, None),
            None => BoardTool::Select(None),
        };
    }
    pub(super) fn end_track_mode(&mut self){
        if matches!(self.tool, BoardTool::Track(..)){
            self.tool = BoardTool::Select(None);
        }
    }
    //Goes back to picking parts, putting away the part being placed or ending the track mode
    pub(super) fn end_tool(&mut self){
        if !matches!(self.tool, BoardTool::Select(_)){
            self.tool = BoardTool::Select(None);
        }
    }
    //A right-click (without panning) puts away the part being placed or ends the track mode
    pub(super) fn right_click(&mut self){
        self.end_tool();
    }
    //Takes mouse input for the board. Mouse events arrive before egui's frame sees them, so a press made while a text
    //field is being typed in waits, along with the input after it, until egui has applied the field (see
    //flush_board_input). Otherwise the press could select another part first, and the field's value would go to it.
    pub(super) fn board_input(&mut self, input: BoardInput, typing: bool){
        if !self.waiting_input.is_empty() || (typing && matches!(input, BoardInput::Press{..})){
            self.waiting_input.push(input);
        } else {
            self.apply_board_input(input);
        }
    }
    //Applies the input that waited for egui's frame
    pub(super) fn flush_board_input(&mut self){
        for input in std::mem::take(&mut self.waiting_input){
            self.apply_board_input(input);
        }
    }
    fn apply_board_input(&mut self, input: BoardInput){
        match input{
            BoardInput::Press{px, screen, keymods} => self.press(px, screen, keymods),
            BoardInput::Move{px, screen, over_ui} => self.pointer_moved(px, screen, over_ui),
            BoardInput::Release => self.release(),
        }
    }
    pub(super) fn press(&mut self, px: (f32, f32), screen_size: (f32, f32), keymods: KeyMods){
        self.cancel_press();
        let press_xy = self.camera.screen_to_world(px, screen_size);
        let press_hex = xy_to_pos(press_xy);
        match self.tool{
            BoardTool::Place(_) => return self.place(press_hex, keymods.shift),
            BoardTool::Track(..) => return self.start_stroke(press_hex),
            BoardTool::Select(_) => {}
        }
        //Arms are picked where they are now (glyphs don't move)
        let stack = self.timeline.curr().parts_at(press_hex);
        if stack.is_empty(){
            self.select(None);
            self.list_hex = None;
            return;
        }
        let cand = self.selection.and_then(|s| stack.iter().position(|&p| p == s)).unwrap_or(0);
        self.list_hex = Some(press_hex);
        self.tool = BoardTool::Select(Some(Press{press_px: px, press_xy, press_hex, stack, cand, wheel_accum: 0.,
            duplicate: keymods.ctrl || keymods.logo, drag: None}));
    }
    //Puts the part on the cursor down; with `keep`, another stays on the cursor
    fn place(&mut self, hex: Pos, keep: bool){
        let Some(placing) = self.tool.placing() else {return};
        let edit = LayoutEdit::Add{part: placing.part.clone(), pos: hex, rot: placing.rot, len: placing.len};
        match self.apply_edit(&edit){
            Ok(()) => if !keep {self.end_tool()},
            Err(err) => self.message = Some(err),
        }
    }
    fn start_stroke(&mut self, hex: Pos){
        let Some(mode) = self.tool.track_mode() else {return};
        let mut stroke = match mode.glyph{
            Some(i) => TrackStroke{path: track_path(self.timeline.base(), i), added: 0, last_hex: hex, refusal: None},
            //A new track starts where it's pressed
            None => {
                if let Err(err) = self.check_overlap(&self.timeline.base().track_overlaps(None, &[hex])){
                    self.message = Some(err);
                    return;
                }
                if !self.allow_overlap && !self.timeline.base().outside_chambers(&[hex]).is_empty(){
                    self.message = Some(TRACK_OUTSIDE_CHAMBERS.to_owned());
                    return;
                }
                TrackStroke{path: vec![hex], added: 0, last_hex: hex, refusal: None}
            }
        };
        self.stroke_hex(&mut stroke, hex);
        self.put_stroke(stroke);
    }
    //The stroke in progress, taken out of the track tool to work on it
    fn take_stroke(&mut self) -> Option<TrackStroke>{
        match &mut self.tool {BoardTool::Track(_, stroke) => stroke.take(), _ => None}
    }
    fn put_stroke(&mut self, stroke: TrackStroke){
        if let BoardTool::Track(_, slot) = &mut self.tool{
            *slot = Some(stroke);
        }
    }
    //Applies the track tool to a hex the stroke entered
    fn stroke_hex(&self, stroke: &mut TrackStroke, hex: Pos){
        let Some(mode) = self.tool.track_mode() else {return};
        let checked = mode.tool != TrackTool::Remove && !self.allow_overlap && !stroke.path.contains(&hex);
        let base = self.timeline.base();
        let result = if checked && !base.track_overlaps(mode.glyph, &[hex]).is_empty(){
            Err("That would overlap another part (see \"Allow overlap\")")
        } else if checked && !base.outside_chambers(&[hex]).is_empty(){
            Err(TRACK_OUTSIDE_CHAMBERS)
        } else {
            stroke_hex(&mut stroke.path, &mut stroke.added, mode.tool, hex, self.allow_gaps, self.allow_overlap)
        };
        stroke.refusal = result.err();
    }
    fn finish_stroke(&mut self){
        let Some(stroke) = self.take_stroke() else {return};
        let Some(glyph) = self.tool.track_mode().map(|m| m.glyph) else {return};
        if glyph.is_some_and(|i| track_path(self.timeline.base(), i) == stroke.path){
            return;
        }
        match self.apply_edit(&LayoutEdit::SetTrack{glyph, path: stroke.path}){
            //A new track keeps being drawn
            Ok(()) => if glyph.is_none(){
                let new = self.timeline.base().glyphs.len()-1;
                self.tool = BoardTool::Track(TrackMode{glyph: Some(new), tool: TrackTool::AddBack}, None);
            }
            Err(err) => self.message = Some(err),
        }
    }
    pub(super) fn pointer_moved(&mut self, px: (f32, f32), screen_size: (f32, f32), over_ui: bool){
        let cursor_xy = self.camera.screen_to_world(px, screen_size);
        self.pointer_over_ui = over_ui;
        //Panning and zooming keep the world under the mouse where it is, so this stays right until it moves again
        if !over_ui{
            self.hovered_hex = Some(xy_to_pos(cursor_xy));
        }
        if let BoardTool::Place(placing) = &mut self.tool{
            placing.cursor_xy = (!over_ui).then_some(cursor_xy);
        }
        if let Some(mut stroke) = self.take_stroke(){
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
            self.put_stroke(stroke);
        }
        let draggable = self.tool.press().is_some() && self.press_lock().is_none();
        let Some(press) = self.tool.press_mut() else {return};
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
        let Some(press) = self.tool.press_mut() else {return};
        press.cand = (press.cand as i32 + step).rem_euclid(press.stack.len() as i32) as usize;
        let locked = self.press_lock().is_some();
        let Some(press) = self.tool.press_mut() else {return};
        if locked{
            press.drag = None;
        } else if let Some(drag) = &mut press.drag{
            drag.rot = 0;
            drag.len = 0;
        }
    }
    pub(super) fn release(&mut self){
        let press = match &mut self.tool{
            BoardTool::Select(press) => press.take(),
            BoardTool::Track(..) => return self.finish_stroke(),
            BoardTool::Place(_) => None,
        };
        let Some(press) = press else {return};
        self.select(Some(press.part()));
        if let Some(drag) = &press.drag{
            if let Err(err) = self.apply_edit(&press.edit(drag)){
                self.message = Some(err);
            }
        }
    }
    //Returns true if the wheel was used to pick a part (rather than zoom)
    pub(super) fn wheel(&mut self, dy: f32) -> bool{
        let Some(press) = self.tool.press_mut() else {return false};
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
            //Lets go of one thing at a time: the press or stroke, then the part being placed or the track mode
            KeyCode::Escape => {
                self.tool = match std::mem::replace(&mut self.tool, BoardTool::Select(None)){
                    BoardTool::Select(None) => return false,
                    BoardTool::Track(mode, Some(_)) => BoardTool::Track(mode, None),
                    _ => BoardTool::Select(None),
                };
            }
            KeyCode::Tab if self.tool.press().is_some() => self.cycle(if keymods.shift {-1} else {1}),
            KeyCode::A | KeyCode::D | KeyCode::W | KeyCode::S => return self.turn_or_resize(keycode),
            _ => return false,
        }
        true
    }
    //A/D turn and W/S lengthen or shorten the part being placed or dragged
    fn turn_or_resize(&mut self, keycode: KeyCode) -> bool{
        let turn = match keycode {KeyCode::A => 1, KeyCode::D => -1, _ => 0};
        let grow = match keycode {KeyCode::W => 1, KeyCode::S => -1, _ => 0};
        if let BoardTool::Place(placing) = &mut self.tool{
            placing.rot = (placing.rot + turn) % 6;
            if let NewPart::Arm(arm_type) = placing.part{
                placing.len = arm_type.clamp_len(placing.len + grow);
            }
            return true;
        }
        let Some(press) = self.tool.press_mut() else {return false};
        let part = press.part();
        let Some(drag) = &mut press.drag else {return false};
        drag.rot += turn;
        if let PartId::Arm(i) = part{
            let arm = &self.timeline.base().arms[i];
            drag.len = arm.arm_type.clamp_len(arm.len + drag.len + grow) - arm.len;
        }
        true
    }
    //The edit the mouse would make now (or else the path being typed would), and how far from its hex the moving
    //part is drawn
    fn pending_edit(&self) -> Option<(LayoutEdit, XYVec)>{
        self.tool_edit().or_else(|| {
            let (glyph, path) = self.typed_track_path()?;
            (path != track_path(self.timeline.base(), glyph))
                .then(|| (LayoutEdit::SetTrack{glyph: Some(glyph), path}, XYVec::zeros()))
        })
    }
    fn tool_edit(&self) -> Option<(LayoutEdit, XYVec)>{
        match &self.tool{
            BoardTool::Track(mode, Some(stroke)) =>
                Some((LayoutEdit::SetTrack{glyph: mode.glyph, path: stroke.path.clone()}, XYVec::zeros())),
            BoardTool::Select(Some(press)) => {
                let drag = press.drag.as_ref()?;
                Some((press.edit(drag), press.drag_offset(drag)))
            }
            BoardTool::Place(placing) => {
                let cursor_xy = placing.cursor_xy?;
                let hex = xy_to_pos(cursor_xy);
                let edit = LayoutEdit::Add{part: placing.part.clone(), pos: hex, rot: placing.rot, len: placing.len};
                Some((edit, cursor_xy - pos_to_xy(hex)))
            }
            BoardTool::Select(None) | BoardTool::Track(_, None) => None,
        }
    }
    //The Path field's text while it's being typed in (for the selected track), and that track's first hex
    fn editing_path_text(&self) -> Option<(&PathText, Pos)>{
        let text = self.path_text.as_ref().filter(|p| p.editing)?;
        let is_track = self.timeline.base().glyphs.get(text.glyph).is_some_and(|g| g.is_track());
        if !is_track || self.selection != Some(PartId::Glyph(text.glyph)) || self.edit_lock().is_some(){
            return None;
        }
        Some((text, track_path(self.timeline.base(), text.glyph)[0]))
    }
    //The path being typed into the Path field (as far as it can be read), and the track it's for
    fn typed_track_path(&self) -> Option<(usize, Vec<Pos>)>{
        let (text, first) = self.editing_path_text()?;
        Some((text.glyph, parse_path_text(&text.text, first).0))
    }
    //The hex a step typed at the Path field's caret would start from, and the typed path's hexes after it,
    //which typing there shifts
    fn typed_caret(&self) -> Option<(Pos, Vec<Pos>)>{
        let (text, first) = self.editing_path_text()?;
        let before: String = text.text.chars().take(text.caret).collect();
        let before = parse_path_text(&before, first).0;
        //The whole text's path starts with the path before the caret: a step cut off by the caret just ends that
        //one early, leaving the step in the tail
        let tail = parse_path_text(&text.text, first).0.split_off(before.len());
        Some((before[before.len()-1], tail))
    }
    //The track mode, the path as the stroke has it so far, and how many hexes the stroke added
    fn track_tool_path(&self) -> Option<(&TrackMode, Vec<Pos>, usize)>{
        let BoardTool::Track(mode, stroke) = &self.tool else {return None};
        match (stroke, mode.glyph){
            (Some(stroke), _) => Some((mode, stroke.path.clone(), stroke.added)),
            (None, Some(glyph)) => Some((mode, track_path(self.timeline.base(), glyph), 0)),
            (None, None) => None,
        }
    }
    //The hexes the track mode can work on (as the stroke has the path so far): the end being added to,
    //or the hexes that can be removed
    fn track_hexes_being_edited(&self) -> Vec<Pos>{
        let Some((mode, path, _)) = self.track_tool_path() else {return Vec::new()};
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
    //The hexes next to the end being added to that the stroke would add (stepping back to retract isn't adding)
    fn track_extension_hexes(&self) -> Vec<Pos>{
        let Some((mode, path, added)) = self.track_tool_path() else {return Vec::new()};
        let end = match mode.tool{
            TrackTool::AddFront => path[0],
            TrackTool::AddBack => path[path.len()-1],
            TrackTool::Remove => return Vec::new(),
        };
        (0..6).map(|r| end + rot_to_pos(r)).filter(|&hex| {
            let mut stroke = TrackStroke{path: path.clone(), added, last_hex: end, refusal: None};
            self.stroke_hex(&mut stroke, hex);
            stroke.path.len() > path.len()
        }).collect()
    }
    //The track an edit moves or reshapes, which the scene draws without
    fn changed_track(&self, edit: &LayoutEdit) -> Option<usize>{
        match edit{
            LayoutEdit::Part(PartEdit{part: PartId::Glyph(i), ..}) if self.timeline.base().glyphs[*i].is_track() => Some(*i),
            LayoutEdit::SetTrack{glyph, ..} => *glyph,
            _ => None,
        }
    }
    fn build_preview(&self, edit: LayoutEdit) -> Preview{
        let base = self.timeline.base();
        let (layout, part) = edit.apply(base);
        let part = part.expect("a pending edit places or changes a part");
        let timestep = self.timeline.curr().timestep;
        let (polymer, track, arm_now, landing) = match part{
            PartId::Glyph(i) => {
                let glyph = &layout.glyphs[i];
                let polymer = glyph.build_polymer().ok().flatten();
                let track = glyph.is_track().then(|| TrackMaps::build([glyph]).unwrap_or_default());
                let landing = glyph.footprint(polymer.as_ref());
                (polymer, track, None, landing)
            }
            //Edits to arms leave the tracks and the loop length as they are
            PartId::Arm(i) => {
                let mut arm = layout.arms[i].clone();
                arm.run_moves(timestep, base.repeat_length, &base.track_maps);
                (None, None, Some(arm), smallvec![layout.arms[i].pos])
            }
        };
        let world = World::setup_sim(&layout).map_err(|e| e.to_string());
        let mut float_world = FloatWorld::new();
        let overlaps = match &world{
            Ok(world) => {
                float_world.generate_static(world);
                let mut overlaps = world.part_overlaps(part);
                overlaps.extend(world.chamber_conflicts(part));
                overlaps.extend(world.isolation_conflicts(part));
                overlaps
            }
            Err(_) => landing.clone(),
        };
        Preview{edit, part, layout, world, float_world, polymer, track, arm_now, timestep, landing, overlaps}
    }
    pub(super) fn drop_preview(&mut self){
        self.preview = None;
        self.scene_tracks = None;
    }
    //Keeps the preview of the pending edit up to date
    pub(super) fn refresh_preview(&mut self){
        if matches!(self.run_state, RunState::FreeRun){
            //Nothing can be edited while running, though parts can still be picked
            if let BoardTool::Select(press) = &mut self.tool{
                if press.as_ref().is_some_and(|p| p.drag.is_some()){
                    *press = None;
                }
            } else {
                self.tool = BoardTool::Select(None);
            }
        }
        let Some((edit, _)) = self.pending_edit() else {
            return self.drop_preview();
        };
        //Every track except the changed one, which is drawn with the preview
        let changed = self.changed_track(&edit);
        if self.scene_tracks.as_ref().map(|(i, _)| *i) != changed{
            self.scene_tracks = changed.map(|i| {
                let others = self.timeline.base().glyphs.iter().enumerate().filter(|&(j, _)| j != i).map(|(_, g)| g);
                (i, TrackMaps::build(others).unwrap_or_default())
            });
        }
        //The timeline can still be on its way to the target while a part is dragged
        if self.preview.as_ref().is_some_and(|p| p.edit == edit && p.timestep == self.timeline.curr().timestep){
            return;
        }
        self.preview = Some(self.build_preview(edit));
    }
    pub(super) fn draw_world(&self, render: &mut RenderDataBase, ctx: &mut Context){
        const CRASH_COLOR: Color = [1., 0.1, 0.1, 1.];
        self.draw_board(render, ctx);
        //Last, so nothing covers it. The board shows the moment of the crash, so it circles the parts that collided.
        if let RunState::Crashed{error, ..} = self.run_state{
            render.draw_ring(ctx, &self.camera, error.location, CRASH_COLOR);
        }
    }
    fn draw_board(&self, render: &mut RenderDataBase, ctx: &mut Context){
        const SELECT_COLOR: Color = [0., 0.8, 1., 1.];
        const OVERLAP_COLOR: Color = [1., 0.1, 0.1, 1.];
        const TRACK_TOOL_COLOR: Color = [1., 0.75, 0., 1.];
        const TRACK_REACH_COLOR: Color = [0.55, 0.4, 0., 1.];
        const HOVER_COLOR: Color = [1., 1., 1., 0.12];
        //A held or placed part shows where it lands instead
        let hover = self.hovered_hex.filter(|_| self.tool.press().is_none() && self.tool.placing().is_none());
        let mut track_hexes = self.track_hexes_being_edited();
        //Where typing in the Path field goes on from, labeled all round with the letters that step there, and the
        //hexes after it in the reach color (the track tools, which use it, end while typing)
        let (caret_hex, caret_tail) = self.typed_caret().unzip();
        let caret_tail = caret_tail.unwrap_or_default();
        track_hexes.extend(caret_hex);
        let reach_hexes = self.track_extension_hexes();
        let Some((preview, (_, extra))) = self.preview.as_ref().zip(self.pending_edit()) else {
            let world = self.timeline.curr();
            let scene = Scene{glyphs: &world.glyphs, tracks: &world.track_maps, polymers: &world.polymers,
                walls: &world.chambers.walls, area: &world.area_touched, float_world: &self.float_world, hidden: None};
            render.draw(ctx, &self.camera, self.area_mode == AreaMode::Shown, &scene);
            render.draw_hex_fills(ctx, &self.camera, hover.as_slice(), HOVER_COLOR);
            if let Some(part) = self.highlighted_part(){
                render.draw_hex_rings(ctx, &self.camera, &world.part_footprint(part), SELECT_COLOR);
            }
            render.draw_hex_rings(ctx, &self.camera, &caret_tail, TRACK_REACH_COLOR);
            render.draw_hex_rings(ctx, &self.camera, &reach_hexes, TRACK_REACH_COLOR);
            render.draw_hex_rings(ctx, &self.camera, &track_hexes, TRACK_TOOL_COLOR);
            if let Some(hex) = caret_hex{
                render.draw_step_letters(ctx, &self.camera, hex, TRACK_TOOL_COLOR);
            }
            return;
        };
        let part = preview.part;
        //Before anything moves, show the world as it would start; otherwise keep showing the current step.
        //A new part isn't in the current step, so hiding it there hides nothing.
        let (world, float_world) = match &preview.world{
            Ok(world) if self.timeline.at_start() => (world, &preview.float_world),
            _ => (self.timeline.curr(), &self.float_world),
        };
        //Tracks are drawn as they are now, except a track being changed, which is drawn with the preview
        let tracks = self.scene_tracks.as_ref().map_or(&self.timeline.base().track_maps, |(_, t)| t);
        let scene = Scene{glyphs: &world.glyphs, tracks, polymers: &world.polymers, walls: &world.chambers.walls,
            area: &world.area_touched, float_world, hidden: Some(part)};
        render.draw(ctx, &self.camera, self.area_mode == AreaMode::Shown, &scene);
        match part{
            PartId::Glyph(i) => render.draw_dragged_glyph(ctx, &self.camera, &preview.layout.glyphs[i],
                preview.polymer.as_ref(), preview.track.as_ref(), extra),
            //How it starts, on its hex, which is where the edit changes it (and shows where the drag snaps to). Then
            //where it will be at this step, following the mouse.
            PartId::Arm(i) => {
                render.draw_arm_start(ctx, &self.camera, &preview.layout.arms[i], i);
                let now = preview.arm_now.as_ref().expect("an arm's preview has it as it is now");
                render.draw_dragged_arm(ctx, &self.camera, now, i, extra);
            }
        }
        render.draw_hex_fills(ctx, &self.camera, hover.as_slice(), HOVER_COLOR);
        render.draw_hex_rings(ctx, &self.camera, &preview.landing, SELECT_COLOR);
        //Under the overlaps, so the tail doesn't hide them
        render.draw_hex_rings(ctx, &self.camera, &caret_tail, TRACK_REACH_COLOR);
        render.draw_hex_rings(ctx, &self.camera, &preview.overlaps, OVERLAP_COLOR);
        render.draw_hex_rings(ctx, &self.camera, &reach_hexes, TRACK_REACH_COLOR);
        render.draw_hex_rings(ctx, &self.camera, &track_hexes, TRACK_TOOL_COLOR);
        if let Some(hex) = caret_hex{
            render.draw_step_letters(ctx, &self.camera, hex, TRACK_TOOL_COLOR);
        }
    }
}

//These edit the board, which needs the editor
#[cfg(all(test, feature = "editor_ui"))]
mod tests {
    use super::*;
    use loaded::TEST_SCREEN as SCREEN;

    fn px(loaded: &Loaded, x: i32, y: i32) -> (f32, f32){
        loaded.test_px(x, y)
    }
    fn press(loaded: &mut Loaded, x: i32, y: i32, keymods: KeyMods){
        let at = px(loaded, x, y);
        loaded.press(at, SCREEN, keymods);
    }
    fn move_to(loaded: &mut Loaded, x: i32, y: i32){
        let at = px(loaded, x, y);
        loaded.pointer_moved(at, SCREEN, false);
    }
    fn bonder(x: i32, y: i32) -> Glyph{
        Glyph{pos: Pos::new(x, y), rot: 0, glyph_type: GlyphType::Bonding}
    }
    const SHIFT: KeyMods = KeyMods{shift: true, ctrl: false, alt: false, logo: false};

    #[test]
    fn dragging_a_glyph_moves_it() {
        let mut loaded = Loaded::for_test(vec![bonder(0, 0)], vec![]);
        press(&mut loaded, 0, 0, KeyMods::default());
        assert_eq!(loaded.tool.press().map(Press::part), Some(PartId::Glyph(0)));
        move_to(&mut loaded, 3, 0);
        loaded.release();
        assert_eq!(loaded.timeline.base().glyphs[0].pos, Pos::new(3, 0));
        assert_eq!(loaded.selection, Some(PartId::Glyph(0)));
        assert!(matches!(loaded.tool, BoardTool::Select(None)));
        assert!(loaded.dirty && loaded.history.can_undo());
    }

    fn small_chamber() -> Chambers {
        Chambers::new(&[(Pos::new(0, 0), ChamberShape::Small)], &[], false)
    }

    #[test]
    fn parts_stay_in_their_chambers_unless_overlap_is_allowed() {
        let mut loaded = Loaded::for_test_in_chambers(vec![bonder(0, 0)], vec![], small_chamber());
        assert!(!loaded.allow_overlap);
        //One hex right puts the bonder's second hex on the wall at (2,0)
        let drag_right = |loaded: &mut Loaded| {
            press(loaded, 0, 0, KeyMods::default());
            move_to(loaded, 1, 0);
            loaded.release();
        };
        drag_right(&mut loaded);
        assert_eq!(loaded.timeline.base().glyphs[0].pos, Pos::new(0, 0));
        assert!(loaded.message.as_ref().is_some_and(|m| m.contains("outside its chamber at (2, 0)")), "{:?}", loaded.message);
        loaded.allow_overlap = true;
        drag_right(&mut loaded);
        assert_eq!(loaded.timeline.base().glyphs[0].pos, Pos::new(1, 0));
    }

    #[test]
    fn loading_a_part_outside_its_chamber_allows_overlap() {
        assert!(!Loaded::for_test_in_chambers(vec![bonder(0, 0)], vec![], small_chamber()).allow_overlap);
        assert!(Loaded::for_test_in_chambers(vec![bonder(1, 0)], vec![], small_chamber()).allow_overlap);
    }

    #[test]
    fn track_stays_in_the_chambers() {
        let mut loaded = Loaded::for_test_in_chambers(vec![], vec![], small_chamber());
        loaded.start_new_track();
        press(&mut loaded, 2, 0, KeyMods::default());
        assert!(loaded.tool.stroke().is_none(), "it can't start on the wall");
        assert_eq!(loaded.message.as_deref(), Some(TRACK_OUTSIDE_CHAMBERS));
        //Drawn from inside, it stops at the wall
        press(&mut loaded, 0, 0, KeyMods::default());
        move_to(&mut loaded, 3, 0);
        loaded.release();
        assert_eq!(track_path(loaded.timeline.base(), 0), vec![Pos::new(0, 0), Pos::new(1, 0)]);
    }

    #[test]
    fn running_cancels_a_drag_but_not_a_press() {
        let mut loaded = Loaded::for_test(vec![bonder(0, 0)], vec![]);
        press(&mut loaded, 0, 0, KeyMods::default());
        loaded.run_state = RunState::FreeRun;
        loaded.refresh_preview();
        assert!(loaded.tool.press().is_some());
        loaded.run_state = RunState::Manual(0);
        move_to(&mut loaded, 3, 0);
        loaded.run_state = RunState::FreeRun;
        loaded.refresh_preview();
        assert!(loaded.tool.press().is_none());
    }

    #[test]
    fn shift_keeps_placing_and_right_click_stops() {
        let mut loaded = Loaded::for_test(vec![], vec![]);
        loaded.start_placing(NewPart::Glyph(GlyphType::Calcification));
        press(&mut loaded, 2, 0, SHIFT);
        assert!(loaded.tool.placing().is_some());
        press(&mut loaded, 4, 0, KeyMods::default());
        assert!(matches!(loaded.tool, BoardTool::Select(None)));
        let placed: Vec<Pos> = loaded.timeline.base().glyphs.iter().map(|g| g.pos).collect();
        assert_eq!(placed, vec![Pos::new(2, 0), Pos::new(4, 0)]);

        loaded.start_placing(NewPart::Arm(ArmType::PlainArm));
        loaded.right_click();
        assert!(matches!(loaded.tool, BoardTool::Select(None)));
    }

    #[test]
    fn a_new_track_keeps_drawing_after_its_first_stroke() {
        let mut loaded = Loaded::for_test(vec![], vec![]);
        loaded.start_new_track();
        press(&mut loaded, 0, 0, KeyMods::default());
        move_to(&mut loaded, 2, 0);
        loaded.release();
        assert_eq!(track_path(loaded.timeline.base(), 0), vec![Pos::new(0, 0), Pos::new(1, 0), Pos::new(2, 0)]);
        assert!(matches!(&loaded.tool, BoardTool::Track(TrackMode{glyph: Some(0), tool: TrackTool::AddBack}, None)));
    }

    #[test]
    fn escape_lets_go_of_one_thing_at_a_time() {
        let mut loaded = Loaded::for_test(vec![], vec![]);
        loaded.start_new_track();
        press(&mut loaded, 0, 0, KeyMods::default());
        assert!(loaded.tool.stroke().is_some());
        assert!(loaded.handle_key(KeyCode::Escape, KeyMods::default()));
        assert!(loaded.tool.stroke().is_none() && loaded.tool.track_mode().is_some());
        assert!(loaded.handle_key(KeyCode::Escape, KeyMods::default()));
        assert!(matches!(loaded.tool, BoardTool::Select(None)));
        assert!(!loaded.handle_key(KeyCode::Escape, KeyMods::default()), "nothing left for Esc to do");
        assert!(loaded.timeline.base().glyphs.is_empty(), "the stroke was dropped, not applied");
    }

    #[test]
    fn extension_hexes_are_the_ones_a_stroke_would_add() {
        let hexes = |list: &[(i32, i32)]| list.iter().map(|&(x, y)| Pos::new(x, y)).collect::<Vec<_>>();
        let track = Glyph{pos: Pos::new(0, 0), rot: 0, glyph_type: GlyphType::Track(vec![Pos::new(0, 0), Pos::new(1, 0)])};
        let mut loaded = Loaded::for_test(vec![track, bonder(2, 0)], vec![]);
        loaded.set_track_tool(0, Some(TrackTool::AddBack));
        //(2,0) is under the bonder, and (0,0) is the hex before the end
        assert_eq!(loaded.track_extension_hexes(), hexes(&[(1, 1), (0, 1), (1, -1), (2, -1)]));
        loaded.allow_overlap = true;
        assert_eq!(loaded.track_extension_hexes(), hexes(&[(2, 0), (1, 1), (0, 1), (1, -1), (2, -1)]));
        //Mid-stroke, stepping back onto (1,0) retracts rather than adds
        press(&mut loaded, 1, 0, KeyMods::default());
        move_to(&mut loaded, 1, 1);
        assert_eq!(loaded.tool.stroke().map(|s| s.path.clone()), Some(hexes(&[(0, 0), (1, 0), (1, 1)])));
        assert_eq!(loaded.track_extension_hexes(), hexes(&[(2, 1), (1, 2), (0, 2), (0, 1), (2, 0)]));
        loaded.release();
        loaded.set_track_tool(0, Some(TrackTool::Remove));
        assert!(loaded.track_extension_hexes().is_empty());
    }

    #[test]
    fn typed_path_text_is_previewed() {
        let hexes = |list: &[(i32, i32)]| list.iter().map(|&(x, y)| Pos::new(x, y)).collect::<Vec<_>>();
        let track = Glyph{pos: Pos::new(0, 0), rot: 0, glyph_type: GlyphType::Track(vec![Pos::new(0, 0), Pos::new(1, 0)])};
        let mut loaded = Loaded::for_test(vec![track], vec![]);
        loaded.select(Some(PartId::Glyph(0)));
        //Typed "dde", then something unreadable, with the caret after the first "d"
        loaded.path_text = Some(PathText{glyph: 0, shown: "d".into(), text: "ddeq".into(), editing: true, caret: 1});
        loaded.refresh_preview();
        assert!(loaded.preview.is_some());
        let typed = hexes(&[(0, 0), (1, 0), (2, 0), (2, 1)]);
        assert_eq!(loaded.pending_edit().map(|(e, _)| e), Some(LayoutEdit::SetTrack{glyph: Some(0), path: typed}));
        assert_eq!(loaded.typed_caret(), Some((Pos::new(1, 0), hexes(&[(2, 0), (2, 1)]))));

        loaded.path_text.as_mut().unwrap().editing = false;
        loaded.refresh_preview();
        assert!(loaded.preview.is_none(), "only shown while typing");
        assert_eq!(loaded.typed_caret(), None);
        assert_eq!(track_path(loaded.timeline.base(), 0), hexes(&[(0, 0), (1, 0)]), "previewing changes nothing");
    }

    #[test]
    fn typed_path_tail_is_the_hexes_after_the_caret() {
        let hexes = |list: &[(i32, i32)]| list.iter().map(|&(x, y)| Pos::new(x, y)).collect::<Vec<_>>();
        let track = Glyph{pos: Pos::new(0, 0), rot: 0, glyph_type: GlyphType::Track(vec![Pos::new(0, 0), Pos::new(1, 0)])};
        let mut loaded = Loaded::for_test(vec![track], vec![]);
        loaded.select(Some(PartId::Glyph(0)));
        let mut caret_at = |caret| {
            loaded.path_text = Some(PathText{glyph: 0, shown: "d".into(), text: "d(2,0)d".into(), editing: true, caret});
            loaded.typed_caret()
        };
        assert_eq!(caret_at(0), Some((Pos::new(0, 0), hexes(&[(1, 0), (3, 0), (4, 0)]))));
        assert_eq!(caret_at(3), Some((Pos::new(1, 0), hexes(&[(3, 0), (4, 0)]))), "inside a jump, it's in the tail");
        assert_eq!(caret_at(7), Some((Pos::new(4, 0), vec![])));
    }

    #[test]
    fn board_input_waits_for_egui_while_typing() {
        let mut loaded = Loaded::for_test(vec![bonder(0, 0)], vec![]);
        let at = px(&loaded, 0, 0);
        loaded.board_input(BoardInput::Press{px: at, screen: SCREEN, keymods: KeyMods::default()}, true);
        loaded.board_input(BoardInput::Release, false);
        assert_eq!(loaded.selection, None, "the press waits, and the release waits behind it");
        loaded.flush_board_input();
        assert_eq!(loaded.selection, Some(PartId::Glyph(0)));

        let empty = px(&loaded, 5, 5);
        loaded.board_input(BoardInput::Press{px: empty, screen: SCREEN, keymods: KeyMods::default()}, false);
        assert_eq!(loaded.selection, None, "without typing, it acts right away");
    }

    #[test]
    fn deselected_path_text_is_dropped() {
        let track = Glyph{pos: Pos::new(0, 0), rot: 0, glyph_type: GlyphType::Track(vec![Pos::new(0, 0), Pos::new(1, 0)])};
        let mut loaded = Loaded::for_test(vec![track], vec![]);
        loaded.select(Some(PartId::Glyph(0)));
        loaded.path_text = Some(PathText{glyph: 0, shown: "d".into(), text: "dde".into(), editing: true, caret: 3});
        loaded.select(None);
        loaded.select(Some(PartId::Glyph(0)));
        assert!(loaded.pending_edit().is_none(), "the abandoned text isn't previewed again");
        assert_eq!(track_path(loaded.timeline.base(), 0), vec![Pos::new(0, 0), Pos::new(1, 0)]);
    }

    fn track_at(x: i32, y: i32) -> Glyph{
        Glyph{pos: Pos::new(x, y), rot: 0, glyph_type: GlyphType::Track(vec![Pos::new(0, 0), Pos::new(1, 0)])}
    }
    fn tracks(loaded: &Loaded) -> Vec<bool>{
        loaded.timeline.base().glyphs.iter().map(Glyph::is_track).collect()
    }

    //Reordering another glyph onto the track's number left the track tool with that number, so the next stroke
    //turned the other glyph into a track
    #[test]
    fn reordering_another_glyph_onto_the_tracks_number_ends_the_track_tool() {
        let mut loaded = Loaded::for_test(vec![bonder(0, 0), track_at(0, 2)], vec![]);
        loaded.select(Some(PartId::Glyph(1)));
        loaded.set_track_tool(1, Some(TrackTool::AddBack));
        assert_eq!(loaded.apply_edit(&LayoutEdit::ReorderGlyph{from: 0, to: 1}), Ok(()));
        //Next to the end of the bonder, which is now glyph 1
        press(&mut loaded, 2, 0, KeyMods::default());
        loaded.release();
        assert_eq!(tracks(&loaded), vec![true, false]);
    }

    #[test]
    fn a_renumbered_track_keeps_its_track_tool() {
        let mut loaded = Loaded::for_test(vec![track_at(0, 2), bonder(0, 0)], vec![]);
        loaded.select(Some(PartId::Glyph(0)));
        loaded.set_track_tool(0, Some(TrackTool::AddBack));
        assert_eq!(loaded.apply_edit(&LayoutEdit::ReorderGlyph{from: 0, to: 1}), Ok(()));
        press(&mut loaded, 2, 2, KeyMods::default());
        loaded.release();
        assert_eq!(tracks(&loaded), vec![false, true]);
        assert_eq!(track_path(loaded.timeline.base(), 1), vec![Pos::new(0, 2), Pos::new(1, 2), Pos::new(2, 2)]);
    }

    #[test]
    fn the_tapes_caret_stays_on_its_arm_when_arms_are_renumbered() {
        let arms = (0..4).map(|i| Arm::new(Pos::new(i * 3, 0), 0, 1, ArmType::PlainArm,
            Tape::new(i as usize, vec![Instr::Grab]))).collect();
        let mut loaded = Loaded::for_test(vec![], arms);
        (loaded.tape_sel.arm, loaded.tape_sel.anchor_arm) = (2, 2);
        //Each arm's tape starts at its original number
        let caret_arm = |loaded: &Loaded| loaded.timeline.base().arms[loaded.tape_sel.arm].instruction_tape.first;
        assert_eq!(loaded.apply_edit(&LayoutEdit::ReorderArm{from: 0, to: 3}), Ok(()));
        assert_eq!(caret_arm(&loaded), 2);
        assert_eq!(loaded.apply_edit(&LayoutEdit::Delete(PartId::Arm(0))), Ok(()));
        assert_eq!(caret_arm(&loaded), 2);
    }

    #[test]
    fn selecting_another_part_ends_the_track_tool() {
        let track = Glyph{pos: Pos::new(0, 2), rot: 0, glyph_type: GlyphType::Track(vec![Pos::new(0, 0), Pos::new(1, 0)])};
        let mut loaded = Loaded::for_test(vec![bonder(0, 0), track], vec![]);
        loaded.set_track_tool(1, Some(TrackTool::AddBack));
        loaded.select(Some(PartId::Glyph(1)));
        assert!(loaded.tool.track_mode().is_some(), "selecting its own track keeps it");
        loaded.select(Some(PartId::Glyph(0)));
        assert!(matches!(loaded.tool, BoardTool::Select(None)));
    }

    //An arm at (0,0) on a track along to (3,0), which it rides to (2,0) by timestep 2
    fn carried_arm() -> Loaded{
        let track = Glyph{pos: Pos::new(0, 0), rot: 0, glyph_type: GlyphType::Track((0..4).map(|x| Pos::new(x, 0)).collect())};
        let arm = Arm::new(Pos::new(0, 0), 0, 1, ArmType::PlainArm, Tape::new(0, vec![Instr::Forward, Instr::Forward]));
        let mut loaded = Loaded::for_test(vec![track], vec![arm]);
        loaded.timeline.advance_to(2.).unwrap();
        loaded
    }
    fn pose(arm: &Arm) -> (Pos, Rot, i32){
        (arm.pos, arm.rot, arm.len)
    }

    #[test]
    fn an_arm_dragged_at_a_later_step_starts_that_much_further_along() {
        let arm = Arm::new(Pos::new(0, 0), 0, 1, ArmType::PlainArm, Tape::new(0, vec![Instr::RotateCounterClockwise]));
        let mut loaded = Loaded::for_test(vec![], vec![arm]);
        loaded.timeline.advance_to(3.).unwrap();
        press(&mut loaded, 0, 0, KeyMods::default());
        move_to(&mut loaded, 2, 0);
        loaded.release();
        assert_eq!(pose(&loaded.timeline.base().arms[0]), (Pos::new(2, 0), 0, 1), "turned as it started");
    }

    //Grabbed far from where it starts, an arm turned around the hex it's dragged to would have its start moved too
    #[test]
    fn an_arm_a_track_carried_off_keeps_its_start_offset_and_turns_in_place() {
        let mut loaded = carried_arm();
        press(&mut loaded, 2, 0, KeyMods::default());
        move_to(&mut loaded, 2, 1);
        assert!(loaded.handle_key(KeyCode::A, KeyMods::default()));
        loaded.release();
        assert_eq!(pose(&loaded.timeline.base().arms[0]), (Pos::new(0, 1), 1, 1));
    }

    #[test]
    fn the_drag_preview_shows_the_arm_where_it_will_be_at_this_step() {
        let mut loaded = carried_arm();
        //Its start moves from (0,0) to (2,0), so the end of the track stops it at (3,0)
        press(&mut loaded, 2, 0, KeyMods::default());
        move_to(&mut loaded, 4, 0);
        loaded.refresh_preview();
        let previewed = loaded.preview.as_ref().and_then(|p| p.arm_now.as_ref()).map(pose);
        assert_eq!(previewed, Some((Pos::new(3, 0), 0, 1)));
        loaded.release();
        loaded.update();
        assert_eq!(loaded.timeline.curr().timestep, 2, "it runs back to the step it was edited at");
        assert_eq!(Some(pose(&loaded.timeline.curr().arms[0])), previewed);
    }
}
