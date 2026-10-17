//Arms, and the moves their instructions make
use super::*;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ArmType {
    PlainArm, DoubleArm, TripleArm, HexArm,
    Piston, VanBerlo, Ravari
}
//How many hexes an arm (other than a wheel) can reach
pub const ARM_LENGTHS: std::ops::RangeInclusive<i32> = 1..=3;
impl ArmType {
    //Wheels hold their atoms permanently and ignore grab/drop
    pub fn is_wheel(&self) -> bool {
        matches!(self, ArmType::VanBerlo | ArmType::Ravari)
    }
    //The nearest length this type of arm can have (wheels only have length 1)
    pub fn clamp_len(&self, len: i32) -> i32 {
        if self.is_wheel() {1} else {len.clamp(*ARM_LENGTHS.start(), *ARM_LENGTHS.end())}
    }
}
#[derive(Debug, Clone)]
pub struct Arm {
    pub pos: Pos,
    //Rotation as tracked by omsim (truncated % 6 after each rotation, so it ranges -5..=5),
    //which collision positions depend on
    pub rot: Rot,
    pub len: i32,
    pub arm_type: ArmType,
    //The tape as written, with its Repeats and Resets: what's edited and saved
    pub(super) tape: Tape,
    //What it runs, decoded from `tape` when the world is set up. Both are private to sim, whose World methods change
    //them together.
    pub(super) decoded: DecodedTape,
    pub grabbing: bool,
    pub halted: bool,
    pub atoms_grabbed: [AtomKey; 6],
}

impl Arm {
    pub fn new(pos: Pos, rot: Rot, len: i32, arm_type: ArmType, tape: Tape) -> Self {
        //omsim reduces the solution file's rotation with a truncating % 6 when loading
        Arm { pos, rot: rot % 6, len, arm_type, tape, decoded: DecodedTape::default(),
            grabbing: false, halted: false,
            atoms_grabbed: [AtomKey::null(); 6],
        }
    }
    pub fn tape(&self) -> &Tape {
        &self.tape
    }
    pub fn decoded(&self) -> &DecodedTape {
        &self.decoded
    }
    pub fn angles_between_arm(arm_type: ArmType) -> Rot {
        use ArmType::*;
        match arm_type {
            PlainArm | Piston => 6,
            DoubleArm => 3,
            TripleArm => 2,
            HexArm | VanBerlo | Ravari => 1,
        }
    }
    pub(super) fn do_motion(&mut self, action: ArmMovement) {
        use ArmMovement::*;
        use Movement::*;
        match action{
            Move(Linear(p)) => self.pos += p,
            Move(Rotation(r,_)) => self.rot = (self.rot + r) % 6,
            LengthAdjust(a) => self.len += a,
            Move(HeldStill) | Pivot(_) => {},
        }
    }
    //The move an instruction makes the arm itself do. Grabbing, dropping and halting don't move it
    //(World::do_instruction does the rest of what they do).
    fn movement(&self, instruction: Instr, track_maps: &TrackMaps) -> ArmMovement {
        use Instr::*;
        use Movement::*;
        use ArmMovement::*;
        let piston = self.arm_type == ArmType::Piston;
        match instruction {
            Extend if piston && self.len < 3 => LengthAdjust(1),
            Retract if piston && self.len > 1 => LengthAdjust(-1),
            RotateCounterClockwise => Move(Rotation(1, self.pos)),
            RotateClockwise => Move(Rotation(-1, self.pos)),
            PivotCounterClockwise => Pivot(1),
            PivotClockwise => Pivot(-1),
            Forward => track_maps.plus.get(&self.pos).map_or(STILL, |&x| Move(Linear(x))),
            Back => track_maps.minus.get(&self.pos).map_or(STILL, |&x| Move(Linear(x))),
            //Decoding expands Repeats and Resets, and a Clash crashes before the arm moves
            Repeat | Reset | Clash => panic!("Unprocessed instruction!"),
            Extend | Retract | Grab | Drop | Halt | Noop | Empty => STILL,
        }
    }
}
const STILL: ArmMovement = ArmMovement::Move(Movement::HeldStill);

impl World {
    //Arm `id` after the timesteps before `until`, found by running only its own moves from where it starts (so this
    //must be a world that hasn't run). Atoms and other arms never change how an arm moves, so this puts it where a sim
    //of those timesteps would (unless the sim crashes first).
    pub fn arm_after(&self, id: usize, until: u64) -> Arm {
        let mut arm = self.arms[id].clone();
        for step in 0..until {
            if arm.halted {
                break;
            }
            match arm.decoded.tape.get(step as usize, self.repeat_length) {
                Instr::Halt | Instr::Clash => arm.halted = true,
                instruction => arm.do_motion(arm.movement(instruction, &self.track_maps)),
            }
        }
        arm
    }
    //Runs the arm's instruction for this timestep. Grabs, drops and halts happen now; the arm's move, and the moves it
    //gives the atoms it holds, are filed in `motion`.
    pub(super) fn do_instruction(&mut self, motion: &mut WorldStepInfo, arm_id: usize, timestep: u64) -> SimResult<()>{
        assert_eq!(motion.arms.len(), arm_id, "Arm motion array not same as arm ID");
        let arm = &mut self.arms[arm_id];
        use Instr::*;
        use Movement::*;
        let arm_type = arm.arm_type;
        let timestep = timestep as usize;
        let instruction = arm.decoded.tape.get(timestep, self.repeat_length);
        //Even once halted: the tape itself is invalid
        if instruction == Clash {
            let error_str = if arm.expansion_at(timestep) == Some(Repeat) {"Instruction inside a Repeat's expansion!"}
                else {"Instruction inside a Reset's expansion!"};
            return Err(sim_error_pos(error_str, arm.pos));
        }
        let was_grabbing = arm.grabbing;
        let action = if arm.halted {STILL} else {match instruction {
            Drop => {
                if !arm_type.is_wheel() {
                    for r in 0..6{
                        let grabbed_atom = arm.atoms_grabbed[r];
                        if !grabbed_atom.is_null(){
                            motion.drop_conduit_check.insert(grabbed_atom);
                        }
                    }
                    arm.atoms_grabbed = [AtomKey::null(); 6];
                    arm.grabbing = false;
                }
                STILL
            }
            Grab => {
                if !arm.grabbing && !arm_type.is_wheel() {
                    arm.grabbing = true;
                    for r in (0..6).step_by(Arm::angles_between_arm(arm.arm_type) as usize) {
                        let grab_pos = arm.pos + (rot_to_pos(arm.rot + r) * arm.len);
                        //The arm would take the visible atom, but in the game nothing can then take the atoms
                        //above it, so the stack can't clear before motion
                        if self.atoms.is_stacked(grab_pos) {
                            return Err(sim_error_pos("Overlapping atoms before motion!", grab_pos));
                        }
                        let current = self.atoms.visible(grab_pos)
                            .filter(|&key| !self.atoms.atom_map[key].immutable_wheel);
                        arm.atoms_grabbed[r as usize] = current.unwrap_or(AtomKey::null());
                        if let Some(key) = current {
                            let atom_type = self.atoms.type_of(&self.atoms.atom_map[key]) as usize;
                            if let Some(count) = atom_type.checked_sub(1).and_then(|i| self.atom_grabs.get_mut(i)) {
                                *count = count.saturating_add(1);
                            }
                        }
                    }
                }
                STILL
            }
            Halt => {arm.halted = true; STILL}
            _ => arm.movement(instruction, &self.track_maps),
        }};
        let rotation_store = arm.rot;
        for (grabber, &atom) in arm.atoms_grabbed.iter().enumerate() {
            if !atom.is_null() {
                let atom_movement = match action{
                    ArmMovement::Pivot(r) => Movement::Rotation(r, self.atoms.atom_map[atom].pos),
                    ArmMovement::Move(m) => m,
                    ArmMovement::LengthAdjust(a) => Linear(rot_to_pos(rotation_store)*a)
                };
                motion.grabs.push(GrabMovement { arm: arm_id, grabber, atom, movement: atom_movement });
                let tmp = motion.atoms.insert(atom, atom_movement);
                match tmp{
                    Some(x) if x != atom_movement => {
                        let error_str = &"Atom moved in multiple directions!";
                        return Err(sim_error_pos(error_str,self.atoms.atom_map[atom].pos));
                    }
                    _ => {}
                }
            }
        }
        motion.arms.push(action);
        motion.grip_toggled.push(arm.grabbing != was_grabbing);
        Ok(())
    }
}
