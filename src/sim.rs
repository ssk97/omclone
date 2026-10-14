use bitflags::bitflags;
#[cfg(feature = "color-eyre")]
use color_eyre::{
    eyre::{ensure, eyre},
    Result,
};
#[cfg(not(feature = "color-eyre"))]
use simple_eyre::{
    eyre::{ensure, eyre},
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
mod track;
pub use track::*;
pub mod coords;
pub use coords::*;
mod tape;
pub use tape::*;
mod atoms;
pub use atoms::*;
mod glyphs;
pub use glyphs::*;
mod polymer;
pub use polymer::*;
mod arms;
pub use arms::*;
mod setup;
mod chambers;
pub use chambers::*;
mod float_world;
pub use float_world::*;

#[derive(Debug, Clone, Copy)]
pub struct SimError{
    pub error_str: &'static str,
    pub location: XYPos,
}
//Names the hex as the editor does elsewhere: "at" its center, or "near" it for a point in between
impl fmt::Display for SimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hex = xy_to_pos(self.location);
        let hex_pos = pos_to_xy(hex);
        let relation = if nalgebra::distance_squared(&hex_pos,&self.location) < 0.1 {"at"} else {"near"};
        write!(f, "{} {relation} ({}, {})", self.error_str, hex.x, hex.y)
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

//A part of the layout: an index into World::glyphs or World::arms
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum PartId {
    Glyph(usize),
    Arm(usize),
}

#[derive(Debug, Default)]
pub struct InitialWorld {
    pub glyphs: Vec<Glyph>,
    pub arms: Vec<Arm>,
    pub chambers: Chambers,
}

#[derive(Debug, Clone)]
pub struct World {
    pub timestep: u64,
    pub atoms: WorldAtoms,
    pub area_touched: FxHashSet<Pos>,
    pub glyphs: Vec<Glyph>,
    pub arms: Vec<Arm>,
    pub chambers: Chambers,
    pub repeat_length: usize,
    pub track_maps: TrackMaps,
    pub cost: i32,
    pub conduit_pairs: ConduitPairMap,
    //Repeating output data, keyed by glyph index
    pub polymers: FxHashMap<usize, Polymer>,
    //The variants of each input with several, as they were before setup spawned the first one (which moved
    //it on to the next), so layout() can restore them. Keyed by glyph index.
    input_variants: Vec<(usize, AtomPattern)>,
}

//A grabbed atom and the movement its arm gives it this timestep
#[derive(Debug, Clone, Copy)]
pub struct GrabMovement {
    pub arm: usize,
    pub grabber: usize, //grabber direction index relative to the arm
    pub atom: AtomKey,
    pub movement: Movement,
}
//A molecule filed in the first half of the timestep, created in the second half at the turn of `glyph`
#[derive(Debug, Clone)]
pub struct Product {
    //The conversion glyph that made it, or the conduit that receives it
    pub glyph: usize,
    pub atoms: SpawnMolecule,
    //A molecule a conduit sent after it was unbonded this half arrives whole, and stays "just unbonded" until it
    //splits at the end of the half it arrives in
    pub unbonded: bool,
}
pub struct WorldStepInfo{
    pub atoms: SecondaryMap<AtomKey, Movement>,
    pub grabs: Vec<GrabMovement>,
    pub grab_atoms: Vec<SmallVec<[AtomKey; 8]>>, //atoms moved by each grab (parallel to grabs)
    pub arms: Vec<ArmMovement>,
    pub grip_toggled: Vec<bool>, //whether each arm grabbed or dropped this step (parallel to arms)
    //In order of glyph once the first half's glyphs are done (they're sorted then)
    pub products: VecDeque<Product>,
    //The atoms unbonded this half
    pub recent_bonds: FxHashSet<AtomKey>,
    pub drop_conduit_check: FxHashSet<AtomKey>, //contains one entry per arm
    //The conduit glyph and a copy of the molecule (as dropped) for each claim, in glyph order
    pub conduit_claims: VecDeque<(usize, SpawnMolecule)>,
    //The atoms conduit clones left behind this half, whose molecules split at the end of the glyph pass
    pub clone_leftovers: Vec<AtomKey>,
}
impl WorldStepInfo{
    pub fn new() -> Self{
        WorldStepInfo{
            atoms: SecondaryMap::new(),
            grabs: Vec::new(),
            grab_atoms: Vec::new(),
            arms: Vec::new(),
            grip_toggled: Vec::new(),
            products: VecDeque::new(),
            recent_bonds:FxHashSet::default(),
            drop_conduit_check:FxHashSet::default(),
            conduit_claims: VecDeque::new(),
            clone_leftovers: Vec::new(),
        }
    }
    pub fn clear(&mut self) {
        self.atoms.clear();
        self.grabs.clear();
        self.grab_atoms.clear();
        self.arms.clear();
        self.grip_toggled.clear();
        self.products.clear();
        self.recent_bonds.clear();
        self.drop_conduit_check.clear();
        self.conduit_claims.clear();
        self.clone_leftovers.clear();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SolutionStats {
    pub cycles: i32,
    pub cost: i32,
    pub area: i32,
    pub instructions: i32,
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
            let pos = self.atoms.atom_map[atom_key].pos;
            self.atoms.remove_from_hex(atom_key, pos);
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
            let pos = atom.pos;
            if mark_area {
                self.area_touched.insert(pos);
            }
            let occupied = self.atoms.is_occupied(pos);
            self.atoms.add_to_hex(atom_key, pos);
            if occupied {
                let error_str = &"Multiple atoms in same hex!";
                return Err(sim_error_pos(error_str,pos));
            }
        }
        Ok(())
    }

    //The hexes a glyph covers, which is what it can overlap. An input with several variants covers its
    //first, which setup has already spawned and moved on from.
    pub fn glyph_footprint(&self, idx: usize) -> SmallVec<[Pos; 8]> {
        let glyph = &self.glyphs[idx];
        match self.input_variants.iter().find(|(id, _)| *id == idx) {
            Some((_, variants)) => variants[0].iter().map(|a| glyph.reposition(a.pos)).collect(),
            None => glyph.footprint(self.polymers.get(&idx)),
        }
    }
    //Arms only cover their base hex
    pub fn part_footprint(&self, part: PartId) -> SmallVec<[Pos; 8]> {
        match part {
            PartId::Glyph(i) => self.glyph_footprint(i),
            PartId::Arm(i) => smallvec![self.arms[i].pos],
        }
    }
    fn is_track_part(&self, part: PartId) -> bool {
        matches!(part, PartId::Glyph(i) if self.glyphs[i].is_track())
    }
    //Parts sharing a hex overlap, except an arm on a track
    fn parts_conflict(&self, a: PartId, b: PartId) -> bool {
        match (a, b) {
            (PartId::Arm(_), other) | (other, PartId::Arm(_)) if self.is_track_part(other) => false,
            _ => a != b,
        }
    }
    fn all_parts(&self) -> impl Iterator<Item=PartId> {
        (0..self.glyphs.len()).map(PartId::Glyph).chain((0..self.arms.len()).map(PartId::Arm))
    }
    //The parts covering a hex, in drawing order from the top: arms (later first), then glyphs (later first)
    pub fn parts_at(&self, hex: Pos) -> Vec<PartId> {
        let arms = (0..self.arms.len()).rev().map(PartId::Arm);
        let glyphs = (0..self.glyphs.len()).rev().map(PartId::Glyph);
        arms.chain(glyphs).filter(|&part| self.part_footprint(part).contains(&hex)).collect()
    }
    //The hexes of `footprint` (belonging to `part`, which may have moved) that overlap any other part,
    //or that it covers twice (a track crossing itself)
    pub fn overlaps_with_others(&self, part: PartId, footprint: &[Pos]) -> SmallVec<[Pos; 8]> {
        let mut overlaps: SmallVec<[Pos; 8]> = SmallVec::new();
        for (i, hex) in footprint.iter().enumerate() {
            if footprint[..i].contains(hex) && !overlaps.contains(hex) {
                overlaps.push(*hex);
            }
        }
        let others = self.all_parts().filter(|&other| self.parts_conflict(part, other));
        self.add_covered(&mut overlaps, others, footprint);
        overlaps
    }
    //The hexes of `hexes` that would overlap a glyph if added to a track (arms can sit on track).
    //`track` is the track itself, if it's already placed.
    pub fn track_overlaps(&self, track: Option<usize>, hexes: &[Pos]) -> SmallVec<[Pos; 8]> {
        let mut overlaps = SmallVec::new();
        let others = (0..self.glyphs.len()).filter(|&i| Some(i) != track).map(PartId::Glyph);
        self.add_covered(&mut overlaps, others, hexes);
        overlaps
    }
    //Adds the hexes of `hexes` that any of `parts` covers to `overlaps`, each once
    fn add_covered(&self, overlaps: &mut SmallVec<[Pos; 8]>, parts: impl Iterator<Item=PartId>, hexes: &[Pos]) {
        for part in parts {
            let footprint = self.part_footprint(part);
            for &hex in hexes {
                if footprint.contains(&hex) && !overlaps.contains(&hex) {
                    overlaps.push(hex);
                }
            }
        }
    }
    pub fn part_overlaps(&self, part: PartId) -> SmallVec<[Pos; 8]> {
        self.overlaps_with_others(part, &self.part_footprint(part))
    }
    pub fn has_overlap(&self) -> bool {
        let mut occupants: FxHashMap<Pos, SmallVec<[PartId; 2]>> = Default::default();
        for part in self.all_parts() {
            for hex in self.part_footprint(part) {
                let here = occupants.entry(hex).or_default();
                //A track crossing itself overlaps too
                if here.iter().any(|&other| other == part || self.parts_conflict(part, other)) {
                    return true;
                }
                here.push(part);
            }
        }
        false
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
        self.claim_conduits(motion);
        self.process_inputs();
        self.process_glyphs(motion, true)?;
        self.process_outputs(motion);
        //Atoms can only share a hex until motion
        if let Some(pos) = self.atoms.any_stack() {
            return Err(sim_error_pos("Overlapping atoms before motion!", pos));
        }

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
        self.process_inputs();
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
        //Nothing is on its way through a conduit between steps
        self.atoms.drop_lone_links();
        self.timestep += 1;
        Ok(())
    }

    pub fn is_complete(&self) -> bool{
        let mut all_outputs_full = true;
        for g in &self.glyphs {
            if let GlyphType::Output(_, i,_) = g.glyph_type {
                all_outputs_full &= i == 0;
            } else if let GlyphType::OutputRepeating(_, i,_) = g.glyph_type {
                all_outputs_full &= i == 0;
            }
        }
        all_outputs_full
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_name_their_hex_without_world_coordinates() {
        let error = |location| SimError{error_str: "atom/atom collision!", location}.to_string();
        let center = pos_to_xy(Pos::new(3, 2));
        assert_eq!(error(center), "atom/atom collision! at (3, 2)");
        assert_eq!(error(center + XYVec::new(0.6, -0.3)), "atom/atom collision! near (3, 2)");
    }
}
