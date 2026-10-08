//Glyphs: what they are, where they sit, and what they do to atoms each half-step
use super::*;

pub type AtomPattern = SmallVec<[Vec<Atom>;1]>;
pub type InOutId = i32;
pub type ConduitId = i32;
//All positions in a GlyphType (patterns, track and conduit hexes) are relative to the glyph,
//as in the solution file. Glyph::reposition/place_atom give their absolute positions.
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
    //A pattern atom moved to its absolute position, with its bonds rotated to match
    pub fn place_atom(&self, rel_atom: &Atom) -> Atom {
        let mut atom = rel_atom.clone();
        atom.pos = self.reposition(rel_atom.pos);
        atom.rotate_connections(self.rot);
        atom
    }
    //The hexes the glyph covers: its area, and what it can overlap.
    //A repeating output covers its displayed polymer, so it needs the polymer data (or covers nothing).
    pub fn footprint(&self, polymer: Option<&Polymer>) -> SmallVec<[Pos; 8]> {
        use GlyphType::*;
        let rel: &[(i32, i32)] = match &self.glyph_type {
            Calcification | Equilibrium => &[(0, 0)],
            Animismus => &[(0, 0), (1, 0), (0, 1), (1, -1)],
            Projection | Bonding | Unbonding | Rejection | Duplication => &[(0, 0), (1, 0)],
            Dispersion => &[(0, 0), (1, 0), (1, -1), (0, -1), (-1, 0)],
            Unification => &[(0, 0), (0, 1), (-1, 1), (0, -1), (1, -1)],
            Purification | TriplexBond => &[(0, 0), (1, 0), (0, 1)],
            MultiBond => &[(0, 0), (1, 0), (0, -1), (-1, 1)],
            Division => &[(0, 0), (1, 0), (-1, 0)],
            Proliferation => &[(0, 0), (1, 0), (0, 1), (1, 1), (-1, 1), (1, -1)],
            Disposal => &[(0, 0), (1, 0), (0, 1), (1, -1), (0, -1), (-1, 0), (-1, 1)],
            Input(meta_pattern, _) | Output(meta_pattern, _, _) => {
                return meta_pattern[0].iter().map(|a| self.reposition(a.pos)).collect();
            }
            OutputRepeating(..) => {
                return polymer.map_or(SmallVec::new(), |p| p.display.iter().map(|a| a.pos).collect());
            }
            Track(hexes) | Conduit(hexes, _) => {
                return hexes.iter().map(|&p| self.reposition(p)).collect();
            }
        };
        rel.iter().map(|&(x, y)| self.reposition(Pos::new(x, y))).collect()
    }
    //Builds a repeating output's polymer data (None for other glyphs)
    pub fn build_polymer(&self) -> Result<Option<Polymer>> {
        let GlyphType::OutputRepeating(meta_pattern, output_count, _) = &self.glyph_type else {return Ok(None)};
        let monomer: Vec<Atom> = meta_pattern[0].iter().map(|a| self.place_atom(a)).collect();
        Ok(Some(Polymer::build(self.pos, &monomer, *output_count / POLYMER_REPETITIONS)?))
    }
    pub fn is_track(&self) -> bool {
        matches!(self.glyph_type, GlyphType::Track(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConduitInfo {
    pub vecids: (usize, usize),
    pub offset_pos: Pos,
    pub offset_rot: Rot, 
}
pub type ConduitPairMap = FxHashMap<i32, ConduitInfo>;

impl World {
    pub(super) fn process_inputs(&mut self, motion: &mut WorldStepInfo) -> SimResult<()>{
        use GlyphType::*;
        for glyph in self.glyphs.iter_mut() {
            let atoms = &mut self.atoms;
            if let Input(meta_pattern,_) = &glyph.glyph_type{
                let atom_spawn_points = &meta_pattern[0];
                if atom_spawn_points.iter().all(|a| atoms.check_empty(motion, glyph.reposition(a.pos))){
                    //The whole input is one molecule, even if it's disjoint
                    atoms.create_molecule(atom_spawn_points.iter().map(|a| glyph.place_atom(a)))?;
                    if let Input(meta_pattern,_) = &mut glyph.glyph_type{
                        let tmp = meta_pattern.remove(0);
                        meta_pattern.push(tmp);
                    }
                }
            }
        }
        Ok(())
    }
    //Returns the molecule if it exactly matches the pattern (same atoms and bonds, nothing extra)
    //and none of its atoms are grabbed. Separate molecules can't stand in for one disjoint molecule,
    //and a disjoint molecule can't stand in for separate ones.
    fn match_output(atoms: &WorldAtoms, motion: &WorldStepInfo, glyph: &Glyph, pattern: &[Atom]) -> Option<MoleculeKey> {
        let &first_key = atoms.locs.get(&glyph.reposition(pattern.first()?.pos))?;
        let molecule_key = atoms.atom_map[first_key].molecule;
        if atoms.molecules[molecule_key].atoms.len() != pattern.len() {
            return None;
        }
        for a in pattern {
            let &atom_key = atoms.locs.get(&glyph.reposition(a.pos))?;
            let atom = &atoms.atom_map[atom_key];
            if atom.molecule != molecule_key || !atom.matches_pattern(a, glyph) || motion.atoms.contains_key(atom_key) {
                return None;
            }
        }
        Some(molecule_key)
    }
    pub(super) fn process_outputs(&mut self, motion: &mut WorldStepInfo){
        use GlyphType::*;
        for (id, glyph) in self.glyphs.iter_mut().enumerate() {
            let atoms = &mut self.atoms;
            if let Output(meta_pattern, ..) = &glyph.glyph_type{
                if let Some(molecule_key) = World::match_output(atoms, motion, glyph, &meta_pattern[0]) {
                    atoms.take_molecule(molecule_key);
                    let Output(meta_pattern, output_count, _) = &mut glyph.glyph_type else {unreachable!()};
                    if *output_count > 0{
                        *output_count -= 1;
                    }
                    let tmp = meta_pattern.remove(0);
                    meta_pattern.push(tmp);
                }
                continue;
            }
            match &mut glyph.glyph_type{
                OutputRepeating(_, output_count,_) => {
                    let polymer = self.polymers.get_mut(&id).expect("Polymer data not found!");
                    polymer.update(atoms);
                    *output_count = polymer.remaining_outputs();
                },
                _ => {}
            }
        }
    }
    pub(super) fn process_glyphs(&mut self, motion: &mut WorldStepInfo, first_half: bool) -> SimResult<()>{
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
                        //Only metals are copied (wheel atoms included)
                        let m = atoms.get_type(pos_proM).filter(|m| m.metal_id().is_some());
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
                        let hexes: SmallVec<[Pos; 8]> = atom_teleport.iter().map(|&p| pos + rotate(p, rot as Rot)).collect();
                        World::conduit_process(atoms, id, &hexes, pos, conduit_info, motion);
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
    fn conduit_process(atoms: &mut WorldAtoms, glyph_id: usize, positions: &[Pos], origin: Pos, conduit_info: &mut ConduitInfo, motion: &mut WorldStepInfo){
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
}
