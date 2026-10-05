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
    let mut float_world = FloatWorld::new();
    let mut motions = WorldStepInfo::new();
    while !world.is_complete() && world.timestep < max_timestep {
        let step = world.run_step(check_area || expect_collision, &mut motions, &mut float_world);
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
        Arm::new(Pos::new(-1, 0), 0, 1, ArmType::PlainArm, Tape { first: 0, instructions })
    }
    fn run(glyphs: Vec<Glyph>, arms: Vec<Arm>, steps: usize) -> World {
        let mut world = World::setup_sim(&InitialWorld { glyphs, arms }).unwrap();
        let mut motions = WorldStepInfo::new();
        let mut float_world = FloatWorld::new();
        for _ in 0..steps {
            world.run_step(false, &mut motions, &mut float_world).unwrap();
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
