//Atoms, the bonds between them, and the molecules they form
use super::*;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    //Compares everything except molecule membership, against a pattern atom placed by the glyph
    pub fn matches_pattern(&self, pattern: &Atom, glyph: &Glyph) -> bool {
        self.pos == glyph.reposition(pattern.pos) && self.atom_type == pattern.atom_type
            && self.immutable_wheel == pattern.immutable_wheel
            && (0..6).all(|i| self.connections[normalize_dir(i + glyph.rot) as usize] == pattern.connections[i as usize])
    }
    pub fn rotate_connections(&mut self, r: Rot) {
        let mut new_connections = [Bonds::NO_BOND; 6];
        for i in 0..6 {
            new_connections[normalize_dir(i + r) as usize] = self.connections[i as usize];
        }
        self.connections = new_connections;
    }
}

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
    pub(super) fn new() -> WorldAtoms {
        WorldAtoms {
            atom_map: SlotMap::with_key(),
            locs: Default::default(),
            molecules: SlotMap::with_key(),
        }
    }
    pub(super) fn get_atom_mut(&mut self, loc: Pos) -> Option<&mut Atom> {
        let &key = self.locs.get(&loc)?;
        Some(&mut self.atom_map[key])
    }
    pub(super) fn get_type(&self, loc: Pos) -> Option<AtomType> {
        let &key = self.locs.get(&loc)?;
        let atom = self.atom_map.get(key).expect("Inconsistent atoms (type check)");
        Some(atom.atom_type)
    }
    //Like get_type, but wheel atoms are invisible (as they are to most glyphs)
    pub(super) fn get_nonwheel_type(&self, loc: Pos) -> Option<AtomType> {
        let &key = self.locs.get(&loc)?;
        let atom = self.atom_map.get(key).expect("Inconsistent atoms (nonwheel type check)");
        if atom.immutable_wheel {return None};
        Some(self.atom_map.get(key)?.atom_type)
    }
    pub fn molecule_len(&self, key: AtomKey) -> usize {
        self.molecules[self.atom_map[key].molecule].atoms.len()
    }
    //Consumable atoms are unbonded single-atom molecules
    pub(super) fn is_lone_atom(&self, key: AtomKey) -> bool {
        let atom = self.atom_map.get(key).expect("Inconsistent atoms (consume check)");
        atom.connections == [Bonds::NO_BOND;6] && self.molecule_len(key) == 1
    }
    pub(super) fn get_consumable_type(&self, motion: &WorldStepInfo, loc: Pos) -> Option<AtomType> {
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
    pub(super) fn get_consumable_type_immediate(&self, motion: &WorldStepInfo, loc: Pos) -> Option<AtomType> {
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
    pub(super) fn check_empty(&self, motion: &WorldStepInfo, loc: Pos) -> bool {
        if self.locs.contains_key(&loc) {
            return false;
        }
        return !motion.spawning_atoms.iter().flatten().any(|x| x.pos == loc);
    }

    //Removes the atom from the board and from its molecule (deleting the molecule if now empty)
    pub(super) fn remove_atom(&mut self, key: AtomKey) -> Atom {
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
    pub(super) fn destroy_atom_at(&mut self, loc: Pos) {
        let atom_key = *self.locs.get(&loc).expect("Destroying nonatom!");
        self.remove_atom(atom_key);
    }
    //Removes every atom of the molecule from the board, returning them
    pub(super) fn take_molecule(&mut self, molecule_key: MoleculeKey) -> SpawnMolecule {
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
    pub(super) fn create_molecule(&mut self, atoms: impl IntoIterator<Item=Atom>) -> SimResult<MoleculeKey> {
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
    pub(super) fn merge_molecules(&mut self, key1: AtomKey, key2: AtomKey) {
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
    pub(super) fn split_molecule(&mut self, molecule_key: MoleculeKey) {
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
