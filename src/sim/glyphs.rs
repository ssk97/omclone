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
    pub(super) fn process_inputs(&mut self) {
        use GlyphType::*;
        //Every input checks for atoms before any spawns, so overlapping inputs all spawn. Atoms waiting to be
        //produced don't block them.
        let unblocked: SmallVec<[usize; 8]> = self.glyphs.iter().enumerate().filter(|(_, glyph)| match &glyph.glyph_type {
            Input(meta_pattern, _) => !meta_pattern[0].iter().any(|a| self.atoms.is_occupied(glyph.reposition(a.pos))),
            _ => false,
        }).map(|(id, _)| id).collect();
        for id in unblocked {
            let glyph = &self.glyphs[id];
            let Input(meta_pattern, _) = &glyph.glyph_type else {unreachable!()};
            //The whole input is one molecule, even if it's disjoint
            self.atoms.create_molecule(meta_pattern[0].iter().map(|a| {
                let mut atom = glyph.place_atom(a);
                //The pattern's bonds are in the input's own frame
                atom.disjoint_safe = !a.has_rightward_bonds();
                atom
            }));
            if let Input(meta_pattern, _) = &mut self.glyphs[id].glyph_type {
                let tmp = meta_pattern.remove(0);
                meta_pattern.push(tmp);
            }
        }
    }
    //Returns the molecule if it exactly matches the pattern (same atoms and bonds, nothing extra)
    //and none of its atoms are grabbed. Separate molecules can't stand in for one disjoint molecule,
    //and a disjoint molecule can't stand in for separate ones.
    //The molecule is the one visible on the hex of the pattern's origin atom, or of its first atom if the origin has
    //none (like omsim's center_atom_index). Its atoms hidden under others still count.
    fn match_output(atoms: &WorldAtoms, motion: &WorldStepInfo, glyph: &Glyph, pattern: &[Atom]) -> Option<MoleculeKey> {
        let center = pattern.iter().find(|a| a.pos == Pos::zeros()).or(pattern.first())?;
        let molecule_key = atoms.atom_map[atoms.visible(glyph.reposition(center.pos))?].molecule;
        let molecule = &atoms.molecules[molecule_key].atoms;
        let matches = molecule.len() == pattern.len() && molecule.iter().all(|&key| {
            !motion.atoms.contains_key(key) && pattern.iter().any(|a| atoms.matches_pattern(key, a, glyph))
        });
        matches.then_some(molecule_key)
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
            if let (Some(key1), Some(key2)) = (atoms.visible(*loc1), atoms.visible(*loc2)) {
                if need_fire && [key1, key2].iter().any(|&key| atoms.type_of(&atoms.atom_map[key]) != AtomType::Fire) {return;}
                let [atom1,atom2] = atoms.atom_map.get_disjoint_mut([key1,key2]).expect("Inconsistent atoms!");
                if (atom1.immutable_wheel || atom2.immutable_wheel) {return;}
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
        //Files a single-atom product, to be created at this glyph's turn in the second half
        fn produce(motion: &mut WorldStepInfo, glyph: usize, atom: Atom) {
            motion.products.push_back(Product { glyph, atoms: smallvec![atom], unbonded: false });
        }
        use AtomType::*;
        use GlyphType::*;
        for (id, glyph) in self.glyphs.iter_mut().enumerate() {
            let atoms = &mut self.atoms;
            if !first_half {
                while let Some(product) = motion.products.pop_front_if(|p| p.glyph == id) {
                    let molecule = atoms.create_molecule(product.atoms);
                    if product.unbonded {
                        motion.recent_bonds.extend(atoms.molecules[molecule].atoms.iter().copied());
                    }
                }
            }
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
            match &mut glyph.glyph_type{
                Calcification => {
                    if let Some(element) = atoms.get_nonwheel_type(pos) {
                        if element.is_element() {
                            atoms.set_type(pos, Salt);
                        }
                    }
                }
                Animismus => {
                    if first_half{
                        let a1 = atoms.get_consumable_type(motion, pos);
                        let a2 = atoms.get_consumable_type(motion, pos_bi);
                        let o1 = atoms.can_produce_at(pos_tri);
                        let o2 = atoms.can_produce_at(pos_ani);
                        if (Some(Salt),Some(Salt),true,true) == (a1,a2,o1,o2) {
                            atoms.destroy_atom_at(pos);
                            atoms.destroy_atom_at(pos_bi);
                            produce(motion, id, Atom::new(pos_tri, Vitae));
                            produce(motion, id, Atom::new(pos_ani, Mors));
                        }
                    }
                }
                Projection => {
                    let qs = atoms.get_consumable_type(motion, pos);
                    let metal = atoms.get_nonwheel_type(pos_bi);
                    if let (Some(Quicksilver), Some(metal)) = (qs, metal){
                        if let Some(newtype) = metal.metal_id().and_then(|x:u8| AtomType::id_to_metal(x + 1)){
                            atoms.destroy_atom_at(pos);
                            atoms.set_type(pos_bi, newtype);
                        }
                    }
                }
                Rejection => {
                    let metal = atoms.get_nonwheel_type(pos);
                    let open = atoms.can_produce_at(pos_bi);
                    if let (true, Some(metal)) = (open, metal){
                        if let Some(newtype) = metal.metal_id().and_then(|x:u8| AtomType::id_to_metal(x - 1)){
                            atoms.set_type(pos, newtype);
                            atoms.create_molecule([Atom::new(pos_bi, Quicksilver)]);
                        }
                    }
                }
                Dispersion => {
                    if first_half{
                        let q = atoms.get_consumable_type(motion, pos);
                        let o1 = atoms.can_produce_at(pos_bi);
                        let o2 = atoms.can_produce_at(pos_disp2);
                        let o3 = atoms.can_produce_at(pos_disp3);
                        let o4 = atoms.can_produce_at(pos_disp4);
                        if (Some(Quintessence),true,true,true,true) == (q,o1,o2,o3,o4){
                            atoms.destroy_atom_at(pos);
                            produce(motion, id, Atom::new(pos_bi, Earth));
                            produce(motion, id, Atom::new(pos_disp2, Water));
                            produce(motion, id, Atom::new(pos_disp3, Fire));
                            produce(motion, id, Atom::new(pos_disp4, Air));
                        }
                    }
                }
                Unification => {
                    if first_half{
                        let output = atoms.can_produce_at(pos);
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
                                produce(motion, id, Atom::new(pos, Quintessence));
                            }
                        }
                    }
                }
                Purification => {
                    if first_half{
                        let a1 = atoms.get_consumable_type(motion, pos);
                        let a2 = atoms.get_consumable_type(motion, pos_bi);
                        let o = atoms.can_produce_at(pos_tri);
                        match (a1,a2,o){
                            (Some(a1),Some(a2),true) if a1 == a2 => {
                                let next = a1.metal_id().and_then(|x:u8| AtomType::id_to_metal(x + 1));
                                if let Some(newtype) = next {
                                    atoms.destroy_atom_at(pos);
                                    atoms.destroy_atom_at(pos_bi);
                                    produce(motion, id, Atom::new(pos_tri, newtype));
                                }
                            }
                            _ => ()
                        }
                    }
                }
                Division => {
                    if first_half{
                        let a = atoms.get_consumable_type(motion, pos);
                        let o1 = atoms.can_produce_at(pos_bi);
                        let o2 = atoms.can_produce_at(pos_div);
                        if let (Some(a),true,true) = (a,o1,o2) {
                            let next1 = a.metal_id().and_then(|x:u8| AtomType::id_to_metal(x/2));
                            let next2 = a.metal_id().and_then(|x:u8| AtomType::id_to_metal((x+1)/2));
                            if let (Some(next1), Some(next2)) = (next1, next2) {
                                atoms.destroy_atom_at(pos);
                                produce(motion, id, Atom::new(pos_bi, next1));
                                produce(motion, id, Atom::new(pos_div, next2));
                            }
                        }
                    }
                }
                Duplication => {
                    let source = atoms.get_type(pos);
                    let salt = atoms.get_nonwheel_type(pos_bi);
                    if let (Some(elem), Some(Salt)) = (source, salt){
                        if elem.is_element() {
                            atoms.set_type(pos_bi, elem);
                        }
                    }
                }
                Proliferation => {
                    if first_half{
                        //Only metals are copied (wheel atoms included)
                        let m = atoms.get_type(pos_proM).filter(|m| m.metal_id().is_some());
                        let qs = atoms.get_consumable_type(motion, pos_proQS);
                        let o = atoms.can_produce_at(pos_proO);
                        if let (Some(m),Some(Quicksilver),true) = (m,qs,o) {
                            atoms.destroy_atom_at(pos_proQS);
                            produce(motion, id, Atom::new(pos_proO, m));
                        }
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
                    if let (Some(key1), Some(key2)) = (atoms.visible(pos), atoms.visible(pos_bi)){
                        let bond1 = atoms.atom_map[key1].connections[rot];
                        let bond2 = atoms.atom_map[key2].connections[(rot+3)%6];
                        if bond1.intersects(Bonds::DYNAMIC_BOND) && bond2.intersects(Bonds::DYNAMIC_BOND) {
                            atoms.atom_map[key1].connections[rot] = Bonds::NO_BOND;
                            atoms.atom_map[key2].connections[(rot+3)%6] = Bonds::NO_BOND;
                            //The molecule splits at the end of the glyph pass
                            motion.recent_bonds.insert(key1);
                            motion.recent_bonds.insert(key2);
                        }
                    }
                }
                Disposal => {
                    if let Some(_) = atoms.get_consumable_type(motion, pos){
                        atoms.destroy_atom_at(pos);
                    }
                },
                Conduit(_, conduit_id) => {
                    if first_half {
                        let conduit_info = self.conduit_pairs.get(conduit_id).expect("conduit info not found!");
                        while let Some((_, claim)) = motion.conduit_claims.pop_front_if(|(conduit, _)| *conduit == id) {
                            World::send_claim(atoms, motion, claim, id, pos, conduit_info);
                        }
                    }
                },
                Equilibrium | Track(_) | Input(..) | Output(..) | OutputRepeating(..) => {},
            }
        }
        if first_half {
            assert!(motion.conduit_claims.is_empty(), "Conduit claims left over");
            //Conduits file deliveries under their receivers, which can come earlier in glyph order
            motion.products.make_contiguous().sort_by_key(|p| p.glyph);
        } else {
            assert!(motion.products.is_empty(), "Products left over");
        }
        //What a conduit clone left behind splits only now, and its pieces keep their place
        if !motion.clone_leftovers.is_empty() {
            self.atoms.split_in_place(motion.clone_leftovers.drain(..));
        }
        //Molecules unbonded this half split into their pieces only now, after every glyph has seen them whole, and the
        //pieces go on top
        if !motion.recent_bonds.is_empty() {
            let raised = self.atoms.split_and_raise(motion.recent_bonds.iter().copied());
            //omsim's rule for stacked molecules unbonded together, which the game agrees with: it errors when the
            //upper atom's own bonds could get mixed up with the lower one's
            if let Some(pos) = self.atoms.unsafe_overlap(&raised) {
                return Err(sim_error_pos("Disjoint bonds aren't supported!", pos));
            }
        }
        Ok(())
    }
    //Each conduit, in glyph order, claims every just-dropped molecule lying entirely on its hexes with none of its
    //atoms grabbed, unless an earlier conduit claimed it. A claim records the molecule as it is now.
    pub(super) fn claim_conduits(&self, motion: &mut WorldStepInfo) {
        if motion.drop_conduit_check.is_empty() {
            return;
        }
        for (id, glyph) in self.glyphs.iter().enumerate() {
            let GlyphType::Conduit(hexes, _) = &glyph.glyph_type else {continue};
            let hexes: SmallVec<[Pos; 8]> = hexes.iter().map(|&p| glyph.reposition(p)).collect();
            for &hex in &hexes {
                let Some(key) = self.atoms.visible(hex) else {continue};
                if !motion.drop_conduit_check.contains(&key) {continue;}
                let molecule = &self.atoms.molecules[self.atoms.atom_map[key].molecule].atoms;
                let claimable = molecule.iter().all(|&a|
                    !motion.atoms.contains_key(a) && hexes.contains(&self.atoms.atom_map[a].pos));
                if claimable {
                    let copy = molecule.iter().map(|&a| {
                        let mut atom = self.atoms.atom_map[a].clone();
                        atom.molecule = MoleculeKey::null();
                        atom
                    }).collect();
                    for a in molecule {
                        motion.drop_conduit_check.remove(a);
                    }
                    motion.conduit_claims.push_back((id, copy));
                }
            }
        }
    }
    //At the conduit's first-half turn, sends what it claimed to the other conduit of its pair, where it appears at
    //that conduit's turn in the second half. The claimed hexes are the ones the copy's atoms are on.
    fn send_claim(atoms: &mut WorldAtoms, motion: &mut WorldStepInfo, mut claim: SpawnMolecule, sender: usize, origin: Pos, conduit_info: &ConduitInfo) {
        //Something took one of the atoms
        if claim.iter().any(|a| !atoms.is_occupied(a.pos)) {
            return;
        }
        let on_claim: SmallVec<[AtomKey; 8]> = claim.iter().map(|a| atoms.visible(a.pos).unwrap()).collect();
        //If a bonder joined the molecule to an atom outside the claim, the conduit sends a copy of the claim instead.
        //That holds even when an unbonder has removed the bond again, since molecules only split at the end of the
        //glyph pass. Dangling bonds don't join molecules, so they never count.
        let joined_outside = on_claim.iter().any(|&key| atoms.molecules[atoms.atom_map[key].molecule].atoms.iter()
            .any(|&a| !claim.iter().any(|c| c.pos == atoms.atom_map[a].pos)));
        let mut pieces: SmallVec<[(SpawnMolecule, bool); 2]> = SmallVec::new();
        if joined_outside {
            //The atoms left behind lose the bonds the copy has, which can split their molecule at the end of the glyph
            //pass. Splitting it now would hide the join from a later claim on it.
            //Their types are linked to the copy's from now on, so the copy has the type they have now, not the one
            //claimed, and gets any change made while it's on its way (game: exp22a–c).
            for (copied, &key) in claim.iter_mut().zip(&on_claim) {
                atoms.link_copy(key, copied);
                let atom = &mut atoms.atom_map[key];
                for dir in 0..6 {
                    atom.connections[dir].remove(copied.connections[dir]);
                }
            }
            motion.clone_leftovers.extend(on_claim.iter().copied());
            pieces.push((claim, false));
        } else {
            //The atoms there now, molecule by molecule. One unbonded this half hasn't split yet, so it goes whole.
            let mut groups: SmallVec<[(MoleculeKey, SmallVec<[AtomKey; 8]>); 2]> = SmallVec::new();
            for &key in &on_claim {
                let molecule = atoms.atom_map[key].molecule;
                match groups.iter_mut().find(|(m, _)| *m == molecule) {
                    Some((_, keys)) => keys.push(key),
                    None => groups.push((molecule, smallvec![key])),
                }
            }
            for (_, keys) in groups {
                let unbonded = keys.iter().any(|key| motion.recent_bonds.contains(key));
                pieces.push((keys.into_iter().map(|key| atoms.remove_atom(key)).collect(), unbonded));
            }
        }
        let mut offset_pos = conduit_info.offset_pos;
        let mut offset_rot = conduit_info.offset_rot;
        let receiver = if conduit_info.vecids.0 == sender{
            offset_pos *= -1;
            offset_rot = normalize_dir(offset_rot*-1);
            conduit_info.vecids.1
        } else {
            assert!(conduit_info.vecids.1 == sender, "Conduit invalid vec id");
            conduit_info.vecids.0
        };
        for (mut molecule, unbonded) in pieces {
            for atom in &mut molecule {
                //Judged in the frame the atoms leave from
                atom.disjoint_safe |= !atom.has_rightward_bonds();
                atom.pos = rotate_around(atom.pos,offset_rot,origin)+offset_pos;
                atom.rotate_connections(offset_rot);
            }
            motion.products.push_back(Product { glyph: receiver, atoms: molecule, unbonded });
        }
    }
}
