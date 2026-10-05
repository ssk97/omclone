#![allow(unused_parens)]
use bitflags::bitflags;
#[cfg(feature = "color-eyre")]
use color_eyre::{
    eyre::{bail, ensure, eyre},
    Result,
};
#[cfg(not(feature = "color-eyre"))]
use simple_eyre::{
    eyre::{bail, ensure, eyre},
    Result,
};
pub use nalgebra::Vector2;
use slotmap::{new_key_type, Key, SecondaryMap, SlotMap};
use std::collections::VecDeque;
use smallvec::{SmallVec, smallvec};
use rustc_hash::{FxHashMap, FxHashSet};
use std::{fmt,error};
use enum_primitive_derive::Primitive;

mod collision;
pub mod coords;
pub use coords::*;

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
            //Found in some solution files; omsim treats it as a blank cell, the same as a noop
            b'I' => Some(Noop),
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
             'o' =>  Some(Noop                  ),
             'b' =>  Some(Halt                  ),
             ' ' =>  Some(Empty                 ),
             _ => None
        }
    }
}

#[derive(Debug, Clone)]
pub struct Tape {
    pub first: usize,
    pub instructions: Vec<Instr>,
    //Cached `instructions.contains(&Halt)`, since it's needed every step. The methods below keep it
    //up to date; anything else that adds or removes a Halt in `instructions` must call update_halts.
    halts: bool,
}
impl Tape {
    pub fn new(first: usize, instructions: Vec<Instr>) -> Self {
        let mut tape = Tape { first, instructions, halts: false };
        tape.update_halts();
        tape
    }
    pub fn update_halts(&mut self) {
        self.halts = self.instructions.contains(&Instr::Halt);
    }
    //Halting tapes run once; all others loop
    pub fn halts(&self) -> bool {
        self.halts
    }
    pub fn clear(&mut self) {
        self.instructions.clear();
        self.first = 0;
        self.halts = false;
    }
    pub fn get(&self, timestep: usize, loop_len: usize) -> Instr {
        use Instr::Empty;
        if timestep >= self.first{
            let after_first = timestep - self.first;
            if self.halts{
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
    //Moves leading empty instructions into `first`. A tape with no instructions starts at 0,
    //like omsim does for arms without instructions.
    fn trim_start(&mut self) {
        let leading = self.instructions.iter().take_while(|&&i| i == Instr::Empty).count();
        self.instructions.drain(..leading);
        self.first += leading;
        if self.instructions.is_empty() {
            self.first = 0;
        }
    }
    //Inserts instructions at the absolute position `index`, or writes over existing ones if `overwrite`
    pub fn insert(&mut self, index: usize, new: &[Instr], overwrite: bool) {
        if new.is_empty() {return;}
        if index < self.first {
            let gap = self.first - index;
            self.instructions.splice(0..0, std::iter::repeat(Instr::Empty).take(gap));
            self.first = index;
        }
        let start = index - self.first;
        if self.instructions.len() < start {
            self.instructions.resize(start, Instr::Empty);
        }
        if overwrite {
            let end = start + new.len();
            if self.instructions.len() < end {
                self.instructions.resize(end, Instr::Empty);
            }
            self.instructions[start..end].copy_from_slice(new);
        } else {
            self.instructions.splice(start..start, new.iter().copied());
        }
        self.trim_start();
        self.update_halts();
    }
    //Deletes the absolute positions in `range`, or blanks them if `overwrite`
    pub fn delete(&mut self, range: std::ops::Range<usize>, overwrite: bool) {
        let old_first = self.first;
        let old_end = old_first + self.instructions.len();
        let start = range.start.clamp(old_first, old_end) - old_first;
        let end = range.end.clamp(old_first, old_end) - old_first;
        if overwrite {
            for i in start..end {
                self.instructions[i] = Instr::Empty;
            }
        } else {
            self.instructions.drain(start..end);
            //Deleted blank space before the first instruction shifts it earlier
            let blanks_before = range.end.min(old_first).saturating_sub(range.start);
            self.first -= blanks_before;
        }
        self.trim_start();
        self.update_halts();
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
    //Null for atoms that aren't on the board (patterns, spawn queue)
    pub molecule: MoleculeKey,
}
impl Atom {
    pub fn new(pos: Pos, atom_type: AtomType) -> Atom {
        Atom {
            pos,
            atom_type,
            connections: [Bonds::NO_BOND; 6],
            immutable_wheel: false,
            molecule: MoleculeKey::null(),
        }
    }
    //Compares everything except molecule membership
    pub fn matches_pattern(&self, pattern: &Atom) -> bool {
        self.pos == pattern.pos && self.atom_type == pattern.atom_type
            && self.connections == pattern.connections && self.immutable_wheel == pattern.immutable_wheel
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
impl ArmType {
    //Wheels hold their atoms permanently and ignore grab/drop
    pub fn is_wheel(&self) -> bool {
        matches!(self, ArmType::VanBerlo | ArmType::Ravari)
    }
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
    fn do_motion(&mut self, action: ArmMovement) {
        use ArmMovement::*;
        use Movement::*;
        match action{
            Move(Linear(p)) => self.pos += p,
            Move(Rotation(r,_)) => self.rot = (self.rot + r) % 6,
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


//Number of monomers shown for (and required by) a repeating output
pub const POLYMER_REPETITIONS: i32 = 6;
//Repeating ("infinite") output data, following omsim's decode.c repeat_molecule and sim.c polymer checks.
//All positions are global (computed after the output glyph is repositioned).
#[derive(Debug, Clone)]
pub struct Polymer {
    //The monomer's atoms (repetition marker removed)
    pub monomer: Vec<Atom>,
    //The second repetition of the monomer, in the same order as `monomer`. Its bonds include the
    //fixed-up bonds between monomers, so it's what atoms are compared against.
    pub second_monomer: Vec<Atom>,
    pub origin: Pos,
    //Position of the pattern atom whose molecule is checked: the origin if the monomer has an atom
    //there, otherwise the monomer's first atom (like omsim's center_atom_index)
    pub center: Pos,
    //Basis of "polymer space": u along the repetition direction, v across it
    pub dir_u: Pos,
    pub dir_v: Pos,
    pub monomer_width: i32,
    pub min_v: i32,
    //Range of u (within the first monomer) that each row covers; atoms outside it are ignored garbage
    pub row_min_u: Vec<i32>,
    pub row_max_u: Vec<i32>,
    pub outputs_per_repetition: i32,
    //How many monomers have been validated so far
    pub validated: i32,
}
fn gcd(mut a: i32, mut b: i32) -> i32 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}
impl Polymer {
    //Returns the polymer data along with the atoms to display (6 repetitions plus placeholders).
    //- The marker can be anywhere: each repetition's origin goes to the previous repetition's marker.
    //- Like omsim, a placeholder goes where a bond from the 6th repetition reaches the 7th repetition.
    pub fn build(origin: Pos, raw: &[Atom], outputs_per_repetition: i32) -> Result<(Polymer, Vec<Atom>)> {
        use AtomType::RepeatingOutputMarker;
        let mut markers = raw.iter().filter(|a| a.atom_type == RepeatingOutputMarker);
        let marker = markers.next().ok_or_else(|| eyre!("Repeating atom marker expected but not found!"))?;
        ensure!(markers.next().is_none(), "Multiple repeating atom markers!");
        let monomer: Vec<Atom> = raw.iter().filter(|a| a.atom_type != RepeatingOutputMarker).cloned().collect();
        ensure!(!monomer.is_empty(), "Empty infinite product!");
        let offset = marker.pos - origin;
        let divisor = gcd(offset.x, offset.y).abs();
        ensure!(divisor != 0, "Repetition marker at the output origin!");
        let dir_u = offset / divisor;
        let dir_v = Pos::new(-dir_u.y, dir_u.x + dir_u.y);
        let center = if monomer.iter().any(|a| a.pos == origin) {origin} else {monomer[0].pos};
        let mut polymer = Polymer {
            monomer, second_monomer: Vec::new(),
            origin, center, dir_u, dir_v, monomer_width: 0,
            min_v: 0, row_min_u: Vec::new(), row_max_u: Vec::new(),
            outputs_per_repetition, validated: 0,
        };
        polymer.monomer_width = polymer.polymer_pos(marker.pos).x;
        let n = polymer.monomer.len();

        let mut expanded = Vec::with_capacity(n * POLYMER_REPETITIONS as usize);
        for i in 0..POLYMER_REPETITIONS {
            for a in &polymer.monomer {
                expanded.push(Atom { pos: a.pos + offset * i, ..a.clone() });
            }
        }
        //Fix up bonds between adjacent atoms in different monomers (one-directional, as in omsim)
        for i in 0..expanded.len() {
            for j in i + 1..expanded.len() {
                if let Some(dir) = pos_to_rot(expanded[j].pos - expanded[i].pos) {
                    let bond = expanded[i].connections[dir as usize];
                    expanded[j].connections[normalize_dir(dir + 3) as usize] |= bond;
                }
            }
        }
        polymer.second_monomer = expanded[n..2 * n].to_vec();

        let monomer_positions: FxHashSet<Pos> = polymer.monomer.iter().map(|a| a.pos).collect();
        let mut placeholders = vec![marker.pos + offset * (POLYMER_REPETITIONS - 1)];
        for a in &polymer.monomer {
            for dir in 0..6 {
                if a.connections[dir as usize] == Bonds::NO_BOND {continue;}
                //A bond to the next monomer's copy of an atom
                let target = a.pos + rot_to_pos(dir) - offset;
                if monomer_positions.contains(&target) {
                    placeholders.push(target + offset * POLYMER_REPETITIONS);
                }
            }
        }
        placeholders.sort_by_key(|p| (p.x, p.y));
        placeholders.dedup();

        let vs: Vec<i32> = expanded.iter().map(|a| polymer.polymer_pos(a.pos).y).collect();
        polymer.min_v = *vs.iter().min().unwrap();
        let max_v = *vs.iter().max().unwrap();
        ensure!((max_v as i64) - (polymer.min_v as i64) <= 99999, "Infinite product with too many rows!");
        let rows = (max_v - polymer.min_v + 1) as usize;
        polymer.row_min_u = vec![i32::MAX; rows];
        polymer.row_max_u = vec![i32::MIN; rows];
        //Mark every row crossed by the first monomer, including rows between adjacent atoms (oblique polymers)
        for i in 0..n {
            let pi = polymer.polymer_pos(polymer.monomer[i].pos);
            for j in i + 1..n {
                if pos_to_rot(polymer.monomer[j].pos - polymer.monomer[i].pos).is_none() {continue;}
                let (mut p, mut q) = (pi, polymer.polymer_pos(polymer.monomer[j].pos));
                if p.y > q.y {std::mem::swap(&mut p, &mut q);}
                for v in p.y + 1..q.y {
                    let u = p.x + ((q.x - p.x) * (v - p.y)) / (q.y - p.y);
                    polymer.widen_row(v, u);
                }
            }
            polymer.widen_row(pi.y, pi.x);
        }
        //Rows with no atoms are infinitely wide: atoms there always invalidate the polymer
        for row in 0..rows {
            if polymer.row_min_u[row] == i32::MAX && polymer.row_max_u[row] == i32::MIN {
                polymer.row_min_u[row] = i32::MIN;
                polymer.row_max_u[row] = i32::MAX;
            }
        }

        let mut display = expanded;
        display.extend(placeholders.into_iter().map(|p| Atom::new(p, RepeatingOutputMarker)));
        Ok((polymer, display))
    }
    fn widen_row(&mut self, v: i32, u: i32) {
        let row = (v - self.min_v) as usize;
        self.row_min_u[row] = self.row_min_u[row].min(u);
        self.row_max_u[row] = self.row_max_u[row].max(u);
    }
    pub fn polymer_pos(&self, p: Pos) -> Pos {
        let d = p - self.origin;
        Pos::new(self.dir_v.y * d.x - self.dir_v.x * d.y, -self.dir_u.y * d.x + self.dir_u.x * d.y)
    }
    //Atoms outside the rows of the polymer, or beyond either end of a row, are allowed garbage
    fn is_ignored(&self, pos: Pos, monomer_count: i32) -> bool {
        let p = self.polymer_pos(pos);
        let row = p.y - self.min_v;
        if row < 0 || row as usize >= self.row_min_u.len() {
            return false;
        }
        let row = row as usize;
        p.x < self.row_min_u[row] || (self.row_max_u[row] != i32::MAX
            && p.x as i64 > self.row_max_u[row] as i64 + ((monomer_count - 1) * self.monomer_width) as i64)
    }
    fn check(&self, atoms: &WorldAtoms, molecule_key: MoleculeKey, monomer_count: i32) -> bool {
        let molecule = &atoms.molecules[molecule_key].atoms;
        let mut ignorable_atoms = molecule.len() as i64 - (monomer_count as i64) * (self.monomer.len() as i64);
        if ignorable_atoms < 0 {
            return false;
        }
        for &key in molecule {
            let atom = &atoms.atom_map[key];
            if self.is_ignored(atom.pos, monomer_count) {
                ignorable_atoms -= 1;
                if ignorable_atoms < 0 {
                    return false;
                }
                continue;
            }
            let atom_pp = self.polymer_pos(atom.pos);
            let target = self.second_monomer.iter().find(|m| {
                let m_pp = self.polymer_pos(m.pos);
                if (atom_pp.x - m_pp.x) % self.monomer_width != 0 || atom_pp.y != m_pp.y {
                    return false;
                }
                //Offset by 1 since we compare against the second monomer
                let monomer_index = 1 + (atom_pp.x - m_pp.x) / self.monomer_width;
                monomer_index >= 0 && monomer_index < monomer_count
            });
            let Some(target) = target else {return false};
            if atom.atom_type != target.atom_type {
                return false;
            }
            for dir in 0..6 {
                //Bonds that lead to an ignored position are allowed to differ
                if atom.connections[dir as usize] != target.connections[dir as usize]
                    && !self.is_ignored(atom.pos + rot_to_pos(dir), monomer_count) {
                    return false;
                }
            }
        }
        true
    }
    //Validates as many additional monomers as the molecule at the center allows
    fn update(&mut self, atoms: &WorldAtoms) {
        let Some(&key) = atoms.locs.get(&self.center) else {return};
        let molecule_key = atoms.atom_map[key].molecule;
        let mut monomer_count = self.validated + 1;
        while self.check(atoms, molecule_key, monomer_count) {
            self.validated = monomer_count;
            monomer_count += 1;
        }
    }
    pub fn remaining_outputs(&self) -> i32 {
        (POLYMER_REPETITIONS - self.validated).max(0) * self.outputs_per_repetition
    }
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
    //Repeating output data, keyed by glyph index
    pub polymers: FxHashMap<usize, Polymer>,
}
type ContuitPairMap = FxHashMap<i32, ConduitInfo>;
new_key_type! { pub struct AtomKey; pub struct MoleculeKey; }
//A molecule is a set of atoms that always move together. Usually it's the atoms connected by bonds,
//but it can also be disjoint (e.g. from a disjoint input). Bonding merges molecules, and unbonding
//recalculates the molecule from its bonds, splitting off every disconnected part.
#[derive(Debug, Clone, Default)]
pub struct Molecule {
    pub atoms: SmallVec<[AtomKey; 8]>,
}
//A molecule waiting to be created in the second half of the timestep
pub type SpawnMolecule = SmallVec<[Atom; 1]>;
#[derive(Debug, Clone)]
pub struct WorldAtoms {
    pub atom_map: SlotMap<AtomKey, Atom>,
    pub locs: FxHashMap<Pos, AtomKey>,
    pub molecules: SlotMap<MoleculeKey, Molecule>,
}
impl WorldAtoms {
    fn new() -> WorldAtoms {
        WorldAtoms {
            atom_map: SlotMap::with_key(),
            locs: Default::default(),
            molecules: SlotMap::with_key(),
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
    //Like get_type, but wheel atoms are invisible (as they are to most glyphs)
    fn get_nonwheel_type(&self, loc: Pos) -> Option<AtomType> {
        let &key = self.locs.get(&loc)?;
        let atom = self.atom_map.get(key).expect("Inconsistent atoms (nonwheel type check)");
        if atom.immutable_wheel {return None};
        Some(self.atom_map.get(key)?.atom_type)
    }
    pub fn molecule_len(&self, key: AtomKey) -> usize {
        self.molecules[self.atom_map[key].molecule].atoms.len()
    }
    //Consumable atoms are unbonded single-atom molecules
    fn is_lone_atom(&self, key: AtomKey) -> bool {
        let atom = self.atom_map.get(key).expect("Inconsistent atoms (consume check)");
        atom.connections == [Bonds::NO_BOND;6] && self.molecule_len(key) == 1
    }
    fn get_consumable_type(&self, motion: &WorldStepInfo, loc: Pos) -> Option<AtomType> {
        let &key = self.locs.get(&loc)?;
        if motion.atoms.contains_key(key) || motion.recent_bonds.contains(&key) {
            return None;
        }
        if !self.is_lone_atom(key){
            return None
        }
        //Don't need to check is_berlo since the berlo arm will always be grabbing it (applying a motion)
        Some(self.atom_map[key].atom_type)
    }
    //Same as above except doesn't have the unbond check
    fn get_consumable_type_immediate(&self, motion: &WorldStepInfo, loc: Pos) -> Option<AtomType> {
        let &key = self.locs.get(&loc)?;
        if motion.atoms.contains_key(key) {
            return None;
        }
        if !self.is_lone_atom(key){
            return None
        }
        //Don't need to check is_berlo since the berlo arm will always be grabbing it (applying a motion)
        Some(self.atom_map[key].atom_type)
    }
    fn check_empty(&self, motion: &WorldStepInfo, loc: Pos) -> bool {
        if self.locs.contains_key(&loc) {
            return false;
        }
        return !motion.spawning_atoms.iter().flatten().any(|x| x.pos == loc);
    }

    //Removes the atom from the board and from its molecule (deleting the molecule if now empty)
    fn remove_atom(&mut self, key: AtomKey) -> Atom {
        let mut atom = self.atom_map.remove(key)
            .expect("Inconsistent atoms (remove)!");
        let atom_key = self.locs.remove(&atom.pos).expect("Removing nonatom!");
        if atom_key != key {
            panic!("Inconsistent atoms (remove key)!");
        }
        let molecule = &mut self.molecules[atom.molecule].atoms;
        let index = molecule.iter().position(|&k| k == key).expect("Inconsistent molecule (remove)!");
        molecule.swap_remove(index);
        if molecule.is_empty() {
            self.molecules.remove(atom.molecule);
        }
        atom.molecule = MoleculeKey::null();
        atom
    }
    //Note: only use on non-grabbed atoms!
    fn destroy_atom_at(&mut self, loc: Pos) {
        let atom_key = *self.locs.get(&loc).expect("Destroying nonatom!");
        self.remove_atom(atom_key);
    }
    //Removes every atom of the molecule from the board, returning them
    fn take_molecule(&mut self, molecule_key: MoleculeKey) -> SpawnMolecule {
        let molecule = self.molecules.remove(molecule_key).expect("Inconsistent molecule (take)!");
        molecule.atoms.iter().map(|&key| {
            let mut atom = self.atom_map.remove(key).expect("Inconsistent atoms (take)!");
            let atom_key = self.locs.remove(&atom.pos).expect("Taking nonatom!");
            if atom_key != key {
                panic!("Inconsistent atoms (take key)!");
            }
            atom.molecule = MoleculeKey::null();
            atom
        }).collect()
    }
    fn create_molecule(&mut self, atoms: impl IntoIterator<Item=Atom>) -> SimResult<MoleculeKey> {
        let molecule_key = self.molecules.insert(Molecule::default());
        for mut atom in atoms {
            atom.molecule = molecule_key;
            let pos = atom.pos;
            let key = self.atom_map.insert(atom);
            self.molecules[molecule_key].atoms.push(key);
            let output = self.locs.insert(pos, key);
            if let Some(_old_atom) = output{
                return Err(SimError{error_str:"Atom creation on hex with existing atom!",location:pos_to_xy(pos)});
            }
        }
        Ok(molecule_key)
    }
    //Bonding directly combines the two molecules
    fn merge_molecules(&mut self, key1: AtomKey, key2: AtomKey) {
        let m1 = self.atom_map[key1].molecule;
        let m2 = self.atom_map[key2].molecule;
        if m1 == m2 {return;}
        let (big, small) = if self.molecules[m1].atoms.len() >= self.molecules[m2].atoms.len() {(m1, m2)} else {(m2, m1)};
        let small_atoms = self.molecules.remove(small).expect("Inconsistent molecule (merge)!").atoms;
        for &key in &small_atoms {
            self.atom_map[key].molecule = big;
        }
        self.molecules[big].atoms.extend(small_atoms);
    }
    //Unbonding recalculates the molecule from its bonds, separating all disjoint parts.
    //A bond only connects two atoms if both atoms have it (dangling bonds are ignored).
    fn split_molecule(&mut self, molecule_key: MoleculeKey) {
        let atoms = std::mem::take(&mut self.molecules[molecule_key].atoms);
        let mut visited = FxHashSet::<AtomKey>::default();
        let mut stack = Vec::new();
        let mut first = true;
        for &start in &atoms {
            if !visited.insert(start) {continue;}
            let target = if first {first = false; molecule_key} else {self.molecules.insert(Molecule::default())};
            stack.push(start);
            while let Some(key) = stack.pop() {
                self.atom_map[key].molecule = target;
                self.molecules[target].atoms.push(key);
                let atom = &self.atom_map[key];
                for dir in 0..6 {
                    if !atom.connections[dir as usize].intersects(Bonds::DYNAMIC_BOND) {continue;}
                    let Some(&next_key) = self.locs.get(&(atom.pos + rot_to_pos(dir))) else {continue};
                    let next = &self.atom_map[next_key];
                    //Unvisited atoms of the original molecule still have the original key
                    if next.molecule != molecule_key || visited.contains(&next_key) {continue;}
                    if !next.connections[normalize_dir(dir + 3) as usize].intersects(Bonds::DYNAMIC_BOND) {continue;}
                    visited.insert(next_key);
                    stack.push(next_key);
                }
            }
        }
    }
}

//A grabbed atom and the movement its arm gives it this timestep
#[derive(Debug, Clone, Copy)]
pub struct GrabMovement {
    pub arm: usize,
    pub grabber: usize, //grabber direction index relative to the arm
    pub atom: AtomKey,
    pub movement: Movement,
}
pub struct WorldStepInfo{
    pub atoms: SecondaryMap<AtomKey, Movement>,
    pub grabs: Vec<GrabMovement>,
    pub grab_atoms: Vec<SmallVec<[AtomKey; 8]>>, //atoms moved by each grab (parallel to grabs)
    pub arms: Vec<ArmMovement>,
    pub spawning_atoms: VecDeque<SpawnMolecule>, //One entry per molecule
    pub recent_bonds: FxHashSet<AtomKey>,
    pub drop_conduit_check: FxHashSet<AtomKey>, //contains one entry per arm
    pub active_glyphs: VecDeque<usize>, //One entry per glyph activation; conduits add one per molecule sent
}
impl WorldStepInfo{
    pub fn new() -> Self{
        WorldStepInfo{
            atoms: SecondaryMap::new(),
            grabs: Vec::new(),
            grab_atoms: Vec::new(),
            arms: Vec::new(),
            spawning_atoms:VecDeque::new(),
            recent_bonds:FxHashSet::default(),
            drop_conduit_check:FxHashSet::default(),
            active_glyphs:VecDeque::new(),
        }
    }
    pub fn clear(&mut self) {
        self.atoms.clear();
        self.grabs.clear();
        self.grab_atoms.clear();
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
    pub len: f32,//in hexes
    pub arm_type: ArmType,
    pub grabbing: bool,
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
                len: arm_len,
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
            let new_arm = FloatArm{
                pos: pos_to_xy(arm.pos),
                rot: rot_to_angle(arm.rot),
                len: arm.len as f32,
                arm_type: arm.arm_type,
                grabbing: arm.grabbing,
            };
            self.arms_xy.push(new_arm);
        }
    }
}

//Running the world
impl World {
    //sets up a move for the specified atom and all atoms in its molecule
    fn premove_atoms(&self, motion: &mut WorldStepInfo, atom_key: AtomKey, movement: Movement) -> SimResult<()> {
        let molecule_key = self.atoms.atom_map[atom_key].molecule;
        let mut moved = SmallVec::new();
        for &this_key in &self.atoms.molecules[molecule_key].atoms {
            let maybe_move = motion.atoms.get(this_key);
            if let Some(curr_move) = maybe_move {
                if curr_move != &movement {
                    let error_str = &"Atom moved in multiple directions!";
                    return Err(sim_error_pos(error_str,self.atoms.atom_map[this_key].pos));
                }
            } else {
                motion.atoms.insert(this_key, movement);
                moved.push(this_key);
            }
        }
        motion.grab_atoms.push(moved);
        Ok(())
    }

    //Finalize atom movement
    fn apply_motion(&mut self, motion: &WorldStepInfo, mark_area: bool) -> SimResult<()> {
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
            if mark_area {
                self.area_touched.insert(atom.pos);
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
            Repeat | Reset => {
                panic!("Unprocessed instruction!");
            }
            Halt => {arm.halted = true; STILL}
            Noop | Empty => STILL,
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
                Conduit(conduit_hexes,_) => {
                    add_all(&mut self.area_touched,conduit_hexes.iter().copied())
                },
                Equilibrium => {
                    add_all(&mut self.area_touched,[pos])
                }
            }?;
        }
        for arm in &self.arms {
            self.area_touched.insert(arm.pos);
        }
        self.mark_arm_area();
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
                        //The whole input is one molecule, even if it's disjoint
                        atoms.create_molecule(atom_spawn_points.iter().cloned())?;
                        let tmp = meta_pattern.remove(0);
                        meta_pattern.push(tmp);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
    //Returns the molecule if it exactly matches the pattern (same atoms and bonds, nothing extra)
    //and none of its atoms are grabbed. Separate molecules can't stand in for one disjoint molecule,
    //and a disjoint molecule can't stand in for separate ones.
    fn match_output(atoms: &WorldAtoms, motion: &WorldStepInfo, pattern: &[Atom]) -> Option<MoleculeKey> {
        let &first_key = atoms.locs.get(&pattern.first()?.pos)?;
        let molecule_key = atoms.atom_map[first_key].molecule;
        if atoms.molecules[molecule_key].atoms.len() != pattern.len() {
            return None;
        }
        for a in pattern {
            let &atom_key = atoms.locs.get(&a.pos)?;
            let atom = &atoms.atom_map[atom_key];
            if atom.molecule != molecule_key || !atom.matches_pattern(a) || motion.atoms.contains_key(atom_key) {
                return None;
            }
        }
        Some(molecule_key)
    }
    fn process_outputs(&mut self, motion: &mut WorldStepInfo){
        use GlyphType::*;
        for (id, glyph) in self.glyphs.iter_mut().enumerate() {
            let atoms = &mut self.atoms;
            match &mut glyph.glyph_type{
                Output(meta_pattern, output_count,_) => {
                    if let Some(molecule_key) = World::match_output(atoms, motion, &meta_pattern[0]) {
                        atoms.take_molecule(molecule_key);
                        if *output_count > 0{
                            *output_count -= 1;
                        }
                        let tmp = meta_pattern.remove(0);
                        meta_pattern.push(tmp);
                    }
                },
                OutputRepeating(_, output_count,_) => {
                    let polymer = self.polymers.get_mut(&id).expect("Polymer data not found!");
                    polymer.update(atoms);
                    *output_count = polymer.remaining_outputs();
                },
                _ => {}
            }
        }
    }
    fn process_glyphs(&mut self, motion: &mut WorldStepInfo, first_half: bool) -> SimResult<()>{
        //Adds the bond to both atoms unless either already has a bond from `unless` in that direction.
        //Bonds can be one-sided (dangling bonds from puzzle molecules), so both sides are checked.
        fn add_bond(atoms: &mut WorldAtoms, loc1: &Pos, loc2: &Pos, bond_type: Bonds, unless: Bonds, need_fire: bool) {
            let rot = pos_to_rot(loc2 - loc1).unwrap() as usize;
            if let (Some(&key1), Some(&key2)) = (atoms.locs.get(loc1), atoms.locs.get(loc2)) {
                let [atom1,atom2] = atoms.atom_map.get_disjoint_mut([key1,key2]).expect("Inconsistent atoms!");
                if (atom1.immutable_wheel || atom2.immutable_wheel) {return;}
                if need_fire && (atom1.atom_type != AtomType::Fire || atom2.atom_type != AtomType::Fire) {return;}
                if atom1.connections[rot].intersects(unless) || atom2.connections[(rot + 3) % 6].intersects(unless) {return;}
                atom1.connections[rot] |= bond_type;
                atom2.connections[(rot + 3) % 6] |= bond_type;
                atoms.merge_molecules(key1, key2);
            }
        }
        fn try_bond(atoms: &mut WorldAtoms, loc1: &Pos, loc2: &Pos) {
            add_bond(atoms, loc1, loc2, Bonds::NORMAL, Bonds::TRIPLEX, false);
        }
        fn try_triplex_bond(atoms: &mut WorldAtoms, loc1: &Pos, loc2: &Pos, bond_type: Bonds ) {
            add_bond(atoms, loc1, loc2, bond_type, Bonds::NORMAL, true);
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
                assert!(a.len() == 1 && a[0] == atom);
                atoms.create_molecule(a)?;
                Ok(())
            }
            fn pop_make(atoms:&mut WorldAtoms, motion:&mut WorldStepInfo) -> SimResult<()>{
                atoms.create_molecule(motion.spawning_atoms.pop_front().unwrap())?;
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
                            motion.spawning_atoms.push_back(smallvec![Atom::new(pos_tri, Vitae)]);
                            motion.spawning_atoms.push_back(smallvec![Atom::new(pos_ani, Mors)]);
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
                    let metal = atoms.get_nonwheel_type(pos_bi);
                    if let (Some(Quicksilver), Some(metal)) = (qs, metal){
                        if let Some(newtype) = metal.metal_id().and_then(|x:u8| AtomType::id_to_metal(x + 1)){
                            atoms.destroy_atom_at(pos);
                            atoms.get_atom_mut(pos_bi).unwrap().atom_type = newtype;
                        }
                    }
                }
                Rejection => {
                    let metal = atoms.get_nonwheel_type(pos);
                    let open = atoms.check_empty(motion, pos_bi);
                    if let (true, Some(metal)) = (open, metal){
                        if let Some(newtype) = metal.metal_id().and_then(|x:u8| AtomType::id_to_metal(x - 1)){
                            atoms.get_atom_mut(pos).unwrap().atom_type = newtype;
                            atoms.create_molecule([Atom::new(pos_bi, Quicksilver)])?;
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
                            motion.spawning_atoms.push_back(smallvec![Atom::new(pos_bi, Earth)]);
                            motion.spawning_atoms.push_back(smallvec![Atom::new(pos_disp2, Water)]);
                            motion.spawning_atoms.push_back(smallvec![Atom::new(pos_disp3, Fire)]);
                            motion.spawning_atoms.push_back(smallvec![Atom::new(pos_disp4, Air)]);
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
                                motion.spawning_atoms.push_back(smallvec![Atom::new(pos, Quintessence)]);
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
                                    motion.spawning_atoms.push_back(smallvec![Atom::new(pos_tri, newtype)]);
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
                                motion.spawning_atoms.push_back(smallvec![Atom::new(pos_bi, next1)]);
                                motion.spawning_atoms.push_back(smallvec![Atom::new(pos_div, next2)]);
                                motion.active_glyphs.push_back(id);
                            }
                        }
                    } else if motion.active_glyphs.front() == Some(&id){
                        pop_make(&mut self.atoms, motion)?;
                        pop_make(&mut self.atoms, motion)?;
                        motion.active_glyphs.pop_front();
                    }
                }
                Duplication => {
                    let source = atoms.get_type(pos);
                    let salt = atoms.get_nonwheel_type(pos_bi);
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
                            motion.spawning_atoms.push_back(smallvec![Atom::new(pos_proO, m)]);
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
                        let bond2 = atoms.atom_map[key2].connections[(rot+3)%6];
                        if bond1.intersects(Bonds::DYNAMIC_BOND) && bond2.intersects(Bonds::DYNAMIC_BOND) {
                            atoms.atom_map[key1].connections[rot] = Bonds::NO_BOND;
                            atoms.atom_map[key2].connections[(rot+3)%6] = Bonds::NO_BOND;
                            motion.recent_bonds.insert(key1);
                            motion.recent_bonds.insert(key2);
                            //Recalculate the molecule, separating all disjoint parts
                            atoms.split_molecule(atoms.atom_map[key1].molecule);
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
        let mut offset_pos = conduit_info.offset_pos;
        let mut offset_rot = conduit_info.offset_rot;
        if conduit_info.vecids.0 == glyph_id{
            offset_pos *= -1;
            offset_rot = normalize_dir(offset_rot*-1);
        } else {
            assert!(conduit_info.vecids.1 == glyph_id, "Conduit invalid vec id");
        }
        for pos in positions{
            //For each position on the conduit, if it has an atom that was just dropped:
            let Some(&key) = atoms.locs.get(pos) else {continue};
            if !motion.drop_conduit_check.contains(&key) {continue;}
            //Send the whole molecule if all of its atoms are on the conduit and none are grabbed
            let molecule_key = atoms.atom_map[key].molecule;
            let sendable = atoms.molecules[molecule_key].atoms.iter().all(|&a|
                !motion.atoms.contains_key(a) && positions.contains(&atoms.atom_map[a].pos));
            if sendable {
                let mut molecule = atoms.take_molecule(molecule_key);
                for atom in &mut molecule {
                    atom.pos = rotate_around(atom.pos,offset_rot,origin)+offset_pos;
                    atom.rotate_connections(offset_rot);
                }
                motion.spawning_atoms.push_back(molecule);
                motion.active_glyphs.push_back(glyph_id);
            }
        }
    }

    //mark_area enables area tracking and collision checks
    pub fn run_step(&mut self, mark_area: bool, motion: &mut WorldStepInfo) -> SimResult<()> {
        self.prepare_step(motion, mark_area)?;
        if mark_area{
            let substep_count = self.substep_count(motion);
            for substep in 1..substep_count{
                self.collide_substep(motion, substep, substep_count)?;
            }
        }
        self.finalize_step(motion, mark_area)?;
        Ok(())
    }
    pub fn prepare_step(&mut self, motion: &mut WorldStepInfo, mark_area: bool) -> SimResult<()> {
        motion.clear();
        for i in 0..self.arms.len() {
            self.do_instruction(motion, i, self.timestep)?;
        }
        if mark_area {
            self.mark_arm_area();
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
        motion.atoms.clear();
        for i in 0..motion.grabs.len() {
            let grab = motion.grabs[i];
            self.premove_atoms(motion, grab.atom, grab.movement)?;
        }
        motion.recent_bonds.clear();
        Ok(())
    }
    pub fn finalize_step(&mut self, motion: &mut WorldStepInfo, mark_area: bool) -> SimResult<()> {
        self.apply_motion(motion, mark_area)?;
        if mark_area {
            self.mark_arm_area();
        }
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
                    PivotCounterClockwise | PivotClockwise | Empty | Halt | Noop => {}
                    Reset | Repeat => {bail!("Instruction {:?} not basic move!",instr);}
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
                Empty | Halt => {}

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
        let len = instructions.len();
        original.instruction_tape = Tape::new(original.instruction_tape.first, instructions);
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
            polymers: Default::default(),
        };
        for (id, g) in world.glyphs.iter_mut().enumerate() {
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
            if let OutputRepeating(meta_pattern, output_count, _) = &mut g.glyph_type {
                let (polymer, display) = Polymer::build(g.pos, &meta_pattern[0], *output_count / POLYMER_REPETITIONS)?;
                *meta_pattern = smallvec![display];
                world.polymers.insert(id, polymer);
            }
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
                .filter(|&&a| !matches!(a, Instr::Empty | Instr::Halt | Instr::Noop)).count() as i32;
            if world.repeat_length < instr_len && !a.instruction_tape.halts(){
                world.repeat_length = instr_len;
            }
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
        world.initial_area()?;
        world.timestep = world.get_first_timestep();
        Ok(world)
    }
    pub fn get_first_timestep(&self) -> u64{
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
