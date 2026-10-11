//Setting up a world from a layout, and the program (tapes and loop length) it runs
use super::*;

impl World {
    fn initial_area(&mut self) {
        for idx in 0..self.glyphs.len() {
            //Unlike glyph_footprint, an input with several variants counts the one it's on now
            let footprint = self.glyphs[idx].footprint(self.polymers.get(&idx));
            self.area_touched.extend(footprint);
        }
        for arm in &self.arms {
            self.area_touched.insert(arm.pos);
        }
        self.mark_arm_area();
    }
}

//What a reset instruction has to undo: the arm's changes since the last repeat or reset
#[derive(Clone)]
struct ResetState {
    piston: bool,
    base_len: i32,
    len: i32,
    rot: i32,
    grabbing: bool,
    track: Option<TrackPosition>,
}
impl ResetState {
    fn new(arm: &Arm, track_maps: &TrackMaps) -> Self {
        ResetState {
            piston: arm.arm_type == ArmType::Piston,
            base_len: arm.len,
            len: arm.len,
            rot: 0,
            grabbing: false,
            track: track_maps.start_position(arm.pos),
        }
    }
    fn apply(&mut self, instr: Instr, track_maps: &TrackMaps) {
        use Instr::*;
        match instr {
            Extend if self.piston && self.len < 3 => self.len += 1,
            Retract if self.piston && self.len > 1 => self.len -= 1,
            RotateCounterClockwise => self.rot += 1,
            RotateClockwise => self.rot -= 1,
            Grab => self.grabbing = true,
            Drop => self.grabbing = false,
            Forward | Back => if let Some(track) = &mut self.track {
                track.step(track_maps, instr == Forward);
            }
            _ => {}
        }
    }
    //The instructions that undo it: drop, retract, rotate back the shorter way, move back along
    //the track, then extend
    fn undo(&self, track_maps: &TrackMaps) -> Vec<Instr> {
        use Instr::*;
        let mut out = Vec::new();
        if self.grabbing {
            out.push(Drop);
        }
        let len_diff = self.len - self.base_len;
        out.extend(std::iter::repeat(Retract).take(len_diff.max(0) as usize));
        let mut rot = self.rot;
        while rot > 3 {rot -= 6;}
        while rot < -3 {rot += 6;}
        let rotate = if rot > 0 {RotateClockwise} else {RotateCounterClockwise};
        out.extend(std::iter::repeat(rotate).take(rot.unsigned_abs() as usize));
        let steps = self.track.map_or(0, |t| t.steps_to_return(track_maps));
        let step = if steps > 0 {Back} else {Forward};
        out.extend(std::iter::repeat(step).take(steps.unsigned_abs() as usize));
        out.extend(std::iter::repeat(Extend).take((-len_diff).max(0) as usize));
        out
    }
}

//Setup stuff
impl World {
    //modifies the original arm to have the new instructions
    fn normalize_instructions(
        original: &mut Arm,
        track_maps: &TrackMaps,
    ) -> Result<()> {
        use Instr::*;
        let old_instructions = &original.instruction_tape.instructions;
        let mut instructions = Vec::with_capacity(old_instructions.len() + 8);
        let mut repeat_source = 0;
        let mut repeat_ending = 0;
        let mut any_nonrepeat = false;
        let mut curr = 0;
        let fresh = ResetState::new(original, track_maps);
        let mut to_undo = fresh.clone();
        while curr < old_instructions.len() {
            let instr = old_instructions[curr];
            if !any_nonrepeat && !matches!(instr, Repeat | Empty ){
                any_nonrepeat = true;
                repeat_source = curr;
            }
            match instr {
                Extend|Retract|
                RotateCounterClockwise|RotateClockwise|
                Grab|Drop|
                Forward|Back|
                PivotCounterClockwise|PivotClockwise => {
                    instructions.push(instr);
                    curr += 1;
                    to_undo.apply(instr, track_maps);
                    repeat_ending = curr;
                }
                Empty | Halt => {
                    instructions.push(instr);
                    curr += 1;
                }

                Repeat => {
                    let rep_len = repeat_ending - repeat_source;
                    if rep_len == 0 {
                        instructions.push(Empty);
                        curr += 1;
                    } else {
                        for i in 0..rep_len {
                            let copied_instr = instructions[repeat_source + i];
                            ensure!( i == 0|| old_instructions.get(curr + i).unwrap_or(&Empty) == &Empty,
                                "Repeat instruction {:?} overlaps with {:?} on {}/{}. Repeat {}-{}",
                                copied_instr,old_instructions.get(curr + i),
                                curr,i,repeat_source,repeat_ending
                            );
                            instructions.push(copied_instr);
                        }
                        curr += rep_len;
                        any_nonrepeat = false;
                        //A repeated block starts afresh, so a later reset only undoes what follows it
                        to_undo = fresh.clone();
                    }
                }
                Reset => {
                    let mut reset_vec = to_undo.undo(track_maps);
                    to_undo = fresh.clone();
                    if reset_vec.len() == 0 {
                        reset_vec.push(Empty)
                    };
                    for i in 0..reset_vec.len() {
                        ensure!(
                            i == 0 || old_instructions.get(curr + i).unwrap_or(&Empty) == &Empty,
                            "Reset instruction {:?} overlaps with {:?} on {}/{} (curr {})",
                            reset_vec[i],
                            old_instructions.get(curr + i),
                            i, reset_vec.len(),
                            curr,
                        );
                        instructions.push(reset_vec[i]);
                    }
                    curr += reset_vec.len();
                    repeat_ending = curr;
                }
                Noop => {
                    //Kept (rather than becoming Empty) so saving writes it back, preserving the loop length
                    instructions.push(Noop);
                    curr += 1;
                    //A noop takes a cycle, so like omsim it extends the repeated block (and the loop)
                    repeat_ending = curr;
                }
            };
        }
        instructions.shrink_to_fit();
        original.instruction_tape = Tape::new(original.instruction_tape.first, instructions);
        Ok(())
    }

    pub fn setup_sim(init: &InitialWorld) -> Result<Self> {
        let mut world = World {
            timestep: 0,
            atoms: WorldAtoms::new(),
            area_touched: Default::default(),
            glyphs: init.glyphs.clone(),
            arms: init.arms.clone(),
            repeat_length: 0,
            track_maps: Default::default(),
            cost: 0,
            conduit_pairs: Default::default(),
            polymers: Default::default(),
            input_variants: Vec::new(),
        };
        for (id, g) in world.glyphs.iter().enumerate() {
            use GlyphType::*;
            world.cost += match &g.glyph_type {
                Calcification | Bonding | Unbonding => 10,
                Animismus | Projection | Dispersion | Purification | Duplication |
                    Unification | TriplexBond | Rejection | Division => 20,
                MultiBond => 30,
                Proliferation => 40,
                Disposal | Equilibrium | Output(_, _,_)| OutputRepeating(_, _,_) | Input(_,_) | Conduit(_,_) => 0,
                Track(v) => (v.len() as i32) * 5,
            };
            if let Some(polymer) = g.build_polymer()? {
                world.polymers.insert(id, polymer);
            }
        }
        world.track_maps = TrackMaps::build(&world.glyphs)?;
        for a in &mut world.arms {
            use ArmType::*;
            world.cost += match a.arm_type {
                PlainArm => 20,
                DoubleArm | TripleArm | HexArm | VanBerlo | Ravari => 30,
                Piston => 40,
            };
            World::normalize_instructions(a, &world.track_maps)?;
            if a.arm_type == VanBerlo || a.arm_type == Ravari {
                use AtomType::*;
                a.grabbing = true;
                let atom_setup: [AtomType; 6] = match a.arm_type{
                    VanBerlo => [Salt, Water, Air, Salt, Fire, Earth],
                    Ravari => [Tin, Lead, Gold, Silver, Copper, Iron],
                    _ => unreachable!()
                };
                for i in 0..6 {
                    let pos = a.pos + rot_to_pos((i as Rot) + a.rot);
                    let atom_type = atom_setup[i];
                    let molecule_key = world.atoms.create_molecule([Atom {
                        pos, atom_type, connections: [Bonds::NO_BOND; 6], immutable_wheel: true,
                        molecule: MoleculeKey::null(),
                    }])?;
                    a.atoms_grabbed[i] = world.atoms.molecules[molecule_key].atoms[0];
                }
            }
        }
        world.repeat_length = world.min_repeat_length();

        world.input_variants = world.glyphs.iter().enumerate().filter_map(|(id, g)| match &g.glyph_type {
            GlyphType::Input(variants, _) if variants.len() > 1 => Some((id, variants.clone())),
            _ => None,
        }).collect();
        world.process_inputs(&mut WorldStepInfo::new())?;
        let mut unfinished_conduit_count = 0;
        for (id, glyph) in world.glyphs.iter_mut().enumerate() {
            if let GlyphType::Conduit(_pos,conduit_id) = &glyph.glyph_type {
                if let Some(pair_data) = world.conduit_pairs.get_mut(conduit_id){
                    ensure!(pair_data.vecids.0 == pair_data.vecids.1,"Conduit {conduit_id} already modified!");
                    pair_data.vecids.1 = id;
                    unfinished_conduit_count -= 1;
                    pair_data.offset_pos -= glyph.pos;
                    pair_data.offset_rot = normalize_dir(pair_data.offset_rot-glyph.rot);
                } else {
                    let conduit_info = ConduitInfo{
                        vecids: (id,id), offset_pos: glyph.pos, offset_rot: glyph.rot};
                    world.conduit_pairs.insert(*conduit_id, conduit_info);
                    unfinished_conduit_count += 1;
                }
            }
        }
        ensure!(unfinished_conduit_count == 0,"Conduits not properly matched!");
        world.initial_area();
        world.timestep = world.get_first_timestep();
        Ok(world)
    }
    //The parts, ready to be set up again (the tapes are already normalized, which setup leaves as they are)
    pub fn layout(&self) -> InitialWorld {
        let mut glyphs = self.glyphs.clone();
        for (id, variants) in &self.input_variants {
            if let GlyphType::Input(current, _) = &mut glyphs[*id].glyph_type {
                *current = variants.clone();
            }
        }
        InitialWorld {
            glyphs,
            arms: self.arms.iter().map(|a| Arm::new(a.pos, a.rot, a.len, a.arm_type, a.instruction_tape.clone())).collect(),
        }
    }
    pub fn get_first_timestep(&self) -> u64{
        if self.arms.len() == 0 { return 0 }
        self.arms.iter().map(|a|a.instruction_tape.first as u64).fold(u64::MAX, std::cmp::min)
    }
    //The shortest loop that fits every looping tape (halting tapes run once, so they don't count)
    pub fn min_repeat_length(&self) -> usize{
        self.arms.iter().filter(|a| !a.instruction_tape.halts())
            .map(|a| a.instruction_tape.instructions.len()).max().unwrap_or(0)
    }
    //Sets the loop length, returning the first timestep it can affect
    pub fn set_repeat_length(&mut self, length: usize) -> usize{
        let old = self.repeat_length;
        self.repeat_length = length;
        //Arms start looping at their first instruction plus the loop length
        self.get_first_timestep() as usize + old.min(length)
    }
    //Puts back the tapes and loop length from `saved`, a version of this world with the same parts.
    //Returns the first timestep that can act differently, if anything changed.
    pub fn restore_program(&mut self, saved: World) -> Option<usize>{
        let loop_len = self.repeat_length;
        debug_assert_eq!(self.arms.len(), saved.arms.len());
        let mut earliest = None;
        for (arm, saved) in self.arms.iter_mut().zip(saved.arms){
            if let Some(step) = arm.instruction_tape.first_difference(&saved.instruction_tape, loop_len){
                earliest = Some(earliest.map_or(step, |e: usize| e.min(step)));
            }
            arm.instruction_tape = saved.instruction_tape;
        }
        if saved.repeat_length != loop_len{
            let step = self.set_repeat_length(saved.repeat_length);
            earliest = Some(earliest.map_or(step, |e| e.min(step)));
        }
        earliest
    }
    //The instructions in the tapes as they are now (with Repeat and Reset already expanded, which the game counts too)
    pub fn instruction_count(&self) -> i32 {
        self.arms.iter().flat_map(|a| &a.instruction_tape.instructions)
            .filter(|i| !matches!(i, Instr::Empty | Instr::Halt | Instr::Noop)).count() as i32
    }
    pub fn get_stats(&self) -> SolutionStats {
        let cycles = (self.timestep-self.get_first_timestep()) as i32;
        let cost = self.cost;
        let area = self.area_touched.len() as i32;
        let instructions = self.instruction_count();
        SolutionStats {cycles,cost,area,instructions}
    }
}
