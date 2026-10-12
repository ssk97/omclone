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
    //Set on an atom an input or conduit creates with no bond pointing right, up-right or down-right in the frame it's
    //created in. omsim's rule (which the game agrees with) lets such an atom sit above another when both their
    //molecules were just unbonded; see process_glyphs.
    pub disjoint_safe: bool,
}
//Directions 0, 1 and 5
const RIGHTWARD: [usize; 3] = [0, 1, 5];
impl Atom {
    pub fn new(pos: Pos, atom_type: AtomType) -> Atom {
        Atom {
            pos,
            atom_type,
            connections: [Bonds::NO_BOND; 6],
            immutable_wheel: false,
            molecule: MoleculeKey::null(),
            disjoint_safe: false,
        }
    }
    pub fn has_rightward_bonds(&self) -> bool {
        RIGHTWARD.iter().any(|&dir| self.connections[dir] != Bonds::NO_BOND)
    }
    //Compares type, bonds and wheel status against a pattern atom placed by the glyph
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
//but it can also be disjoint (e.g. from a disjoint input). Bonding merges molecules. Unbonding recalculates the
//molecule from its bonds, splitting off every disconnected part, but (like the game) only once the half-step's
//glyphs are done.
#[derive(Debug, Clone, Default)]
pub struct Molecule {
    pub atoms: SmallVec<[AtomKey; 8]>,
    //The molecule's place in molecule order, which decides which atom on a hex is visible (see WorldAtoms::visible)
    pub z: u64,
}
//A molecule waiting to be created in the second half of the timestep
pub type SpawnMolecule = SmallVec<[Atom; 1]>;
#[derive(Debug, Clone)]
pub struct WorldAtoms {
    pub atom_map: SlotMap<AtomKey, Atom>,
    //Every atom on each hex, in the order they arrived. Hexes with none have no entry.
    locs: FxHashMap<Pos, SmallVec<[AtomKey; 1]>>,
    //The hexes with several atoms
    stacked: FxHashSet<Pos>,
    pub molecules: SlotMap<MoleculeKey, Molecule>,
    //Above every molecule's z so far
    next_z: u64,
}
impl WorldAtoms {
    pub(super) fn new() -> WorldAtoms {
        WorldAtoms {
            atom_map: SlotMap::with_key(),
            locs: Default::default(),
            stacked: Default::default(),
            molecules: SlotMap::with_key(),
            next_z: 0,
        }
    }
    //The atom glyphs, arms, conduits and outputs see on a hex: the one from the lowest molecule in molecule order,
    //which is the oldest. Newer molecules go on top (and the game draws them over it). Two atoms of one molecule on
    //a hex tie, and the one that arrived first wins.
    pub fn visible(&self, loc: Pos) -> Option<AtomKey> {
        match self.locs.get(&loc)?.as_slice() {
            &[key] => Some(key),
            keys => keys.iter().copied().min_by_key(|&key| self.molecules[self.atom_map[key].molecule].z),
        }
    }
    pub(super) fn is_occupied(&self, loc: Pos) -> bool {
        self.locs.contains_key(&loc)
    }
    //Whether a conversion glyph or Rejection can put an atom here: they treat a wheel atom as empty space
    pub(super) fn can_produce_at(&self, loc: Pos) -> bool {
        self.visible(loc).map_or(true, |key| self.atom_map[key].immutable_wheel)
    }
    pub(super) fn is_stacked(&self, loc: Pos) -> bool {
        self.stacked.contains(&loc)
    }
    //Any hex with several atoms
    pub(super) fn any_stack(&self) -> Option<Pos> {
        self.stacked.iter().next().copied()
    }
    //A hex where an atom of the given molecules sits above another of theirs, and isn't disjoint_safe
    pub(super) fn unsafe_overlap(&self, molecules: &[MoleculeKey]) -> Option<Pos> {
        self.stacked.iter().copied().find(|loc| {
            let mut atoms: SmallVec<[&Atom; 4]> = self.locs[loc].iter().map(|&key| &self.atom_map[key])
                .filter(|atom| molecules.contains(&atom.molecule)).collect();
            atoms.sort_by_key(|atom| self.molecules[atom.molecule].z);
            atoms.iter().skip(1).any(|atom| !atom.disjoint_safe)
        })
    }
    pub(super) fn add_to_hex(&mut self, key: AtomKey, loc: Pos) {
        let keys = self.locs.entry(loc).or_default();
        keys.push(key);
        if keys.len() == 2 {
            self.stacked.insert(loc);
        }
    }
    pub(super) fn remove_from_hex(&mut self, key: AtomKey, loc: Pos) {
        let keys = self.locs.get_mut(&loc).expect("Removing nonatom!");
        let index = keys.iter().position(|&k| k == key).expect("Inconsistent atoms (remove from hex)!");
        //Keeps the arrival order that ties in visible() depend on
        keys.remove(index);
        match keys.len() {
            0 => {self.locs.remove(&loc);}
            1 => {self.stacked.remove(&loc);}
            _ => {}
        }
    }
    //A z above every molecule so far, putting a molecule on top
    fn top_z(&mut self) -> u64 {
        self.next_z += 1;
        self.next_z - 1
    }
    //The molecules of these atoms, each once and in molecule order. Atoms that no longer exist are skipped.
    fn molecules_of(&self, atoms: impl Iterator<Item=AtomKey>) -> SmallVec<[MoleculeKey; 4]> {
        let mut molecules: SmallVec<[MoleculeKey; 4]> = atoms.filter_map(|key| self.atom_map.get(key)).map(|a| a.molecule).collect();
        //Pieces split off by a conduit share a z, so the key breaks ties (and brings duplicates together)
        molecules.sort_by_key(|&m| (self.molecules[m].z, m));
        molecules.dedup();
        molecules
    }
    //Splits the molecules of these atoms into their pieces, which keep the molecule's place in molecule order
    pub(super) fn split_in_place(&mut self, atoms: impl Iterator<Item=AtomKey>) {
        for m in self.molecules_of(atoms) {
            self.split_molecule(m);
        }
    }
    //Splits the molecules of these unbonded atoms into their pieces and puts the pieces on top, keeping the
    //molecules' order. Returns the pieces.
    pub(super) fn split_and_raise(&mut self, atoms: impl Iterator<Item=AtomKey>) -> SmallVec<[MoleculeKey; 4]> {
        let mut pieces = SmallVec::new();
        for m in self.molecules_of(atoms) {
            pieces.extend(self.split_molecule(m));
        }
        for &m in &pieces {
            self.molecules[m].z = self.top_z();
        }
        pieces
    }
    pub(super) fn get_atom_mut(&mut self, loc: Pos) -> Option<&mut Atom> {
        let key = self.visible(loc)?;
        Some(&mut self.atom_map[key])
    }
    pub(super) fn get_type(&self, loc: Pos) -> Option<AtomType> {
        Some(self.atom_map[self.visible(loc)?].atom_type)
    }
    //Like get_type, but wheel atoms are invisible (as they are to most glyphs)
    pub(super) fn get_nonwheel_type(&self, loc: Pos) -> Option<AtomType> {
        let atom = &self.atom_map[self.visible(loc)?];
        if atom.immutable_wheel {return None};
        Some(atom.atom_type)
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
        let key = self.visible(loc)?;
        if motion.atoms.contains_key(key) || motion.recent_bonds.contains(&key) {
            return None;
        }
        if !self.is_lone_atom(key){
            return None
        }
        //Don't need to check is_berlo since the berlo arm will always be grabbing it (applying a motion)
        Some(self.atom_map[key].atom_type)
    }

    //Removes the atom from the board and from its molecule (deleting the molecule if now empty)
    pub(super) fn remove_atom(&mut self, key: AtomKey) -> Atom {
        let mut atom = self.atom_map.remove(key)
            .expect("Inconsistent atoms (remove)!");
        self.remove_from_hex(key, atom.pos);
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
        let atom_key = self.visible(loc).expect("Destroying nonatom!");
        self.remove_atom(atom_key);
    }
    //Removes every atom of the molecule from the board, wherever they are in their stacks, returning them
    pub(super) fn take_molecule(&mut self, molecule_key: MoleculeKey) -> SpawnMolecule {
        let molecule = self.molecules.remove(molecule_key).expect("Inconsistent molecule (take)!");
        let mut taken = SpawnMolecule::new();
        for &key in &molecule.atoms {
            let mut atom = self.atom_map.remove(key).expect("Inconsistent atoms (take)!");
            self.remove_from_hex(key, atom.pos);
            atom.molecule = MoleculeKey::null();
            taken.push(atom);
        }
        taken
    }
    //The new molecule goes on top of any atoms already on its hexes
    pub(super) fn create_molecule(&mut self, atoms: impl IntoIterator<Item=Atom>) -> MoleculeKey {
        let z = self.top_z();
        let molecule_key = self.molecules.insert(Molecule { atoms: SmallVec::new(), z });
        for mut atom in atoms {
            atom.molecule = molecule_key;
            let pos = atom.pos;
            let key = self.atom_map.insert(atom);
            self.molecules[molecule_key].atoms.push(key);
            self.add_to_hex(key, pos);
        }
        molecule_key
    }
    //Bonding directly combines the two molecules. Like the game, the result goes on top at once, so glyphs later
    //in the same half see the new order. A bond within one molecule leaves it where it is.
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
        self.molecules[big].z = self.top_z();
    }
    //Recalculates the molecule from its bonds, separating all disjoint parts, and returns the parts.
    //A bond only connects two atoms if both atoms have it (dangling bonds are ignored).
    //The parts keep the molecule's place in molecule order.
    pub(super) fn split_molecule(&mut self, molecule_key: MoleculeKey) -> SmallVec<[MoleculeKey; 4]> {
        let atoms = std::mem::take(&mut self.molecules[molecule_key].atoms);
        let z = self.molecules[molecule_key].z;
        let mut visited = FxHashSet::<AtomKey>::default();
        let mut stack = Vec::new();
        let mut parts = SmallVec::new();
        for &start in &atoms {
            if !visited.insert(start) {continue;}
            let target = if parts.is_empty() {molecule_key} else {self.molecules.insert(Molecule { atoms: SmallVec::new(), z })};
            parts.push(target);
            stack.push(start);
            while let Some(key) = stack.pop() {
                self.atom_map[key].molecule = target;
                self.molecules[target].atoms.push(key);
                let atom = &self.atom_map[key];
                for dir in 0..6 {
                    if !atom.connections[dir as usize].intersects(Bonds::DYNAMIC_BOND) {continue;}
                    //On a stacked hex, the bond leads to whichever atom there is in the molecule and bonded back
                    let Some(neighbors) = self.locs.get(&(atom.pos + rot_to_pos(dir))) else {continue};
                    for &next_key in neighbors {
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
        parts
    }
}
