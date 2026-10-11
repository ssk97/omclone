use crate::sim::*;
use crate::parser::*;
use std::{fs,fs::File, io::BufReader, collections::HashMap, path::Path};

//Set OMCLONE_CHECK_AREA=1 to also check area (much slower, since it enables collision checks)
fn check_area_enabled() -> bool {
    std::env::var("OMCLONE_CHECK_AREA").map_or(false, |v| v != "0" && !v.is_empty())
}

const PUZZLE_DIR:&str = "test/puzzle";
type PuzzleMap = HashMap<String, FullPuzzle>;
fn read_puzzle_recurse(puzzle_map:&mut PuzzleMap, directory: fs::ReadDir){
    for f in directory{
        if let Ok(f) = f{
            let ftype = f.file_type().unwrap();
            if ftype.is_dir(){
                read_puzzle_recurse(puzzle_map,fs::read_dir(f.path()).unwrap());
            } else if ftype.is_file(){
                let f_puzzle = File::open(f.path()).unwrap();
                let fname = f.file_name().into_string().unwrap().replace(".puzzle","");
                if let Ok(puzzle) = parse_puzzle(&mut BufReader::new(f_puzzle)){
                    puzzle_map.insert(fname, puzzle);
                } else {
                    println!("Puzzle failed to load: {:?}",f.path());
                }
            }
        }
    }
}

#[derive(Default, Debug)]
struct TestResults{
    total: usize,
    passed: usize,
    expected_collisions: usize,
    stat_mismatches: usize,
    area_mismatches: usize,
    sim_errors: usize,
    setup_errors: usize,
    panics: usize,
}

//With `rebuild`, the world is set up a second time from its own layout (as part edits do) before running
fn check_solution(results: &mut TestResults, fpath:&Path, puzzle_map: &PuzzleMap, rebuild: bool){
    let check_area = check_area_enabled();
    let f_sol = File::open(fpath).unwrap();
    let sol_maybe = parse_solution(&mut BufReader::new(f_sol));
    let sol = match sol_maybe{
        Err(e) => {println!("Failed to parse solution {:?}: {}",fpath, e); return;}
        Ok(s) => s
    };
    let Some(puzzle) = puzzle_map.get(&sol.puzzle_name) else {
        /*println!("Can't find puzzle {}",sol.puzzle_name);*/ return;
    };
    results.total += 1;
    //Same convention as omsim: unsolved solutions (no stats, or zero cost and instructions) are expected to collide
    let expect_collision = match &sol.stats {
        None => true,
        Some(s) => s.cost == 0 && s.instructions == 0,
    };
    let init = match puzzle_prep(&puzzle, &sol){
        Err(e) => {println!("Failed to prepare {:?}: {}",fpath, e); results.setup_errors += 1; return}
        Ok(s) => s
    };
    let mut world = match World::setup_sim(&init){
        Err(e) => {println!("Failed to setup {:?}: {}",fpath, e); results.setup_errors += 1; return}
        Ok(s) => s
    };
    if rebuild {
        let rebuilt = match World::setup_sim(&world.layout()){
            Err(e) => {println!("Failed to rebuild {:?}: {}",fpath, e); results.setup_errors += 1; return}
            Ok(s) => s
        };
        let same_tapes = world.arms.iter().zip(&rebuilt.arms).all(|(a, b)|
            a.instruction_tape.first == b.instruction_tape.first && a.instruction_tape.instructions == b.instruction_tape.instructions);
        if !same_tapes || world.cost != rebuilt.cost || world.repeat_length != rebuilt.repeat_length
            || world.instruction_count() != rebuilt.instruction_count() || world.area_touched != rebuilt.area_touched {
            println!("Rebuild changed the world: {:?}", fpath);
            results.setup_errors += 1;
            return;
        }
        world = rebuilt;
    }
    //Stop a bit after the expected completion time, since running longer can't produce a match
    let first_timestep = world.timestep;
    let max_timestep = match &sol.stats {
        Some(s) if !expect_collision => first_timestep + (s.cycles.max(0) as u64) + 100,
        _ => first_timestep + 200_000,
    };
    let mut motions = WorldStepInfo::new();
    while !world.is_complete() && world.timestep < max_timestep {
        let step = world.run_step(check_area || expect_collision, &mut motions);
        if let Err(e) = step{
            if expect_collision {
                results.expected_collisions += 1;
                results.passed += 1;
            } else {
                println!("Simulation error on step {} puzzle {}: {:?}: {}",world.timestep,sol.puzzle_name,fpath, e);
                results.sim_errors += 1;
            }
            return;
        }
    }
    if expect_collision {
        println!("Expected collision but none happened: {:?} puzzle {}",fpath,sol.puzzle_name);
        results.sim_errors += 1;
        return;
    }
    let mut newstats = world.get_stats();
    let oldstats = sol.stats.unwrap();
    let area_checked = check_area && !puzzle.production;
    if !area_checked {newstats.area = oldstats.area};
    let stats_ok = newstats.cycles == oldstats.cycles && newstats.cost == oldstats.cost
        && newstats.instructions == oldstats.instructions;
    if !stats_ok {
        results.stat_mismatches += 1;
        println!("Stats don't match! {:?} and puzzle {}\n{:?} vs true {:?}",fpath,sol.puzzle_name,newstats, oldstats);
    } else if newstats.area != oldstats.area {
        results.area_mismatches += 1;
        println!("Area doesn't match! {:?} and puzzle {}: {} vs true {}",fpath,sol.puzzle_name,newstats.area, oldstats.area);
    } else {
        results.passed += 1;
    }
    if results.total % 100 == 0{
        println!("Current progress: {}/{}",results.passed,results.total);
    }
}

#[test]
#[ignore = "Long running test, only run on explicit request"]
fn check_all(){
    sweep(false);
}
//Like check_all, but each world is set up again from its own layout first
#[test]
#[ignore = "Long running test, only run on explicit request"]
fn check_all_rebuilt(){
    sweep(true);
}
fn sweep(rebuild: bool){
    let mut puzzle_map = PuzzleMap::new();
    read_puzzle_recurse(&mut puzzle_map, fs::read_dir(PUZZLE_DIR).unwrap());
    fn read_solution_recurse(results: &mut TestResults, puzzle_map: &PuzzleMap, directory: fs::ReadDir, rebuild: bool){
        for f in directory{
            if let Ok(f) = f{
                let ftype = f.file_type().unwrap();
                if ftype.is_dir(){
                    read_solution_recurse(results, puzzle_map,fs::read_dir(f.path()).unwrap(), rebuild);
                } else if ftype.is_file(){
                    let fpath = f.path();
                    let lname = fpath.to_str().unwrap().to_ascii_lowercase();
                    if lname.contains(".solution"){
                        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(||
                            check_solution(results, &fpath, puzzle_map, rebuild)));
                        if outcome.is_err(){
                            //total was already counted inside check_solution
                            println!("Panic while checking {:?}", fpath);
                            results.panics += 1;
                        }
                    }
                }
            }
        }
    }
    let mut results = TestResults::default();
    read_solution_recurse(&mut results, &puzzle_map, fs::read_dir("test/solution").unwrap(), rebuild);
    read_solution_recurse(&mut results, &puzzle_map, fs::read_dir("test/om-leaderboard-master").unwrap(), rebuild);
    println!("area checked: {}", check_area_enabled());
    println!("{:?}", results);
    println!("final score = {}/{}",results.passed,results.total);
}

//Small hand-built worlds checking the disjoint molecule rules
mod molecule_tests {
    use crate::sim::*;
    use smallvec::smallvec;
    use AtomType::*;
    use Instr::*;

    fn atom(x: i32, y: i32, atom_type: AtomType) -> Atom {
        Atom::new(Pos::new(x, y), atom_type)
    }
    fn bonded(mut atoms: Vec<Atom>, bonds: &[(usize, usize)]) -> Vec<Atom> {
        for &(a, b) in bonds {
            let rot = pos_to_rot(atoms[b].pos - atoms[a].pos).expect("test bond must be adjacent");
            atoms[a].connections[rot as usize] = Bonds::NORMAL;
            atoms[b].connections[normalize_dir(rot + 3) as usize] = Bonds::NORMAL;
        }
        atoms
    }
    fn glyph(x: i32, y: i32, glyph_type: GlyphType) -> Glyph {
        Glyph { pos: Pos::new(x, y), rot: 0, glyph_type }
    }
    fn input(atoms: Vec<Atom>) -> Glyph {
        glyph(0, 0, GlyphType::Input(smallvec![atoms], 0))
    }
    fn output(atoms: Vec<Atom>) -> Glyph {
        glyph(0, 0, GlyphType::Output(smallvec![atoms], 6, 0))
    }
    //Plain arm at (-1,0) pointing at (0,0)
    fn arm(instructions: Vec<Instr>) -> Arm {
        Arm::new(Pos::new(-1, 0), 0, 1, ArmType::PlainArm, Tape::new(0, instructions))
    }
    fn run(glyphs: Vec<Glyph>, arms: Vec<Arm>, steps: usize) -> World {
        let mut world = World::setup_sim(&InitialWorld { glyphs, arms }).unwrap();
        let mut motions = WorldStepInfo::new();
        for _ in 0..steps {
            world.run_step(false, &mut motions).unwrap();
        }
        world
    }
    fn key_at(world: &World, x: i32, y: i32) -> Option<AtomKey> {
        world.atoms.locs.get(&Pos::new(x, y)).copied()
    }
    fn molecule_at(world: &World, x: i32, y: i32) -> MoleculeKey {
        world.atoms.atom_map[key_at(world, x, y).expect("expected an atom")].molecule
    }
    fn output_count(world: &World, index: usize) -> i32 {
        match world.glyphs.iter().filter_map(|g| match g.glyph_type {
            GlyphType::Output(_, count, _) => Some(count),
            _ => None,
        }).nth(index) {
            Some(count) => count,
            None => panic!("no output {index}"),
        }
    }
    //Rotating counterclockwise around (-1,0) sends (x,0) to (-1, x+1)

    #[test]
    fn grabbing_moves_whole_disjoint_molecule() {
        let world = run(vec![input(vec![atom(0, 0, Salt), atom(2, 0, Salt)])],
            vec![arm(vec![Grab, RotateCounterClockwise])], 2);
        assert!(key_at(&world, -1, 1).is_some());
        assert!(key_at(&world, -1, 3).is_some(), "far part of the disjoint molecule should move too");
        assert_eq!(molecule_at(&world, -1, 1), molecule_at(&world, -1, 3));
    }

    #[test]
    fn unbonding_separates_all_disjoint_parts() {
        let parts = bonded(vec![atom(0, 0, Salt), atom(1, 0, Salt), atom(3, 0, Salt)], &[(0, 1)]);
        let world = run(vec![input(parts), glyph(0, 0, GlyphType::Unbonding)],
            vec![arm(vec![Grab, RotateCounterClockwise])], 2);
        assert!(key_at(&world, -1, 1).is_some());
        assert!(key_at(&world, 1, 0).is_some(), "unbonded atom should stay behind");
        assert!(key_at(&world, 3, 0).is_some(), "disjoint part should be split off and stay behind");
        assert!(key_at(&world, -1, 2).is_none() && key_at(&world, -1, 4).is_none());
        assert_ne!(molecule_at(&world, 1, 0), molecule_at(&world, 3, 0));
    }

    #[test]
    fn bonding_merges_molecules() {
        let world = run(vec![
                glyph(0, 0, GlyphType::Input(smallvec![vec![atom(0, 0, Salt)]], 0)),
                glyph(1, 0, GlyphType::Input(smallvec![vec![atom(0, 0, Salt)]], 1)),
                glyph(0, 0, GlyphType::Bonding)],
            vec![arm(vec![Grab, RotateCounterClockwise])], 2);
        assert!(key_at(&world, -1, 1).is_some());
        assert!(key_at(&world, -1, 2).is_some(), "bonded atom should move with the grabbed one");
        assert_eq!(molecule_at(&world, -1, 1), molecule_at(&world, -1, 2));
    }

    #[test]
    fn separate_molecules_dont_fill_disjoint_output() {
        let pattern = vec![atom(0, 0, Salt), atom(2, 0, Salt)];
        let separate = run(vec![
                glyph(0, 0, GlyphType::Input(smallvec![vec![atom(0, 0, Salt)]], 0)),
                glyph(2, 0, GlyphType::Input(smallvec![vec![atom(0, 0, Salt)]], 1)),
                output(pattern.clone())], vec![], 3);
        assert_eq!(output_count(&separate, 0), 6, "two separate molecules must not count as the disjoint output");
        let joined = run(vec![input(pattern.clone()), output(pattern)], vec![], 3);
        assert!(output_count(&joined, 0) < 6, "the disjoint molecule itself should be accepted");
    }

    #[test]
    fn disjoint_molecule_doesnt_fill_separate_outputs() {
        let world = run(vec![input(vec![atom(0, 0, Salt), atom(2, 0, Salt)]),
                output(vec![atom(0, 0, Salt)]),
                glyph(2, 0, GlyphType::Output(smallvec![vec![atom(0, 0, Salt)]], 6, 1))], vec![], 3);
        assert_eq!(output_count(&world, 0), 6);
        assert_eq!(output_count(&world, 1), 6);
    }

    #[test]
    fn glyphs_dont_consume_part_of_disjoint_molecule() {
        let disposed = run(vec![input(vec![atom(0, 0, Salt), atom(3, 0, Salt)]),
                glyph(0, 0, GlyphType::Disposal)], vec![], 2);
        assert!(key_at(&disposed, 0, 0).is_some(), "disposal must not take part of a molecule");
        let lone = run(vec![input(vec![atom(0, 0, Salt)]), glyph(0, 0, GlyphType::Disposal)], vec![], 2);
        assert!(key_at(&lone, 0, 0).is_none(), "disposal should take a lone atom");

        let projected = run(vec![input(vec![atom(0, 0, Quicksilver), atom(3, 0, Salt)]),
                glyph(1, 0, GlyphType::Input(smallvec![vec![atom(0, 0, Lead)]], 1)),
                glyph(0, 0, GlyphType::Projection)], vec![], 1);
        assert_eq!(projected.atoms.atom_map[key_at(&projected, 1, 0).unwrap()].atom_type, Lead,
            "projection must not consume quicksilver that is part of a molecule");
    }

    #[test]
    fn conduit_sends_disjoint_molecule_only_when_fully_on_it() {
        let conduit_hexes = vec![Pos::new(0, 0), Pos::new(1, 0), Pos::new(2, 0)];
        let conduits = || vec![
            glyph(0, 0, GlyphType::Conduit(conduit_hexes.clone(), 100)),
            glyph(10, 0, GlyphType::Conduit(conduit_hexes.clone(), 100))];
        let mut glyphs = conduits();
        glyphs.push(input(vec![atom(0, 0, Salt), atom(2, 0, Salt)]));
        let sent = run(glyphs, vec![arm(vec![Grab, Drop])], 2);
        assert!(key_at(&sent, 10, 0).is_some() && key_at(&sent, 12, 0).is_some());
        assert_eq!(molecule_at(&sent, 10, 0), molecule_at(&sent, 12, 0), "the molecule should arrive whole");

        let mut glyphs = conduits();
        glyphs.push(input(vec![atom(0, 0, Salt), atom(4, 0, Salt)]));
        let kept = run(glyphs, vec![arm(vec![Grab, Drop])], 2);
        assert!(key_at(&kept, 10, 0).is_none(), "a molecule partly off the conduit must not be sent");
    }
}

//Repeating ("infinite") output expansion and matching
mod polymer_tests {
    use crate::sim::*;
    use smallvec::smallvec;
    use AtomType::*;

    fn atom(x: i32, y: i32, atom_type: AtomType) -> Atom {
        Atom::new(Pos::new(x, y), atom_type)
    }
    //Bonds between existing atoms, or dangling bonds toward an empty hex
    fn bond(atoms: &mut Vec<Atom>, from: (i32, i32), to: (i32, i32)) {
        let (from, to) = (Pos::new(from.0, from.1), Pos::new(to.0, to.1));
        let rot = pos_to_rot(to - from).unwrap();
        for a in atoms.iter_mut() {
            if a.pos == from { a.connections[rot as usize] |= Bonds::NORMAL; }
            if a.pos == to { a.connections[normalize_dir(rot + 3) as usize] |= Bonds::NORMAL; }
        }
    }
    fn polymer_world(glyphs: Vec<Glyph>) -> World {
        World::setup_sim(&InitialWorld { glyphs, arms: vec![] }).unwrap()
    }
    fn displayed(world: &World) -> Vec<Atom> {
        world.polymers.values().next().expect("no repeating output").display.clone()
    }
    fn at(atoms: &[Atom], x: i32, y: i32) -> &Atom {
        atoms.iter().find(|a| a.pos == Pos::new(x, y)).unwrap_or_else(|| panic!("no atom at {x},{y}"))
    }

    #[test]
    fn expansion_fixes_bonds_and_adds_placeholders() {
        //A at the origin bonds to the marker; B above it has a dangling bond to the next monomer's B
        let mut monomer = vec![atom(0, 0, Salt), atom(0, 1, Salt), atom(1, 0, RepeatingOutputMarker)];
        bond(&mut monomer, (0, 0), (0, 1));
        bond(&mut monomer, (0, 0), (1, 0));
        bond(&mut monomer, (0, 1), (1, 1));
        let world = polymer_world(vec![Glyph { pos: Pos::new(0, 0), rot: 0,
            glyph_type: GlyphType::OutputRepeating(smallvec![monomer], 6, 0) }]);
        let shown = displayed(&world);
        let markers: Vec<Pos> = shown.iter().filter(|a| a.atom_type == RepeatingOutputMarker).map(|a| a.pos).collect();
        assert_eq!(shown.len(), 12 + markers.len());
        assert_eq!(markers, vec![Pos::new(6, 0), Pos::new(6, 1)], "placeholders go where the 6th repeat's bonds lead");
        let middle = at(&shown, 3, 0);
        assert!(middle.connections[0].contains(Bonds::NORMAL) && middle.connections[3].contains(Bonds::NORMAL),
            "bonds between monomers should be fixed up on both atoms");
        let upper = at(&shown, 3, 1);
        assert!(upper.connections[0].contains(Bonds::NORMAL) && upper.connections[3].contains(Bonds::NORMAL));
    }

    #[test]
    fn rotated_oblique_expansion() {
        //Marker diagonal from the origin (a disjoint polymer), glyph rotated twice
        let monomer = vec![atom(0, 0, Salt), atom(1, 1, RepeatingOutputMarker)];
        let origin = Pos::new(2, 3);
        let world = polymer_world(vec![Glyph { pos: origin, rot: 2,
            glyph_type: GlyphType::OutputRepeating(smallvec![monomer], 6, 0) }]);
        let shown = displayed(&world);
        let step = rotate(Pos::new(1, 1), 2);
        for i in 0..6 {
            assert_eq!(at(&shown, (origin + step * i).x, (origin + step * i).y).atom_type, Salt);
        }
        let marker = origin + step * 6;
        assert_eq!(at(&shown, marker.x, marker.y).atom_type, RepeatingOutputMarker);
        assert_eq!(shown.len(), 7);
    }

    //A chain of `length` bonded salts starting at the origin, delivered by an input on top of the output
    fn chain_world(length: i32, rot: Rot) -> World {
        let mut monomer = vec![atom(0, 0, Salt), atom(1, 0, RepeatingOutputMarker)];
        bond(&mut monomer, (0, 0), (1, 0));
        let mut chain: Vec<Atom> = (0..length).map(|x| atom(x, 0, Salt)).collect();
        for x in 0..length - 1 {
            bond(&mut chain, (x, 0), (x + 1, 0));
        }
        let mut world = polymer_world(vec![
            Glyph { pos: Pos::new(0, 0), rot, glyph_type: GlyphType::Input(smallvec![chain], 0) },
            Glyph { pos: Pos::new(0, 0), rot, glyph_type: GlyphType::OutputRepeating(smallvec![monomer], 6, 0) }]);
        let mut motions = WorldStepInfo::new();
        world.run_step(false, &mut motions).unwrap();
        world
    }
    fn remaining(world: &World) -> i32 {
        world.glyphs.iter().find_map(|g| match g.glyph_type {
            GlyphType::OutputRepeating(_, count, _) => Some(count),
            _ => None,
        }).unwrap()
    }

    #[test]
    fn six_monomers_complete_the_output() {
        for rot in 0..6 {
            let full = chain_world(6, rot);
            assert!(full.is_complete(), "6 monomers should complete the output (rotation {rot})");
            let short = chain_world(5, rot);
            assert!(!short.is_complete());
            assert_eq!(remaining(&short), 1, "5 monomers validated (rotation {rot})");
        }
    }
}

//Instruction tape editing, as done by the editor's tape grid
mod tape_edit_tests {
    use crate::sim::*;
    use Instr::*;

    fn tape(first: usize, instructions: Vec<Instr>) -> Tape {
        Tape::new(first, instructions)
    }
    fn assert_tape(t: &Tape, first: usize, instructions: &[Instr]) {
        assert_eq!((t.first, t.instructions.as_slice()), (first, instructions));
    }

    #[test]
    fn insert_before_first() {
        let mut t = tape(5, vec![RotateClockwise, Grab]);
        t.insert(2, &[Extend], false);
        assert_tape(&t, 2, &[Extend, Empty, Empty, Empty, RotateClockwise, Grab]);
    }

    #[test]
    fn overwrite_before_first_keeps_instructions() {
        let mut t = tape(5, vec![RotateClockwise, Grab]);
        t.insert(2, &[Extend], true);
        assert_tape(&t, 2, &[Extend, Empty, Empty, RotateClockwise, Grab]);

        let mut t = tape(5, vec![RotateClockwise, Grab]);
        t.insert(4, &[Extend, Extend], true);
        assert_tape(&t, 4, &[Extend, Extend, Grab]);

        let mut t = tape(0, vec![RotateClockwise]);
        t.insert(3, &[Grab], true);
        assert_tape(&t, 0, &[RotateClockwise, Empty, Empty, Grab]);
    }

    #[test]
    fn delete_from_leading_blanks_into_instructions() {
        //Deleting chars 3..7 of "     RG w" leaves "    w"
        let mut t = tape(5, vec![RotateClockwise, Grab, Empty, Extend]);
        t.delete(3..7, false);
        assert_tape(&t, 4, &[Extend]);

        let mut t = tape(5, vec![RotateClockwise]);
        t.delete(1..3, false);
        assert_tape(&t, 3, &[RotateClockwise]);
    }

    #[test]
    fn overwrite_delete_and_empty_tapes() {
        let mut t = tape(2, vec![RotateClockwise, Grab, Extend]);
        t.delete(1..4, true);
        assert_tape(&t, 4, &[Extend]);

        //A tape without instructions starts at 0
        let mut t = tape(2, vec![RotateClockwise, Grab]);
        t.delete(0..4, false);
        assert_tape(&t, 0, &[]);

        let mut t = tape(3, vec![Grab]);
        t.insert(1, &[], false);
        assert_tape(&t, 3, &[Grab]);
    }

    #[test]
    fn blanks_left_at_the_end_are_dropped() {
        //Blanking the last instruction no longer holds the loop as long
        let mut t = tape(1, vec![RotateClockwise, Grab, Extend]);
        t.delete(3..4, true);
        assert_tape(&t, 1, &[RotateClockwise, Grab]);
        //Nor does writing blanks past the end
        t.insert(5, &[Empty, Empty], true);
        assert_tape(&t, 1, &[RotateClockwise, Grab]);
        t.insert(5, &[Drop, Empty], false);
        assert_tape(&t, 1, &[RotateClockwise, Grab, Empty, Empty, Drop]);
    }

    #[test]
    fn differences_start_at_the_first_changed_step() {
        let a = tape(2, vec![RotateClockwise, Grab, Drop]);
        assert_eq!(a.first_difference(&a.clone(), 10), None);
        assert_eq!(a.first_difference(&tape(2, vec![RotateClockwise, Extend, Drop]), 10), Some(3));
        assert_eq!(a.first_difference(&tape(1, vec![Extend, RotateClockwise, Grab, Drop]), 10), Some(1));
        //Only blanks at the end differ
        assert_eq!(a.first_difference(&tape(2, vec![RotateClockwise, Grab, Drop, Empty]), 10), Some(5));
        //Halting changes what happens once the loop would start over
        assert_eq!(a.first_difference(&tape(2, vec![RotateClockwise, Grab, Drop, Empty, Empty, Halt]), 4), Some(6));
    }
}

//Repeat blocks, wheels, polymers, and the solution file format
mod regression_tests {
    use crate::parser::*;
    use crate::sim::*;
    use smallvec::smallvec;
    use AtomType::*;
    use Instr::*;

    fn arm(x: i32, y: i32, arm_type: ArmType, instructions: Vec<Instr>) -> Arm {
        Arm::new(Pos::new(x, y), 0, 1, arm_type, Tape::new(0, instructions))
    }
    fn glyph(x: i32, y: i32, rot: Rot, glyph_type: GlyphType) -> Glyph {
        Glyph { pos: Pos::new(x, y), rot, glyph_type }
    }
    fn run(glyphs: Vec<Glyph>, arms: Vec<Arm>, steps: usize) -> World {
        let mut world = World::setup_sim(&InitialWorld { glyphs, arms }).unwrap();
        let mut motions = WorldStepInfo::new();
        for _ in 0..steps {
            world.run_step(false, &mut motions).unwrap();
        }
        world
    }
    fn type_at(world: &World, x: i32, y: i32) -> Option<AtomType> {
        world.atoms.locs.get(&Pos::new(x, y)).map(|&key| world.atoms.atom_map[key].atom_type)
    }
    fn empty_puzzle() -> FullPuzzle {
        FullPuzzle {
            puzzle_name: String::new(), creator_id: 0, allowed_bitfield: 0,
            inputs: vec![], outputs: vec![], output_multiplier: 1, production: false,
        }
    }
    //Saves the world as a solution file and loads it back
    fn round_trip(world: &World, solution_name: &str) -> (Vec<u8>, FullSolution, InitialWorld) {
        let solution = create_solution(world, String::from("puzzle"), String::from(solution_name));
        let mut data = Vec::new();
        write_solution(&mut data, &solution).unwrap();
        let parsed = parse_solution(&mut data.as_slice()).unwrap();
        let init = puzzle_prep(&empty_puzzle(), &parsed).unwrap();
        (data, parsed, init)
    }

    #[test]
    fn noop_before_repeat_is_repeated() {
        //Like omsim, the repeated block runs through the Noop: R _ R _
        let world = run(vec![], vec![arm(0, 0, ArmType::PlainArm, vec![RotateClockwise, Noop, Repeat])], 0);
        assert_eq!(world.arms[0].instruction_tape.instructions, [RotateClockwise, Noop, RotateClockwise, Noop]);
        assert_eq!(world.repeat_length, 4);
        //A halt between moves in a repeated block is copied rather than rejected
        let world = run(vec![], vec![arm(0, 0, ArmType::PlainArm, vec![RotateClockwise, Halt, RotateClockwise, Repeat])], 0);
        assert_eq!(world.arms[0].instruction_tape.instructions,
            [RotateClockwise, Halt, RotateClockwise, RotateClockwise, Halt, RotateClockwise]);
    }

    #[test]
    fn ravari_wheel_keeps_its_atoms() {
        let world = run(vec![], vec![arm(0, 0, ArmType::Ravari, vec![Drop, Grab, RotateCounterClockwise])], 3);
        assert!(world.arms[0].grabbing);
        //Tin starts at (1,0) and rotates with the wheel
        assert_eq!(type_at(&world, 0, 1), Some(Tin));
        assert_eq!(world.atoms.atom_map.len(), 6);
    }

    #[test]
    fn projection_and_rejection_ignore_wheel_atoms() {
        //Ravari's Tin is at (1,0)
        let projection = run(vec![
                glyph(2, 0, 0, GlyphType::Input(smallvec![vec![Atom::new(Pos::new(0, 0), Quicksilver)]], 0)),
                glyph(2, 0, 3, GlyphType::Projection)],
            vec![arm(0, 0, ArmType::Ravari, vec![])], 1);
        assert_eq!(type_at(&projection, 1, 0), Some(Tin));
        assert_eq!(type_at(&projection, 2, 0), Some(Quicksilver));

        let rejection = run(vec![glyph(1, 0, 0, GlyphType::Rejection)],
            vec![arm(0, 0, ArmType::Ravari, vec![])], 1);
        assert_eq!(type_at(&rejection, 1, 0), Some(Tin));
        assert_eq!(type_at(&rejection, 2, 0), None);
    }

    #[test]
    fn proliferation_only_copies_metals() {
        let input = |x: i32, y: i32, atom_type: AtomType, id: i32|
            glyph(x, y, 0, GlyphType::Input(smallvec![vec![Atom::new(Pos::new(0, 0), atom_type)]], id));
        //The metal goes at (-1,1), quicksilver at (1,1), and the copy comes out at (1,-1)
        let with = |source: AtomType| run(vec![glyph(0, 0, 0, GlyphType::Proliferation),
            input(-1, 1, source, 0), input(1, 1, Quicksilver, 1)], vec![], 1);
        assert_eq!(type_at(&with(Lead), 1, -1), Some(Lead));
        assert_eq!(type_at(&with(Salt), 1, -1), None);
        assert_eq!(type_at(&with(Quicksilver), 1, -1), None);
    }

    #[test]
    fn dangling_bond_without_next_monomer_gets_no_placeholder() {
        let mut a = Atom::new(Pos::new(0, 0), Salt);
        a.connections[0] = Bonds::NORMAL; //to the next monomer
        a.connections[1] = Bonds::NORMAL; //to an empty hex
        let world = run(vec![glyph(0, 0, 0, GlyphType::OutputRepeating(
            smallvec![vec![a, Atom::new(Pos::new(1, 0), RepeatingOutputMarker)]], 6, 0))], vec![], 0);
        let shown = &world.polymers[&0].display;
        let markers: Vec<Pos> = shown.iter().filter(|a| a.atom_type == RepeatingOutputMarker).map(|a| a.pos).collect();
        assert_eq!(markers, vec![Pos::new(6, 0)]);
    }

    #[test]
    fn polymer_ignores_other_atoms_at_origin() {
        //The monomer has no atom at the output's origin; a separate salt sits there
        let mut monomer = vec![Atom::new(Pos::new(0, 1), Salt), Atom::new(Pos::new(1, 0), RepeatingOutputMarker)];
        monomer[0].connections[0] = Bonds::NORMAL;
        let mut chain: Vec<Atom> = (0..6).map(|x| Atom::new(Pos::new(x, 1), Salt)).collect();
        for x in 0..5 {
            chain[x].connections[0] = Bonds::NORMAL;
            chain[x + 1].connections[3] = Bonds::NORMAL;
        }
        let world = run(vec![
                glyph(0, 0, 0, GlyphType::Input(smallvec![chain], 0)),
                glyph(0, 0, 0, GlyphType::Input(smallvec![vec![Atom::new(Pos::new(0, 0), Salt)]], 1)),
                glyph(0, 0, 0, GlyphType::OutputRepeating(smallvec![monomer], 6, 0))],
            vec![], 1);
        assert!(world.is_complete());
    }

    #[test]
    fn long_names_round_trip() {
        let world = run(vec![], vec![], 0);
        let name = "x".repeat(200);
        let (data, parsed, _) = round_trip(&world, &name);
        assert_eq!(parsed.solution_name, name);
        //.NET's 7-bit encoding of the puzzle name's length (6), then the solution name's (200)
        assert_eq!(data[4], 6);
        assert_eq!(data[11..13], [0xC8, 0x01]);
    }

    #[test]
    fn saved_loop_length_noop() {
        //An arm already spans the loop: no Noop needed
        let world = run(vec![], vec![
            arm(0, 0, ArmType::PlainArm, vec![RotateClockwise]),
            arm(5, 0, ArmType::PlainArm, vec![RotateClockwise, Empty, RotateClockwise])], 0);
        let (_, _, init) = round_trip(&world, "a");
        assert!(init.arms.iter().all(|a| !a.instruction_tape.instructions.contains(&Noop)));

        //A longer loop is kept with a Noop on an arm that loops (not the halted one)
        let mut world = run(vec![], vec![
            arm(0, 0, ArmType::PlainArm, vec![RotateClockwise, Halt]),
            arm(5, 0, ArmType::PlainArm, vec![RotateClockwise, RotateClockwise])], 0);
        world.repeat_length = 5;
        let (_, _, init) = round_trip(&world, "a");
        assert!(!init.arms[0].instruction_tape.instructions.contains(&Noop));
        assert_eq!(World::setup_sim(&init).unwrap().repeat_length, 5);
    }

    #[test]
    fn saved_noops_keep_loop_length() {
        //R O C runs R _ R _: the trailing Noop is the only thing making the loop 4 long
        let world = run(vec![], vec![arm(0, 0, ArmType::PlainArm, vec![RotateClockwise, Noop, Repeat])], 0);
        assert_eq!(world.repeat_length, 4);
        let (_, _, init) = round_trip(&world, "a");
        assert_eq!(World::setup_sim(&init).unwrap().repeat_length, 4);

        //Leading and trailing Noops are both saved
        let world = run(vec![], vec![arm(0, 0, ArmType::PlainArm, vec![Noop, RotateClockwise, Noop])], 0);
        let (_, _, init) = round_trip(&world, "a");
        assert_eq!(init.arms[0].instruction_tape.first, 0);
        assert_eq!(World::setup_sim(&init).unwrap().repeat_length, 3);
    }

    #[test]
    fn saved_loop_length_ignores_blank_padding() {
        //Trailing blanks (like a final Reset with nothing to undo) aren't saved, so they can't hold the loop length
        let mut world = run(vec![], vec![arm(0, 0, ArmType::PlainArm, vec![RotateClockwise, RotateClockwise])], 0);
        world.repeat_length = 5;
        world.arms[0].instruction_tape.instructions.resize(5, Empty);
        let (_, _, init) = round_trip(&world, "a");
        assert_eq!(World::setup_sim(&init).unwrap().repeat_length, 5);
    }

    #[test]
    fn saved_loop_length_noop_skips_empty_tapes() {
        //The Noop must go on an arm with instructions: alone on an empty tape, it spans just one step
        let mut world = run(vec![], vec![
            arm(0, 0, ArmType::PlainArm, vec![]),
            arm(5, 0, ArmType::PlainArm, vec![RotateClockwise, RotateClockwise])], 0);
        world.repeat_length = 5;
        let (_, _, init) = round_trip(&world, "a");
        assert!(init.arms[0].instruction_tape.instructions.is_empty());
        assert!(init.arms[1].instruction_tape.instructions.contains(&Noop));
        assert_eq!(World::setup_sim(&init).unwrap().repeat_length, 5);
    }

    #[test]
    fn tape_edits_track_halting() {
        let mut tape = Tape::new(0, vec![RotateClockwise]);
        assert!(!tape.halts());
        tape.insert(1, &[Halt], false);
        assert!(tape.halts());
        assert_eq!(tape.get(5, 2), Empty, "halting tapes don't loop");
        tape.delete(1..2, false);
        assert!(!tape.halts());
        assert_eq!(tape.get(2, 2), RotateClockwise);
    }

    #[test]
    fn track_rotation_is_ignored() {
        let track = glyph(2, 2, 1, GlyphType::Track(vec![Pos::new(0, 0), Pos::new(1, 0)]));
        let world = run(vec![track], vec![], 0);
        let (_, _, init) = round_trip(&world, "a");
        let loaded = World::setup_sim(&init).unwrap();
        let GlyphType::Track(hexes) = &loaded.glyphs[0].glyph_type else {unreachable!()};
        let hexes: Vec<Pos> = hexes.iter().map(|&p| loaded.glyphs[0].reposition(p)).collect();
        assert_eq!(hexes, vec![Pos::new(2, 2), Pos::new(3, 2)]);
    }

    #[test]
    fn layout_keeps_input_variant_order() {
        //Setup spawns the first variant and moves on to the second, but a rebuild must start from the first again
        let variants = smallvec![vec![Atom::new(Pos::new(0, 0), Salt)], vec![Atom::new(Pos::new(0, 0), Water)]];
        let world = run(vec![glyph(0, 0, 0, GlyphType::Input(variants, 0))], vec![], 0);
        let rebuilt = World::setup_sim(&world.layout()).unwrap();
        assert_eq!(type_at(&rebuilt, 0, 0), Some(Salt));
        assert_eq!(rebuilt.glyphs[0].glyph_type, world.glyphs[0].glyph_type);
    }

    #[test]
    fn halting_tapes_dont_set_loop_length() {
        //A halting tape runs once, so it doesn't count toward the loop, however long it is
        let mut world = run(vec![], vec![
            arm(0, 0, ArmType::PlainArm, vec![RotateClockwise, RotateClockwise]),
            arm(5, 0, ArmType::PlainArm, vec![RotateClockwise; 5].into_iter().chain([Halt]).collect())], 0);
        assert_eq!(world.repeat_length, 2);
        assert_eq!(world.min_repeat_length(), 2);
        //Without its Halt, it loops
        world.arms[1].instruction_tape.delete(5..6, false);
        assert_eq!(world.min_repeat_length(), 5);
    }
}

//The world shown partway through a step
mod float_world_tests {
    use crate::sim::*;
    use Instr::*;

    //An arm's grip a quarter of the way through `step` (counting from 0) of running `instructions`
    fn grip_during(instructions: Vec<Instr>, step: usize) -> f32 {
        let arm = Arm::new(Pos::new(0, 0), 0, 1, ArmType::PlainArm, Tape::new(0, instructions));
        let mut world = World::setup_sim(&InitialWorld { glyphs: vec![], arms: vec![arm] }).unwrap();
        let mut motion = WorldStepInfo::new();
        for _ in 0..step {
            world.run_step(false, &mut motion).unwrap();
        }
        world.prepare_step(&mut motion, false).unwrap();
        let mut float_world = FloatWorld::new();
        float_world.regenerate(&world, &motion, 0.25);
        float_world.arms_xy[0].grip
    }

    #[test]
    fn grip_closes_and_opens_over_the_step() {
        assert_eq!(grip_during(vec![Grab, Drop], 0), 0.25);
        assert_eq!(grip_during(vec![Grab, Drop], 1), 0.75);
    }

    #[test]
    fn grip_holds_when_grab_or_drop_changes_nothing() {
        assert_eq!(grip_during(vec![Grab, Grab], 1), 1.);
        assert_eq!(grip_during(vec![Drop], 0), 0.);
    }
}


//Footprints, picking stacks, and overlap
mod part_tests {
    use crate::sim::*;
    use smallvec::smallvec;

    fn glyph(x: i32, y: i32, rot: Rot, glyph_type: GlyphType) -> Glyph {
        Glyph { pos: Pos::new(x, y), rot, glyph_type }
    }
    fn arm(x: i32, y: i32) -> Arm {
        Arm::new(Pos::new(x, y), 0, 1, ArmType::PlainArm, Tape::new(0, vec![]))
    }
    fn world(glyphs: Vec<Glyph>, arms: Vec<Arm>) -> World {
        World::setup_sim(&InitialWorld { glyphs, arms }).unwrap()
    }
    fn track(x: i32, y: i32, path: &[(i32, i32)]) -> Glyph {
        glyph(x, y, 0, GlyphType::Track(path.iter().map(|&(a, b)| Pos::new(a, b)).collect()))
    }

    #[test]
    fn footprints_follow_position_and_rotation() {
        let w = world(vec![
            glyph(2, 2, 1, GlyphType::Bonding),
            glyph(3, 3, 2, GlyphType::Input(smallvec![vec![Atom::new(Pos::new(1, 0), AtomType::Salt)]], 0)),
        ], vec![arm(7, 7)]);
        assert_eq!(w.glyph_footprint(0).to_vec(), vec![Pos::new(2, 2), Pos::new(2, 2) + rotate(Pos::new(1, 0), 1)]);
        assert_eq!(w.glyph_footprint(1).to_vec(), vec![Pos::new(3, 3) + rotate(Pos::new(1, 0), 2)]);
        assert_eq!(w.part_footprint(PartId::Arm(0)).to_vec(), vec![Pos::new(7, 7)]);
    }

    #[test]
    fn inputs_cover_their_first_variant() {
        //Setup spawns the first variant and moves on to the second, but the first is what's on the board
        let variants = smallvec![vec![Atom::new(Pos::new(0, 0), AtomType::Salt)], vec![Atom::new(Pos::new(1, 0), AtomType::Water)]];
        let w = world(vec![glyph(3, 3, 0, GlyphType::Input(variants, 0))], vec![arm(4, 3)]);
        assert_eq!(w.glyph_footprint(0).to_vec(), vec![Pos::new(3, 3)]);
        assert_eq!(w.parts_at(Pos::new(3, 3)), vec![PartId::Glyph(0)]);
        assert!(!w.has_overlap(), "the second variant would cover the arm's hex");
    }

    #[test]
    fn stack_is_in_drawing_order() {
        let w = world(vec![glyph(0, 0, 0, GlyphType::Bonding), glyph(0, 0, 0, GlyphType::Calcification)],
            vec![arm(0, 0), arm(5, 5), arm(0, 0)]);
        use PartId::*;
        assert_eq!(w.parts_at(Pos::new(0, 0)), vec![Arm(2), Arm(0), Glyph(1), Glyph(0)]);
        assert_eq!(w.parts_at(Pos::new(1, 0)), vec![Glyph(0)]);
        assert!(w.parts_at(Pos::new(2, 0)).is_empty());
    }

    #[test]
    fn arms_may_sit_on_tracks_but_nothing_else() {
        let on_track = world(vec![track(0, 0, &[(0, 0), (1, 0)])], vec![arm(0, 0)]);
        assert!(!on_track.has_overlap());
        assert!(on_track.part_overlaps(PartId::Arm(0)).is_empty());

        let on_glyph = world(vec![glyph(5, 0, 0, GlyphType::Bonding)], vec![arm(6, 0)]);
        assert!(on_glyph.has_overlap());
        assert_eq!(on_glyph.part_overlaps(PartId::Arm(0)).to_vec(), vec![Pos::new(6, 0)]);
        assert_eq!(on_glyph.part_overlaps(PartId::Glyph(0)).to_vec(), vec![Pos::new(6, 0)]);

        let crossing_tracks = world(vec![track(0, 0, &[(0, 0), (1, 0)]), track(1, 0, &[(0, 0), (0, 1)])], vec![]);
        assert!(crossing_tracks.has_overlap());
        assert_eq!(crossing_tracks.part_overlaps(PartId::Glyph(1)).to_vec(), vec![Pos::new(1, 0)]);

        let apart = world(vec![glyph(0, 0, 0, GlyphType::Bonding), glyph(0, 1, 0, GlyphType::Bonding)], vec![arm(3, 3)]);
        assert!(!apart.has_overlap());

        let crossing_itself = world(vec![track(0, 0, &[(0, 0), (1, 0), (1, 1), (0, 1), (0, 0)])], vec![]);
        assert!(crossing_itself.has_overlap());
        assert_eq!(crossing_itself.part_overlaps(PartId::Glyph(0)).to_vec(), vec![Pos::new(0, 0)]);
    }

    #[test]
    fn track_overlaps_skip_arms_and_the_track_itself() {
        let w = world(vec![track(0, 0, &[(0, 0), (1, 0)]), glyph(5, 0, 0, GlyphType::Bonding)], vec![arm(3, 0)]);
        let hexes = [Pos::new(1, 0), Pos::new(3, 0), Pos::new(6, 0)];
        assert_eq!(w.track_overlaps(Some(0), &hexes).to_vec(), vec![Pos::new(6, 0)]);
        assert_eq!(w.track_overlaps(None, &hexes).to_vec(), vec![Pos::new(1, 0), Pos::new(6, 0)]);
    }

    #[test]
    fn moved_footprint_overlap_ignores_its_old_spot() {
        let w = world(vec![glyph(0, 0, 0, GlyphType::Bonding), glyph(4, 0, 0, GlyphType::Bonding)], vec![]);
        //Glyph 0 moved one hex right still covers (1,0), which only it covered before
        assert!(w.overlaps_with_others(PartId::Glyph(0), &[Pos::new(1, 0), Pos::new(2, 0)]).is_empty());
        assert_eq!(w.overlaps_with_others(PartId::Glyph(0), &[Pos::new(3, 0), Pos::new(4, 0)]).to_vec(), vec![Pos::new(4, 0)]);
    }
}

//Reshaping track: path text and strokes over the board
mod track_editing_tests {
    use crate::sim::*;
    use TrackTool::*;

    fn hexes(path: &[(i32, i32)]) -> Vec<Pos> {
        path.iter().map(|&(x, y)| Pos::new(x, y)).collect()
    }

    #[test]
    fn path_text_round_trips() {
        let first = Pos::new(3, -2);
        let path: Vec<Pos> = hexes(&[(0, 0), (1, 0), (1, 1), (0, 2), (-1, 2), (-1, 1), (0, 0), (3, -2), (3, -1)])
            .into_iter().map(|p| p + first).collect();
        let text = path_to_text(&path);
        assert_eq!(text, "dewazx(3,-2)e");
        assert_eq!(parse_path_text(&text, first), (path, None));
        assert_eq!(path_to_text(&[first]), "");
        assert_eq!(parse_path_text("", first), (vec![first], None));
    }

    #[test]
    fn path_text_keeps_what_it_can_read() {
        let o = Pos::zeros();
        assert_eq!(parse_path_text(" D d	E ", o), (hexes(&[(0, 0), (1, 0), (2, 0), (2, 1)]), None));
        assert_eq!(parse_path_text("ddqdd", o), (hexes(&[(0, 0), (1, 0), (2, 0)]), Some(2)));
        assert_eq!(parse_path_text("d( -2 , +3 )d", o), (hexes(&[(0, 0), (1, 0), (-1, 3), (0, 3)]), None));
        //Reading stops at the start of the jump that can't be read
        for broken in ["d(1,2", "d(1)", "d(,2)", "d(0,0)d", "d(1;2)", "d(99999999999,0)", "d(1048576,0)(1048576,0)",
            "d(-2147483648,0)"] {
            assert_eq!(parse_path_text(broken, o), (hexes(&[(0, 0), (1, 0)]), Some(1)), "{broken}");
        }
        //A byte offset, after skipped whitespace and wider characters
        assert_eq!(parse_path_text("é d ✓", o).1, Some(0));
        assert_eq!(parse_path_text(" d ✓", o).1, Some(3));
    }

    #[test]
    fn gaps_are_steps_to_non_neighbors() {
        assert_eq!(gap_count(&hexes(&[(0, 0)])), 0);
        assert_eq!(gap_count(&hexes(&[(0, 0), (1, 0), (3, 0), (3, 1), (0, 0)])), 2);
    }

    //Runs a stroke over `over` from a track with path `path`
    fn stroke(path: &[(i32, i32)], tool: TrackTool, over: &[(i32, i32)], gaps: bool, overlap: bool)
        -> (Vec<Pos>, Vec<std::result::Result<bool, &'static str>>)
    {
        let mut path = hexes(path);
        let mut added = 0;
        let results = hexes(over).into_iter().map(|h| stroke_hex(&mut path, &mut added, tool, h, gaps, overlap)).collect();
        (path, results)
    }

    #[test]
    fn adding_extends_either_end_and_going_back_retracts() {
        let (path, _) = stroke(&[(0, 0), (1, 0)], AddBack, &[(1, 0), (2, 0), (3, 0), (2, 0)], false, false);
        assert_eq!(path, hexes(&[(0, 0), (1, 0), (2, 0)]));
        let (path, _) = stroke(&[(0, 0), (1, 0)], AddFront, &[(0, 0), (-1, 0), (-1, 1)], false, false);
        assert_eq!(path, hexes(&[(-1, 1), (-1, 0), (0, 0), (1, 0)]));
        //Only what the stroke added is retracted, and the hex before the end is never added (even with overlap)
        let (path, results) = stroke(&[(0, 0), (1, 0)], AddBack, &[(0, 0)], false, true);
        assert_eq!(path, hexes(&[(0, 0), (1, 0)]));
        assert!(results[0].is_err());
        let (path, results) = stroke(&[(0, 0), (1, 0)], AddFront, &[(-1, 0), (0, 0), (1, 0)], false, true);
        assert_eq!(path, hexes(&[(0, 0), (1, 0)]));
        assert_eq!(results[..2], [Ok(true), Ok(true)]);
        assert!(results[2].is_err());
    }

    #[test]
    fn adding_refuses_gaps_and_overlap_unless_allowed() {
        let (path, results) = stroke(&[(0, 0)], AddBack, &[(2, 0)], false, false);
        assert_eq!((path, results[0].is_err()), (hexes(&[(0, 0)]), true));
        let (path, _) = stroke(&[(0, 0)], AddBack, &[(2, 0)], true, false);
        assert_eq!(path, hexes(&[(0, 0), (2, 0)]));

        let square = [(0, 0), (1, 0), (1, 1), (0, 1)];
        let (path, results) = stroke(&square, AddBack, &[(0, 0)], false, false);
        assert_eq!((path, results[0].is_err()), (hexes(&square), true));
        let (path, _) = stroke(&square, AddBack, &[(0, 0)], false, true);
        assert_eq!(path.len(), 5);
    }

    #[test]
    fn removing_takes_ends_and_keeps_one_hex() {
        let line = [(0, 0), (1, 0), (2, 0)];
        let (path, results) = stroke(&line, Remove, &[(1, 0), (5, 5), (2, 0), (0, 0), (1, 0)], false, false);
        assert_eq!(path, hexes(&[(1, 0)]));
        assert!(results[0].is_err(), "a middle hex leaves a gap");
        assert_eq!(results[1], Ok(false), "not on the track");
        assert!(results[4].is_err(), "the last hex stays");
        let (path, _) = stroke(&line, Remove, &[(1, 0)], true, false);
        assert_eq!(path, hexes(&[(0, 0), (2, 0)]));
    }
}

//Track movement and how reset instructions undo it
mod track_tests {
    use crate::sim::*;
    use Instr::*;

    fn track(path: &[(i32, i32)]) -> Glyph {
        Glyph { pos: Pos::new(0, 0), rot: 0, glyph_type: GlyphType::Track(path.iter().map(|&(a, b)| Pos::new(a, b)).collect()) }
    }
    fn maps(tracks: &[&[(i32, i32)]]) -> TrackMaps {
        TrackMaps::build(&tracks.iter().map(|t| track(t)).collect::<Vec<_>>()).unwrap()
    }
    fn at(map: &TrackMap, x: i32, y: i32) -> Option<(i32, i32)> {
        map.get(&Pos::new(x, y)).map(|p| (p.x, p.y))
    }
    //The arm's tape after setup expands its reset instructions
    fn normalized(tracks: &[&[(i32, i32)]], arm_pos: (i32, i32), instructions: Vec<Instr>) -> Vec<Instr> {
        let arm = Arm::new(Pos::new(arm_pos.0, arm_pos.1), 0, 1, ArmType::PlainArm, Tape::new(0, instructions));
        let glyphs = tracks.iter().map(|t| track(t)).collect();
        let world = World::setup_sim(&InitialWorld { glyphs, arms: vec![arm] }).unwrap();
        world.arms[0].instruction_tape.instructions.clone()
    }

    #[test]
    fn non_looping_ends_block_later_tracks() {
        let m = maps(&[&[(0, 0), (1, 0)], &[(1, 0), (2, 0)]]);
        assert_eq!(at(&m.plus, 0, 0), Some((1, 0)));
        assert_eq!(at(&m.plus, 1, 0), None);
        assert_eq!(at(&m.minus, 1, 0), Some((-1, 0)));
        assert_eq!(at(&m.minus, 2, 0), Some((-1, 0)));
        assert_eq!(at(&m.minus, 0, 0), None);
    }

    #[test]
    fn adjacent_ends_loop_unless_two_hexes() {
        let looping = maps(&[&[(0, 0), (1, 0), (0, 1)]]);
        assert_eq!(at(&looping.plus, 0, 1), Some((0, -1)));
        assert_eq!(at(&looping.minus, 0, 0), Some((0, 1)));
        let pair = maps(&[&[(0, 0), (1, 0)]]);
        assert_eq!(at(&pair.plus, 1, 0), None);
        assert_eq!(at(&pair.minus, 0, 0), None);
    }

    #[test]
    fn self_intersecting_track_start_keeps_its_back_move() {
        //Revisits its start, but the start's (blocked) back move wins over the revisit's
        let m = maps(&[&[(0, 0), (1, 0), (0, 1), (0, 0), (0, -1), (0, -2)]]);
        assert_eq!(at(&m.minus, 0, 0), None);
        assert_eq!(at(&m.plus, 0, 0), Some((1, 0)));
        assert_eq!(at(&m.minus, 0, -1), Some((0, 1)));
    }

    #[test]
    fn reset_takes_the_shorter_way_round_a_loop() {
        let square: &[(i32, i32)] = &[(0, 0), (1, 0), (1, 1), (0, 1)];
        assert_eq!(normalized(&[square], (0, 0), vec![Forward, Forward, Forward, Reset]),
            vec![Forward, Forward, Forward, Forward]);
        //Ties undo the steps taken
        assert_eq!(normalized(&[square], (0, 0), vec![Forward, Forward, Reset]),
            vec![Forward, Forward, Back, Back]);
        assert_eq!(normalized(&[square], (0, 0), vec![Back, Back, Reset]),
            vec![Back, Back, Forward, Forward]);
    }

    #[test]
    fn reset_follows_the_arms_own_track() {
        //Like test/solution/edge-cases/just-salt-overlapped-track-reset: the earlier one-hex track
        //blocks the back move, but the game's reset counts it along the arm's own track anyway
        assert_eq!(normalized(&[&[(1, 0)], &[(0, 0), (1, 0)]], (0, 0), vec![Forward, Back, Reset]),
            vec![Forward, Back, Empty]);
        //Moving off a non-looping end doesn't count
        assert_eq!(normalized(&[&[(0, 0), (1, 0)]], (0, 0), vec![Forward, Forward, Reset]),
            vec![Forward, Forward, Back]);
        //Off the track, track moves do nothing
        assert_eq!(normalized(&[&[(0, 0), (1, 0)]], (5, 5), vec![Forward, Reset]),
            vec![Forward, Empty]);
    }
}

//Moving the editor's cursor around and editing tapes must give the same worlds as simulating straight through
mod timeline_tests {
    use crate::sim::*;
    use crate::timeline::*;
    use smallvec::smallvec;
    use Instr::*;

    //An arm moving salt from an input at (0,0) to an output at (-1,1), `repeats` times per loop
    fn mover(repeats: usize) -> InitialWorld {
        mover_filling(repeats, 1_000_000)
    }
    //Like mover, with an output that's full after `products`
    fn mover_filling(repeats: usize, products: i32) -> InitialWorld {
        let salt = |x, y| Atom::new(Pos::new(x, y), AtomType::Salt);
        let glyphs = vec![
            Glyph { pos: Pos::new(0, 0), rot: 0, glyph_type: GlyphType::Input(smallvec![vec![salt(0, 0)]], 0) },
            Glyph { pos: Pos::new(-1, 1), rot: 0, glyph_type: GlyphType::Output(smallvec![vec![salt(0, 0)]], products, 0) },
        ];
        let tape = [Grab, RotateCounterClockwise, Drop, RotateClockwise].repeat(repeats);
        let arms = vec![Arm::new(Pos::new(-1, 0), 0, 1, ArmType::PlainArm, Tape::new(0, tape))];
        InitialWorld { glyphs, arms }
    }
    fn straight(init: &InitialWorld, steps: usize) -> World {
        let mut world = World::setup_sim(init).unwrap();
        let mut motion = WorldStepInfo::new();
        for _ in 0..steps {
            world.run_step(true, &mut motion).unwrap();
        }
        world
    }
    //What two worlds should agree on
    fn summary(world: &World) -> (u64, Vec<(i32, i32, String)>, Vec<(Pos, Rot, i32, bool)>, usize, Vec<i32>) {
        let mut atoms: Vec<_> = world.atoms.atom_map.values()
            .map(|a| (a.pos.x, a.pos.y, format!("{:?}", a.atom_type))).collect();
        atoms.sort();
        let arms = world.arms.iter().map(|a| (a.pos, a.rot, a.len, a.grabbing)).collect();
        let outputs = world.glyphs.iter().filter_map(|g| match g.glyph_type {
            GlyphType::Output(_, count, _) => Some(count),
            _ => None,
        }).collect();
        (world.timestep, atoms, arms, world.area_touched.len(), outputs)
    }

    #[test]
    fn moving_back_matches_straight_run() {
        let init = mover(1);
        let mut timeline = Timeline::new(World::setup_sim(&init).unwrap(), true);
        timeline.advance_to(300.).unwrap();
        timeline.rewind(120);
        assert!(timeline.time() <= 120.);
        timeline.advance_to(150.5).unwrap();
        timeline.rewind(140);
        timeline.advance_to(299.6).unwrap();
        timeline.advance_to(300.).unwrap();
        assert_eq!(summary(timeline.curr()), summary(&straight(&init, 300)));
    }

    #[test]
    fn coming_forward_again_reuses_checkpoints() {
        let init = mover(1);
        let mut timeline = Timeline::new(World::setup_sim(&init).unwrap(), true);
        timeline.advance_to(300.).unwrap();
        let checkpoints = timeline.checkpoint_steps();
        timeline.rewind(120);
        timeline.advance_to(300.).unwrap();
        assert_eq!(timeline.checkpoint_steps(), checkpoints);
        assert_eq!(summary(timeline.curr()), summary(&straight(&init, 300)));
    }

    #[test]
    fn tape_edit_matches_fresh_setup() {
        //Long enough that step 200 is the first time its instruction runs
        let mut timeline = Timeline::new(World::setup_sim(&mover(100)).unwrap(), true);
        timeline.advance_to(300.).unwrap();
        //Skip one grab: the arm moves around empty for a loop
        timeline.base_mut().arms[0].instruction_tape.insert(200, &[Empty], true);
        assert!(timeline.program_changed(200));
        assert!(timeline.checkpoint_steps().iter().all(|&step| step <= 200));
        timeline.advance_to(300.).unwrap();
        let fresh = straight(&timeline.base().layout(), 300);
        assert_eq!(summary(timeline.curr()), summary(&fresh));
        assert_ne!(summary(timeline.curr()), summary(&straight(&mover(100), 300)));
    }

    //Undoing a tapes-only edit puts back the old program, re-simulating only from where it acts differently
    #[test]
    fn restored_program_matches_fresh_setup() {
        let mut timeline = Timeline::new(World::setup_sim(&mover(100)).unwrap(), true);
        let saved = timeline.base().clone();
        timeline.base_mut().arms[0].instruction_tape.insert(200, &[Empty], false);
        let longer = timeline.base().min_repeat_length() + 3;
        let earliest = timeline.base_mut().set_repeat_length(longer).min(200);
        timeline.program_changed(earliest as u64);
        timeline.advance_to(450.).unwrap();

        let earliest = timeline.base_mut().restore_program(saved.clone()).unwrap();
        assert_eq!(earliest, 200);
        timeline.program_changed(earliest as u64);
        assert!(timeline.checkpoint_steps().iter().all(|&step| step <= 200));
        timeline.advance_to(450.).unwrap();
        assert_eq!(summary(timeline.curr()), summary(&straight(&saved.layout(), 450)));
        assert_eq!(timeline.base().repeat_length, saved.repeat_length);
        //Nothing left to put back
        assert_eq!(timeline.base_mut().restore_program(saved), None);
    }

    #[test]
    fn edit_ahead_of_cursor_doesnt_move_it() {
        let mut timeline = Timeline::new(World::setup_sim(&mover(100)).unwrap(), true);
        timeline.advance_to(100.).unwrap();
        timeline.base_mut().arms[0].instruction_tape.insert(250, &[Empty], true);
        assert!(!timeline.program_changed(250));
        assert_eq!(timeline.time(), 100.);
        assert_eq!(timeline.curr().arms[0].instruction_tape.instructions, timeline.base().arms[0].instruction_tape.instructions);
    }

    #[test]
    fn checkpoints_stay_few_and_dense_near_the_cursor() {
        let mut timeline = Timeline::new(World::setup_sim(&mover(1)).unwrap(), false);
        let end = 20_000;
        timeline.advance_to(end as f64).unwrap();
        let behind: Vec<u64> = timeline.checkpoint_steps().iter().rev().map(|&step| end - step).collect();
        assert_eq!(behind.len(), MAX_CHECKPOINTS);
        assert_eq!(behind[..2], [0, CHECKPOINT_SPACING]);
        //Further back, they only get further apart
        let gaps: Vec<u64> = behind.windows(2).map(|w| w[1] - w[0]).collect();
        assert!(gaps.windows(2).all(|g| g[0] <= g[1]), "gaps going back: {gaps:?}");
        assert!(*behind.last().unwrap() > end/4, "oldest is only {} back", behind.last().unwrap());
    }

    //The cycles and area of a straight run once it completes
    fn completed(init: &InitialWorld) -> (i32, i32) {
        let mut world = World::setup_sim(init).unwrap();
        let mut motion = WorldStepInfo::new();
        while !world.is_complete() {
            world.run_step(true, &mut motion).unwrap();
        }
        let stats = world.get_stats();
        (stats.cycles, stats.area)
    }
    fn completion(timeline: &Timeline) -> Option<(i32, i32)> {
        timeline.completion().map(|c| (c.cycles, c.area))
    }

    #[test]
    fn completion_is_kept_until_an_edit_before_it() {
        let init = mover_filling(100, 6);
        let expected = completed(&init);
        let mut timeline = Timeline::new(World::setup_sim(&init).unwrap(), true);
        timeline.advance_to(10.).unwrap();
        assert_eq!(completion(&timeline), None, "not run that far yet");
        timeline.advance_to(100.).unwrap();
        assert_eq!(completion(&timeline), Some(expected));
        timeline.rewind(5);
        assert_eq!(completion(&timeline), Some(expected), "moving back changes nothing");

        timeline.base_mut().arms[0].instruction_tape.insert(200, &[Empty], true);
        timeline.program_changed(200);
        assert_eq!(completion(&timeline), Some(expected), "an edit after it completed");

        //Skip the grab at step 8, so the output fills a loop later
        timeline.base_mut().arms[0].instruction_tape.insert(8, &[Empty], true);
        timeline.program_changed(8);
        assert_eq!(completion(&timeline), None);
        timeline.advance_to(100.).unwrap();
        let edited = completed(&timeline.base().layout());
        assert_eq!(completion(&timeline), Some(edited));
        assert_eq!(edited.0, expected.0 + 4);
    }

    #[test]
    fn instruction_count_follows_tape_edits() {
        let mut timeline = Timeline::new(World::setup_sim(&mover(2)).unwrap(), true);
        assert_eq!(timeline.base().instruction_count(), 8);
        timeline.base_mut().arms[0].instruction_tape.insert(2, &[Empty], true);
        assert_eq!(timeline.base().instruction_count(), 7);
        assert_eq!(World::setup_sim(&timeline.base().layout()).unwrap().instruction_count(), 7);
    }

    #[test]
    fn changing_the_first_timestep_starts_over() {
        let mut timeline = Timeline::new(World::setup_sim(&mover(1)).unwrap(), true);
        timeline.advance_to(100.).unwrap();
        //A blank before the first instruction delays the whole tape
        timeline.base_mut().arms[0].instruction_tape.insert(0, &[Empty], false);
        assert!(timeline.program_changed(0));
        assert!(timeline.checkpoint_steps().is_empty());
        assert_eq!(timeline.curr().timestep, 1);
        assert!(timeline.at_start());
    }
}
