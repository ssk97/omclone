#![allow(unused_parens)]
use bitflags::bitflags;
#[cfg(feature = "color-eyre")]
use color_eyre::{
    eyre::{bail, ensure},
    Result,
};
#[cfg(not(feature = "color-eyre"))]
use simple_eyre::{
    eyre::{bail, ensure},
    Result,
};
pub use nalgebra::{Vector2, Point2};
use slotmap::{new_key_type, Key, SecondaryMap, SlotMap};
use std::collections::VecDeque;
use smallvec::SmallVec;
use rustc_hash::{FxHashMap, FxHashSet};
use std::{f32::consts::PI,fmt,error};
use enum_primitive_derive::Primitive;

#[derive(Debug)]
pub struct SimError{
    pub error_str: &'static str,
    pub location: XYPos,
}
impl fmt::Display for SimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hex = xy_to_pos(self.location);
        let hex_pos = pos_to_xy(hex);
        if nalgebra::distance_squared(&hex_pos,&self.location) < 0.1{
            write!(f, "{} at {:?}", self.error_str,hex)
        } else {
            write!(f, "{} near {:?} (at {:?})", self.error_str,hex,self.location)
        }
    }
}
impl error::Error for SimError{}
pub type SimResult<T> = std::result::Result<T,SimError>;
fn sim_error_pos(error_str: &'static str, location: Pos) -> SimError{
    SimError{error_str, location: pos_to_xy(location)}
}
pub type Rot = i32;
pub type Pos = Vector2<i32>;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Instr {
    RotateClockwise, RotateCounterClockwise,
    Extend, Retract,
    Grab, Drop,
    PivotClockwise, PivotCounterClockwise,
    Forward, Back,
    Repeat, Reset,
    Noop, Halt, Empty,
}
impl Instr {
    pub fn to_byte(&self) -> u8 {
        use Instr::*;
        match self {
            RotateClockwise         => b'R',
            RotateCounterClockwise  => b'r',
            Extend                  => b'E',
            Retract                 => b'e',
            Grab                    => b'G',
            Drop                    => b'g',
            PivotClockwise          => b'P',
            PivotCounterClockwise   => b'p',
            Forward                 => b'A',
            Back                    => b'a',
            Repeat                  => b'C',
            Reset                   => b'X',
            Noop                    => b'O',
            Halt                   => b'B',
            Empty                   => b' ',
        }
    }
    pub fn from_byte(input: u8) -> Option<Self> {
        use Instr::*;
        match input {
            b'R' => Some(RotateClockwise),
            b'r' => Some(RotateCounterClockwise),
            b'E' => Some(Extend),
            b'e' => Some(Retract),
            b'G' => Some(Grab),
            b'g' => Some(Drop),
            b'P' => Some(PivotClockwise),
            b'p' => Some(PivotCounterClockwise),
            b'A' => Some(Forward),
            b'a' => Some(Back),
            b'C' => Some(Repeat),
            b'X' => Some(Reset),
            b'O' => Some(Noop),
            b'B' => Some(Halt),
            b' ' => Some(Empty),
            _ => None,
        }
    }
    pub fn to_char(&self) -> char {
        use Instr::*;
        match self {
            RotateClockwise         => 'd',
            RotateCounterClockwise  => 'a',
            Extend                  => 'w',
            Retract                 => 's',
            Grab                    => 'f',
            Drop                    => 'r',
            PivotClockwise          => 'e',
            PivotCounterClockwise   => 'q',
            Forward                 => 'g',
            Back                    => 't',
            Repeat                  => 'C',
            Reset                   => 'X',
            Noop                    => 'O',
            Halt                    => 'B',
            Empty                   => ' ',
        }
    }
    pub fn from_char(input: char) -> Option<Self> {
        use Instr::*;
        match input {
             'd' =>  Some(RotateClockwise       ),
             'a' =>  Some(RotateCounterClockwise),
             'w' =>  Some(Extend                ),
             's' =>  Some(Retract               ),
             'f' =>  Some(Grab                  ),
             'r' =>  Some(Drop                  ),
             'e' =>  Some(PivotClockwise        ),
             'q' =>  Some(PivotCounterClockwise ),
             'g' =>  Some(Forward               ),
             't' =>  Some(Back                  ),
             //'C' =>  Some(Repeat                ),
             //'X' =>  Some(Reset                 ),
             //'O' =>  Some(Noop                  ),
             'B' =>  Some(Halt                  ),
             ' ' =>  Some(Empty                 ),
             _ => None
        }
    }
}

#[derive(Debug, Clone)]
pub struct Tape {
    pub first: usize,
    pub instructions: Vec<Instr>,
}
impl Tape {
    pub fn get(&self, timestep: usize, loop_len: usize) -> Instr {
        use Instr::Empty;
        if timestep >= self.first{
            let after_first = timestep - self.first;
            if self.instructions.contains(&Instr::Halt){
                return *self
                    .instructions
                    .get(after_first)
                    .unwrap_or(&Empty)
            } else if loop_len > 0{
                return *self
                    .instructions
                    .get(after_first % loop_len)
                    .unwrap_or(&Empty)
            }
        }
        return Empty
    }
    pub fn to_string(&self) -> String{
        let mut output = " ".repeat(self.first);
        for i in &self.instructions{
            output.push(i.to_char());
        }
        output
    }
    pub fn noop_clear_and_string(&mut self) -> String{
        while self.instructions.get(0).unwrap_or(&Instr::Noop) == &Instr::Empty
        {
            self.first += 1;
            self.instructions.remove(0);
        }
        self.to_string()
    }
}

pub fn normalize_dir(r: Rot) -> Rot {
    return r.rem_euclid(6);
}

pub fn pos_to_rot(input: Pos) -> Option<Rot> {
    match (input.x, input.y) {
        ( 1, 0) => Some(0),
        ( 0, 1) => Some(1),
        (-1, 1) => Some(2),
        (-1, 0) => Some(3),
        ( 0,-1) => Some(4),
        ( 1,-1) => Some(5),
        _ => None,
    }
}
pub fn rot_dist_to_pos(n: i32, angle: Rot) -> Pos {
    match normalize_dir(angle) {
        0 => Pos::new( n, 0),
        1 => Pos::new( 0, n),
        2 => Pos::new(-n, n),
        3 => Pos::new(-n, 0),
        4 => Pos::new( 0,-n),
        5 => Pos::new( n,-n),
        _ => panic!("Invalid Rotation"),
    }
}
pub fn rot_to_pos(angle: Rot) -> Pos {
    rot_dist_to_pos(1, angle)
}
pub fn rotate(pos: Pos, angle: Rot) -> Pos {
    match normalize_dir(angle) {
        0 => pos, //  ( pos.x      ,       pos.y)
        1 => Pos::new(      -pos.y , pos.x+pos.y),
        2 => Pos::new(-pos.x-pos.y , pos.x      ),
        3 => Pos::new(-pos.x       ,      -pos.y),
        4 => Pos::new(       pos.y ,-pos.x-pos.y),
        5 => Pos::new( pos.x+pos.y ,-pos.x      ),
        _ => panic!("Invalid Rotation"),
    }
}

pub fn rotate_around(pos: Pos, angle: Rot, pivot: Pos) -> Pos {
    rotate(pos - pivot, angle) + pivot
}

bitflags! {
    pub struct Bonds: u8 {
        const NO_BOND     =0b0000_0000;
        const NORMAL      =0b0000_0001;
        const TRIPLEX_R   =0b0000_0010;
        const TRIPLEX_K   =0b0000_0100;
        const TRIPLEX_Y   =0b0000_1000;
        const TRIPLEX     =0b0000_1110;
        const DYNAMIC_BOND=0b0000_1111;
        const CONDUIT_BOND=0b0001_0000;
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Primitive)]
#[repr(u8)]
pub enum AtomType {
    ConduitSpace = 0,
    Salt=1, Air=2, Earth=3, Fire=4, Water=5,
    Quicksilver=6, Gold=7, Silver=8, Copper=9, Iron=10, Tin=11, Lead=12,
    Vitae=13, Mors=14, RepeatingOutputMarker=15, Quintessence=16,
}
impl AtomType {
    pub fn is_element(&self) -> bool {
        use AtomType::*;
        match &self {
            Air | Earth | Fire | Water => true,
            _ => false,
        }
    }
    pub fn metal_id(&self) -> Option<u8>{
        use AtomType::*;
        match &self {
            Lead => Some(1),
            Tin => Some(2),
            Iron => Some(3),
            Copper => Some(4),
            Silver => Some(5),
            Gold => Some(6),
            _ => None,
        }
    }
    pub fn id_to_metal(metal: u8) -> Option<AtomType> {
        use AtomType::*;
        match metal {
            1 => Some(Lead),
            2 => Some(Tin),
            3 => Some(Iron),
            4 => Some(Copper),
            5 => Some(Silver),
            6 => Some(Gold),
            _ => None,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Atom {
    pub pos: Pos,
    pub atom_type: AtomType,
    pub connections: [Bonds; 6],
    pub immutable_wheel: bool,
}
impl Atom {
    pub fn new(pos: Pos, atom_type: AtomType) -> Atom {
        Atom {
            pos,
            atom_type,
            connections: [Bonds::NO_BOND; 6],
            immutable_wheel: false,
        }
    }
    pub fn rotate_connections(&mut self, r: Rot) {
        let mut new_connections = [Bonds::NO_BOND; 6];
        for i in 0..6 {
            new_connections[normalize_dir(i + r) as usize] = self.connections[i as usize];
        }
        self.connections = new_connections;
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Movement {
    HeldStill,
    Linear(Pos),
    Rotation(Rot, Pos),
}
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ArmMovement{
    Move(Movement),
    Pivot(Rot),
    LengthAdjust(i32),
}

pub type AtomPattern = SmallVec<[Vec<Atom>;1]>;
pub type InOutId = i32;
pub type ConduitId = i32;
//Note: pre-setup, AtomPatterns are local and must be offset/rotated. After, they are global.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GlyphType {
    Calcification, Animismus, Projection,
    Dispersion, Purification, Duplication, Unification,
    Bonding, Unbonding, TriplexBond, MultiBond,
    Disposal, Equilibrium,
    Rejection, Division, Proliferation,
    Track(Vec<Pos>), Conduit(Vec<Pos>, ConduitId),
    Input(AtomPattern, InOutId), Output(AtomPattern, i32, InOutId),
    OutputRepeating(AtomPattern, i32, InOutId),
}//Warning: Currently these Pos are relative in InitialWorld/preprocess, and absolute for World/during processing
//This should probably be made consistent
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ArmType {
    PlainArm, DoubleArm, TripleArm, HexArm,
    Piston, VanBerlo, Ravari
}
#[derive(Debug, Clone)]
pub struct Glyph {
    pub pos: Pos,
    pub rot: Rot,
    pub glyph_type: GlyphType,
}
impl Glyph {
    pub fn reposition(&self, rel_pos: Pos) -> Pos {
        self.pos + rotate(rel_pos, self.rot)
    }
    //returns true if it was a glyph that can be repositioned
    pub fn reposition_pattern(&mut self) -> bool {
        use GlyphType::*;
        match &mut self.glyph_type {
            Input(meta_pattern,_) 
            | Output(meta_pattern, _,_) 
            | OutputRepeating(meta_pattern, _,_)  => {
                for pat in meta_pattern{
                    for a in pat {
                        a.pos = self.pos + rotate(a.pos, self.rot);
                        a.rotate_connections(self.rot);
                    }
                }
                true
            }
            Conduit(locs,_id) => {
                let p = self.pos;
                for a in locs.iter_mut() {
                    *a = p + rotate(*a, self.rot);
                }
                true
            }
            Track(locs) => {
                for a in locs {
                    *a = self.pos + rotate(*a, self.rot);
                }
                true
            }
            _ => false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Arm {
    pub pos: Pos,
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
        Arm { pos, rot, len, arm_type, instruction_tape,
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
    fn do_motion(&mut self, action: ArmMovement) {
        use ArmMovement::*;
        use Movement::*;
        match action{
            Move(Linear(p)) => self.pos += p,
            Move(Rotation(r,_)) => self.rot += r,
            LengthAdjust(a) => self.len += a,
            Move(HeldStill) | Pivot(_) => {},
        }
    }
}

#[derive(Debug)]
pub struct InitialWorld {
    pub glyphs: Vec<Glyph>,
    pub arms: Vec<Arm>,
}

pub type TrackMap = FxHashMap<Pos, Pos>;

#[derive(Debug, Clone, Default)]
pub struct TrackMaps{
    pub plus: TrackMap,
    pub minus: TrackMap,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConduitInfo {
    pub vecids: (usize, usize),
    pub offset_pos: Pos,
    pub offset_rot: Rot, 
}
#[derive(Debug, Clone)]
pub struct World {
    pub timestep: u64,
    pub atoms: WorldAtoms,
    pub area_touched: FxHashSet<Pos>,
    pub glyphs: Vec<Glyph>,
    pub arms: Vec<Arm>,
    pub repeat_length: usize,
    pub track_maps: TrackMaps,
    pub cost: i32,
    pub instruction_count: i32,
    pub conduit_pairs: ContuitPairMap,
}
type ContuitPairMap = FxHashMap<i32, ConduitInfo>;
new_key_type! { pub struct AtomKey; }
#[derive(Debug, Clone)]
pub struct WorldAtoms {
    pub atom_map: SlotMap<AtomKey, Atom>,
    pub locs: FxHashMap<Pos, AtomKey>,
}
impl WorldAtoms {
    fn new() -> WorldAtoms {
        WorldAtoms {
            atom_map: SlotMap::with_key(),
            locs: Default::default(),
        }
    }
    fn get_atom_mut(&mut self, loc: Pos) -> Option<&mut Atom> {
        let &key = self.locs.get(&loc)?;
        Some(&mut self.atom_map[key])
    }
    fn get_type(&self, loc: Pos) -> Option<AtomType> {
        let &key = self.locs.get(&loc)?;
        let atom = self.atom_map.get(key).expect("Inconsistent atoms (type check)");
        Some(atom.atom_type)
    }
    fn get_nonberlo_type(&self, loc: Pos) -> Option<AtomType> {
        let &key = self.locs.get(&loc)?;
        let atom = self.atom_map.get(key).expect("Inconsistent atoms (nonberlo type check)");
        if atom.immutable_wheel {return None};
        Some(self.atom_map.get(key)?.atom_type)
    }
    fn get_consumable_type(&self, motion: &WorldStepInfo, loc: Pos) -> Option<AtomType> {
        let &key = self.locs.get(&loc)?;
        if motion.atoms.contains_key(key) || motion.recent_bonds.contains(&key) {
            return None;
        }
        let atom = self.atom_map.get(key).expect("Inconsistent atoms (consume check)");
        if atom.connections != [Bonds::NO_BOND;6]{
            return None
        }
        //Don't need to check is_berlo since the berlo arm will always be grabbing it (applying a motion)
        Some(atom.atom_type)
    }
    //Same as above except doesn't have the unbond check
    fn get_consumable_type_immediate(&self, motion: &WorldStepInfo, loc: Pos) -> Option<AtomType> {
        let &key = self.locs.get(&loc)?;
        if motion.atoms.contains_key(key) {
            return None;
        }
        let atom = self.atom_map.get(key).expect("Inconsistent atoms (consume check)");
        if atom.connections != [Bonds::NO_BOND;6]{
            return None
        }
        //Don't need to check is_berlo since the berlo arm will always be grabbing it (applying a motion)
        Some(atom.atom_type)
    }
    fn check_empty(&self, motion: &WorldStepInfo, loc: Pos) -> bool {
        if self.locs.contains_key(&loc) {
            return false;
        }
        return !motion.spawning_atoms.iter().any(|x| x.pos == loc);
    }

    //Note: only use on non-grabbed atoms!
    fn destroy_atom_at(&mut self, loc: Pos) {
        let atom_key = self.locs.remove(&loc).expect("Destroying nonatom!");
        let atom = self.atom_map.remove(atom_key)
            .expect("Inconsistent atoms (destroy)!");
        if atom.pos != loc {
            panic!("Inconsistent atoms (destroy loc)!");
        }
    }
    fn take_atom(&mut self, key: AtomKey) -> Atom {
        let atom = self.atom_map.remove(key)
            .expect("Inconsistent atoms (take)!");
        let loc = atom.pos;
        let atom_key = self.locs.remove(&loc).expect("Destroying nonatom!");
        if atom_key != key {
            panic!("Inconsistent atoms (take key)!");
        }
        return atom;
    }
    fn create_atom(&mut self, atom: Atom) -> SimResult<AtomKey> {
        let key = self.atom_map.insert(atom);
        let atom_ref = &self.atom_map[key];
        let output = self.locs.insert(atom_ref.pos, key);
        if let Some(_old_atom) = output{
            return Err(SimError{error_str:"Atom creation on hex with existing atom!",location:pos_to_xy(atom_ref.pos)});
        }
        Ok(key)
    }
}

pub struct WorldStepInfo{
    pub atoms: SecondaryMap<AtomKey, Movement>,
    pub arms: Vec<ArmMovement>,
    pub spawning_atoms: VecDeque<Atom>,
    pub recent_bonds: FxHashSet<AtomKey>,
    pub drop_conduit_check: FxHashSet<AtomKey>, //contains one entry per arm
    pub active_glyphs: VecDeque<usize>, //When glyph is a conduit, repeat for number of atoms sent
}
impl WorldStepInfo{
    pub fn new() -> Self{
        WorldStepInfo{
            atoms: SecondaryMap::new(),
            arms: Vec::new(),
            spawning_atoms:VecDeque::new(),
            recent_bonds:FxHashSet::default(),
            drop_conduit_check:FxHashSet::default(),
            active_glyphs:VecDeque::new(),
        }
    }
    pub fn clear(&mut self) {
        self.atoms.clear();
        self.arms.clear();
        self.spawning_atoms.clear();
        self.recent_bonds.clear();
        self.drop_conduit_check.clear();
        self.active_glyphs.clear();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SolutionStats {
    pub cycles: i32,
    pub cost: i32,
    pub area: i32,
    pub instructions: i32,
}

pub type XYPos = Point2<f32>;
pub type XYVec = Vector2<f32>;
#[derive(Debug, Copy, Clone)]
pub struct FloatAtom{
    pub pos: XYPos,
    pub rot: f32,
    pub atom_type: AtomType,
    pub connections: [Bonds; 6],
}
impl From<&Atom> for FloatAtom{
	fn from(a: &Atom) -> FloatAtom {
		return FloatAtom{
			pos: pos_to_xy(a.pos),
			rot: 0.,
			atom_type: a.atom_type,
			connections: a.connections,
		}
	}
}
pub struct FloatArm{
    pub pos: XYPos,
    pub rot: f32,
    pub len: f32,
    pub arm_type: ArmType,
    pub grabbing: bool,
}
pub struct FloatWorld{//Might add arms, maybe some other animation stuff too
    pub portion: f32,
    pub atoms_xy: Vec<FloatAtom>,
    pub arms_xy: Vec<FloatArm>
}
fn pos_to_xy(input: Pos) -> XYPos{
    let a = input.x as f32;
    let b = input.y as f32;
    XYPos::new(a*2.+b,b*f32::sqrt(3.))
}
fn rot_to_angle(r: Rot) -> f32{
    (-r as f32)*PI/3.
}
fn xy_to_simple_pos(input: XYPos) -> Pos{
    let b = input.y/f32::sqrt(3.);
    let a = (input.x-b)/2.;
    Pos::new(a.round() as i32, b.round() as i32)
}
//https://stackoverflow.com/questions/7705228/hexagonal-grids-how-do-you-find-which-hexagon-a-point-is-in/23370350#23370350
fn xy_to_pos(input: XYPos) -> Pos{
    //x=a*2.+b, y=b*f32::sqrt(3.)
    let b = input.y/f32::sqrt(3.);
    let a = (input.x-b)/2.;
    // check closest to (p+1,q), (p,q+1), (p-1,q) or (p,q-1)
    let (p, q) = (a.round(), b.round());
    let (lambda, mu) = (a-p, b-q);
    // opposite signs, so we are guaranteed to be inside hexagon (p,q)
    let base_hex = Pos::new(p as i32, q as i32);
    if lambda * mu < 0.0 {return base_hex;}
    let distance = nalgebra::distance_squared(&pos_to_xy(base_hex),&input);
    // inside circle, so guaranteed inside hexagon (p,q)
    if distance < 1. {return base_hex;}

    // same sign, but which end of the parallelogram are we?
    let sign = lambda.signum();
    let candidate = if ( lambda.abs() > mu.abs() ) {(p+sign,q)} else {(p,q+sign)};
    let candidate_hex = Pos::new(candidate.0 as i32, candidate.1 as i32);
    let distance2 = nalgebra::distance_squared(&pos_to_xy(candidate_hex),&input);
    if distance < distance2 {base_hex} else {candidate_hex}
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
                    let offset_xy = pos_to_xy(offset)-XYPos::origin();
                    return (xy+(offset_xy*amount), 0.)
                },
                Rotation(r, pivot) => {
                    let pivot_xy = pos_to_xy(pivot);
                    let offset = xy-pivot_xy;
                    let rot = rot_to_angle(r)*amount;
                    return (pivot_xy+(nalgebra::Rotation2::new(-rot)*offset), rot)
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
                atom_type: atom.atom_type,
                connections: atom.connections,
            };
            self.atoms_xy.push(f_atom);
        }
        assert_eq!(motion.arms.len(), world.arms.len());
        for a_index in 0..world.arms.len(){
            let arm = &world.arms[a_index];
            let mut arm_len = arm.len as f32;
            let motion = motion.arms[a_index];
            let new_motion = match motion{
                Move(m) => m,
                LengthAdjust(l) => {
                    arm_len += (l as f32)*portion;
                    HeldStill
                },
                Pivot(_) => HeldStill,
            };
            let (xy, angle) = apply_movement(arm.pos, new_motion, portion);
            let new_arm = FloatArm{
                pos: xy,
                rot: angle+rot_to_angle(arm.rot),
                len: arm_len*2.,
                arm_type: arm.arm_type,
                grabbing: arm.grabbing,
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
                atom_type: atom.atom_type,
                connections: atom.connections,
            };
            self.atoms_xy.push(f_atom);
        }
        for a_index in 0..world.arms.len(){
            let arm = &world.arms[a_index];
            let arm_len = arm.len as f32;
            let new_arm = FloatArm{
                pos: pos_to_xy(arm.pos),
                rot: rot_to_angle(arm.rot),
                len: arm_len*2.,
                arm_type: arm.arm_type,
                grabbing: arm.grabbing,
            };
            self.arms_xy.push(new_arm);
        }
    }
}

//Running the world
impl World {
    //sets up a move for the specified atom and all atoms connected to it
    fn premove_atoms(&self, motion: &mut WorldStepInfo, atom_key: AtomKey, movement: Movement) -> SimResult<()> {
        let mut moving_atoms = VecDeque::<AtomKey>::new();
        moving_atoms.push_back(atom_key);

        while let Some(this_key) = moving_atoms.pop_front() {
            let maybe_move = motion.atoms.get(this_key);
            if let Some(curr_move) = maybe_move {
                if curr_move != &movement {
                    println!("was {:?},applying {:?}",curr_move,movement);
                    let error_str = &"Atom moved in multiple directions!";
                    return Err(sim_error_pos(error_str,self.atoms.atom_map[this_key].pos));
                }
            } else {
                motion.atoms.insert(this_key, movement);
                let atom = &self.atoms.atom_map[this_key];
                for dir in 0..6 {
                    if atom.connections[dir as usize].intersects(Bonds::DYNAMIC_BOND) {
                        let newpos = atom.pos + rot_to_pos(dir);
                        let newkey = *self.atoms.locs.get(&newpos).expect("Inconsistent atoms (movement prep)");
                        moving_atoms.push_back(newkey);
                    }
                }
            }
        }
        Ok(())
    }

    //Finalize atom movement
    fn apply_motion(&mut self, motion: &WorldStepInfo) -> SimResult<()> {
        for i in 0..self.arms.len() {
            self.arms[i].do_motion(motion.arms[i]);
        }
        //remove all grabbed atoms from the grid
        for (atom_key, _action) in motion.atoms.iter() {
            let atom = &mut self.atoms.atom_map[atom_key];
            let loc_check = self.atoms.locs.remove(&atom.pos);
            if loc_check != Some(atom_key) {
                panic!("Inconsistent atoms (move loc)!");
            }
        }
        //add them back in at their new locations
        for (atom_key, action) in motion.atoms.iter() {
            let atom = &mut self.atoms.atom_map[atom_key];
            match action {
                Movement::Linear(p) => atom.pos += p,
                Movement::Rotation(r, p) => {
                    atom.pos = rotate_around(atom.pos, *r, *p);
                    atom.rotate_connections(*r);
                }
                Movement::HeldStill => (),
            }
            let current = self.atoms.locs.insert(atom.pos, atom_key);
            if current != None {
                let error_str = &"Multiple atoms in same hex!";
                return Err(sim_error_pos(error_str,atom.pos));
            }
        }
        Ok(())
    }

    //Returns the movement the arm is going to do. Returns size of the largest molecule rotated
    fn do_instruction(&mut self, motion: &mut WorldStepInfo, arm_id: usize, timestep: u64) -> SimResult<()>{
        assert_eq!(motion.arms.len(), arm_id, "Arm motion array not same as arm ID");
        let arm = &mut self.arms[arm_id];
        use ArmType::*;
        use Instr::*;
        use Movement::*;
        use ArmMovement::*;
        let arm_type = arm.arm_type;
        let tape = &arm.instruction_tape;
        let timestep = timestep as usize;
        let instruction = tape.get(timestep, self.repeat_length);
        const STILL: ArmMovement = Move(HeldStill);
        let action = if arm.halted {STILL} else {match instruction {
            Extend => {
                if arm_type == Piston && arm.len < 3 {
                    LengthAdjust(1)
                } else { STILL }
            }
            Retract => {
                if arm_type == Piston && arm.len > 1 {
                    LengthAdjust(-1)
                } else { STILL }
            }
            RotateCounterClockwise => {
                Move(Rotation(1, arm.pos))
            }
            RotateClockwise => {
                Move(Rotation(-1, arm.pos))
            }
            PivotCounterClockwise => Pivot(1),
            PivotClockwise => Pivot(-1),
            Drop => {
                if arm_type != VanBerlo {
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
                if !arm.grabbing && arm_type != VanBerlo {
                    arm.grabbing = true;
                    for r in (0..6).step_by(Arm::angles_between_arm(arm.arm_type) as usize) {
                        let grab_pos = arm.pos + (rot_to_pos(arm.rot + r) * arm.len);
                        let null_key = AtomKey::null();
                        let mut current = self.atoms.locs.get(&grab_pos).unwrap_or(&null_key);
                        if current != &null_key{
                            if self.atoms.atom_map[*current].immutable_wheel{
                                current = &null_key;
                            }
                        }
                        arm.atoms_grabbed[r as usize] = *current;
                    }
                }
                STILL
            }
            Forward => {
                self.track_maps.plus.get(&arm.pos)
                    .map_or(STILL, |&x| {Move(Linear(x))})
            }
            Back => {
                self.track_maps.minus.get(&arm.pos)
                    .map_or(STILL, |&x| {Move(Linear(x))})
            }
            Repeat | Reset | Noop => {
                panic!("Unprocessed instruction!");
            }
            Halt => {arm.halted = true; STILL}
            Empty => STILL,
        }};
        let rotation_store = arm.rot;
        for atom in arm.atoms_grabbed {
            if !atom.is_null() {
                let atom_movement = match action{
                    ArmMovement::Pivot(r) => Movement::Rotation(r, self.atoms.atom_map[atom].pos),
                    ArmMovement::Move(m) => m,
                    ArmMovement::LengthAdjust(a) => Linear(rot_to_pos(rotation_store)*a)
                };
                let tmp = motion.atoms.insert(atom, atom_movement);
                match tmp{
                    Some(x) if x != atom_movement => {
                        println!("was {:?},applying {:?}",atom_movement,x);
                        let error_str = &"Atom moved in multiple directions!";
                        return Err(sim_error_pos(error_str,self.atoms.atom_map[atom].pos));
                    }
                    _ => {}
                }
            }
        }
        motion.arms.push(action);
        Ok(())
    }

    fn initial_area(&mut self) -> SimResult<()>{
        use GlyphType::*;
        for glyph in &mut self.glyphs {
            let pos = glyph.pos; //primary position
            let pos_bi = glyph.reposition(Pos::new(1, 0)); //position for all 2-sized glyphs
            let pos_tri = glyph.reposition(Pos::new(0, 1)); //third (closely packed) position

            let pos_ani = glyph.reposition(Pos::new(1, -1));

            let pos_disp2 = glyph.reposition(Pos::new(1, -1));
            let pos_disp3 = glyph.reposition(Pos::new(0, -1));
            let pos_disp4 = glyph.reposition(Pos::new(-1, 0));

            let pos_multi2 = glyph.reposition(Pos::new(0, -1));
            let pos_multi3 = glyph.reposition(Pos::new(-1, 1));

            let pos_unif2 = glyph.reposition(Pos::new(-1, 1));
            let pos_unif3 = glyph.reposition(Pos::new(0, -1));
            let pos_unif4 = glyph.reposition(Pos::new(1, -1));

            let pos_div = glyph.reposition(Pos::new(-1, 0));

            let pos_proQS = glyph.reposition(Pos::new(1, 1));
            let pos_proM = glyph.reposition(Pos::new(-1, 1));
            let pos_proO = glyph.reposition(Pos::new(1, -1)); //The three blank spaces are pos/bi/tri
            fn add_all(area_set: &mut FxHashSet<Pos>,input: impl IntoIterator<Item=Pos>) -> SimResult<()>{
                for p in input{
                    area_set.insert(p);
                    /*if !area_set.insert(p){
                        return Err(SimError{error_str:"Overlap detected!", location:pos_to_xy(p)});
                    }*/
                }
                Ok(())
            }
            match &glyph.glyph_type{
                Calcification => {
                    add_all(&mut self.area_touched,[pos])
                }
                Animismus => {
                    add_all(&mut self.area_touched,[pos, pos_bi, pos_tri, pos_ani])
                }
                Projection | Bonding | Unbonding | Rejection | Duplication=> {
                    add_all(&mut self.area_touched,[pos, pos_bi])
                }
                Dispersion => {
                    add_all(&mut self.area_touched,[pos, pos_bi,pos_disp2,pos_disp3,pos_disp4])
                }
                Unification => {
                    add_all(&mut self.area_touched,[pos, pos_tri,pos_unif2,pos_unif3,pos_unif4])
                }
                Purification | TriplexBond => {
                    add_all(&mut self.area_touched,[pos, pos_bi, pos_tri])
                }
                MultiBond => {
                    add_all(&mut self.area_touched,[pos, pos_bi,pos_multi2,pos_multi3])
                }
                Division => {
                    add_all(&mut self.area_touched,[pos, pos_bi,pos_div])
                }
                Proliferation => {
                    add_all(&mut self.area_touched,[pos, pos_bi, pos_tri, pos_proQS, pos_proM, pos_proO])
                }
                Input(meta_pattern,_) 
                | Output(meta_pattern, _,_)
                | OutputRepeating(meta_pattern, _,_)=> {
                    let atom_points = &meta_pattern[0];
                    add_all(&mut self.area_touched,atom_points.iter().map(|a|a.pos))
                }
                Track(pos_list) => {
                    add_all(&mut self.area_touched,pos_list.iter().copied())
                },
                Disposal => {
                    add_all(&mut self.area_touched,[pos, pos_bi, pos_tri, pos_ani, pos_disp3,pos_disp4,pos_unif2])
                },
                Conduit(_atom_teleport,_) => {
                    Ok(())//TODO
                },
                Equilibrium => {
                    add_all(&mut self.area_touched,[pos])
                }
            }?;
        }
        Ok(())
    }
    fn process_inputs(&mut self, motion: &mut WorldStepInfo) -> SimResult<()>{
        use GlyphType::*;
        for glyph in self.glyphs.iter_mut() {
            let atoms = &mut self.atoms;
            match &mut glyph.glyph_type{
                Input(meta_pattern,_) => {
                    let atom_spawn_points = &meta_pattern[0];
                    if atom_spawn_points.iter().all(|a| atoms.check_empty(motion,a.pos)){
                        for a in atom_spawn_points{
                            atoms.create_atom(a.clone())?;
                        }
                        let tmp = meta_pattern.remove(0);
                        meta_pattern.push(tmp);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
    fn process_outputs(&mut self, motion: &mut WorldStepInfo){
        use GlyphType::*;
        for glyph in self.glyphs.iter_mut() {
            let atoms = &mut self.atoms;
            match &mut glyph.glyph_type{
                Output(meta_pattern, output_count,_) => {
                    let atom_drop_points = &meta_pattern[0];
                    let full_match = atom_drop_points.iter().all( |a|-> bool {
                            let try_key = atoms.locs.get(&a.pos);
                            if let Some(&atom_key) = try_key{
                                &atoms.atom_map[atom_key] == a && !motion.atoms.contains_key(atom_key)
                            } else {false}
                        });
                    if full_match {
                        for a in atom_drop_points{
                            atoms.destroy_atom_at(a.pos);
                        }
                        if *output_count > 0{
                            *output_count -= 1;
                        }
                        let tmp = meta_pattern.remove(0);
                        meta_pattern.push(tmp);
                    }
                },
                OutputRepeating(meta_pattern, output_count,_) => {
                    let atom_drop_points = &meta_pattern[0];
                    let full_match = atom_drop_points.iter().all( |a|-> bool {
                            let try_key = atoms.locs.get(&a.pos);
                            if let Some(&atom_key) = try_key{
                                &atoms.atom_map[atom_key] == a
                            } else {false}
                        });
                    if full_match {
                        //TODO: Accurately detect partial infinites
                        let tmp = meta_pattern.remove(0);
                        meta_pattern.push(tmp);
                        if *output_count > 0{
                            *output_count = 0;
                        }
                    }
                },
                _ => {}
            }
        }
    }
    fn process_glyphs(&mut self, motion: &mut WorldStepInfo, first_half: bool) -> SimResult<()>{
        fn try_bond(atoms: &mut WorldAtoms, loc1: &Pos, loc2: &Pos) {
            let rot = pos_to_rot(loc2 - loc1).unwrap() as usize;
            if let (Some(&key1), Some(&key2)) = (atoms.locs.get(loc1), atoms.locs.get(loc2)) {
                let [atom1,atom2] = atoms.atom_map.get_disjoint_mut([key1,key2]).expect("Inconsistent atoms!");
                if (atom1.immutable_wheel || atom2.immutable_wheel) {return;}
                let bond1 = atom1.connections[rot];
                assert_eq!(atom2.connections[(rot + 3) % 6], bond1, "Inconsistent bonds");
                if bond1 == Bonds::NO_BOND {
                    atom1.connections[rot] = Bonds::NORMAL;
                    atom2.connections[(rot + 3) % 6] = Bonds::NORMAL;
                }
            }
        }
        fn try_triplex_bond(atoms: &mut WorldAtoms, loc1: &Pos, loc2: &Pos, bond_type: Bonds ) {
            let rot = pos_to_rot(loc2 - loc1).unwrap() as usize;
            if let (Some(&key1), Some(&key2)) = (atoms.locs.get(loc1), atoms.locs.get(loc2)) {
                let [atom1,atom2] = atoms.atom_map.get_disjoint_mut([key1,key2]).expect("Inconsistent atoms!");
                if (atom1.immutable_wheel || atom2.immutable_wheel) {return;}
                if (atom1.atom_type != AtomType::Fire || atom2.atom_type != AtomType::Fire) {return;}
                let bond1 = atom1.connections[rot];
                assert_eq!(atom2.connections[(rot + 3) % 6], bond1, "Inconsistent bonds");
                if (bond1 & !Bonds::TRIPLEX).is_empty() {
                    atom1.connections[rot] |= bond_type;
                    atom2.connections[(rot + 3) % 6] |= bond_type;
                }
            }
        }
        use AtomType::*;
        use GlyphType::*;
        for (id, glyph) in self.glyphs.iter_mut().enumerate() {
            let atoms = &mut self.atoms;
            let rot = normalize_dir(glyph.rot) as usize;

            let pos = glyph.pos; //primary position
            let pos_bi = glyph.reposition(Pos::new(1, 0)); //position for all 2-sized glyphs
            let pos_tri = glyph.reposition(Pos::new(0, 1)); //third (closely packed) position

            let pos_ani = glyph.reposition(Pos::new(1, -1));

            let pos_disp2 = glyph.reposition(Pos::new(1, -1));
            let pos_disp3 = glyph.reposition(Pos::new(0, -1));
            let pos_disp4 = glyph.reposition(Pos::new(-1, 0));

            let pos_multi2 = glyph.reposition(Pos::new(0, -1));
            let pos_multi3 = glyph.reposition(Pos::new(-1, 1));

            let pos_unif2 = glyph.reposition(Pos::new(-1, 1));
            let pos_unif3 = glyph.reposition(Pos::new(0, -1));
            let pos_unif4 = glyph.reposition(Pos::new(1, -1));

            let pos_div = glyph.reposition(Pos::new(-1, 0));

            let pos_proQS = glyph.reposition(Pos::new(1, 1));
            let pos_proM = glyph.reposition(Pos::new(-1, 1));
            let pos_proO = glyph.reposition(Pos::new(1, -1)); //The three blank spaces are pos/bi/tri
            fn pop_check_make(atoms:&mut WorldAtoms, motion:&mut WorldStepInfo, atom: Atom) -> SimResult<()>{
                let a = motion.spawning_atoms.pop_front().unwrap();
                assert!(a == atom);
                atoms.create_atom(a)?;
                Ok(())
            }
            fn pop_make(atoms:&mut WorldAtoms, motion:&mut WorldStepInfo) -> SimResult<()>{
                atoms.create_atom(motion.spawning_atoms.pop_front().unwrap())?;
                Ok(())
            }
            match &mut glyph.glyph_type{
                Calcification => {
                    if let Some(atom) = atoms.get_atom_mut(pos) {
                        if atom.atom_type.is_element() && !atom.immutable_wheel{
                            atom.atom_type = AtomType::Salt;
                        }
                    }
                }
                Animismus => {
                    if first_half{
                        let a1 = atoms.get_consumable_type(motion, pos);
                        let a2 = atoms.get_consumable_type(motion, pos_bi);
                        let o1 = atoms.check_empty(motion, pos_tri);
                        let o2 = atoms.check_empty(motion, pos_ani);
                        if (Some(Salt),Some(Salt),true,true) == (a1,a2,o1,o2) {
                            atoms.destroy_atom_at(pos);
                            atoms.destroy_atom_at(pos_bi);
                            motion.spawning_atoms.push_back(Atom::new(pos_tri, Vitae));
                            motion.spawning_atoms.push_back(Atom::new(pos_ani, Mors));
                            motion.active_glyphs.push_back(id);
                        }
                    } else if motion.active_glyphs.front() == Some(&id){
                        pop_check_make(&mut self.atoms, motion, Atom::new(pos_tri, Vitae))?;
                        pop_check_make(&mut self.atoms, motion, Atom::new(pos_ani, Mors))?;
                        motion.active_glyphs.pop_front();
                    }
                }
                Projection => {
                    let qs = atoms.get_consumable_type(motion, pos);
                    let metal = atoms.get_type(pos_bi);
                    if let (Some(Quicksilver), Some(metal)) = (qs, metal){
                        if let Some(newtype) = metal.metal_id().and_then(|x:u8| AtomType::id_to_metal(x + 1)){
                            atoms.destroy_atom_at(pos);
                            atoms.get_atom_mut(pos_bi).unwrap().atom_type = newtype;
                        }
                    }
                }
                Rejection => {
                    let metal = atoms.get_type(pos);
                    let open = atoms.check_empty(motion, pos_bi);
                    if let (true, Some(metal)) = (open, metal){
                        if let Some(newtype) = metal.metal_id().and_then(|x:u8| AtomType::id_to_metal(x - 1)){
                            atoms.get_atom_mut(pos).unwrap().atom_type = newtype;
                            atoms.create_atom(Atom::new(pos_bi, Quicksilver))?;
                        }
                    }
                }
                Dispersion => {
                    if first_half{
                        let q = atoms.get_consumable_type(motion, pos);
                        let o1 = atoms.check_empty(motion, pos_bi);
                        let o2 = atoms.check_empty(motion, pos_disp2);
                        let o3 = atoms.check_empty(motion, pos_disp3);
                        let o4 = atoms.check_empty(motion, pos_disp4);
                        if (Some(Quintessence),true,true,true,true) == (q,o1,o2,o3,o4){
                            atoms.destroy_atom_at(pos);
                            motion.spawning_atoms.push_back(Atom::new(pos_bi, Earth));
                            motion.spawning_atoms.push_back(Atom::new(pos_disp2, Water));
                            motion.spawning_atoms.push_back(Atom::new(pos_disp3, Fire));
                            motion.spawning_atoms.push_back(Atom::new(pos_disp4, Air));
                            motion.active_glyphs.push_back(id);
                        }
                    } else if motion.active_glyphs.front() == Some(&id){
                        pop_check_make(&mut self.atoms, motion, Atom::new(pos_bi, Earth))?;
                        pop_check_make(&mut self.atoms, motion, Atom::new(pos_disp2, Water))?;
                        pop_check_make(&mut self.atoms, motion, Atom::new(pos_disp3, Fire))?;
                        pop_check_make(&mut self.atoms, motion, Atom::new(pos_disp4, Air))?;
                        motion.active_glyphs.pop_front();
                    }
                }
                Unification => {
                    if first_half{
                        let output = atoms.check_empty(motion, pos);
                        let a1 = atoms.get_consumable_type(motion, pos_tri);
                        let a2 = atoms.get_consumable_type(motion, pos_unif2);
                        let a3 = atoms.get_consumable_type(motion, pos_unif3);
                        let a4 = atoms.get_consumable_type(motion, pos_unif4);
                        if let (true,Some(a),Some(b),Some(c),Some(d)) = (output, a1, a2, a3, a4){
                            let set = [a,b,c,d];
                            if set.contains(&Earth) && set.contains(&Water) && set.contains(&Fire) && set.contains(&Air){
                                atoms.destroy_atom_at(pos_tri);
                                atoms.destroy_atom_at(pos_unif2);
                                atoms.destroy_atom_at(pos_unif3);
                                atoms.destroy_atom_at(pos_unif4);
                                motion.spawning_atoms.push_back(Atom::new(pos, Quintessence));
                                motion.active_glyphs.push_back(id);
                            }
                        }
                    } else if motion.active_glyphs.front() == Some(&id){
                        pop_check_make(&mut self.atoms, motion, Atom::new(pos, Quintessence))?;
                        motion.active_glyphs.pop_front();
                    }
                }
                Purification => {
                    if first_half{
                        let a1 = atoms.get_consumable_type(motion, pos);
                        let a2 = atoms.get_consumable_type(motion, pos_bi);
                        let o = atoms.check_empty(motion, pos_tri);
                        match (a1,a2,o){
                            (Some(a1),Some(a2),true) if a1 == a2 => {
                                let next = a1.metal_id().and_then(|x:u8| AtomType::id_to_metal(x + 1));
                                if let Some(newtype) = next {
                                    atoms.destroy_atom_at(pos);
                                    atoms.destroy_atom_at(pos_bi);
                                    motion.spawning_atoms.push_back(Atom::new(pos_tri, newtype));
                                    motion.active_glyphs.push_back(id);
                                }
                            }
                            _ => ()
                        }
                    } else if motion.active_glyphs.front() == Some(&id){
                        pop_make(&mut self.atoms, motion)?;
                        motion.active_glyphs.pop_front();
                    }
                }
                Division => {
                    if first_half{
                        let a = atoms.get_consumable_type(motion, pos);
                        let o1 = atoms.check_empty(motion, pos_bi);
                        let o2 = atoms.check_empty(motion, pos_div);
                        if let (Some(a),true,true) = (a,o1,o2) {
                            let next1 = a.metal_id().and_then(|x:u8| AtomType::id_to_metal(x/2));
                            let next2 = a.metal_id().and_then(|x:u8| AtomType::id_to_metal((x+1)/2));
                            if let (Some(next1), Some(next2)) = (next1, next2) {
                                atoms.destroy_atom_at(pos);
                                motion.spawning_atoms.push_back(Atom::new(pos_bi, next1));
                                motion.spawning_atoms.push_back(Atom::new(pos_div, next2));
                                motion.active_glyphs.push_back(id);
                            }
                        }
                    } else if motion.active_glyphs.front() == Some(&id){
                        pop_make(&mut self.atoms, motion)?;
                        motion.active_glyphs.pop_front();
                    }
                }
                Duplication => {
                    let source = atoms.get_type(pos);
                    let salt = atoms.get_nonberlo_type(pos_bi);
                    if let (Some(elem), Some(Salt)) = (source, salt){
                        if elem.is_element() {
                            atoms.get_atom_mut(pos_bi).unwrap().atom_type = elem;
                        }
                    }
                }
                Proliferation => {
                    if first_half{
                        let m = atoms.get_type(pos_proM);
                        let qs = atoms.get_consumable_type(motion, pos_proQS);
                        let o = atoms.check_empty(motion, pos_proO);
                        if let (Some(m),Some(Quicksilver),true) = (m,qs,o) {
                            atoms.destroy_atom_at(pos_proQS);
                            motion.spawning_atoms.push_back(Atom::new(pos_proO, m));
                            motion.active_glyphs.push_back(id);
                        }
                    } else if motion.active_glyphs.front() == Some(&id){
                        pop_make(&mut self.atoms, motion)?;
                        motion.active_glyphs.pop_front();
                    }
                }
                Bonding => {
                    try_bond(atoms, &pos, &pos_bi);
                }
                MultiBond => {
                    try_bond(atoms, &pos, &pos_bi);
                    try_bond(atoms, &pos, &pos_multi2);
                    try_bond(atoms, &pos, &pos_multi3);
                }
                TriplexBond => {
                    try_triplex_bond(atoms, &pos, &pos_bi, Bonds::TRIPLEX_K);
                    try_triplex_bond(atoms, &pos, &pos_tri, Bonds::TRIPLEX_Y);
                    try_triplex_bond(atoms, &pos_bi, &pos_tri, Bonds::TRIPLEX_R);
                }
                Unbonding => {
                    if let (Some(&key1), Some(&key2)) = (atoms.locs.get(&pos), atoms.locs.get(&pos_bi)){
                        let bond1 = atoms.atom_map[key1].connections[rot];
                        assert_eq!(atoms.atom_map[key2].connections[(rot+3)%6], bond1, "Inconsistent bonds");
                        if bond1.intersects(Bonds::DYNAMIC_BOND) {
                            atoms.atom_map[key1].connections[rot] = Bonds::NO_BOND;
                            atoms.atom_map[key2].connections[(rot+3)%6] = Bonds::NO_BOND;
                            motion.recent_bonds.insert(key1);
                            motion.recent_bonds.insert(key2);
                        }
                    }
                }
                Disposal => {
                    if let Some(_) = atoms.get_consumable_type_immediate(motion, pos){
                        atoms.destroy_atom_at(pos);
                    }
                },
                Conduit(atom_teleport,conduit_id) => {
                    if first_half {
                        let conduit_info = self.conduit_pairs.get_mut(conduit_id).expect("conduit info not found!");
                        World::conduit_process(atoms, id, atom_teleport, glyph.pos, conduit_info, motion);
                    } else if motion.active_glyphs.front() == Some(&id){
                        while motion.active_glyphs.front() == Some(&id){
                            motion.active_glyphs.pop_front();
                            pop_make(&mut self.atoms, motion)?;
                        }
                    }
                },
                Equilibrium | Track(_) | Input(..) | Output(..) | OutputRepeating(..) => {},
            }
        }
        if !first_half{
            assert_eq!((motion.active_glyphs.len(), motion.spawning_atoms.len()), (0,0));
        }
        Ok(())
    }
    fn conduit_process(atoms: &mut WorldAtoms, glyph_id: usize, positions: &Vec<Pos>, origin: Pos, conduit_info: &mut ConduitInfo, motion: &mut WorldStepInfo){
        let mut viewed_atoms = FxHashSet::<AtomKey>::default();
        let mut check_atoms = VecDeque::<AtomKey>::new();

        let mut offset_pos = conduit_info.offset_pos;
        let mut offset_rot = conduit_info.offset_rot;
        if conduit_info.vecids.0 == glyph_id{
            offset_pos *= -1;
            offset_rot = normalize_dir(offset_rot*-1);
        } else {
            assert!(conduit_info.vecids.1 == glyph_id, "Conduit invalid vec id");
        }
        for pos in positions{
            //For each position on the conduit, if it has an atom on it:
            if let Some(key) = atoms.locs.get(pos){
                //Closure to check if all atoms in the molecule are on the conduit
                let mut atoms_send_check = |first_key: AtomKey| -> bool {
                    check_atoms.push_back(first_key);
                    while let Some(this_key) = check_atoms.pop_front(){
                        if viewed_atoms.contains(&this_key) {continue;}
                        if motion.atoms.contains_key(this_key) {return false;}
                        viewed_atoms.insert(this_key);
                        
                        let atom = &atoms.atom_map[this_key];
                        for dir in 0..6 {
                            if atom.connections[dir as usize].intersects(Bonds::DYNAMIC_BOND) {
                                let newpos = atom.pos + rot_to_pos(dir);
                                if !positions.contains(&newpos) {return false;}
                                let newkey = *atoms.locs.get(&newpos).expect("Inconsistent atoms (conduits)");
                                check_atoms.push_back(newkey);
                            }
                        }
                    }
                    return true;
                };
                //If the atom was just dropped and all connected are on the conduit, move the atoms to the new position via spawning
                if motion.drop_conduit_check.contains(key) && atoms_send_check(*key){
                    for a in &viewed_atoms{
                        let mut atom = atoms.take_atom(*a);
                        atom.pos = rotate_around(atom.pos,offset_rot,origin)+offset_pos;
                        atom.rotate_connections(offset_rot);
                        motion.spawning_atoms.push_back(atom);
                        motion.active_glyphs.push_back(glyph_id);
                    }
                }
                viewed_atoms.clear();
                check_atoms.clear();
            }
        }
    }

    pub fn mark_area_and_collide<'a>(&mut self, float_world: &FloatWorld, spawning_atoms: impl Iterator<Item = &'a Atom>) -> SimResult<()>{
        //atom radius = 29/41 or 1/sqrt(2)
        //spawning atom = 15/41
        //arm base = 20/41
        //grabber, cabinet = 24/41, 20/41
        const ATOM_RADIUS: f32 = 29./41.;
        const ARM_RADIUS: f32 = 20./41.;
        const SPAWNING_ATOM_RADIUS: f32 = 15./41.;
        const ATOM_ATOM_RADIUS_SQUARED: f32 = (ATOM_RADIUS*2.)*(ATOM_RADIUS*2.);
        const ATOM_ARM_RADIUS_SQUARED: f32 = (ATOM_RADIUS+ARM_RADIUS)*(ATOM_RADIUS+ARM_RADIUS);
        const ATOM_SPAWN_RADIUS_SQUARED: f32 = (ATOM_RADIUS+SPAWNING_ATOM_RADIUS)*(ATOM_RADIUS+SPAWNING_ATOM_RADIUS);
        fn make_candidates(primary: Pos) -> [Pos;7]{
            [
                Pos::new(primary.x, primary.y),
                Pos::new(primary.x+1, primary.y),
                Pos::new(primary.x-1, primary.y),
                Pos::new(primary.x, primary.y+1),
                Pos::new(primary.x, primary.y-1),
                Pos::new(primary.x-1, primary.y+1),
                Pos::new(primary.x+1, primary.y-1),
            ]
        }
        fn mark_point(area_set: &mut FxHashSet<Pos>, point: XYPos){
            let primary = xy_to_simple_pos(point);
            let candidates = make_candidates(primary);
            for hex in candidates{
                let distance = nalgebra::distance_squared(&pos_to_xy(hex),&point);
                if distance < ATOM_ATOM_RADIUS_SQUARED{
                    area_set.insert(hex);
                }
            }
        }
        fn collide(atom_collisions: &mut FxHashMap<Pos, SmallVec<[XYPos;2]>>, point: XYPos, check_dist:f32, error_str: &'static str) -> SimResult<()>{
            let primary = xy_to_simple_pos(point);
            let candidates = make_candidates(primary);
            for check in candidates{
                let possible_vec = atom_collisions.get(&check);
                if let Some(atoms) = possible_vec{
                    for atom_point in atoms{
                        if nalgebra::distance_squared(atom_point,&point) < check_dist {
                            let error_point = nalgebra::center(atom_point, &point);
                            return Err(SimError{error_str, location:error_point});
                        }
                    }
                }
            }
            Ok(())
        }
        let mut atom_collisions = FxHashMap::with_capacity_and_hasher(float_world.atoms_xy.len(),Default::default());
        for atom in &float_world.atoms_xy{
            let primary = xy_to_simple_pos(atom.pos);
            mark_point(&mut self.area_touched, atom.pos);
            collide(&mut atom_collisions, atom.pos, ATOM_ATOM_RADIUS_SQUARED, "atom/atom collision!")?;
            atom_collisions.entry(primary).or_insert_with(SmallVec::new).push(atom.pos);
        }
        for arm in &float_world.arms_xy{
            mark_point(&mut self.area_touched, arm.pos);
            collide(&mut atom_collisions, arm.pos, ATOM_ARM_RADIUS_SQUARED,"atom/arm collision!")?;
            for r in (0..6).step_by(Arm::angles_between_arm(arm.arm_type) as usize) {
                let angle = arm.rot+rot_to_angle(r);
                let offset = nalgebra::Rotation2::new(-angle)*XYVec::new(arm.len, 0.);
                mark_point(&mut self.area_touched, arm.pos+offset);
                //arm lengths are doubled in floatworld
                if arm.len > 3.0{
                    let offset = nalgebra::Rotation2::new(-angle)*XYVec::new(2., 0.);
                    mark_point(&mut self.area_touched, arm.pos+offset);
                }
                if arm.len > 5.0{
                    let offset = nalgebra::Rotation2::new(-angle)*XYVec::new(4., 0.);
                    mark_point(&mut self.area_touched, arm.pos+offset);
                }
            }
        }
        for spawning_atom in spawning_atoms{
            collide(&mut atom_collisions, pos_to_xy(spawning_atom.pos), ATOM_SPAWN_RADIUS_SQUARED, "atom/spawning atom collision!")?;
        }
        Ok(())
    }

    pub fn run_step(&mut self, mark_area: bool, motion: &mut WorldStepInfo, float_world: &mut FloatWorld) -> SimResult<()> {
        self.prepare_step(motion)?;
        if mark_area{
            let substep_count = self.substep_count(motion);
            for substep in 0..substep_count{
                let portion = substep as f32 / substep_count as f32;
                float_world.regenerate(self, motion, portion);
                self.mark_area_and_collide(float_world, motion.spawning_atoms.iter())?;
            }
        }
        self.finalize_step(motion)?;
        Ok(())
    }
    //returns the step size to be used for this timestep
    pub fn prepare_step(&mut self, motion: &mut WorldStepInfo) -> SimResult<()> {
        motion.clear();
        for i in 0..self.arms.len() {
            self.do_instruction(motion, i, self.timestep)?;
        }
        self.process_inputs(motion)?;
        self.process_glyphs(motion, true)?;
        self.process_outputs(motion);
        
        for i in 0..self.arms.len() {
            let position = self.arms[i].pos;
            for j in i+1..self.arms.len() {
                if position == self.arms[j].pos {
                    let location = pos_to_xy(position);
                    return Err(SimError{location, error_str: &"Arm-Arm collision!"});
                }
            }
        }
        let tmp = motion.atoms.clone();
        motion.atoms.clear();
        for (atom, movement) in tmp{
            self.premove_atoms(motion, atom, movement)?;
        }
        motion.recent_bonds.clear();
        Ok(())
    }
    pub fn substep_count(&self, motion: &WorldStepInfo) -> usize{
        let mut max_radius:f64 = 1.;
        for (atom_key, movement) in &motion.atoms{
            if let Movement::Rotation(_rot, center) = movement{
                let atom_pos = self.atoms.atom_map[atom_key].pos;
                let distance = nalgebra::distance(&pos_to_xy(atom_pos),&pos_to_xy(*center)) as f64;
                if max_radius < distance {
                    max_radius = distance;
                }
            }
        }
        max_radius *= 2.;
        usize::pow(2,max_radius.log2().round() as u32).max(8)
        //usize::max(10, (max_radius*2.0) as usize)
    }
    pub fn finalize_step(&mut self, motion: &mut WorldStepInfo) -> SimResult<()> {
        self.apply_motion(motion)?;
        self.process_inputs(motion)?;
        self.process_glyphs(motion, false)?;
        motion.clear();
        for arm in &self.arms {
            for atomkey in arm.atoms_grabbed{
                motion.atoms.insert(atomkey, Movement::HeldStill);
                //does nothing if atomkey is null
            }
        }
        self.process_outputs(motion);
        motion.clear();
        self.timestep += 1;
        Ok(())
    }

    pub fn is_complete(&self) -> bool{
        let mut all_outputs_full = true;
        for g in &self.glyphs {
            if let GlyphType::Output(_, i,_) = g.glyph_type {
                all_outputs_full &= (i == 0);
            } else if let GlyphType::OutputRepeating(_, i,_) = g.glyph_type {
                all_outputs_full &= (i == 0);
            }
        }
        all_outputs_full
    }
}

//Setup stuff
impl World {
    fn add_track(track_maps: &mut TrackMaps, track_pos:&Vec<Pos>) -> Result<()> {
        ensure!(track_pos.len() > 0, "Track of length 0!");
        fn try_insert(map: &mut TrackMap, key: Pos, value: Pos){
            if !map.contains_key(&key){
                map.insert(key,value);
            }
        }
        //first do all except the possible loopback
        for track_pair in track_pos.windows(2) {
            let (t_prev, t_now) = (track_pair[0], track_pair[1]);
            let offset = t_now - t_prev;
            try_insert(&mut track_maps.minus, t_now, -offset);
            try_insert(&mut track_maps.plus, t_prev, offset);
        }
        let first = *track_pos.first().unwrap();
        let last = *track_pos.last().unwrap();
        let offset = first - last;
        //check for looping on first/last
        if track_pos.len() > 2 && pos_to_rot(offset).is_some() {
            try_insert(&mut track_maps.minus, first, -offset);
            try_insert(&mut track_maps.plus, last, offset);
        } else {
            //If not looping, block future track overlaps here
            try_insert(&mut track_maps.minus, first, Pos::new(0,0));
            try_insert(&mut track_maps.plus, last, Pos::new(0,0));
        }
        Ok(())
    }
    fn clean_track(track_maps: &mut TrackMaps){
        //Remove {0,0} track offsets that were used to block track overlaps
        let pos_nil = Pos::new(0,0);
        track_maps.plus.retain(|_, v|->bool {
            v != &pos_nil});
        track_maps.minus.retain(|_, v|->bool {
            v != &pos_nil});
    }

    //modifies the original arm to have the new instructions
    //returns the length of the tape (repetition size)
    fn normalize_instructions(
        original: &mut Arm,
        track_maps: &TrackMaps,
    ) -> Result<usize> {
        use ArmType::*;
        use Instr::*;
        let arm_type = original.arm_type;
        let old_instructions = &original.instruction_tape.instructions;
        let mut instructions = Vec::with_capacity(old_instructions.len() + 8);
        let mut repeat_source = 0;
        let mut repeat_ending = 0;
        let mut any_nonrepeat = false;
        let mut curr = 0;
        let mut track_steps = 0;
        let mut rot_diff = 0;
        let mut len = original.len;
        let mut known_grab = false;
        let mut position_check = original.pos;
        fn get_track_loop_length(track_map: &TrackMap, pos: Pos) -> Option<i32>{
            let mut my_pos = pos;
            let mut loop_length = 0;
            loop{
                let offset = track_map.get(&my_pos)?;
                my_pos += offset;
                loop_length += 1;
                if my_pos == pos {return Some(loop_length);}
                if loop_length as usize > track_map.len() {return None;}
            }
        }
        let track_loop = get_track_loop_length(&track_maps.plus, original.pos);
        while curr < old_instructions.len() {
            let instr = old_instructions[curr];
            if !any_nonrepeat && !matches!(instr, Repeat | Empty ){
                any_nonrepeat = true;
                repeat_source = curr;
            }
            if !matches!(instr, Repeat | Reset | Noop) {
                instructions.push(instr);
                curr += 1;
            }
            let mut basic_move = |instr: Instr|->Result<()>{
                match instr{
                    Extend => {
                        if arm_type == Piston && len < 3 {
                            len += 1;
                        }
                    }
                    Retract => {
                        if arm_type == Piston && len > 1 {
                            len -= 1;
                        }
                    }
                    RotateCounterClockwise => {
                        rot_diff += 1;
                    }
                    RotateClockwise => {
                        rot_diff -= 1;
                    }
                    Grab => known_grab = true,
                    Drop => known_grab = false,
                    Forward => {
                        if let Some(offset) = track_maps.plus.get(&position_check){
                            track_steps += 1;
                            position_check += offset;
                        }
                    }
                    Back => {
                        if let Some(offset) = track_maps.minus.get(&position_check){
                            track_steps -= 1;
                            position_check += offset;
                        }
                    }
                    PivotCounterClockwise | PivotClockwise | Empty => {}
                    Reset | Repeat | Noop | Halt => {bail!("Instruction {:?} not basic move!",instr);}
                }
                Ok(())
            };
            match instr {
                Extend|Retract|
                RotateCounterClockwise|RotateClockwise|
                Grab|Drop|
                Forward|Back|
                PivotCounterClockwise|PivotClockwise => {
                    basic_move(instr)?;
                    repeat_ending = curr;
                }
                Empty => {}

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
                            basic_move(copied_instr)?;
                        }
                        curr += rep_len;
                        any_nonrepeat = false;

                        known_grab = false;
                        rot_diff = 0;
                        len = original.len;
                        track_steps = 0;
                        position_check = original.pos;
                    }
                }
                Reset => {
                    let mut reset_vec = Vec::new();
                    if known_grab {
                        reset_vec.push(Drop);
                        known_grab = false;
                    }
                    while len > original.len {
                        reset_vec.push(Retract);
                        len -= 1;
                    }
                    while rot_diff > 3 {rot_diff -= 6;}
                    while rot_diff < -3 {rot_diff += 6;}
                    while rot_diff > 0 {
                        reset_vec.push(RotateClockwise);
                        rot_diff -= 1;
                    }
                    while rot_diff < 0 {
                        reset_vec.push(RotateCounterClockwise);
                        rot_diff += 1;
                    }
                    // look for a path forward on the track that's shorter than
                    // the path backward.
                    if let Some(loop_len) = track_loop{
                        while track_steps > loop_len/2 {
                            track_steps -= loop_len;
                        }
                        while track_steps < -loop_len/2 {
                            track_steps += loop_len
                        }
                    }
                    while track_steps > 0 {
                        reset_vec.push(Back);
                        track_steps -= 1;
                    }
                    while track_steps < 0 {
                        reset_vec.push(Forward);
                        track_steps += 1;
                    }
                    while len < original.len {
                        reset_vec.push(Extend);
                        len += 1;
                    }
                    position_check = original.pos;

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
                Halt => {
                    instructions.push(Halt);
                    curr += 1;
                }
                Noop => {
                    instructions.push(Empty);
                    curr += 1;
                }
            };
        }
        instructions.shrink_to_fit();
        let len = instructions.len();
        original.instruction_tape = Tape {
            first: original.instruction_tape.first,
            instructions,
        };
        Ok(len)
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
            instruction_count: 0,
            conduit_pairs: Default::default(),
        };
        for g in &mut world.glyphs {
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
            g.reposition_pattern(); //reposition input/output/conduits
            if let Track(track_data) = &g.glyph_type {
                World::add_track(&mut world.track_maps, track_data)?;
            }
        }
        World::clean_track(&mut world.track_maps);
        for a in &mut world.arms {
            use ArmType::*;
            world.cost += match a.arm_type {
                PlainArm => 20,
                DoubleArm | TripleArm | HexArm | VanBerlo | Ravari => 30,
                Piston => 40,
            };
            let instr_len = World::normalize_instructions(a, &world.track_maps)?;
            world.instruction_count += a.instruction_tape.instructions.iter()
                .filter(|&&a| a != Instr::Empty && a != Instr::Halt).count() as i32;
            if world.repeat_length < instr_len && a.instruction_tape.instructions.iter().all(|x| *x != Instr::Halt){
                world.repeat_length = instr_len;
            }
            if a.arm_type == VanBerlo || a.arm_type == Ravari {
                use AtomType::*;
                a.grabbing = true;
                let atom_setup: [AtomType; 6] = match a.arm_type{
                    VanBerlo => [Salt, Water, Air, Salt, Fire, Earth],
                    Ravari => [Lead, Tin, Iron, Copper, Silver, Gold],
                    _ => unreachable!()
                };
                for i in 0..6 {
                    let pos = a.pos + rot_to_pos((i as Rot) + a.rot);
                    let atom_type = atom_setup[i];
                    let key = world.atoms.create_atom(Atom {
                        pos, atom_type, connections: [Bonds::NO_BOND; 6], immutable_wheel: true
                    })?;
                    a.atoms_grabbed[i] = key;
                }
            }
        }
        
        let mut unfinished_conduit_count = 0;
        for (id, glyph) in world.glyphs.iter_mut().enumerate() {
            if let GlyphType::Input(meta_pattern,_) = &mut glyph.glyph_type {
                let atom_spawn_points = &meta_pattern[0];
                if atom_spawn_points.iter().all(|a| world.atoms.locs.get(&a.pos) == None){
                    for a in atom_spawn_points{
                        world.atoms.create_atom(a.clone())?;
                    }
                    let tmp = meta_pattern.remove(0);
                    meta_pattern.push(tmp);
                }
            }
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
        world.initial_area()?;
        world.timestep = world.get_first_timestep();
        Ok(world)
    }
    fn get_first_timestep(&self) -> u64{
        if self.arms.len() == 0 { return 0 }
        self.arms.iter().map(|a|a.instruction_tape.first as u64).fold(u64::MAX, std::cmp::min)
    }
    pub fn get_stats(&self) -> SolutionStats {
        let cycles = (self.timestep-self.get_first_timestep()) as i32;
        let cost = self.cost;
        let area = self.area_touched.len() as i32;
        let instructions = self.instruction_count;
        SolutionStats {cycles,cost,area,instructions}
    }
}

pub fn get_default_path_strs() -> (&'static str, &'static str, &'static str){
    const DEFAULT_PATHS: &str = include_str!("default_paths.txt");
    let mut path_data = DEFAULT_PATHS.lines();
    let base = path_data.next().unwrap();
    let puzzle = path_data.next().unwrap();
    let solution = path_data.next().unwrap();
    (base, puzzle, solution)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rotation_tests() {
        for r in 0..6 {
            assert_eq!(rot_dist_to_pos(1, r), rotate(Pos::new(1, 0), r));
            assert_eq!(rot_dist_to_pos(2, r), rotate(Pos::new(2, 0), r));
            assert_eq!(rotate(Pos::new(3, 1), r + 1), rotate(Pos::new(-1, 4), r));
        }
    }
}
