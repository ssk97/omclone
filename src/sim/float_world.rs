//The world partway through a timestep's motion, for showing it in between
use super::*;

#[derive(Debug, Copy, Clone)]
pub struct FloatAtom{
    pub pos: XYPos,
    pub rot: f32,
    pub atom_type: AtomType,
    pub connections: [Bonds; 6],
}
//For pattern atoms (inputs, outputs and polymers)
impl From<&Atom> for FloatAtom{
	fn from(a: &Atom) -> FloatAtom {
		return FloatAtom{
			pos: pos_to_xy(a.pos),
			rot: 0.,
			atom_type: a.pattern_type(),
			connections: a.connections,
		}
	}
}
pub struct FloatArm{
    pub pos: XYPos,
    pub rot: f32,
    pub len: f32,//in hexes
    pub arm_type: ArmType,
    pub grip: f32,//how far the grabbers have closed: 0 open, 1 holding
}
//The grip of an arm that isn't grabbing or dropping
pub fn steady_grip(grabbing: bool) -> f32 {
    if grabbing {1.} else {0.}
}
pub struct FloatWorld{//Might add arms, maybe some other animation stuff too
    pub portion: f32,
    pub atoms_xy: Vec<FloatAtom>,
    pub arms_xy: Vec<FloatArm>
}
impl FloatWorld{
    pub fn new() -> Self{
        FloatWorld{
            portion: 0.,
            atoms_xy: Vec::new(),
            arms_xy: Vec::new(),
        }
    }
    pub fn regenerate(&mut self, world: &World, motion: &WorldStepInfo, portion: f32) {
        self.arms_xy.clear();
        self.atoms_xy.clear();
        self.portion = portion;
        use Movement::*;
        use ArmMovement::*;
        fn apply_movement(base: Pos, movement: Movement, amount: f32) -> (XYPos, f32){
            let xy = pos_to_xy(base);
            match movement{
                Linear(offset) => {
                    return (xy+(pos_to_xy_vec(offset)*amount), 0.)
                },
                Rotation(r, pivot) => {
                    let pivot_xy = pos_to_xy(pivot);
                    let offset = xy-pivot_xy;
                    let rot = rot_to_angle(r)*amount;
                    return (pivot_xy+(nalgebra::Rotation2::new(rot)*offset), rot)
                }
                HeldStill => {
                    return (xy, 0.)
                }
            }
        }
        for (atom_key, atom) in &world.atoms.atom_map{
            let movement = motion.atoms.get(atom_key).unwrap_or(&HeldStill);
            let (xy, angle) = apply_movement(atom.pos, *movement, portion);
            let f_atom = FloatAtom{
                pos: xy,
                rot: angle,
                atom_type: world.atoms.type_of(atom),
                connections: atom.connections,
            };
            self.atoms_xy.push(f_atom);
        }
        assert_eq!(motion.arms.len(), world.arms.len());
        for a_index in 0..world.arms.len(){
            let arm = &world.arms[a_index];
            let mut arm_len = arm.len as f32;
            let new_motion = match motion.arms[a_index]{
                Move(m) => m,
                LengthAdjust(l) => {
                    arm_len += (l as f32)*portion;
                    HeldStill
                },
                Pivot(_) => HeldStill,
            };
            let (xy, angle) = apply_movement(arm.pos, new_motion, portion);
            //The instruction has already set grabbing to how the step ends
            let grip = match (motion.grip_toggled[a_index], arm.grabbing){
                (true, true) => portion,
                (true, false) => 1.-portion,
                (false, grabbing) => steady_grip(grabbing),
            };
            let new_arm = FloatArm{
                pos: xy,
                rot: angle+rot_to_angle(arm.rot),
                len: arm_len,
                arm_type: arm.arm_type,
                grip,
            };
            self.arms_xy.push(new_arm);
        }
    }
    pub fn generate_static(&mut self, world: &World) {
        self.arms_xy.clear();
        self.atoms_xy.clear();
        for (_atom_key, atom) in &world.atoms.atom_map{
            let xy = pos_to_xy(atom.pos);
            let f_atom = FloatAtom{
                pos: xy,
                rot: 0.,
                atom_type: world.atoms.type_of(atom),
                connections: atom.connections,
            };
            self.atoms_xy.push(f_atom);
        }
        for a_index in 0..world.arms.len(){
            let arm = &world.arms[a_index];
            let new_arm = FloatArm{
                pos: pos_to_xy(arm.pos),
                rot: rot_to_angle(arm.rot),
                len: arm.len as f32,
                arm_type: arm.arm_type,
                grip: steady_grip(arm.grabbing),
            };
            self.arms_xy.push(new_arm);
        }
    }
}
