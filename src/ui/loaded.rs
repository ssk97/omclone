//The loaded solution: its timeline and the editor state around it
use super::*;

pub(super) enum RunState{
    //Crashed holds the timestep it crashed in
    Manual(usize), FreeRun, Crashed(usize)
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
    pub(super) history: UndoHistory<World>,
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
        self.history.record(self.timeline.base().clone());
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
    //Goes back to the starting world (with its tapes) from before the last layout edit
    pub(super) fn undo(&mut self){
        if self.history_lock().is_some() || !self.history.can_undo(){
            return;
        }
        let previous = self.history.undo(self.timeline.base().clone()).unwrap();
        self.restore_base(previous);
    }
    pub(super) fn redo(&mut self){
        if self.history_lock().is_some() || !self.history.can_redo(){
            return;
        }
        let next = self.history.redo(self.timeline.base().clone()).unwrap();
        self.restore_base(next);
    }
    //Parts may be numbered differently afterwards, so nothing stays selected
    fn restore_base(&mut self, world: World){
        self.replace_base(world);
        self.stack_view = None;
        self.select(None);
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
        let base = self.timeline.base_mut();
        let old = base.repeat_length;
        base.repeat_length = length;
        if self.max_timestep < length{
            self.max_timestep = length;
        }
        //Arms start looping at their first instruction plus the loop length
        self.timeline.base().get_first_timestep() as usize + old.min(length)
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
