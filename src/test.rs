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

fn check_solution(results: &mut TestResults, fpath:&Path, puzzle_map: &PuzzleMap){
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
fn check_all(){
    let mut puzzle_map = PuzzleMap::new();
    read_puzzle_recurse(&mut puzzle_map, fs::read_dir(PUZZLE_DIR).unwrap());
    fn read_solution_recurse(results: &mut TestResults, puzzle_map: &PuzzleMap, directory: fs::ReadDir){
        for f in directory{
            if let Ok(f) = f{
                let ftype = f.file_type().unwrap();
                if ftype.is_dir(){
                    read_solution_recurse(results, puzzle_map,fs::read_dir(f.path()).unwrap());
                } else if ftype.is_file(){
                    let fpath = f.path();
                    let lname = fpath.to_str().unwrap().to_ascii_lowercase();
                    if lname.contains(".solution"){
                        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(||
                            check_solution(results, &fpath, puzzle_map)));
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
    read_solution_recurse(&mut results, &puzzle_map, fs::read_dir("test/solution").unwrap());
    read_solution_recurse(&mut results, &puzzle_map, fs::read_dir("test/om-leaderboard-master").unwrap());
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
        for g in &world.glyphs {
            if let GlyphType::OutputRepeating(pattern, _, _) = &g.glyph_type {
                return pattern[0].clone();
            }
        }
        panic!("no repeating output");
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

//Instruction tape editing, as done by the editor's tape text boxes
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
    fn dangling_bond_without_next_monomer_gets_no_placeholder() {
        let mut a = Atom::new(Pos::new(0, 0), Salt);
        a.connections[0] = Bonds::NORMAL; //to the next monomer
        a.connections[1] = Bonds::NORMAL; //to an empty hex
        let world = run(vec![glyph(0, 0, 0, GlyphType::OutputRepeating(
            smallvec![vec![a, Atom::new(Pos::new(1, 0), RepeatingOutputMarker)]], 6, 0))], vec![], 0);
        let GlyphType::OutputRepeating(shown, _, _) = &world.glyphs[0].glyph_type else {unreachable!()};
        let markers: Vec<Pos> = shown[0].iter().filter(|a| a.atom_type == RepeatingOutputMarker).map(|a| a.pos).collect();
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
        //Trailing blanks (like from "space ends") aren't saved, so they can't hold the loop length
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
        assert_eq!(hexes, &vec![Pos::new(2, 2), Pos::new(3, 2)]);
    }
}

