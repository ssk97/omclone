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
pub(super) struct Loaded{
    pub(super) timeline: Timeline,
    pub(super) float_world: FloatWorld,

    pub(super) max_timestep: usize,
    pub(super) camera: CameraSetup,
    pub(super) tracks: TrackBindings,
    pub(super) solution: parser::FullSolution,
    pub(super) message: Option<String>,

    pub(super) tape_mode: bool,
    pub(super) tape_sel: TapeSelection,
    //The arms and caret timestep after the last tape edit, while more typing there joins its undo step
    pub(super) tape_group: Option<(Range<usize>, usize)>,
    pub(super) run_state: RunState,
    pub(super) show_area: bool,
    pub(super) run_speed: f64,
    pub(super) popup_reload: bool,

    //Part editing
    pub(super) selection: Option<PartId>,
    //The parts at the hex last pressed, top first
    pub(super) stack_view: Option<Vec<PartId>>,
    pub(super) allow_overlap: bool,
    pub(super) press: Option<Press>,
    pub(super) placing: Option<Placing>,
    pub(super) track_mode: Option<TrackMode>,
    pub(super) stroke: Option<TrackStroke>,
    pub(super) allow_gaps: bool,
    pub(super) path_text: Option<PathText>,
    //Whatever the mouse would change now, drawn in place, and the tracks other than one it changes
    pub(super) preview: Option<Preview>,
    pub(super) scene_tracks: Option<(usize, TrackBindings)>,
    pub(super) pointer_over_ui: bool,
    pub(super) history: UndoHistory<World, StepKind>,
    pub(super) tracks_dirty: bool,
    //Track buffers to free once the rendering context is available
    pub(super) dead_tracks: Vec<TrackBindings>,
}
impl Loaded{
    pub(super) fn new(world: World, solution: parser::FullSolution, max_timestep: usize, camera: CameraSetup,
        tracks: TrackBindings) -> Self
    {
        //A solution that already overlaps can keep doing so
        let allow_overlap = world.has_overlap();
        //Also enables collision checks, so edits that collide are caught
        let show_area = true;
        Loaded{
            timeline: Timeline::new(world, show_area),
            float_world: FloatWorld::new(),

            max_timestep,camera,tracks,solution,
            message: None,

            tape_mode: false,
            tape_sel: TapeSelection::default(),
            tape_group: None,
            run_state: RunState::Manual(0),
            show_area,
            run_speed: 0.05,
            popup_reload: false,

            selection: None,
            stack_view: None,
            allow_overlap,
            press: None,
            placing: None,
            track_mode: None,
            stroke: None,
            allow_gaps: false,
            path_text: None,
            preview: None,
            scene_tracks: None,
            pointer_over_ui: false,
            history: UndoHistory::new(),
            tracks_dirty: false,
            dead_tracks: Vec::new(),
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
        self.tracks_dirty = true;
        self.clear_crash();
        self.try_set_target_time(target);
    }
    //Why undo and redo can't be used right now, if they can't
    pub(super) fn history_lock(&self) -> Option<&'static str>{
        if self.press.is_some() || self.stroke.is_some(){
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
        self.stack_view = None;
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
