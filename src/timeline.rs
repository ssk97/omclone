use crate::sim::*;

//Worlds can get large, so only this many are kept besides the start and the cursor
pub const MAX_CHECKPOINTS: usize = 8;
//Checkpoints are taken at multiples of this timestep
pub const CHECKPOINT_SPACING: u64 = 32;
//The substep count assumed before a step is prepared
const DEFAULT_SUBSTEPS: usize = 8;

//The simulation as the editor sees it: the starting world, a cursor that can move to any (fractional) time,
//and a few checkpoints so that moving back or editing a tape only re-simulates from nearby.
pub struct Timeline {
    //The tapes and loop length (the program) here are the ones every world runs
    base: World,
    curr: World,
    motion: WorldStepInfo,
    substep: usize,
    substep_count: usize,
    time: f64,
    mark_area: bool,
    //Worlds at substep 0, before running their timestep, sorted by timestep. A checkpoint only holds state:
    //it gets the program from base when restored, so it stays valid for program changes at or after its timestep.
    checkpoints: Vec<World>,
    //Known once the cursor has run that far. Every run is the same, so moving back keeps it.
    completion: Option<Completion>,
}
//When the run filled every output
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Completion {
    //Counted like the game does (see World::get_stats)
    pub cycles: i32,
    //Only counted while mark_area is on
    pub area: i32,
    //The timestep after the one that filled the last output
    timestep: u64,
}
//Gives a world base's program
fn sync_program(world: &mut World, base: &World){
    world.repeat_length = base.repeat_length;
    for (arm, base_arm) in world.arms.iter_mut().zip(&base.arms){
        arm.instruction_tape = base_arm.instruction_tape.clone();
    }
}
impl Timeline{
    pub fn new(base: World, mark_area: bool) -> Self{
        Timeline{curr: base.clone(), base, motion: WorldStepInfo::new(), substep: 0, substep_count: DEFAULT_SUBSTEPS,
            time: 0., mark_area, checkpoints: Vec::new(), completion: None}
    }
    pub fn base(&self) -> &World{
        &self.base
    }
    //For changing the program; program_changed must follow
    pub fn base_mut(&mut self) -> &mut World{
        &mut self.base
    }
    pub fn curr(&self) -> &World{
        &self.curr
    }
    pub fn motion(&self) -> &WorldStepInfo{
        &self.motion
    }
    pub fn time(&self) -> f64{
        self.time
    }
    //Nothing has moved yet
    pub fn at_start(&self) -> bool{
        self.curr.timestep == self.base.timestep && self.substep == 0 && self.time <= self.curr.timestep as f64
    }
    pub fn completion(&self) -> Option<Completion>{
        self.completion
    }
    #[cfg(test)]
    pub fn checkpoint_steps(&self) -> Vec<u64>{
        self.checkpoints.iter().map(|c| c.timestep).collect()
    }

    //Moves the cursor back to `step`, from the latest world at or before it
    pub fn rewind(&mut self, step: u64){
        let world = match self.checkpoints.iter().rposition(|c| c.timestep <= step){
            Some(i) => self.checkpoints[i].clone(),
            None => self.base.clone(),
        };
        self.restore(world, step);
    }
    fn restore(&mut self, mut world: World, step: u64){
        sync_program(&mut world, &self.base);
        //The start can be shown before its first timestep
        self.time = world.timestep.min(step) as f64;
        self.curr = world;
        self.motion.clear();
        self.substep = 0;
        self.substep_count = DEFAULT_SUBSTEPS;
    }
    //Replaces the starting world (after a part edit), so nothing simulated can be reused
    pub fn replace_base(&mut self, base: World){
        self.base = base;
        self.checkpoints.clear();
        self.completion = None;
        self.rewind(0);
    }
    //The program (tapes or loop length) in base changed, starting from timestep `earliest`.
    //Returns true if the cursor had to move back.
    pub fn program_changed(&mut self, earliest: u64) -> bool{
        let start = self.base.get_first_timestep();
        if start != self.base.timestep{
            //Every world started at the old first timestep, so none can be reused
            self.base.timestep = start;
            self.checkpoints.clear();
            self.completion = None;
            self.rewind(0);
            return true;
        }
        self.checkpoints.retain(|c| c.timestep <= earliest);
        //It still completes then if the step that filled the last output ran before `earliest`
        if self.completion.is_some_and(|c| c.timestep > earliest){
            self.completion = None;
        }
        //A world at `earliest` hasn't run it yet, unless it has started to (or crashed partway through)
        if earliest <= self.curr.timestep{
            self.rewind(earliest);
            true
        } else {
            sync_program(&mut self.curr, &self.base);
            false
        }
    }
    //Area and collisions are only tracked while this is on, so turning it on starts over.
    //Returns true if the cursor had to move back.
    pub fn set_mark_area(&mut self, on: bool) -> bool{
        let restart = on && !self.mark_area;
        self.mark_area = on;
        if restart{
            self.checkpoints.clear();
            //Collisions may now stop it from completing, and the area wasn't counted
            self.completion = None;
            self.rewind(0);
        }
        restart
    }

    //Runs forward to `target` (if it's ahead). A crash leaves the cursor where it happened.
    pub fn advance_to(&mut self, target: f64) -> SimResult<()>{
        if target <= self.time{
            return Ok(());
        }
        let target_step = target.floor() as u64;
        //After moving back, a checkpoint left up ahead skips re-running the way forward
        if let Some(i) = self.checkpoints.iter().rposition(|c| c.timestep <= target_step){
            if self.checkpoints[i].timestep > self.curr.timestep{
                self.restore(self.checkpoints[i].clone(), target_step);
            }
        }
        while self.curr.timestep < target_step || (self.curr.timestep == target_step
            && self.substep < (target.fract()*self.substep_count as f64) as usize){
            if let Err(err) = self.advance_substep(){
                self.time = self.curr.timestep as f64 + self.substep as f64/self.substep_count as f64;
                return Err(err);
            }
        }
        self.time = target;
        Ok(())
    }
    fn advance_substep(&mut self) -> SimResult<()>{
        if self.substep == 0{
            if let Err(err) = self.curr.prepare_step(&mut self.motion, self.mark_area){
                self.motion.clear();
                return Err(err);
            }
            self.substep_count = self.curr.substep_count(&self.motion);
        }
        self.substep += 1;
        if self.mark_area && self.substep < self.substep_count{
            self.curr.collide_substep(&self.motion, self.substep, self.substep_count)?;
        }
        if self.substep == self.substep_count{
            self.substep = 0;
            self.curr.finalize_step(&mut self.motion, self.mark_area)?;
            if self.completion.is_none() && self.curr.is_complete(){
                let stats = self.curr.get_stats();
                self.completion = Some(Completion{cycles: stats.cycles, area: stats.area, timestep: self.curr.timestep});
            }
            if self.curr.timestep % CHECKPOINT_SPACING == 0{
                self.add_checkpoint();
            }
        }
        Ok(())
    }
    fn add_checkpoint(&mut self){
        let cursor = self.curr.timestep;
        let new = self.checkpoints.partition_point(|c| c.timestep < cursor);
        if self.checkpoints.get(new).is_some_and(|c| c.timestep == cursor){
            return;
        }
        self.checkpoints.insert(new, self.curr.clone());
        if self.checkpoints.len() > MAX_CHECKPOINTS{
            let evict = self.eviction_choice(new);
            self.checkpoints.remove(evict);
        }
    }
    //Which checkpoint to drop, given the one just added at the cursor
    fn eviction_choice(&self, new: usize) -> usize{
        //Ones ahead (left from before moving back) only help to come forward again, so they go first.
        //The nearest goes first, so the point the cursor came back from lasts longest.
        if new+1 < self.checkpoints.len(){
            return new+1;
        }
        //Removing a checkpoint merges the gaps around it. The worst case is then a jump back to the newer end
        //of the merged gap, which re-runs the whole gap. Remove the one where that's cheapest relative to how
        //far back the jump is: this keeps the newest ones dense and spaces the rest out geometrically.
        let cursor = self.curr.timestep;
        let merge_cost = |j: usize| {
            let prev = if j == 0 {self.base.timestep} else {self.checkpoints[j-1].timestep};
            let next = self.checkpoints[j+1].timestep;
            (next - prev, cursor - next)
        };
        //The one just before the new one is never chosen: its merged gap ends at the cursor
        (0..new).min_by(|&a, &b| {
            let ((gap_a, dist_a), (gap_b, dist_b)) = (merge_cost(a), merge_cost(b));
            (gap_a*dist_b).cmp(&(gap_b*dist_a))
        }).unwrap_or(0)
    }
}
