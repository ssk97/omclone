//Repeating ("infinite") outputs
use super::*;

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
    //The atoms to display (6 repetitions plus placeholders), which are also the output's area
    pub display: Vec<Atom>,
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
    //Builds the polymer data, including the atoms to display (6 repetitions plus placeholders).
    //`raw` is the monomer in absolute positions.
    //- The marker can be anywhere: each repetition's origin goes to the previous repetition's marker.
    //- Like omsim, a placeholder goes where a bond from the 6th repetition reaches the 7th repetition.
    pub fn build(origin: Pos, raw: &[Atom], outputs_per_repetition: i32) -> Result<Polymer> {
        use AtomType::RepeatingOutputMarker;
        let mut markers = raw.iter().filter(|a| a.pattern_type() == RepeatingOutputMarker);
        let marker = markers.next().ok_or_else(|| eyre!("Repeating atom marker expected but not found!"))?;
        ensure!(markers.next().is_none(), "Multiple repeating atom markers!");
        let monomer: Vec<Atom> = raw.iter().filter(|a| a.pattern_type() != RepeatingOutputMarker).cloned().collect();
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
            outputs_per_repetition, validated: 0, display: Vec::new(),
        };
        polymer.monomer_width = polymer.polymer_pos(marker.pos).x;
        let n = polymer.monomer.len();

        let mut expanded = Vec::with_capacity(n * POLYMER_REPETITIONS as usize);
        for i in 0..POLYMER_REPETITIONS {
            for a in &polymer.monomer {
                let mut repeated = a.clone();
                repeated.pos += offset * i;
                expanded.push(repeated);
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
        polymer.display = display;
        Ok(polymer)
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
    //Atoms beyond either end of a row are allowed garbage. Atoms outside the rows of the polymer aren't (like omsim).
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
            if atoms.type_of(atom) != target.pattern_type() {
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
    pub(super) fn update(&mut self, atoms: &WorldAtoms) {
        let Some(key) = atoms.visible(self.center) else {return};
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
