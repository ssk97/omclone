//Picking, dragging and previewing parts on the board, placing new ones, and reshaping track
use super::*;

//Moving the mouse this far (in pixels) with the button held starts a drag
pub const DRAG_THRESHOLD_PX: f32 = 4.;
//How fast the view moves while a panning key is held, in pixels a second
const KEY_PAN_PX_PER_SEC: f64 = 600.;
//Which way a panning key moves the view, in screen pixels (y down)
fn pan_key_direction(keycode: KeyCode) -> Option<(f32, f32)>{
    match keycode{
        KeyCode::Left | KeyCode::A => Some((-1., 0.)),
        KeyCode::Right | KeyCode::D => Some((1., 0.)),
        KeyCode::Up | KeyCode::W => Some((0., -1.)),
        KeyCode::Down | KeyCode::S => Some((0., 1.)),
        _ => None,
    }
}
const TRACK_OUTSIDE_CHAMBERS: &str = "That would put track outside the chambers (see \"Allow overlap\")";
//What an edit does to a part, for checking whether it's allowed right now
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EditKind{
    //Moving, rotating, resizing, or changing an arm's type
    Move, Delete, Duplicate, Reorder,
}
//What the left mouse button does on the board
pub enum BoardTool{
    //Clicking picks parts, and dragging one moves it (or pans; see Loaded::left_drag_pans), or with Shift, a box picks
    //more. Holds what the button is doing while it's down.
    Select(Option<Held>),
    //Clicking places a new part
    Place(Placing),
    //Clicking and dragging reshape a track. Holds the stroke while the button is down.
    Track(TrackMode, Option<TrackStroke>),
    //A click picks the hex to watch (see Loaded::watched_hex), and a drag pans. Holds the pressed hex while the button
    //is down, until the press becomes a pan.
    PickHex(Option<Pos>),
}
//What the left button is doing in the Select tool
pub enum Held{
    Part(Press),
    Box(BoxSelect),
}
impl BoardTool{
    pub fn press(&self) -> Option<&Press>{
        match self {BoardTool::Select(Some(Held::Part(press))) => Some(press), _ => None}
    }
    fn press_mut(&mut self) -> Option<&mut Press>{
        match self {BoardTool::Select(Some(Held::Part(press))) => Some(press), _ => None}
    }
    pub fn box_select(&self) -> Option<&BoxSelect>{
        match self {BoardTool::Select(Some(Held::Box(selecting))) => Some(selecting), _ => None}
    }
    pub fn placing(&self) -> Option<&Placing>{
        match self {BoardTool::Place(placing) => Some(placing), _ => None}
    }
    pub fn track_mode(&self) -> Option<&TrackMode>{
        match self {BoardTool::Track(mode, _) => Some(mode), _ => None}
    }
    pub fn stroke(&self) -> Option<&TrackStroke>{
        match self {BoardTool::Track(_, stroke) => stroke.as_ref(), _ => None}
    }
}
//Left mouse button input for the board, in logical pixels on a screen of size `screen`
pub enum BoardInput{
    Press{px: (f32, f32), screen: (f32, f32), keymods: KeyMods},
    Move{px: (f32, f32), screen: (f32, f32), over_ui: bool},
    Release,
    //Lets go of the press without a click or drop, as when a second finger touches
    Cancel,
}
//A left press on the board while left drags pan. Until it moves far enough to pan, it may still be a click.
pub struct PanPress{
    press_px: (f32, f32),
    last_px: (f32, f32),
    panning: bool,
    //What the Part window listed before the press, which a pan puts back
    list: PartList,
}
//Shift is held with the left button on the board. A click adds or removes the part there, and a drag draws a box
//that adds the parts it touches.
pub struct BoxSelect{
    press_px: (f32, f32),
    press_hex: Pos,
    //World positions, so the box stays on the same hexes while the view moves
    press_xy: XYPos,
    cursor_xy: XYPos,
    //It moved far enough to be a box rather than a click
    dragged: bool,
}
//The left mouse button is held on the board
pub struct Press{
    press_px: (f32, f32),
    press_xy: XYPos,
    press_hex: Pos,
    //The parts under the pressed hex, top first, and which one is picked
    pub stack: Vec<PartId>,
    pub cand: usize,
    wheel_accum: f32,
    //Ctrl was held, so dragging makes a copy
    pub duplicate: bool,
    pub drag: Option<Drag>,
}
pub struct Drag{
    cursor_xy: XYPos,
    //From the rotate/length hotkeys
    rot: Rot,
    len: i32,
}
//A new part on the cursor, waiting to be placed
pub struct Placing{
    pub part: NewPart,
    rot: Rot,
    len: i32,
    //None while the cursor is off the board
    cursor_xy: Option<XYPos>,
}
//Strokes over the board reshape a track
pub struct TrackMode{
    //None for a new track, until its first hex is placed
    pub glyph: Option<usize>,
    pub tool: TrackTool,
}
//The track's path during a stroke (from pressing the mouse button to releasing it)
pub struct TrackStroke{
    path: Vec<Pos>,
    //Hexes this stroke added, which going back over takes off again
    added: usize,
    last_hex: Pos,
    //Why the last hex entered wasn't used
    pub refusal: Option<&'static str>,
}
//The pending edit, made, for drawing
pub struct Preview{
    edit: LayoutEdit,
    //The parts it places or changes
    parts: Vec<PartId>,
    layout: InitialWorld,
    pub world: std::result::Result<World, String>,
    float_world: FloatWorld,
    //The glyphs among `parts`, with their polymers
    glyphs: Vec<(usize, Option<Polymer>)>,
    //The tracks among `parts`, if any
    tracks: Option<TrackMaps>,
    //The arms among `parts`, as the edit would have them at `timestep`, the timeline's current one (none if the
    //layout can't be set up)
    arms_now: Vec<(usize, Arm)>,
    timestep: u64,
    landing: Vec<Pos>,
    overlaps: Vec<Pos>,
}
impl Preview{
    fn arms_at(world: &std::result::Result<World, String>, parts: &[PartId], timestep: u64) -> Vec<(usize, Arm)>{
        let Ok(world) = world else {return Vec::new()};
        parts.iter().filter_map(|&part| match part {PartId::Arm(i) => Some(i), _ => None})
            .map(|i| (i, world.arm_after(i, timestep))).collect()
    }
    //Moves it to another timestep. Only the arms depend on the time, so the rest is kept.
    fn set_timestep(&mut self, timestep: u64){
        if self.timestep != timestep{
            self.arms_now = Self::arms_at(&self.world, &self.parts, timestep);
            self.timestep = timestep;
        }
    }
}
impl Press{
    pub fn part(&self) -> PartId{
        self.stack[self.cand]
    }
    pub fn kind(&self) -> EditKind{
        if self.duplicate {EditKind::Duplicate} else {EditKind::Move}
    }
    //Where the grabbed point of the part is now
    fn held_xy(&self, drag: &Drag) -> XYPos{
        pos_to_xy(self.press_hex) + (drag.cursor_xy - self.press_xy)
    }
    //How far the dragged part is drawn from its landing spot
    fn drag_offset(&self, drag: &Drag) -> XYVec{
        let held = self.held_xy(drag);
        held - pos_to_xy(xy_to_pos(held))
    }
}
impl Loaded{
    //Phones have only the left button (one finger), so the display build always pans with it
    pub fn left_drag_pans(&self) -> bool{
        !EDITOR_ENABLED || self.left_drag_pans
    }
    //Why the layout can't be changed, if it can't
    pub fn edit_lock(&self) -> Option<&'static str>{
        (!EDITOR_ENABLED).then_some("Editing is disabled")
    }
    //Why the part can't be edited this way right now, if it can't
    pub fn part_lock(&self, part: PartId, kind: EditKind) -> Option<&'static str>{
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
    //Why some of the parts can't be edited this way right now, if any can't
    pub fn parts_lock(&self, parts: &[PartId], kind: EditKind) -> Option<&'static str>{
        parts.iter().find_map(|&part| self.part_lock(part, kind))
    }
    //The parts an edit of this kind can change, out of `parts`. Copying or deleting a group leaves out the puzzle's
    //own parts; if those are all there is, all of `parts` are kept, so the edit is refused with the reason.
    pub fn editable_parts(&self, parts: Vec<PartId>, kind: EditKind) -> Vec<PartId>{
        let editable: Vec<PartId> = parts.iter().copied().filter(|&part| self.part_lock(part, kind).is_none()).collect();
        if editable.is_empty() {parts} else {editable}
    }
    fn layout_edit_lock(&self, edit: &LayoutEdit) -> Option<&'static str>{
        match edit{
            LayoutEdit::Part(e) => self.parts_lock(&e.parts, EditKind::Move),
            LayoutEdit::Duplicate(e) => self.parts_lock(&e.parts, EditKind::Duplicate),
            LayoutEdit::Delete(parts) => self.parts_lock(parts, EditKind::Delete),
            LayoutEdit::Reorder{kind, moved, ..} => {
                let parts: Vec<PartId> = moved.iter().map(|&i| kind.part(i)).collect();
                self.parts_lock(&parts, EditKind::Reorder)
            }
            LayoutEdit::Add{..} | LayoutEdit::SetTrack{..} => self.edit_lock(),
        }
    }
    //Why the held parts can't be dragged, if they can't
    pub fn press_lock(&self) -> Option<&'static str>{
        let press = self.tool.press()?;
        let parts = self.editable_parts(self.grabbed_parts(press), press.kind());
        self.parts_lock(&parts, press.kind())
    }
    //The parts a press drags: the selection if the pressed part is in it, otherwise just that part
    fn grabbed_parts(&self, press: &Press) -> Vec<PartId>{
        if self.selection.contains(&press.part()) {self.selection.iter().copied().collect()} else {vec![press.part()]}
    }
    //The parts to outline: the ones the press would drag while the mouse is held, otherwise the selection
    pub fn highlighted_parts(&self) -> Vec<PartId>{
        match self.tool.press(){
            Some(press) => self.grabbed_parts(press),
            None => self.selection.iter().copied().collect(),
        }
    }
    //The selected part, if there's just one
    pub fn single_selection(&self) -> Option<PartId>{
        let mut parts = self.selection.iter();
        match (parts.next(), parts.next()) {(Some(&part), None) => Some(part), _ => None}
    }
    //Selects just this part, or nothing
    pub fn select(&mut self, part: Option<PartId>){
        self.set_selection(part.into_iter().collect());
    }
    //A track tool ends when anything but its track is selected (a new track has none yet, so it keeps on
    //when nothing is selected). So does Path text, which would otherwise be shown again unapplied.
    pub fn set_selection(&mut self, parts: BTreeSet<PartId>){
        self.selection = parts;
        let single = self.single_selection();
        if self.tool.track_mode().is_some_and(|m| m.glyph.map(PartId::Glyph) != single){
            self.end_track_mode();
        }
        if self.path_text.as_ref().is_some_and(|p| Some(PartId::Glyph(p.glyph)) != single){
            self.path_text = None;
        }
    }
    //Adds the part to the selection, or takes it out if it's there
    pub fn toggle_selected(&mut self, part: PartId){
        let mut parts = self.selection.clone();
        if !parts.remove(&part){
            parts.insert(part);
        }
        self.set_selection(parts);
    }
    //Selects every part, and lists them all
    pub fn select_all(&mut self){
        let world = self.timeline.base();
        let all = (0..world.glyphs.len()).map(PartId::Glyph).chain((0..world.arms.len()).map(PartId::Arm)).collect();
        self.set_selection(all);
        self.list = PartList::All;
    }
    //Lists the selection, after a pick that changed it
    fn list_selection(&mut self){
        self.list = PartList::Parts(self.selection.iter().copied().collect());
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
    //Makes the edit if it's allowed. The parts it adds are selected, and copies are listed too.
    pub fn apply_edit(&mut self, edit: &LayoutEdit) -> std::result::Result<(), String>{
        if matches!(edit, LayoutEdit::Part(e) if e.is_noop()){
            return Ok(());
        }
        if let Some(reason) = self.layout_edit_lock(edit){
            return Err(reason.to_owned());
        }
        let (layout, parts) = edit.apply(self.timeline.base());
        let world = self.set_up(&layout)?;
        for &part in &parts{
            self.check_overlap(&world.part_overlaps(part))?;
            self.check_chambers(&world, part)?;
        }
        self.commit_world(world);
        self.follow_renumbering(edit);
        if edit.is_structural(){
            self.set_selection(parts.into_iter().collect());
        }
        if matches!(edit, LayoutEdit::Duplicate(_)){
            self.list_selection();
        }
        Ok(())
    }
    //Keeps the selection, the list, the track tool, the Path text and the ends of the tapes' selected arms on the
    //parts they had through `edit`, which may renumber them. Deleted parts leave the selection and list, and the tool
    //and text end if their track is deleted. Clicks and drags in the windows hold numbers this can't reach, so they
    //check numbering_version instead.
    fn follow_renumbering(&mut self, edit: &LayoutEdit){
        if !edit.renumbers(){
            return;
        }
        self.numbering_version += 1;
        if let PartList::Parts(parts) = &mut self.list{
            *parts = parts.iter().filter_map(|&part| edit.renumbered(part)).collect();
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
        //Last, so the track tool and Path text it checks against already have their new numbers
        let selection = self.selection.iter().filter_map(|&part| edit.renumbered(part)).collect();
        self.set_selection(selection);
    }
    //Lets go of what the held button is doing, without a click or a drop. Returns true if it was doing anything.
    fn cancel_press(&mut self) -> bool{
        let held = match &mut self.tool{
            BoardTool::Select(held) => held.take().is_some(),
            BoardTool::Track(_, stroke) => stroke.take().is_some(),
            BoardTool::PickHex(hex) => hex.take().is_some(),
            BoardTool::Place(_) => false,
        };
        self.pan.take().is_some() || held
    }
    //Ending the pick any way but a click leaves no hex watched
    pub fn start_picking_hex(&mut self){
        self.watched_hex = None;
        self.tool = BoardTool::PickHex(None);
    }
    pub fn start_placing(&mut self, part: NewPart){
        self.tool = BoardTool::Place(Placing{part, rot: 0, len: 1, cursor_xy: None});
    }
    pub fn start_new_track(&mut self){
        self.select(None);
        self.tool = BoardTool::Track(TrackMode{glyph: None, tool: TrackTool::AddBack}, None);
    }
    //Sets (or with None, ends) the board tool for reshaping the track
    pub fn set_track_tool(&mut self, glyph: usize, tool: Option<TrackTool>){
        self.tool = match tool{
            Some(tool) => BoardTool::Track(TrackMode{glyph: Some(glyph), tool}, None),
            None => BoardTool::Select(None),
        };
    }
    pub fn end_track_mode(&mut self){
        if matches!(self.tool, BoardTool::Track(..)){
            self.tool = BoardTool::Select(None);
        }
    }
    //Goes back to picking parts, putting away the part being placed or ending the track mode or hex picking
    pub fn end_tool(&mut self){
        if !matches!(self.tool, BoardTool::Select(_)){
            self.tool = BoardTool::Select(None);
        }
    }
    //A right-click (without panning) puts away the part being placed or ends the track mode or hex picking
    pub fn right_click(&mut self){
        self.end_tool();
    }
    //Takes mouse input for the board. Mouse events arrive before egui's frame sees them, so a press made while a text
    //field is being typed in waits, along with the input after it, until egui has applied the field (see
    //flush_board_input). Otherwise the press could select another part first, and the field's value would go to it.
    pub fn board_input(&mut self, input: BoardInput, typing: bool){
        if !self.waiting_input.is_empty() || (typing && matches!(input, BoardInput::Press{..})){
            self.waiting_input.push(input);
        } else {
            self.apply_board_input(input);
        }
    }
    //Applies the input that waited for egui's frame
    pub fn flush_board_input(&mut self){
        for input in std::mem::take(&mut self.waiting_input){
            self.apply_board_input(input);
        }
    }
    fn apply_board_input(&mut self, input: BoardInput){
        match input{
            BoardInput::Press{px, screen, keymods} => self.press(px, screen, keymods),
            BoardInput::Move{px, screen, over_ui} => self.pointer_moved(px, screen, over_ui),
            BoardInput::Release => self.release(),
            BoardInput::Cancel => {self.cancel_press();}
        }
    }
    fn press(&mut self, px: (f32, f32), screen_size: (f32, f32), keymods: KeyMods){
        self.cancel_press();
        let press_xy = self.camera.screen_to_world(px, screen_size);
        let press_hex = xy_to_pos(press_xy);
        match self.tool{
            BoardTool::Place(_) => return self.place(press_hex, keymods.shift),
            BoardTool::Track(..) => return self.start_stroke(press_hex),
            BoardTool::PickHex(_) | BoardTool::Select(_) => {}
        }
        let picking = matches!(self.tool, BoardTool::PickHex(_));
        //Shift picks more parts, even with left drags panning
        if !picking && keymods.shift{
            let selecting = BoxSelect{press_px: px, press_hex, press_xy, cursor_xy: press_xy, dragged: false};
            self.tool = BoardTool::Select(Some(Held::Box(selecting)));
            return;
        }
        if picking || self.left_drag_pans(){
            self.pan = Some(PanPress{press_px: px, last_px: px, panning: false, list: self.list.clone()});
        }
        if picking{
            self.tool = BoardTool::PickHex(Some(press_hex));
            return;
        }
        //Arms are picked where they are now (glyphs don't move)
        let stack = self.timeline.curr().parts_at(press_hex);
        if stack.is_empty(){
            //A press that may become a pan waits to see if it's a click (see release)
            if self.pan.is_none(){
                self.select(None);
                self.list = PartList::Nothing;
            }
            return;
        }
        let cand = stack.iter().position(|p| self.selection.contains(p)).unwrap_or(0);
        let press = Press{press_px: px, press_xy, press_hex, stack, cand, wheel_accum: 0.,
            duplicate: keymods.ctrl || keymods.logo, drag: None};
        //Pressing a group lists its hex only if it turns out to be a click (see release), so moving the group keeps
        //it listed
        if self.grabbed_parts(&press).len() == 1{
            self.list = PartList::Hex(press_hex);
        }
        self.tool = BoardTool::Select(Some(Held::Part(press)));
    }
    //The edit that dropping the press's parts here would make (snapped to the nearest hex)
    fn press_edit(&self, press: &Press, drag: &Drag) -> LayoutEdit{
        let landing = xy_to_pos(press.held_xy(drag));
        let delta = landing - press.press_hex;
        let parts = self.editable_parts(self.grabbed_parts(press), press.kind());
        //A lone arm turns in place (see PartEdit), and anything else turns around the held hex
        let pivot = match parts[..]{
            [PartId::Arm(i)] => self.timeline.base().arms[i].pos + delta,
            _ => landing,
        };
        let edit = PartEdit{parts, delta, rot: drag.rot, pivot, len: drag.len, arm_type: None};
        if press.duplicate {LayoutEdit::Duplicate(edit)} else {LayoutEdit::Part(edit)}
    }
    //The parts the box would add: those with any hex in it, where they are now
    fn parts_in_box(&self, selecting: &BoxSelect) -> Vec<PartId>{
        let world = self.timeline.curr();
        let parts = (0..world.glyphs.len()).map(PartId::Glyph).chain((0..world.arms.len()).map(PartId::Arm));
        parts.filter(|&part| world.part_footprint(part).iter()
            .any(|&hex| hex_meets_rect(hex, selecting.press_xy, selecting.cursor_xy))).collect()
    }
    fn finish_box(&mut self, selecting: BoxSelect){
        if selecting.dragged{
            let mut parts = self.selection.clone();
            parts.extend(self.parts_in_box(&selecting));
            self.set_selection(parts);
        } else {
            let Some(&top) = self.timeline.curr().parts_at(selecting.press_hex).first() else {return};
            self.toggle_selected(top);
        }
        self.list_selection();
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
    fn pointer_moved(&mut self, px: (f32, f32), screen_size: (f32, f32), over_ui: bool){
        //First, so the cursor's world position below is where it is after panning
        if let Some(mut pan) = self.pan.take(){
            let (dx, dy) = (px.0 - pan.press_px.0, px.1 - pan.press_px.1);
            if !pan.panning && dx*dx + dy*dy > DRAG_THRESHOLD_PX*DRAG_THRESHOLD_PX{
                pan.panning = true;
                self.list = pan.list.clone();
                self.cancel_press();
            }
            if pan.panning{
                self.camera.pan_pixels(px.0 - pan.last_px.0, px.1 - pan.last_px.1);
            }
            pan.last_px = px;
            self.pan = Some(pan);
        }
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
        if let BoardTool::Select(Some(Held::Box(selecting))) = &mut self.tool{
            selecting.cursor_xy = cursor_xy;
            let (dx, dy) = (px.0 - selecting.press_px.0, px.1 - selecting.press_px.1);
            selecting.dragged |= dx*dx + dy*dy > DRAG_THRESHOLD_PX*DRAG_THRESHOLD_PX;
        }
        let draggable = self.tool.press().is_some() && self.press_lock().is_none() && !self.left_drag_pans();
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
    fn release(&mut self){
        let pan = self.pan.take();
        let held = match &mut self.tool{
            BoardTool::Select(held) => held.take(),
            BoardTool::Track(..) => return self.finish_stroke(),
            BoardTool::Place(_) => None,
            BoardTool::PickHex(hex) => {
                if let Some(hex) = hex.take(){
                    self.watched_hex = Some(hex);
                    self.end_tool();
                }
                return;
            }
        };
        let press = match held{
            Some(Held::Part(press)) => press,
            Some(Held::Box(selecting)) => return self.finish_box(selecting),
            None => {
                //A click on an empty hex, which the press left for now. A pan or a cancel took the pan out.
                if pan.is_some_and(|p| !p.panning){
                    self.select(None);
                    self.list = PartList::Nothing;
                }
                return;
            }
        };
        let Some(drag) = &press.drag else {
            self.select(Some(press.part()));
            self.list = PartList::Hex(press.press_hex);
            return;
        };
        let edit = self.press_edit(&press, drag);
        //A group stays selected
        if self.grabbed_parts(&press).len() == 1{
            self.select(Some(press.part()));
        }
        if let Err(err) = self.apply_edit(&edit){
            self.message = Some(err);
        }
    }
    //Returns true if the wheel was used to pick a part (rather than zoom)
    pub fn wheel(&mut self, dy: f32) -> bool{
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
    //A part follows the mouse, and A/D/W/S change it rather than pan
    fn moving_part(&self) -> bool{
        self.tool.placing().is_some() || self.tool.press().is_some_and(|p| p.drag.is_some())
    }
    //Starts panning while the key is held, if it's a panning key (see keys_pan)
    fn pan_key_down(&mut self, keycode: KeyCode, keymods: KeyMods) -> bool{
        //A held key's repeats are taken even once a part moves, so they don't turn it
        if self.pan_keys_held.contains(&keycode){
            return true;
        }
        let usable = self.keys_pan && !keymods.ctrl && !keymods.logo && !self.moving_part();
        if !usable || pan_key_direction(keycode).is_none(){
            return false;
        }
        self.pan_keys_held.push(keycode);
        true
    }
    //Called for every key released, even while typing, so a held key can't get stuck
    pub fn key_up(&mut self, keycode: KeyCode){
        self.pan_keys_held.retain(|&k| k != keycode);
    }
    //Pans for the panning keys held, through a frame that took `dt` seconds. Returns true if the view moved.
    pub fn pan_with_keys(&mut self, dt: f64) -> bool{
        let (dx, dy) = self.pan_keys_held.iter().filter_map(|&k| pan_key_direction(k))
            .fold((0., 0.), |(x, y), (dx, dy)| (x + dx, y + dy));
        if self.moving_part() || (dx, dy) == (0., 0.){
            return false;
        }
        let step = (KEY_PAN_PX_PER_SEC*dt) as f32;
        //The board moves the other way, as the view moves over it
        self.camera.pan_pixels(-dx*step, -dy*step);
        true
    }
    //Returns true if the board used the key
    pub fn handle_key(&mut self, keycode: KeyCode, keymods: KeyMods) -> bool{
        if self.pan_key_down(keycode, keymods){
            return true;
        }
        let command = keymods.ctrl || keymods.logo;
        //Only picking parts and fading the atoms work without the editor
        let select_all = keycode == KeyCode::A && command && !self.moving_part();
        if !EDITOR_ENABLED && !select_all && !matches!(keycode, KeyCode::Tab | KeyCode::Escape | KeyCode::Key1){
            return false;
        }
        match keycode{
            KeyCode::Z if command => if keymods.shift {self.redo()} else {self.undo()},
            KeyCode::Y if command => self.redo(),
            KeyCode::A if select_all => {
                self.cancel_press();
                self.select_all();
            }
            KeyCode::Delete | KeyCode::Backspace => {
                let parts = self.highlighted_parts();
                if parts.is_empty(){
                    return false;
                }
                self.cancel_press();
                if let Err(err) = self.apply_edit(&LayoutEdit::Delete(self.editable_parts(parts, EditKind::Delete))){
                    self.message = Some(err);
                }
            }
            //Lets go of one thing at a time: the press, stroke or pan, then the part being placed, the track mode or
            //hex picking, then the selection
            KeyCode::Escape => {
                if !self.cancel_press(){
                    if !matches!(self.tool, BoardTool::Select(None)){
                        self.end_tool();
                    } else if !self.selection.is_empty(){
                        self.select(None);
                    } else {
                        return false;
                    }
                }
            }
            KeyCode::Tab if self.tool.press().is_some() => self.cycle(if keymods.shift {-1} else {1}),
            KeyCode::A | KeyCode::D | KeyCode::W | KeyCode::S => return self.turn_or_resize(keycode),
            //Until 1 is released (see ui.rs)
            KeyCode::Key1 => self.atoms_faded = true,
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
        let Some(press) = self.tool.press() else {return false};
        let base = self.timeline.base();
        let arms: Vec<&Arm> = self.grabbed_parts(press).into_iter()
            .filter_map(|part| match part {PartId::Arm(i) => Some(&base.arms[i]), _ => None}).collect();
        let Some(drag) = self.tool.press_mut().and_then(|p| p.drag.as_mut()) else {return false};
        drag.rot += turn;
        //Each arm's length is clamped on its own (see PartEdit). The shared change goes only as far as some arm can
        //still follow it, so the other way works at once.
        let reach = |len: i32| arms.iter().map(move |arm| arm.arm_type.clamp_len(len) - arm.len);
        if let (Some(shortest), Some(longest)) = (reach(i32::MIN).min(), reach(i32::MAX).max()){
            drag.len = (drag.len + grow).clamp(shortest, longest);
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
            BoardTool::Select(Some(Held::Part(press))) => {
                let drag = press.drag.as_ref()?;
                Some((self.press_edit(press, drag), press.drag_offset(drag)))
            }
            BoardTool::Place(placing) => {
                let cursor_xy = placing.cursor_xy?;
                let hex = xy_to_pos(cursor_xy);
                let edit = LayoutEdit::Add{part: placing.part.clone(), pos: hex, rot: placing.rot, len: placing.len};
                Some((edit, cursor_xy - pos_to_xy(hex)))
            }
            BoardTool::Select(_) | BoardTool::Track(_, None) | BoardTool::PickHex(_) => None,
        }
    }
    //The Path field's text while it's being typed in (for the selected track), and that track's first hex
    fn editing_path_text(&self) -> Option<(&PathText, Pos)>{
        let text = self.path_text.as_ref().filter(|p| p.editing)?;
        let is_track = self.timeline.base().glyphs.get(text.glyph).is_some_and(|g| g.is_track());
        if !is_track || self.single_selection() != Some(PartId::Glyph(text.glyph)) || self.edit_lock().is_some(){
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
    //The tracks an edit moves or reshapes, which the scene draws without
    fn changed_tracks(&self, edit: &LayoutEdit) -> Vec<usize>{
        let glyphs = &self.timeline.base().glyphs;
        match edit{
            LayoutEdit::Part(e) => e.parts.iter()
                .filter_map(|&part| match part {PartId::Glyph(i) if glyphs[i].is_track() => Some(i), _ => None}).collect(),
            LayoutEdit::SetTrack{glyph, ..} => glyph.iter().copied().collect(),
            _ => Vec::new(),
        }
    }
    fn build_preview(&self, edit: LayoutEdit) -> Preview{
        let base = self.timeline.base();
        let (layout, parts) = edit.apply(base);
        assert!(!parts.is_empty(), "a pending edit places or changes parts");
        let timestep = self.timeline.curr().timestep;
        let world = self.set_up(&layout);
        let mut glyphs = Vec::new();
        let mut tracks = Vec::new();
        let mut landing = Vec::new();
        for &part in &parts{
            match part{
                PartId::Glyph(i) => {
                    let glyph = &layout.glyphs[i];
                    let polymer = glyph.build_polymer().ok().flatten();
                    landing.extend(glyph.footprint(polymer.as_ref()));
                    if glyph.is_track(){
                        tracks.push(glyph);
                    }
                    glyphs.push((i, polymer));
                }
                PartId::Arm(i) => landing.push(layout.arms[i].pos),
            }
        }
        let tracks = (!tracks.is_empty()).then(|| TrackMaps::build(tracks).unwrap_or_default());
        let arms_now = Preview::arms_at(&world, &parts, timestep);
        let mut float_world = FloatWorld::new();
        let overlaps = match &world{
            Ok(world) => {
                float_world.generate_static(world);
                let mut overlaps = Vec::new();
                for &part in &parts{
                    overlaps.extend(world.part_overlaps(part));
                    overlaps.extend(world.chamber_conflicts(part));
                    overlaps.extend(world.isolation_conflicts(part));
                }
                overlaps
            }
            Err(_) => landing.clone(),
        };
        Preview{edit, parts, layout, world, float_world, glyphs, tracks, arms_now, timestep, landing, overlaps}
    }
    pub fn drop_preview(&mut self){
        self.preview = None;
        self.scene_tracks = None;
    }
    //Keeps the preview of the pending edit up to date
    pub fn refresh_preview(&mut self){
        let Some((edit, _)) = self.pending_edit() else {
            return self.drop_preview();
        };
        //Every track except the changed ones, which are drawn with the preview
        let changed = self.changed_tracks(&edit);
        if self.scene_tracks.as_ref().map_or(&[][..], |(tracks, _)| tracks) != changed{
            self.scene_tracks = (!changed.is_empty()).then(|| {
                let glyphs = self.timeline.base().glyphs.iter().enumerate();
                let others = glyphs.filter(|(j, _)| !changed.contains(j)).map(|(_, g)| g);
                let maps = TrackMaps::build(others).unwrap_or_default();
                (changed, maps)
            });
        }
        //The timeline can move while a part is dragged: on its way to the target, or running
        let timestep = self.timeline.curr().timestep;
        match &mut self.preview{
            Some(preview) if preview.edit == edit => preview.set_timestep(timestep),
            _ => self.preview = Some(self.build_preview(edit)),
        }
    }
    pub fn draw_world(&self, render: &mut RenderDataBase, ctx: &mut Context){
        const CRASH_COLOR: Color = [1., 0.1, 0.1, 1.];
        const WATCH_COLOR: Color = [1., 0.8, 0., 1.];
        self.draw_board(render, ctx);
        if let Some(hex) = self.watched_hex{
            render.draw_ring(ctx, &self.camera, pos_to_xy(hex), WATCH_COLOR);
        }
        //Last, so nothing covers it. The board shows the moment of the crash, so it circles the parts that collided.
        if let RunState::Crashed{error, ..} = self.run_state{
            render.draw_ring(ctx, &self.camera, error.location, CRASH_COLOR);
        }
    }
    //Draws the box being dragged out with Shift, over the board and under the windows. `screen_size` is the board's,
    //in logical pixels, which are egui's points.
    pub fn draw_box_select(&self, egui_ctx: &egui::Context, screen_size: (f32, f32)){
        let Some(selecting) = self.tool.box_select().filter(|s| s.dragged) else {return};
        let corner = |at| {
            let (x, y) = self.camera.world_to_screen(at, screen_size);
            egui::pos2(x, y)
        };
        let rect = egui::Rect::from_two_pos(corner(selecting.press_xy), corner(selecting.cursor_xy));
        let color = egui_ctx.global_style().visuals.selection.stroke.color;
        let painter = egui_ctx.layer_painter(egui::LayerId::background());
        painter.rect_filled(rect, 0., color.gamma_multiply(0.15));
        painter.rect_stroke(rect, 0., egui::Stroke::new(1., color), egui::StrokeKind::Inside);
    }
    fn draw_board(&self, render: &mut RenderDataBase, ctx: &mut Context){
        const SELECT_COLOR: Color = [0., 0.8, 1., 1.];
        const OVERLAP_COLOR: Color = [1., 0.1, 0.1, 1.];
        const TRACK_TOOL_COLOR: Color = [1., 0.75, 0., 1.];
        const TRACK_REACH_COLOR: Color = [0.55, 0.4, 0., 1.];
        const HOVER_COLOR: Color = [1., 1., 1., 0.12];
        //A held or placed part shows where it lands instead, and a box what it picks
        let hover = self.hovered_hex.filter(|_| matches!(self.tool, BoardTool::Select(None) | BoardTool::Track(..)
            | BoardTool::PickHex(_)));
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
                walls: &world.chambers.walls, area: &world.area_touched, float_world: &self.float_world, hidden: &[],
                faded_atoms: self.atoms_faded};
            render.draw(ctx, &self.camera, self.area_mode == AreaMode::Shown, &scene);
            render.draw_hex_fills(ctx, &self.camera, hover.as_slice(), HOVER_COLOR);
            let mut outlined = self.highlighted_parts();
            if let Some(selecting) = self.tool.box_select().filter(|s| s.dragged){
                outlined.extend(self.parts_in_box(selecting));
            }
            let outlined: Vec<Pos> = outlined.into_iter().flat_map(|part| world.part_footprint(part)).collect();
            render.draw_hex_rings(ctx, &self.camera, &outlined, SELECT_COLOR);
            render.draw_hex_rings(ctx, &self.camera, &caret_tail, TRACK_REACH_COLOR);
            render.draw_hex_rings(ctx, &self.camera, &reach_hexes, TRACK_REACH_COLOR);
            render.draw_hex_rings(ctx, &self.camera, &track_hexes, TRACK_TOOL_COLOR);
            if let Some(hex) = caret_hex{
                render.draw_step_letters(ctx, &self.camera, hex, TRACK_TOOL_COLOR);
            }
            return;
        };
        //Before anything moves, show the world as it would start; otherwise keep showing the current step.
        //New parts aren't in the current step, so hiding them there hides nothing.
        let (world, float_world) = match &preview.world{
            Ok(world) if self.timeline.at_start() => (world, &preview.float_world),
            _ => (self.timeline.curr(), &self.float_world),
        };
        //Tracks are drawn as they are now, except the tracks being changed, which are drawn with the preview
        let tracks = self.scene_tracks.as_ref().map_or(&self.timeline.base().track_maps, |(_, t)| t);
        let scene = Scene{glyphs: &world.glyphs, tracks, polymers: &world.polymers, walls: &world.chambers.walls,
            area: &world.area_touched, float_world, hidden: &preview.parts, faded_atoms: self.atoms_faded};
        render.draw(ctx, &self.camera, self.area_mode == AreaMode::Shown, &scene);
        for (i, polymer) in &preview.glyphs{
            render.draw_dragged_glyph(ctx, &self.camera, &preview.layout.glyphs[*i], polymer.as_ref(), extra);
        }
        if let Some(tracks) = &preview.tracks{
            render.draw_dragged_tracks(ctx, &self.camera, tracks, extra);
        }
        //Arms as they start, on their hexes, which is where the edit changes them (and shows where the drag snaps to).
        //Then where they will be at this step, following the mouse.
        let starts: Vec<(usize, &Arm)> = preview.parts.iter()
            .filter_map(|&part| match part {PartId::Arm(i) => Some((i, &preview.layout.arms[i])), _ => None}).collect();
        render.draw_arm_starts(ctx, &self.camera, &starts);
        let now: Vec<(usize, &Arm)> = preview.arms_now.iter().map(|(i, arm)| (*i, arm)).collect();
        render.draw_dragged_arms(ctx, &self.camera, &now, extra);
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
    use loaded::{TEST_FRAME, TEST_SCREEN as SCREEN};

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
        assert_eq!(loaded.single_selection(), Some(PartId::Glyph(0)));
        assert!(matches!(loaded.tool, BoardTool::Select(None)));
        assert!(loaded.dirty && loaded.history.can_undo());
    }

    fn panning(glyphs: Vec<Glyph>) -> Loaded{
        let mut loaded = Loaded::for_test(glyphs, vec![]);
        loaded.left_drag_pans = true;
        loaded
    }
    fn click(loaded: &mut Loaded, x: i32, y: i32){
        press(loaded, x, y, KeyMods::default());
        loaded.release();
    }

    #[test]
    fn with_left_drag_panning_a_drag_pans_instead_of_moving_the_part() {
        let mut loaded = panning(vec![bonder(0, 0)]);
        let (from, to) = (px(&loaded, 0, 0), px(&loaded, 3, 0));
        let grabbed = loaded.camera.screen_to_world(from, SCREEN);
        loaded.press(from, SCREEN, KeyMods::default());
        loaded.pointer_moved(to, SCREEN, false);
        loaded.release();
        assert!((loaded.camera.screen_to_world(to, SCREEN) - grabbed).norm() < 1e-3, "the board follows the cursor");
        assert_eq!(loaded.timeline.base().glyphs[0].pos, Pos::new(0, 0));
        assert_eq!(loaded.single_selection(), None, "a pan selects nothing");
        assert_eq!(loaded.list, PartList::Nothing);
        assert!(!loaded.dirty);
    }

    #[test]
    fn with_left_drag_panning_only_clicks_change_the_selection() {
        let mut loaded = panning(vec![bonder(0, 0)]);
        click(&mut loaded, 0, 0);
        assert_eq!(loaded.single_selection(), Some(PartId::Glyph(0)));
        assert_eq!(loaded.list, PartList::Hex(Pos::new(0, 0)));
        //A drag from an empty hex, or from another part, keeps both
        let glyph_px = px(&loaded, 0, 0);
        for from in [(5, 5), (0, 0)]{
            press(&mut loaded, from.0, from.1, KeyMods::default());
            let at = px(&loaded, from.0, from.1);
            loaded.pointer_moved((at.0 + 50., at.1), SCREEN, false);
            loaded.release();
            assert_eq!(loaded.single_selection(), Some(PartId::Glyph(0)), "dragged from {from:?}");
            assert_eq!(loaded.list, PartList::Hex(Pos::new(0, 0)), "dragged from {from:?}");
        }
        assert_ne!(px(&loaded, 0, 0), glyph_px, "it panned");
        click(&mut loaded, 5, 5);
        assert_eq!(loaded.single_selection(), None);
        assert_eq!(loaded.list, PartList::Nothing);
    }

    #[test]
    fn with_left_drag_panning_tab_still_picks_from_the_held_stack() {
        let mut loaded = panning(vec![bonder(0, 0), bonder(1, 0)]);
        press(&mut loaded, 1, 0, KeyMods::default());
        let top = loaded.tool.press().map(Press::part);
        loaded.handle_key(KeyCode::Tab, KeyMods::default());
        let other = loaded.tool.press().map(Press::part);
        assert!(other.is_some() && other != top);
        loaded.release();
        assert_eq!(loaded.single_selection(), other);
    }

    #[test]
    fn with_left_drag_panning_clicks_still_place_parts() {
        let mut loaded = panning(vec![]);
        loaded.start_placing(NewPart::Glyph(GlyphType::Calcification));
        click(&mut loaded, 2, 0);
        assert_eq!(loaded.timeline.base().glyphs.iter().map(|g| g.pos).collect::<Vec<_>>(), vec![Pos::new(2, 0)]);
    }

    #[test]
    fn cancel_drops_a_drag_without_dropping_the_part() {
        let mut loaded = Loaded::for_test(vec![bonder(0, 0)], vec![]);
        press(&mut loaded, 0, 0, KeyMods::default());
        move_to(&mut loaded, 3, 0);
        loaded.board_input(BoardInput::Cancel, false);
        loaded.release();
        assert_eq!(loaded.timeline.base().glyphs[0].pos, Pos::new(0, 0));
        assert_eq!(loaded.single_selection(), None);
        //And a pan press on an empty hex, which a release would otherwise take as a click
        let mut loaded = panning(vec![bonder(0, 0)]);
        click(&mut loaded, 0, 0);
        press(&mut loaded, 5, 5, KeyMods::default());
        loaded.board_input(BoardInput::Cancel, false);
        loaded.release();
        assert_eq!(loaded.single_selection(), Some(PartId::Glyph(0)));
    }

    #[test]
    fn held_pan_keys_move_the_view_until_released() {
        let mut loaded = Loaded::for_test(vec![bonder(0, 0)], vec![]);
        let start = px(&loaded, 0, 0);
        assert!(!loaded.handle_key(KeyCode::Right, KeyMods::default()), "off unless chosen");
        loaded.keys_pan = true;
        assert!(loaded.handle_key(KeyCode::D, KeyMods::default()));
        assert!(loaded.handle_key(KeyCode::Up, KeyMods::default()));
        assert!(loaded.pan_with_keys(TEST_FRAME));
        let moved = px(&loaded, 0, 0);
        assert!(moved.0 < start.0 && moved.1 > start.1, "the view went right and up: {start:?} -> {moved:?}");
        loaded.key_up(KeyCode::D);
        loaded.key_up(KeyCode::Up);
        assert!(!loaded.pan_with_keys(TEST_FRAME));
        assert_eq!(px(&loaded, 0, 0), moved);
    }

    #[test]
    fn a_pan_key_held_from_before_a_drag_does_not_turn_the_part() {
        let mut loaded = Loaded::for_test(vec![bonder(0, 0)], vec![]);
        loaded.keys_pan = true;
        assert!(loaded.handle_key(KeyCode::D, KeyMods::default()));
        press(&mut loaded, 0, 0, KeyMods::default());
        move_to(&mut loaded, 3, 0);
        //The held key's repeats
        loaded.handle_key(KeyCode::D, KeyMods::default());
        loaded.handle_key(KeyCode::D, KeyMods::default());
        loaded.release();
        assert_eq!(loaded.timeline.base().glyphs[0].rot, 0);
    }

    #[test]
    fn pan_keys_turn_a_dragged_part_instead() {
        let mut loaded = Loaded::for_test(vec![bonder(0, 0)], vec![]);
        loaded.keys_pan = true;
        press(&mut loaded, 0, 0, KeyMods::default());
        move_to(&mut loaded, 3, 0);
        let before = px(&loaded, 0, 0);
        assert!(loaded.handle_key(KeyCode::A, KeyMods::default()));
        assert!(!loaded.pan_with_keys(TEST_FRAME));
        assert_eq!(px(&loaded, 0, 0), before);
        loaded.release();
        assert_eq!(loaded.timeline.base().glyphs[0].rot, 1);
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
    fn a_drag_while_running_moves_the_part() {
        let mut loaded = Loaded::for_test(vec![bonder(0, 0)], vec![]);
        loaded.run_state = RunState::FreeRun;
        press(&mut loaded, 0, 0, KeyMods::default());
        move_to(&mut loaded, 3, 0);
        loaded.update(TEST_FRAME);
        loaded.refresh_preview();
        assert!(loaded.preview.is_some());
        loaded.release();
        assert_eq!(loaded.timeline.base().glyphs[0].pos, Pos::new(3, 0));
        assert!(matches!(loaded.run_state, RunState::FreeRun));
    }

    //Esc used to drop the part but leave the pan, so the release deselected as if the board was clicked
    #[test]
    fn with_left_drag_panning_esc_while_held_keeps_the_selection() {
        let mut loaded = panning(vec![bonder(0, 0)]);
        click(&mut loaded, 0, 0);
        press(&mut loaded, 0, 0, KeyMods::default());
        assert!(loaded.handle_key(KeyCode::Escape, KeyMods::default()));
        loaded.release();
        assert_eq!(loaded.single_selection(), Some(PartId::Glyph(0)));
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
        assert_eq!(loaded.single_selection(), None, "the press waits, and the release waits behind it");
        loaded.flush_board_input();
        assert_eq!(loaded.single_selection(), Some(PartId::Glyph(0)));

        let empty = px(&loaded, 5, 5);
        loaded.board_input(BoardInput::Press{px: empty, screen: SCREEN, keymods: KeyMods::default()}, false);
        assert_eq!(loaded.single_selection(), None, "without typing, it acts right away");
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
        assert_eq!(loaded.apply_edit(&LayoutEdit::Reorder{kind: PartKind::Glyph, moved: vec![0], to: 1}), Ok(()));
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
        assert_eq!(loaded.apply_edit(&LayoutEdit::Reorder{kind: PartKind::Glyph, moved: vec![0], to: 1}), Ok(()));
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
        let caret_arm = |loaded: &Loaded| loaded.timeline.base().arms[loaded.tape_sel.arm].tape().first;
        assert_eq!(loaded.apply_edit(&LayoutEdit::Reorder{kind: PartKind::Arm, moved: vec![0], to: 3}), Ok(()));
        assert_eq!(caret_arm(&loaded), 2);
        assert_eq!(loaded.apply_edit(&LayoutEdit::Delete(vec![PartId::Arm(0)])), Ok(()));
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
        let previewed = loaded.preview.as_ref().and_then(|p| p.arms_now.first().map(|(_, arm)| arm)).map(pose);
        assert_eq!(previewed, Some((Pos::new(3, 0), 0, 1)));
        loaded.release();
        loaded.update(TEST_FRAME);
        assert_eq!(loaded.timeline.curr().timestep, 2, "it runs back to the step it was edited at");
        assert_eq!(Some(pose(&loaded.timeline.curr().arms[0])), previewed);
    }

    //The tapes can be typed in while a part is held
    #[test]
    fn the_drag_preview_runs_the_arm_with_tapes_changed_while_dragging() {
        let mut loaded = carried_arm();
        loaded.run_state = RunState::Manual(2);
        press(&mut loaded, 2, 0, KeyMods::default());
        move_to(&mut loaded, 4, 0);
        loaded.refresh_preview();
        loaded.timeline.base_mut().set_tape(0, Tape::new(0, vec![Instr::RotateClockwise]));
        loaded.finish_tape_edit(0);
        loaded.update(TEST_FRAME);
        loaded.refresh_preview();
        let previewed = loaded.preview.as_ref().and_then(|p| p.arms_now.first().map(|(_, arm)| arm)).map(pose);
        loaded.release();
        loaded.update(TEST_FRAME);
        assert_eq!(loaded.timeline.curr().timestep, 2);
        assert_eq!(Some(pose(&loaded.timeline.curr().arms[0])), previewed);
    }

    //Dragged onto a track, an arm's Reset runs longer, which makes the loop longer. The preview kept the loop from
    //before.
    #[test]
    fn the_drag_preview_runs_the_arm_with_the_loop_the_edit_gives() {
        let track = Glyph{pos: Pos::new(0, 0), rot: 0, glyph_type: GlyphType::Track((0..4).map(|x| Pos::new(x, 0)).collect())};
        //Off the track, the Reset has nothing to undo
        let arm = Arm::new(Pos::new(0, 2), 0, 1, ArmType::PlainArm, Tape::new(0, vec![Instr::Forward, Instr::Forward, Instr::Reset]));
        let mut loaded = Loaded::for_test(vec![track], vec![arm]);
        assert_eq!(loaded.timeline.base().repeat_length, 3);
        loaded.timeline.advance_to(5.).unwrap();
        press(&mut loaded, 0, 2, KeyMods::default());
        move_to(&mut loaded, 0, 0);
        loaded.refresh_preview();
        let previewed = loaded.preview.as_ref().and_then(|p| p.arms_now.first().map(|(_, arm)| arm)).map(pose);
        loaded.release();
        loaded.update(TEST_FRAME);
        //On the track, it runs F F t t
        assert_eq!(loaded.timeline.base().repeat_length, 4);
        assert_eq!(loaded.timeline.curr().timestep, 5);
        assert_eq!(Some(pose(&loaded.timeline.curr().arms[0])), previewed);
    }

    const CTRL: KeyMods = KeyMods{shift: false, ctrl: true, alt: false, logo: false};
    fn selected(parts: &[PartId]) -> BTreeSet<PartId>{
        parts.iter().copied().collect()
    }
    fn shift_click(loaded: &mut Loaded, x: i32, y: i32){
        press(loaded, x, y, SHIFT);
        loaded.release();
    }
    fn drag(loaded: &mut Loaded, from: (i32, i32), to: (i32, i32), keymods: KeyMods){
        press(loaded, from.0, from.1, keymods);
        move_to(loaded, to.0, to.1);
        loaded.release();
    }
    fn arm_at(x: i32, y: i32) -> Arm{
        Arm::new(Pos::new(x, y), 0, 1, ArmType::PlainArm, Tape::new(0, vec![Instr::Grab, Instr::RotateClockwise]))
    }
    fn input_at(x: i32, y: i32) -> Glyph{
        let salt = vec![Atom::new(Pos::new(0, 0), AtomType::Salt)];
        Glyph{pos: Pos::new(x, y), rot: 0, glyph_type: GlyphType::Input(smallvec![salt], 0)}
    }
    fn glyph_positions(loaded: &Loaded) -> Vec<Pos>{
        loaded.timeline.base().glyphs.iter().map(|g| g.pos).collect()
    }

    #[test]
    fn shift_click_adds_or_removes_a_part_and_lists_the_selection() {
        let mut loaded = Loaded::for_test(vec![bonder(0, 0), bonder(5, 0)], vec![]);
        shift_click(&mut loaded, 0, 0);
        shift_click(&mut loaded, 5, 0);
        assert_eq!(loaded.selection, selected(&[PartId::Glyph(0), PartId::Glyph(1)]));
        shift_click(&mut loaded, 1, 0);
        assert_eq!(loaded.selection, selected(&[PartId::Glyph(1)]), "a second Shift+click takes it out");
        assert_eq!(loaded.list, PartList::Parts(vec![PartId::Glyph(1)]));
        shift_click(&mut loaded, 9, 9);
        assert_eq!(loaded.selection, selected(&[PartId::Glyph(1)]), "an empty hex changes nothing");
        click(&mut loaded, 0, 0);
        assert_eq!(loaded.selection, selected(&[PartId::Glyph(0)]), "a click selects only that part");
        assert_eq!(loaded.list, PartList::Hex(Pos::new(0, 0)));
    }

    #[test]
    fn a_shift_drag_adds_the_parts_its_box_touches() {
        let track = Glyph{pos: Pos::new(-3, 3), rot: 0, glyph_type: GlyphType::Track((0..10).map(|x| Pos::new(x, 0)).collect())};
        let mut loaded = Loaded::for_test(vec![bonder(0, 0), bonder(5, 0), track, bonder(10, 10)], vec![]);
        shift_click(&mut loaded, 10, 10);
        //From the middle of (5,0) to the middle of (5,3), which the track passes through
        drag(&mut loaded, (5, 0), (5, 3), SHIFT);
        assert_eq!(loaded.selection, selected(&[PartId::Glyph(1), PartId::Glyph(2), PartId::Glyph(3)]));
        assert_eq!(glyph_positions(&loaded)[1], Pos::new(5, 0), "starting on a part, it still draws a box");
        //A box within one hex picks every part on it
        let mut loaded = Loaded::for_test(vec![bonder(0, 0)], vec![arm_at(0, 0)]);
        press(&mut loaded, 0, 0, SHIFT);
        let at = px(&loaded, 0, 0);
        loaded.pointer_moved((at.0 + 5., at.1 + 3.), SCREEN, false);
        loaded.release();
        assert_eq!(loaded.selection, selected(&[PartId::Glyph(0), PartId::Arm(0)]));
        //Without Shift, a drag on an empty hex picks nothing
        drag(&mut loaded, (4, 4), (0, 0), KeyMods::default());
        assert!(loaded.selection.is_empty());
    }

    #[test]
    fn dragging_a_selected_part_moves_the_group_and_undo_reselects_it() {
        let mut loaded = Loaded::for_test(vec![bonder(0, 0), bonder(5, 5)], vec![arm_at(0, 2)]);
        let group = selected(&[PartId::Glyph(0), PartId::Arm(0)]);
        shift_click(&mut loaded, 0, 0);
        shift_click(&mut loaded, 0, 2);
        drag(&mut loaded, (0, 0), (2, 0), KeyMods::default());
        assert_eq!(glyph_positions(&loaded), vec![Pos::new(2, 0), Pos::new(5, 5)]);
        assert_eq!(loaded.timeline.base().arms[0].pos, Pos::new(2, 2));
        assert_eq!(loaded.selection, group, "the group stays selected");
        assert_eq!(loaded.list, PartList::Parts(group.iter().copied().collect()), "and listed");
        click(&mut loaded, 9, 9);
        loaded.undo();
        assert_eq!(glyph_positions(&loaded), vec![Pos::new(0, 0), Pos::new(5, 5)]);
        assert_eq!(loaded.selection, group, "undo selects what was selected before the edit");
        assert_eq!(loaded.list, PartList::Parts(group.iter().copied().collect()));
        click(&mut loaded, 5, 5);
        loaded.redo();
        assert_eq!(loaded.timeline.base().arms[0].pos, Pos::new(2, 2));
        assert!(loaded.selection.is_empty(), "redo selects what was selected when it was undone");
    }

    #[test]
    fn a_group_turns_around_the_held_hex() {
        let mut loaded = Loaded::for_test(vec![bonder(0, 0)], vec![arm_at(2, 0)]);
        loaded.select_all();
        //By the arm, moved up a hex, then turned counterclockwise
        press(&mut loaded, 2, 0, KeyMods::default());
        move_to(&mut loaded, 2, 1);
        assert!(loaded.handle_key(KeyCode::A, KeyMods::default()));
        loaded.release();
        let base = loaded.timeline.base();
        assert_eq!(pose(&base.arms[0]), (Pos::new(2, 1), 1, 1));
        //The bonder was two hexes left of the arm, which turns to two hexes down-right
        assert_eq!((base.glyphs[0].pos, base.glyphs[0].rot), (Pos::new(2, 1) + rotate(Pos::new(-2, 0), 1), 1));
    }

    #[test]
    fn lengthening_a_group_takes_each_arm_as_far_as_it_goes() {
        let short = Arm::new(Pos::new(0, 0), 0, 1, ArmType::PlainArm, Tape::new(0, vec![]));
        let long = Arm::new(Pos::new(4, 0), 0, 3, ArmType::PlainArm, Tape::new(0, vec![]));
        let mut loaded = Loaded::for_test(vec![], vec![short, long]);
        loaded.select_all();
        press(&mut loaded, 0, 0, KeyMods::default());
        move_to(&mut loaded, 0, 1);
        for key in [KeyCode::W, KeyCode::W, KeyCode::W, KeyCode::S]{
            loaded.handle_key(key, KeyMods::default());
        }
        loaded.release();
        let lengths: Vec<i32> = loaded.timeline.base().arms.iter().map(|a| a.len).collect();
        assert_eq!(lengths, vec![2, 3], "after W past the longest length, S steps back at once");
    }

    #[test]
    fn ctrl_dragging_a_group_copies_all_but_the_puzzles_parts() {
        let mut loaded = Loaded::for_test(vec![input_at(0, 0)], vec![arm_at(1, 0)]);
        loaded.select_all();
        drag(&mut loaded, (1, 0), (1, 3), CTRL);
        let base = loaded.timeline.base();
        assert_eq!(glyph_positions(&loaded), vec![Pos::new(0, 0)], "the input stays, uncopied");
        assert_eq!(base.arms.iter().map(|a| a.pos).collect::<Vec<_>>(), vec![Pos::new(1, 0), Pos::new(1, 3)]);
        assert_eq!(base.arms[1].tape().instructions, base.arms[0].tape().instructions);
        assert_eq!(loaded.selection, selected(&[PartId::Arm(1)]), "the copies are selected");
        assert_eq!(loaded.list, PartList::Parts(vec![PartId::Arm(1)]), "and listed");
    }

    #[test]
    fn delete_removes_a_group_but_not_the_puzzles_parts() {
        let mut loaded = Loaded::for_test(vec![input_at(0, 0), bonder(3, 0)], vec![arm_at(5, 0)]);
        assert!(loaded.handle_key(KeyCode::A, CTRL));
        assert_eq!(loaded.list, PartList::All);
        assert!(loaded.handle_key(KeyCode::Delete, KeyMods::default()));
        let base = loaded.timeline.base();
        assert_eq!((glyph_positions(&loaded), base.arms.len()), (vec![Pos::new(0, 0)], 0));
        assert!(loaded.selection.is_empty());
        //With only the input left to delete, it's refused
        loaded.select_all();
        loaded.handle_key(KeyCode::Delete, KeyMods::default());
        assert_eq!(glyph_positions(&loaded), vec![Pos::new(0, 0)]);
        assert!(loaded.message.is_some());
    }

    #[test]
    fn a_group_isnt_dropped_onto_another_part() {
        let mut loaded = Loaded::for_test(vec![bonder(0, 0), bonder(0, 4), bonder(3, 2)], vec![]);
        shift_click(&mut loaded, 0, 0);
        shift_click(&mut loaded, 0, 4);
        //Moves the second bonder onto the third
        drag(&mut loaded, (0, 0), (3, -2), KeyMods::default());
        assert_eq!(glyph_positions(&loaded), vec![Pos::new(0, 0), Pos::new(0, 4), Pos::new(3, 2)]);
        assert!(loaded.message.as_ref().is_some_and(|m| m.contains("overlap")), "{:?}", loaded.message);
    }

    #[test]
    fn deleting_renumbers_the_selection_and_list() {
        let mut loaded = Loaded::for_test(vec![bonder(0, 0), bonder(3, 0), bonder(6, 0)], vec![]);
        shift_click(&mut loaded, 3, 0);
        shift_click(&mut loaded, 6, 0);
        assert_eq!(loaded.apply_edit(&LayoutEdit::Delete(vec![PartId::Glyph(0)])), Ok(()));
        assert!(loaded.selection.is_empty(), "a deletion selects nothing");
        assert_eq!(loaded.list, PartList::Parts(vec![PartId::Glyph(0), PartId::Glyph(1)]), "the list keeps its parts");
        assert_eq!(glyph_positions(&loaded), vec![Pos::new(3, 0), Pos::new(6, 0)]);
    }

    #[test]
    fn reordering_keeps_the_selection_on_its_parts() {
        let mut loaded = Loaded::for_test(vec![bonder(0, 0), bonder(3, 0), bonder(6, 0)], vec![]);
        shift_click(&mut loaded, 0, 0);
        shift_click(&mut loaded, 3, 0);
        let edit = LayoutEdit::Reorder{kind: PartKind::Glyph, moved: vec![0, 1], to: 1};
        assert_eq!(loaded.apply_edit(&edit), Ok(()));
        assert_eq!(glyph_positions(&loaded), vec![Pos::new(6, 0), Pos::new(0, 0), Pos::new(3, 0)]);
        assert_eq!(loaded.selection, selected(&[PartId::Glyph(1), PartId::Glyph(2)]));
    }

    #[test]
    fn esc_clears_the_selection_last() {
        let mut loaded = Loaded::for_test(vec![bonder(0, 0)], vec![]);
        loaded.select_all();
        loaded.start_placing(NewPart::Glyph(GlyphType::Calcification));
        assert!(loaded.handle_key(KeyCode::Escape, KeyMods::default()));
        assert_eq!(loaded.selection.len(), 1, "the part being placed goes first");
        assert!(loaded.handle_key(KeyCode::Escape, KeyMods::default()));
        assert!(loaded.selection.is_empty());
        assert!(!loaded.handle_key(KeyCode::Escape, KeyMods::default()));
    }
}
