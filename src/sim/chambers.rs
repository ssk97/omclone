//Production puzzles' chambers. Their walls count as area, and block atoms and grabbers (see collision.rs). Parts
//belong inside them, which the sim doesn't enforce but the editor does.
use super::*;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ChamberShape {
    Small, SmallWide, SmallWider, Medium, MediumWide, Large,
}
impl ChamberShape {
    //The name a puzzle file uses
    pub fn from_name(name: &str) -> Option<Self> {
        use ChamberShape::*;
        Some(match name {
            "Small" => Small,
            "SmallWide" => SmallWide,
            "SmallWider" => SmallWider,
            "Medium" => Medium,
            "MediumWide" => MediumWide,
            "Large" => Large,
            _ => return None,
        })
    }
    //The inside is every hex within `radius` of the line from (0,0) to (width,0), and the walls are the hexes one
    //further out. These give the same hexes as omsim's tables (from Syx).
    fn radius_and_width(self) -> (i32, i32) {
        use ChamberShape::*;
        match self {
            Small => (1, 0),
            SmallWide => (1, 1),
            SmallWider => (1, 2),
            Medium => (2, 0),
            MediumWide => (2, 1),
            Large => (3, 0),
        }
    }
}

//What a hex is part of. Inside holds the chamber's index in the puzzle.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ChamberCell {
    Outside,
    Inside(usize),
    Wall,
}

#[derive(Debug, Clone, Default)]
pub struct Chambers {
    //Each wall hex once, though chambers next to each other share some
    pub walls: Vec<Pos>,
    //The hexes inside a chamber or in a wall; every other hex is outside
    cells: FxHashMap<Pos, ChamberCell>,
    //Where each conduit pair's two ends start, by conduit id
    conduits: FxHashMap<ConduitId, (ChamberCell, ChamberCell)>,
    //Whether inputs and outputs must be in different chambers
    isolate: bool,
}
impl Chambers {
    //`conduit_starts` are the puzzle's conduits in order, whose ids start at 100 (as omsim assumes)
    pub fn new(chambers: &[(Pos, ChamberShape)], conduit_starts: &[(Pos, Pos)], isolate: bool) -> Self {
        let mut cells = FxHashMap::default();
        for (i, &(pos, shape)) in chambers.iter().enumerate() {
            let (radius, width) = shape.radius_and_width();
            let reach = radius + 1;
            for x in -reach..=width + reach {
                for y in -reach..=reach {
                    let hex = Pos::new(x, y);
                    let distance = (0..=width).map(|w| hex_distance(hex, Pos::new(w, 0))).min().unwrap();
                    let cell = match distance {
                        d if d <= radius => ChamberCell::Inside(i),
                        d if d == reach => ChamberCell::Wall,
                        _ => continue,
                    };
                    //A hex that's in one chamber's wall is a wall even if it's inside another (no game puzzle has that)
                    let entry = cells.entry(pos + hex).or_insert(cell);
                    if cell == ChamberCell::Wall {
                        *entry = cell;
                    }
                }
            }
        }
        let mut walls: Vec<Pos> = cells.iter().filter(|(_, &cell)| cell == ChamberCell::Wall).map(|(&hex, _)| hex).collect();
        walls.sort_by_key(|hex| (hex.x, hex.y));
        let mut chambers = Chambers { walls, cells, conduits: Default::default(), isolate };
        for (i, &(a, b)) in conduit_starts.iter().enumerate() {
            let cells = (chambers.cell(a), chambers.cell(b));
            chambers.conduits.insert(100 + i as ConduitId, cells);
        }
        chambers
    }
    pub fn cell(&self, hex: Pos) -> ChamberCell {
        self.cells.get(&hex).copied().unwrap_or(ChamberCell::Outside)
    }
    //Other puzzles have no chambers, and no chamber rules
    pub fn is_production(&self) -> bool {
        !self.cells.is_empty()
    }
}

//One of a production puzzle's conduit pairs: where its two ends start, and the hexes each end has (relative to it,
//as in a solution file)
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PuzzleConduit {
    pub start_a: Pos,
    pub start_b: Pos,
    pub hexes: Vec<Pos>,
}

bitflags! {
    //The ways omsim finds a solution breaking a production puzzle's rules
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct CabinetViolations: u32 {
        const CONDUIT_ALTERED              = 1 << 0;
        const CONDUIT_IN_WRONG_CABINET     = 1 << 1;
        const GLYPH_OUTSIDE_CABINET        = 1 << 2;
        const ARM_OUTSIDE_CABINET          = 1 << 3;
        const ARM_REACHES_ACROSS_WALL      = 1 << 4;
        const TRACK_OUTSIDE_CABINET        = 1 << 5;
        const INPUT_OUTPUT_OUTSIDE_CABINET = 1 << 6;
        const ISOLATION_VIOLATED           = 1 << 7;
        //Conduits in a puzzle that isn't a production puzzle
        const CONDUIT_IN_FREESPACE         = 1 << 8;
    }
}

//The production puzzle rules about where parts go
impl World {
    //The hexes of `hexes` that aren't inside a chamber, in a production puzzle
    pub fn outside_chambers(&self, hexes: &[Pos]) -> SmallVec<[Pos; 8]> {
        if !self.chambers.is_production() {
            return SmallVec::new();
        }
        hexes.iter().copied().filter(|&hex| !matches!(self.chambers.cell(hex), ChamberCell::Inside(_))).collect()
    }
    //The hexes where a part is somewhere a production puzzle doesn't allow: a glyph must be inside the chambers, an
    //arm's base and grabbers (as it starts) inside one chamber, and a conduit pair needs an end in each of the
    //chambers the puzzle starts them in
    pub fn chamber_conflicts(&self, part: PartId) -> SmallVec<[Pos; 8]> {
        if !self.chambers.is_production() {
            return SmallVec::new();
        }
        match part {
            PartId::Glyph(i) => {
                let footprint = self.glyph_footprint(i);
                if self.conduit_in_wrong_chamber(i) {
                    return footprint;
                }
                self.outside_chambers(&footprint)
            }
            PartId::Arm(i) => {
                let arm = &self.arms[i];
                let home = self.chambers.cell(arm.pos);
                let grabbers = (0..6).step_by(Arm::angles_between_arm(arm.arm_type) as usize)
                    .map(|r| arm.pos + rot_to_pos(arm.rot + r) * arm.len);
                std::iter::once(arm.pos).chain(grabbers)
                    .filter(|&hex| !matches!(home, ChamberCell::Inside(_)) || self.chambers.cell(hex) != home)
                    .collect()
            }
        }
    }
    //Whether glyph `i` is a conduit that isn't in the chamber its pair leaves for it: the one its other end isn't in
    fn conduit_in_wrong_chamber(&self, i: usize) -> bool {
        let GlyphType::Conduit(_, id) = &self.glyphs[i].glyph_type else {return false};
        let (Some(&(a, b)), Some(pair)) = (self.chambers.conduits.get(id), self.conduit_pairs.get(id)) else {return false};
        let other = if pair.vecids.0 == i {pair.vecids.1} else {pair.vecids.0};
        let here = self.chambers.cell(self.glyphs[i].pos);
        let there = self.chambers.cell(self.glyphs[other].pos);
        if there == a {
            here != b
        } else if there == b {
            here != a
        } else {
            here != a && here != b
        }
    }
    //Every hex of an input or output that shares a chamber with the other kind, if the puzzle keeps them apart.
    //Each is in the chamber of its first atom.
    pub fn isolation_conflicts(&self, part: PartId) -> SmallVec<[Pos; 8]> {
        let PartId::Glyph(i) = part else {return SmallVec::new()};
        let Some((is_input, Some(chamber))) = self.chambers.isolate.then(|| self.io_chamber(i)).flatten() else {
            return SmallVec::new();
        };
        let shared = (0..self.glyphs.len()).any(|j| self.io_chamber(j) == Some((!is_input, Some(chamber))));
        if shared {self.glyph_footprint(i)} else {SmallVec::new()}
    }
    //For an input or output: whether it's an input, and the chamber its first atom is inside (if any)
    fn io_chamber(&self, i: usize) -> Option<(bool, Option<usize>)> {
        use GlyphType::*;
        let is_input = match self.glyphs[i].glyph_type {
            Input(..) => true,
            Output(..) | OutputRepeating(..) => false,
            _ => return None,
        };
        let chamber = match self.chambers.cell(*self.glyph_footprint(i).first()?) {
            ChamberCell::Inside(c) => Some(c),
            _ => None,
        };
        Some((is_input, chamber))
    }
    //Whether any part breaks the chamber rules (the solution was made outside the game, or with overlap allowed)
    pub fn breaks_chamber_rules(&self) -> bool {
        self.all_parts().any(|part| !self.chamber_conflicts(part).is_empty() || !self.isolation_conflicts(part).is_empty())
    }
    //omsim's check of the production rules (decode.c's check_production_constraints), for its "cabinet violations"
    //and "conduit violations" metrics, including where it stops early. Unlike chamber_conflicts, it also checks that
    //conduits keep the puzzle's shape. Call it on a world as set up, before any step.
    pub fn cabinet_violations(&self, puzzle_conduits: &[PuzzleConduit]) -> CabinetViolations {
        use CabinetViolations as V;
        use GlyphType::*;
        let mut found = V::empty();
        //Paired up as omsim pairs them: sorted by id, then by order in the file
        let mut conduits: Vec<(ConduitId, usize)> = self.glyphs.iter().enumerate().filter_map(|(i, g)| match &g.glyph_type {
            Conduit(_, id) => Some((*id, i)),
            _ => None,
        }).collect();
        conduits.sort();
        if !self.chambers.is_production() {
            if !conduits.is_empty() {
                found |= V::CONDUIT_IN_FREESPACE;
            }
            return found;
        }
        let cell = |hex: Pos| self.chambers.cell(hex);
        let inside = |hex: Pos| matches!(cell(hex), ChamberCell::Inside(_));

        if conduits.len() != 2 * puzzle_conduits.len() {
            found |= V::CONDUIT_ALTERED;
        } else {
            //Whether this pair's first conduit is in the chamber the puzzle starts its second end in
            let mut swapped = false;
            for (i, &(id, glyph)) in conduits.iter().enumerate() {
                //Conduit ids start at 100
                let puzzle_conduit = usize::try_from(id as i64 - 100).ok().and_then(|index| puzzle_conduits.get(index));
                let (Some(puzzle_conduit), Conduit(hexes, _)) = (puzzle_conduit, &self.glyphs[glyph].glyph_type) else {
                    found |= V::CONDUIT_ALTERED;
                    break;
                };
                if hexes.len() != puzzle_conduit.hexes.len() {
                    found |= V::CONDUIT_ALTERED;
                    break;
                }
                let expected = cell(if i % 2 == 0 {puzzle_conduit.start_a} else {puzzle_conduit.start_b});
                if i % 2 == 0 {
                    swapped = cell(self.glyphs[glyph].pos) != expected;
                }
                let checked = match self.conduit_pairs.get(&id) {
                    Some(pair) if swapped => if pair.vecids.0 == glyph {pair.vecids.1} else {pair.vecids.0},
                    _ => glyph,
                };
                if cell(self.glyphs[checked].pos) != expected {
                    found |= V::CONDUIT_IN_WRONG_CABINET;
                    break;
                }
                if *hexes != puzzle_conduit.hexes {
                    found |= V::CONDUIT_ALTERED;
                }
            }
        }

        for (i, glyph) in self.glyphs.iter().enumerate() {
            if matches!(glyph.glyph_type, Conduit(..) | Track(_) | Input(..) | Output(..) | OutputRepeating(..)) {
                continue;
            }
            if !self.glyph_footprint(i).iter().all(|&hex| inside(hex)) {
                found |= V::GLYPH_OUTSIDE_CABINET;
            }
        }

        for arm in &self.arms {
            let home = cell(arm.pos);
            if !inside(arm.pos) {
                found |= V::ARM_OUTSIDE_CABINET;
                break;
            }
            let grabbers = (0..6).step_by(Arm::angles_between_arm(arm.arm_type) as usize)
                .map(|r| arm.pos + rot_to_pos(arm.rot + r) * arm.len);
            for hex in grabbers {
                if !inside(hex) {
                    found |= V::ARM_OUTSIDE_CABINET;
                    break;
                } else if cell(hex) != home {
                    found |= V::ARM_REACHES_ACROSS_WALL;
                    break;
                }
            }
        }

        let mut track = self.glyphs.iter().filter(|g| g.is_track()).flat_map(|g| g.footprint(None));
        if !track.all(inside) {
            found |= V::TRACK_OUTSIDE_CABINET;
        }

        let io: Vec<(usize, bool)> = self.glyphs.iter().enumerate().filter_map(|(i, g)| match g.glyph_type {
            Input(..) => Some((i, true)),
            Output(..) | OutputRepeating(..) => Some((i, false)),
            _ => None,
        }).collect();
        for &(i, _) in &io {
            if !self.glyph_footprint(i).iter().all(|&hex| inside(hex)) {
                found |= V::INPUT_OUTPUT_OUTSIDE_CABINET;
            }
        }
        if self.chambers.isolate {
            //Each input or output is where its first atom is
            let mut kinds: Vec<(ChamberCell, bool)> = Vec::new();
            for &(i, is_input) in &io {
                let Some(&hex) = self.glyph_footprint(i).first() else {continue};
                let here = cell(hex);
                if kinds.iter().any(|&(c, kind)| c == here && kind != is_input) {
                    found |= V::ISOLATION_VIOLATED;
                    break;
                }
                kinds.push((here, is_input));
            }
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ChamberShape::*;

    fn chambers(list: &[((i32, i32), ChamberShape)]) -> Chambers {
        let list: Vec<(Pos, ChamberShape)> = list.iter().map(|&((x, y), shape)| (Pos::new(x, y), shape)).collect();
        Chambers::new(&list, &[], false)
    }
    fn world(glyphs: Vec<Glyph>, arms: Vec<Arm>, chambers: Chambers) -> World {
        World::setup_sim(&InitialWorld { glyphs, arms, chambers }).unwrap()
    }
    fn glyph(x: i32, y: i32, glyph_type: GlyphType) -> Glyph {
        Glyph { pos: Pos::new(x, y), rot: 0, glyph_type }
    }
    fn arm(x: i32, y: i32, len: i32, arm_type: ArmType, instructions: Vec<Instr>) -> Arm {
        Arm::new(Pos::new(x, y), 0, len, arm_type, Tape::new(0, instructions))
    }
    //Atoms in a row to the right of (0,0), each bonded to the next
    fn row(length: i32) -> Vec<Atom> {
        let mut atoms: Vec<Atom> = (0..length).map(|x| Atom::new(Pos::new(x, 0), AtomType::Salt)).collect();
        for x in 1..atoms.len() {
            atoms[x - 1].connections[0] = Bonds::NORMAL;
            atoms[x].connections[3] = Bonds::NORMAL;
        }
        atoms
    }
    //Runs whole steps with collision checks on
    fn run(world: &mut World, steps: usize) -> SimResult<()> {
        let mut motion = WorldStepInfo::new();
        for _ in 0..steps {
            world.run_step(true, &mut motion)?;
        }
        Ok(())
    }

    #[test]
    fn shared_walls_count_as_area_once() {
        let w = world(vec![], vec![], chambers(&[((-2, 0), Small), ((2, 0), Small)]));
        //12 wall hexes each, sharing (0,0)
        assert_eq!(w.get_stats().area, 23);
    }

    #[test]
    fn atoms_carried_into_a_wall_collide() {
        //The arm moves the row one hex right, taking its end from (2,0) into the wall at (3,0)
        let track = glyph(-1, 0, GlyphType::Track(vec![Pos::new(0, 0), Pos::new(1, 0)]));
        let input = glyph(0, 0, GlyphType::Input(smallvec![row(3)], 0));
        let mover = arm(-1, 0, 1, ArmType::PlainArm, vec![Instr::Grab, Instr::Forward]);
        let mut w = world(vec![track, input], vec![mover], chambers(&[((0, 0), Medium)]));
        let err = run(&mut w, 2).expect_err("expected a collision");
        assert_eq!((err.error_str, xy_to_pos(err.location)), ("atom/wall collision!", Pos::new(3, 0)));
    }

    #[test]
    fn a_piston_cant_extend_into_a_wall() {
        let piston = arm(0, 0, 1, ArmType::Piston, vec![Instr::Extend]);
        let mut w = world(vec![], vec![piston], chambers(&[((0, 0), Small)]));
        assert_eq!(run(&mut w, 1).expect_err("expected a collision").error_str, "Grabber hit a chamber wall!");
    }

    #[test]
    fn only_a_long_grabber_bulges_into_a_wall_as_it_turns() {
        let turn = |len: i32| {
            let turner = arm(0, 0, len, ArmType::PlainArm, vec![Instr::RotateCounterClockwise]);
            let mut w = world(vec![], vec![turner], chambers(&[((0, 0), Large)]));
            run(&mut w, 1)
        };
        //From (3,0) to (0,3), the grabber's arc passes over (2,2), one hex further out
        let err = turn(3).expect_err("expected a collision");
        assert_eq!((err.error_str, xy_to_pos(err.location)), ("Grabber hit a chamber wall!", Pos::new(2, 2)));
        turn(2).unwrap();
    }

    #[test]
    fn a_grabber_starting_on_a_wall_fails_though_nothing_moves() {
        let reacher = arm(0, 0, 2, ArmType::PlainArm, vec![]);
        let mut w = world(vec![], vec![reacher], chambers(&[((0, 0), Small)]));
        assert_eq!(run(&mut w, 1).expect_err("expected a collision").error_str, "Grabber hit a chamber wall!");
    }

    #[test]
    fn parts_must_stay_in_their_chambers() {
        let two = || chambers(&[((0, 0), Small), ((4, 0), Small)]);
        let conflicts = |w: &World, part: PartId| w.chamber_conflicts(part).to_vec();
        let bonder = glyph(1, 0, GlyphType::Bonding);
        let reacher = arm(1, 0, 2, ArmType::PlainArm, vec![]);

        let inside = world(vec![glyph(0, 0, GlyphType::Bonding)], vec![arm(-1, 0, 1, ArmType::PlainArm, vec![])], two());
        assert!(!inside.breaks_chamber_rules());

        let w = world(vec![bonder.clone()], vec![reacher.clone()], two());
        assert_eq!(conflicts(&w, PartId::Glyph(0)), [Pos::new(2, 0)], "half on the wall");
        assert_eq!(conflicts(&w, PartId::Arm(0)), [Pos::new(3, 0)], "the grabber reaches into the other chamber");
        assert!(w.breaks_chamber_rules());

        let anywhere = world(vec![bonder], vec![reacher], Chambers::default());
        assert!(conflicts(&anywhere, PartId::Glyph(0)).is_empty() && conflicts(&anywhere, PartId::Arm(0)).is_empty());
        assert!(!anywhere.breaks_chamber_rules());
    }

    #[test]
    fn a_conduit_pair_has_an_end_in_each_chamber() {
        let chambers = || Chambers::new(&[(Pos::new(0, 0), Small), (Pos::new(4, 0), Small)],
            &[(Pos::new(0, 0), Pos::new(4, 0))], false);
        let conduit = |x: i32, y: i32| glyph(x, y, GlyphType::Conduit(vec![Pos::new(0, 0)], 100));
        let conflicts = |ends: [(i32, i32); 2]| {
            let w = world(vec![conduit(ends[0].0, ends[0].1), conduit(ends[1].0, ends[1].1)], vec![], chambers());
            (w.chamber_conflicts(PartId::Glyph(0)).to_vec(), w.chamber_conflicts(PartId::Glyph(1)).to_vec())
        };
        assert_eq!(conflicts([(0, 0), (4, 0)]), (vec![], vec![]));
        assert_eq!(conflicts([(4, 0), (0, 0)]), (vec![], vec![]), "either end can be in either chamber");
        assert_eq!(conflicts([(0, 0), (0, 1)]), (vec![Pos::new(0, 0)], vec![Pos::new(0, 1)]), "both in one chamber");
    }

    #[test]
    fn isolated_inputs_and_outputs_cant_share_a_chamber() {
        let input = glyph(0, 0, GlyphType::Input(smallvec![row(1)], 0));
        let output = |x: i32| glyph(x, 1, GlyphType::Output(smallvec![row(1)], 6, 0));
        let conflicts = |output_x: i32, isolate: bool| {
            let chambers = Chambers::new(&[(Pos::new(0, 0), Small), (Pos::new(4, 0), Small)], &[], isolate);
            let w = world(vec![input.clone(), output(output_x)], vec![], chambers);
            (w.isolation_conflicts(PartId::Glyph(0)).to_vec(), w.isolation_conflicts(PartId::Glyph(1)).to_vec())
        };
        assert_eq!(conflicts(0, true), (vec![Pos::new(0, 0)], vec![Pos::new(0, 1)]));
        assert_eq!(conflicts(3, true), (vec![], vec![]), "in the other chamber");
        assert_eq!(conflicts(0, false), (vec![], vec![]), "the puzzle doesn't keep them apart");
    }
}
