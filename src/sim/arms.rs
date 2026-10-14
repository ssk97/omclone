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
    pub instruction_tape: Tape,
    pub grabbing: bool,
    pub halted: bool,
    pub atoms_grabbed: [AtomKey; 6],
}

impl Arm {
    pub fn new(pos: Pos, rot: Rot, len: i32, arm_type: ArmType, instruction_tape: Tape) -> Self {
        //omsim reduces the solution file's rotation with a truncating % 6 when loading
        Arm { pos, rot: rot % 6, len, arm_type, instruction_tape,
            grabbing: false, halted: false,
            atoms_grabbed: [AtomKey::null(); 6],
        }
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
            Repeat | Reset => panic!("Unprocessed instruction!"),
            Extend | Retract | Grab | Drop | Halt | Noop | Empty => STILL,
        }
    }
    //Runs only the arm's own moves for the timesteps before `until`. Atoms and other arms never change how an arm
    //moves, so this puts it where a sim of those timesteps would (unless the sim crashes first).
    pub fn run_moves(&mut self, until: u64, repeat_length: usize, track_maps: &TrackMaps) {
        for step in 0..until {
            if self.halted {
                return;
            }
            match self.instruction_tape.get(step as usize, repeat_length) {
                Instr::Halt => self.halted = true,
                instruction => self.do_motion(self.movement(instruction, track_maps)),
            }
        }
    }
}
const STILL: ArmMovement = ArmMovement::Move(Movement::HeldStill);

impl World {
    //Runs the arm's instruction for this timestep. Grabs, drops and halts happen now; the arm's move, and the moves it
    //gives the atoms it holds, are filed in `motion`.
    pub(super) fn do_instruction(&mut self, motion: &mut WorldStepInfo, arm_id: usize, timestep: u64) -> SimResult<()>{
        assert_eq!(motion.arms.len(), arm_id, "Arm motion array not same as arm ID");
        let arm = &mut self.arms[arm_id];
        use Instr::*;
        use Movement::*;
        let arm_type = arm.arm_type;
        let tape = &arm.instruction_tape;
        let timestep = timestep as usize;
        let instruction = tape.get(timestep, self.repeat_length);
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
