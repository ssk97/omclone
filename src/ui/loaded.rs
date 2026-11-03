//The loaded solution: its timeline and the editor state around it
use super::*;
use std::ops::Range;

pub enum RunState{
    Manual(usize), FreeRun,
    //The timestep it crashed in, and why
    Crashed{step: usize, error: SimError},
}
//What an undo step changed: tapes-only steps can be undone without re-simulating from the start
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum StepKind{
    Layout, Tapes
}
//Hidden still checks collisions and counts area, so showing it again doesn't re-simulate
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum AreaMode{
    Shown, Hidden, Off
}
//Saved solutions are named as edited by omclone
pub fn saved_solution_name(name: &str) -> String{
    if name.contains("omclone") {name.to_owned()} else {format!("{name}(omclone)")}
}
pub struct Loaded{
    pub timeline: Timeline,
    pub float_world: FloatWorld,

    pub max_timestep: usize,
    pub camera: CameraSetup,
    pub camera_window_open: bool,
    pub help_window_open: bool,
    pub tape_help_window_open: bool,
    //Opened by each crash, and closable until the next
    pub crash_window_open: bool,
    pub solution: parser::FullSolution,
    //For checking the solution against the rules omsim applies when loading it
    puzzle: parser::FullPuzzle,
    pub message: Option<String>,
    //What loading changed from the file, until Save writes the change. Only the editor shows these.
    #[cfg_attr(not(feature = "editor_ui"), allow(dead_code))]
    pub load_notes: Vec<String>,
    //What omsim would reject the solution for, as Save would write it
    #[cfg_attr(not(feature = "editor_ui"), allow(dead_code))]
    pub rule_notes: Vec<String>,
    //The solution changed since it was loaded or saved
    pub dirty: bool,
    //It changed since the emergency save was last updated (see recovery)
    backup_stale: bool,

    pub tape_mode: bool,
    pub tape_sel: TapeSelection,
    //The arms and caret timestep after the last tape edit, while more typing there joins its undo step
    pub tape_group: Option<(Range<usize>, usize)>,
    pub run_state: RunState,
    //The timeline's time as last shown. An edit can move the timeline back to re-simulate from before its change, and
    //it comes straight back here (or to the target, if that's earlier) instead of replaying the steps on the way.
    shown_time: f64,
    pub area_mode: AreaMode,
    //1 is held, so the board's atoms are drawn faintly
    pub atoms_faded: bool,
    pub run_speed: f64,
    pub popup_reload: bool,

    //Part editing
    pub selection: Option<PartId>,
    //The hex last pressed, whose parts the Part window lists as they are now
    pub list_hex: Option<Pos>,
    //The Part window lists every glyph instead
    pub all_glyphs: bool,
    //numbering_version at the latest press, for the clicks and drags in that list (see part_window)
    pub list_press_numbering: u64,
    pub allow_overlap: bool,
    //A left drag on the board pans instead of moving a part (see left_drag_pans)
    pub left_drag_pans: bool,
    pub tool: BoardTool,
    //The left button is held on the board while left drags pan
    pub pan: Option<PanPress>,
    pub allow_gaps: bool,
    pub path_text: Option<PathText>,
    //Whatever the mouse would change now, drawn in place, and the tracks other than one it changes
    pub preview: Option<Preview>,
    pub scene_tracks: Option<(usize, TrackMaps)>,
    pub pointer_over_ui: bool,
    //The hex the mouse was last over, kept while it's over a window
    pub hovered_hex: Option<Pos>,
    //Board input waiting for egui to apply the text field being typed in (see board_input)
    pub waiting_input: Vec<BoardInput>,
    pub history: UndoHistory<World, StepKind>,
    //Counts the changes that may renumber parts: edits that delete or reorder parts, and undoing or redoing a layout
    //edit. Clicks and drags that pick parts by number record it at their press, and do nothing if it changes before
    //they finish.
    pub numbering_version: u64,
}
impl Loaded{
    pub fn new(world: World, solution: parser::FullSolution, puzzle: parser::FullPuzzle, max_timestep: usize,
        camera: CameraSetup) -> Self
    {
        //A solution that already overlaps, or breaks its chambers' rules, can keep doing so
        let allow_overlap = world.has_overlap() || world.breaks_chamber_rules();
        let load_notes = rules::adjustments(&solution).iter().map(|a| a.to_string()).collect();
        //Collision checks are on, so edits that collide are caught
        let mut loaded = Loaded{
            timeline: Timeline::new(world, true),
            float_world: FloatWorld::new(),

            max_timestep,camera,solution,puzzle,
            camera_window_open: false,
            help_window_open: false,
            tape_help_window_open: false,
            crash_window_open: false,
            message: None,
            load_notes,
            rule_notes: Vec::new(),
            dirty: false,
            backup_stale: false,

            tape_mode: false,
            tape_sel: TapeSelection::default(),
            tape_group: None,
            run_state: RunState::Manual(0),
            shown_time: 0.,
            area_mode: AreaMode::Hidden,
            atoms_faded: false,
            run_speed: 0.05,
            popup_reload: false,

            selection: None,
            list_hex: None,
            all_glyphs: false,
            list_press_numbering: 0,
            allow_overlap,
            left_drag_pans: false,
            tool: BoardTool::Select(None),
            pan: None,
            allow_gaps: false,
            path_text: None,
            preview: None,
            scene_tracks: None,
            pointer_over_ui: false,
            hovered_hex: None,
            waiting_input: Vec::new(),
            history: UndoHistory::new(),
            numbering_version: 0,
        };
        loaded.check_rules();
        loaded
    }
    fn check_rules(&mut self){
        let solution = parser::create_solution(self.timeline.base(), String::new(), String::new());
        self.rule_notes = rules::rejections(&self.puzzle, &solution).iter().map(|r| r.note()).collect();
    }
    //Sets up a layout as an edit makes it. It keeps the loop length, unless its tapes need a longer one.
    pub fn set_up(&self, layout: &InitialWorld) -> std::result::Result<World, String>{
        let mut world = World::setup_sim(layout).map_err(|e| e.to_string())?;
        world.repeat_length = world.repeat_length.max(self.timeline.base().repeat_length);
        Ok(world)
    }
    //Replaces the starting world with one from set_up after a layout edit (which can be undone), then runs back to the
    //shown timestep
    pub fn commit_world(&mut self, world: World){
        self.history.record(self.timeline.base().clone(), StepKind::Layout);
        self.tape_group = None;
        self.replace_base(world);
    }
    fn replace_base(&mut self, world: World){
        //The pending edit may be unchanged, but its preview was made from the old base
        self.drop_preview();
        let target = self.timeline.time().floor() as usize;
        self.timeline.replace_base(world);
        self.mark_changed();
        self.clear_crash();
        self.try_set_target_time(target);
    }
    fn mark_changed(&mut self){
        self.dirty = true;
        self.backup_stale = true;
        self.check_rules();
    }
    //The solution as Save writes it
    pub fn solution_bytes(&self) -> Result<Vec<u8>>{
        let solution = parser::create_solution(self.timeline.base(), self.solution.puzzle_name.clone(),
            saved_solution_name(&self.solution.solution_name));
        let mut data = Vec::new();
        parser::write_solution(&mut data, &solution)?;
        Ok(data)
    }
    //Keeps the emergency save up to date with the unsaved changes. `path` is where Save would write.
    pub fn update_backup(&mut self, path: impl FnOnce() -> PathBuf){
        if self.backup_stale && self.dirty{
            if let Ok(data) = self.solution_bytes(){
                recovery::store(data, path());
            }
        }
        self.backup_stale = false;
    }
    //Why undo and redo can't be used right now, if they can't
    pub fn history_lock(&self) -> Option<&'static str>{
        if self.tool.press().is_some() || self.tool.stroke().is_some(){
            Some("Release the mouse button first")
        } else {
            self.edit_lock()
        }
    }
    //Goes back to the starting world (with its tapes) from before the last edit
    pub fn undo(&mut self){
        if self.history_lock().is_some() || !self.history.can_undo(){
            return;
        }
        let (previous, kind) = self.history.undo(self.timeline.base().clone()).unwrap();
        self.restore(previous, kind);
    }
    pub fn redo(&mut self){
        if self.history_lock().is_some() || !self.history.can_redo(){
            return;
        }
        let (next, kind) = self.history.redo(self.timeline.base().clone()).unwrap();
        self.restore(next, kind);
    }
    fn restore(&mut self, world: World, kind: StepKind){
        self.tape_group = None;
        match kind{
            StepKind::Layout => self.restore_base(world),
            StepKind::Tapes => self.restore_tapes(world),
        }
    }
    //Parts may be numbered differently afterwards, so nothing stays selected, and the tapes' selection shrinks
    //back to the caret
    fn restore_base(&mut self, world: World){
        self.replace_base(world);
        self.numbering_version += 1;
        self.select(None);
        self.tape_sel.collapse();
    }
    //Puts back the tapes and loop length from a step that only changed those. The parts are the same, so the
    //selection stays, and only what follows the first changed timestep is simulated again.
    fn restore_tapes(&mut self, world: World){
        let Some(earliest) = self.timeline.base_mut().restore_program(world) else {return};
        let loop_len = self.timeline.base().repeat_length;
        if self.max_timestep < loop_len{
            self.max_timestep = loop_len;
        }
        self.finish_tape_edit(earliest);
    }
    //Keeps the starting world from before a tape edit, unless `joins` and the edit continues the last one: more
    //typing on the same arms, starting where the caret was left. If `extends`, more typing can join this edit.
    //`from` and `to` are the caret before and after the edit.
    pub fn record_tape_edit(&mut self, arms: Range<usize>, from: usize, to: usize, joins: bool, extends: bool){
        if !(joins && self.tape_group.as_ref().is_some_and(|(a, step)| *a == arms && *step == from)){
            self.history.record(self.timeline.base().clone(), StepKind::Tapes);
        }
        self.tape_group = extends.then_some((arms, to));
    }
    //Updates everything that depends on the tapes after they changed, starting from timestep `earliest`
    pub fn finish_tape_edit(&mut self, mut earliest: usize){
        let base = self.timeline.base();
        let longest = longest_tape(&base.arms);
        if self.max_timestep < longest{
            self.max_timestep = longest;
        }
        let min_loop = base.min_repeat_length();
        if base.repeat_length < min_loop{
            earliest = earliest.min(self.change_repeat_length(min_loop));
        }
        self.apply_base_change(earliest);
    }
    //The timestep the tape caret is at: where the timeline is headed, or where it is
    pub fn cursor_step(&self) -> usize{
        match self.run_state{
            RunState::Manual(target) => target,
            RunState::FreeRun | RunState::Crashed{..} => self.timeline.time().floor() as usize,
        }
    }
    //Updates the timeline after the tapes or loop length in its base changed.
    //`earliest_step` is the first timestep whose behavior may have changed.
    fn apply_base_change(&mut self, earliest_step: usize){
        self.mark_changed();
        if self.timeline.program_changed(earliest_step as u64){
            self.clear_crash();
        }
    }
    //Sets the loop length, returning the first timestep it can affect
    fn change_repeat_length(&mut self, length: usize) -> usize{
        if self.max_timestep < length{
            self.max_timestep = length;
        }
        self.timeline.base_mut().set_repeat_length(length)
    }
    //Sets the loop length as an edit of its own, which can be undone
    pub fn edit_repeat_length(&mut self, length: usize){
        self.history.record(self.timeline.base().clone(), StepKind::Tapes);
        self.tape_group = None;
        let earliest_step = self.change_repeat_length(length);
        self.apply_base_change(earliest_step);
    }
    pub fn set_area_mode(&mut self, mode: AreaMode){
        self.area_mode = mode;
        if self.timeline.set_mark_area(mode != AreaMode::Off){
            self.clear_crash();
        }
    }
    //After the timeline moved back: a crash past that point hasn't happened yet, so aim for where it was
    fn clear_crash(&mut self){
        if let RunState::Crashed{step, ..} = self.run_state{
            self.run_state = RunState::Manual(step);
        }
        self.message = None;
    }
    pub fn try_set_target_time(&mut self, target: usize){
        if let RunState::Crashed{step, ..} = self.run_state{
            if step > target{
                self.run_state = RunState::Manual(target);
            }
        } else {
            self.run_state = RunState::Manual(target);
        }
    }
    pub fn update(&mut self){
        let time = self.timeline.time();
        let target_time = match self.run_state{
            RunState::Manual(target_timestep) => {
                let target_timestep = target_timestep as f64;
                //Moves are measured from where the timeline was shown, so one step ahead of there plays out
                let from = time.max(self.shown_time.min(target_timestep));
                if target_timestep > from+1.001{
                    target_timestep
                } else {
                    f64::min(from+self.run_speed,target_timestep)
                }
            },
            RunState::FreeRun => time.max(self.shown_time)+self.run_speed,
            RunState::Crashed{..} => return,
        };
        if target_time < time{
            self.timeline.rewind(target_time as u64);
            self.clear_crash();
        }
        if let Err(error) = self.timeline.advance_to(target_time){
            self.run_state = RunState::Crashed{step: self.timeline.curr().timestep as usize, error};
            self.crash_window_open = true;
        }
        self.shown_time = self.timeline.time();
    }
}

#[cfg(all(test, feature = "editor_ui"))]
impl Loaded{
    //A solution with these parts, as if just loaded, with the board's origin in the middle of the screen
    pub fn for_test(glyphs: Vec<Glyph>, arms: Vec<Arm>) -> Self{
        Self::for_test_in_chambers(glyphs, arms, Chambers::default())
    }
    //The same, for a production puzzle with these chambers
    pub fn for_test_in_chambers(glyphs: Vec<Glyph>, arms: Vec<Arm>, chambers: Chambers) -> Self{
        let world = World::setup_sim(&InitialWorld{glyphs, arms, chambers: chambers.clone()}).unwrap();
        let solution = parser::create_solution(&world, String::new(), String::new());
        let puzzle = parser::FullPuzzle{puzzle_name: String::new(), creator_id: 0, allowed_bitfield: u64::MAX,
            inputs: vec![], outputs: vec![], output_multiplier: 1, chambers, conduits: vec![]};
        Loaded::new(world, solution, puzzle, 100, CameraSetup{scale_base: 0.05, offset: XYVec::zeros()})
    }
    //The pixel at a hex's center, on a screen of size TEST_SCREEN
    pub fn test_px(&self, x: i32, y: i32) -> (f32, f32){
        let scale = self.camera.scale(TEST_SCREEN);
        let world = pos_to_xy(Pos::new(x, y)) + self.camera.offset;
        ((world.x*scale.0 + 1.)/2.*TEST_SCREEN.0, (1. - world.y*scale.1)/2.*TEST_SCREEN.1)
    }
}
#[cfg(all(test, feature = "editor_ui"))]
pub const TEST_SCREEN: (f32, f32) = (1000., 800.);

#[cfg(all(test, feature = "editor_ui"))]
mod tests {
    use super::*;

    #[test]
    fn loop_length_edits_can_be_undone() {
        let arm = Arm::new(Pos::new(0, 0), 0, 1, ArmType::PlainArm, Tape::new(0, vec![Instr::RotateClockwise; 2]));
        let mut loaded = Loaded::for_test(vec![], vec![arm]);
        assert_eq!(loaded.timeline.base().repeat_length, 2);
        assert!(!loaded.dirty, "nothing has changed yet");
        loaded.edit_repeat_length(5);
        assert_eq!(loaded.timeline.base().repeat_length, 5);
        assert!(loaded.dirty);
        loaded.undo();
        assert_eq!(loaded.timeline.base().repeat_length, 2);
        loaded.redo();
        assert_eq!(loaded.timeline.base().repeat_length, 5);
    }

    #[test]
    fn a_crash_keeps_its_timestep_and_place() {
        //The first arm rides the track into the second, which sits at its end
        let track = Glyph{pos: Pos::new(0, 0), rot: 0, glyph_type: GlyphType::Track((0..3).map(|x| Pos::new(x, 0)).collect())};
        let rider = Arm::new(Pos::new(0, 0), 0, 1, ArmType::PlainArm, Tape::new(0, vec![Instr::Forward; 2]));
        let sitter = Arm::new(Pos::new(2, 0), 0, 1, ArmType::PlainArm, Tape::new(0, vec![]));
        let mut loaded = Loaded::for_test(vec![track], vec![rider, sitter]);
        loaded.run_state = RunState::Manual(5);
        loaded.update();
        let RunState::Crashed{step, error} = loaded.run_state else {panic!("expected a crash")};
        assert_eq!((step, error.error_str, xy_to_pos(error.location)), (1, "Arm-Arm collision!", Pos::new(2, 0)));
        assert!(loaded.crash_window_open);
        assert_eq!(loaded.message, None, "the crash has its own window");
    }

    #[test]
    fn only_turning_collisions_back_on_starts_over() {
        let arm = Arm::new(Pos::new(0, 0), 0, 1, ArmType::PlainArm, Tape::new(0, vec![Instr::RotateClockwise; 2]));
        let mut loaded = Loaded::for_test(vec![], vec![arm]);
        loaded.timeline.advance_to(50.).unwrap();
        loaded.set_area_mode(AreaMode::Hidden);
        assert_eq!(loaded.timeline.time(), 50.);
        loaded.set_area_mode(AreaMode::Off);
        loaded.set_area_mode(AreaMode::Shown);
        assert!(loaded.timeline.at_start());
    }
}
