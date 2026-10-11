//The loaded solution: its timeline and the editor state around it
use super::*;
use std::ops::Range;

pub(super) enum RunState{
    //Crashed holds the timestep it crashed in
    Manual(usize), FreeRun, Crashed(usize)
}
//What an undo step changed: tapes-only steps can be undone without re-simulating from the start
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum StepKind{
    Layout, Tapes
}
//Hidden still checks collisions and counts area, so showing it again doesn't re-simulate
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum AreaMode{
    Shown, Hidden, Off
}
//Saved solutions are named as edited by omclone
pub(super) fn saved_solution_name(name: &str) -> String{
    if name.contains("omclone") {name.to_owned()} else {format!("{name}(omclone)")}
}
pub(super) struct Loaded{
    pub(super) timeline: Timeline,
    pub(super) float_world: FloatWorld,

    pub(super) max_timestep: usize,
    pub(super) camera: CameraSetup,
    pub(super) camera_window_open: bool,
    pub(super) solution: parser::FullSolution,
    pub(super) message: Option<String>,
    //The solution changed since it was loaded or saved
    pub(super) dirty: bool,
    //It changed since the emergency save was last updated (see recovery)
    backup_stale: bool,

    pub(super) tape_mode: bool,
    pub(super) tape_sel: TapeSelection,
    //The arms and caret timestep after the last tape edit, while more typing there joins its undo step
    pub(super) tape_group: Option<(Range<usize>, usize)>,
    pub(super) run_state: RunState,
    pub(super) area_mode: AreaMode,
    pub(super) run_speed: f64,
    pub(super) popup_reload: bool,

    //Part editing
    pub(super) selection: Option<PartId>,
    //The hex last pressed, whose parts the Part window lists as they are now
    pub(super) list_hex: Option<Pos>,
    //The Part window lists every glyph instead
    pub(super) all_glyphs: bool,
    pub(super) allow_overlap: bool,
    pub(super) tool: BoardTool,
    pub(super) allow_gaps: bool,
    pub(super) path_text: Option<PathText>,
    //Whatever the mouse would change now, drawn in place, and the tracks other than one it changes
    pub(super) preview: Option<Preview>,
    pub(super) scene_tracks: Option<(usize, TrackMaps)>,
    pub(super) pointer_over_ui: bool,
    //The hex the mouse was last over, kept while it's over a window
    pub(super) hovered_hex: Option<Pos>,
    //Board input waiting for egui to apply the text field being typed in (see board_input)
    pub(super) waiting_input: Vec<BoardInput>,
    pub(super) history: UndoHistory<World, StepKind>,
}
impl Loaded{
    pub(super) fn new(world: World, solution: parser::FullSolution, max_timestep: usize, camera: CameraSetup) -> Self{
        //A solution that already overlaps can keep doing so
        let allow_overlap = world.has_overlap();
        //Collision checks are on, so edits that collide are caught
        Loaded{
            timeline: Timeline::new(world, true),
            float_world: FloatWorld::new(),

            max_timestep,camera,solution,
            camera_window_open: false,
            message: None,
            dirty: false,
            backup_stale: false,

            tape_mode: false,
            tape_sel: TapeSelection::default(),
            tape_group: None,
            run_state: RunState::Manual(0),
            area_mode: AreaMode::Hidden,
            run_speed: 0.05,
            popup_reload: false,

            selection: None,
            list_hex: None,
            all_glyphs: false,
            allow_overlap,
            tool: BoardTool::Select(None),
            allow_gaps: false,
            path_text: None,
            preview: None,
            scene_tracks: None,
            pointer_over_ui: false,
            hovered_hex: None,
            waiting_input: Vec::new(),
            history: UndoHistory::new(),
        }
    }
    //Replaces the starting world after a layout edit (which can be undone), then runs back to the shown timestep
    pub(super) fn commit_world(&mut self, mut world: World){
        world.repeat_length = world.repeat_length.max(self.timeline.base().repeat_length);
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
    }
    //The solution as Save writes it
    pub(super) fn solution_bytes(&self) -> Result<Vec<u8>>{
        let solution = parser::create_solution(self.timeline.base(), self.solution.puzzle_name.clone(),
            saved_solution_name(&self.solution.solution_name));
        let mut data = Vec::new();
        parser::write_solution(&mut data, &solution)?;
        Ok(data)
    }
    //Keeps the emergency save up to date with the unsaved changes. `path` is where Save would write.
    pub(super) fn update_backup(&mut self, path: impl FnOnce() -> PathBuf){
        if self.backup_stale && self.dirty{
            if let Ok(data) = self.solution_bytes(){
                recovery::store(data, path());
            }
        }
        self.backup_stale = false;
    }
    //Why undo and redo can't be used right now, if they can't
    pub(super) fn history_lock(&self) -> Option<&'static str>{
        if self.tool.press().is_some() || self.tool.stroke().is_some(){
            Some("Release the mouse button first")
        } else {
            self.edit_lock()
        }
    }
    //Goes back to the starting world (with its tapes) from before the last edit
    pub(super) fn undo(&mut self){
        if self.history_lock().is_some() || !self.history.can_undo(){
            return;
        }
        let (previous, kind) = self.history.undo(self.timeline.base().clone()).unwrap();
        self.restore(previous, kind);
    }
    pub(super) fn redo(&mut self){
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
    pub(super) fn record_tape_edit(&mut self, arms: Range<usize>, from: usize, to: usize, joins: bool, extends: bool){
        if !(joins && self.tape_group.as_ref().is_some_and(|(a, step)| *a == arms && *step == from)){
            self.history.record(self.timeline.base().clone(), StepKind::Tapes);
        }
        self.tape_group = extends.then_some((arms, to));
    }
    //Updates everything that depends on the tapes after they changed, starting from timestep `earliest`
    pub(super) fn finish_tape_edit(&mut self, mut earliest: usize){
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
    pub(super) fn cursor_step(&self) -> usize{
        match self.run_state{
            RunState::Manual(target) => target,
            RunState::FreeRun | RunState::Crashed(_) => self.timeline.time().floor() as usize,
        }
    }
    //Updates the timeline after the tapes or loop length in its base changed.
    //`earliest_step` is the first timestep whose behavior may have changed.
    pub(super) fn apply_base_change(&mut self, earliest_step: usize){
        self.mark_changed();
        if self.timeline.program_changed(earliest_step as u64){
            self.clear_crash();
        }
    }
    //Sets the loop length, returning the first timestep it can affect
    pub(super) fn change_repeat_length(&mut self, length: usize) -> usize{
        if self.max_timestep < length{
            self.max_timestep = length;
        }
        self.timeline.base_mut().set_repeat_length(length)
    }
    //Sets the loop length as an edit of its own, which can be undone
    pub(super) fn edit_repeat_length(&mut self, length: usize){
        self.history.record(self.timeline.base().clone(), StepKind::Tapes);
        self.tape_group = None;
        let earliest_step = self.change_repeat_length(length);
        self.apply_base_change(earliest_step);
    }
    pub(super) fn set_area_mode(&mut self, mode: AreaMode){
        self.area_mode = mode;
        if self.timeline.set_mark_area(mode != AreaMode::Off){
            self.clear_crash();
        }
    }
    //After the timeline moved back: a crash past that point hasn't happened yet, so aim for where it was
    pub(super) fn clear_crash(&mut self){
        if let RunState::Crashed(step) = self.run_state{
            self.run_state = RunState::Manual(step);
        }
        self.message = None;
    }
    pub(super) fn try_set_target_time(&mut self, target: usize){
        if let RunState::Crashed(step) = self.run_state{
            if step > target{
                self.run_state = RunState::Manual(target);
            }
        } else {
            self.run_state = RunState::Manual(target);
        }
    }
    pub(super) fn update(&mut self){
        let time = self.timeline.time();
        let target_time = match self.run_state{
            RunState::Manual(target_timestep) => {
                let target_timestep = target_timestep as f64;
                if target_timestep > time+1.001{
                    target_timestep
                } else {
                    f64::min(time+self.run_speed,target_timestep)
                }
            },
            RunState::FreeRun => time+self.run_speed,
            RunState::Crashed(_) => return,
        };
        if target_time < time{
            self.timeline.rewind(target_time as u64);
            self.clear_crash();
        }
        if let Err(err) = self.timeline.advance_to(target_time){
            self.run_state = RunState::Crashed(self.timeline.curr().timestep as usize);
            self.message = Some(err.to_string());
        }
    }
}

#[cfg(all(test, feature = "editor_ui"))]
impl Loaded{
    //A solution with these parts, as if just loaded, with the board's origin in the middle of the screen
    pub(super) fn for_test(glyphs: Vec<Glyph>, arms: Vec<Arm>) -> Self{
        let world = World::setup_sim(&InitialWorld{glyphs, arms}).unwrap();
        let solution = parser::create_solution(&world, String::new(), String::new());
        Loaded::new(world, solution, 100, CameraSetup{scale_base: 0.05, offset: XYVec::zeros()})
    }
    //The pixel at a hex's center, on a screen of size TEST_SCREEN
    pub(super) fn test_px(&self, x: i32, y: i32) -> (f32, f32){
        let scale = self.camera.scale(TEST_SCREEN);
        let world = pos_to_xy(Pos::new(x, y)) + self.camera.offset;
        ((world.x*scale.0 + 1.)/2.*TEST_SCREEN.0, (1. - world.y*scale.1)/2.*TEST_SCREEN.1)
    }
}
#[cfg(all(test, feature = "editor_ui"))]
pub(super) const TEST_SCREEN: (f32, f32) = (1000., 800.);

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
