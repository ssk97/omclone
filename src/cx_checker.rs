mod parser;
mod sim;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    fn add_text(s: &str);
    fn add_square(white: bool);
}

fn rule_cx(a: bool, b: bool, c:bool) -> bool{
    let mut tmp = 0;
    if a {tmp += 4};
    if b {tmp += 2};
    if c {tmp += 1};
    const CX_ARR: [bool;8] = [false, true, true, true, false, true, true, false];
    CX_ARR[tmp]
}
fn to_type(a: bool) -> sim::AtomType{
    if a {sim::AtomType::Fire} else {sim::AtomType::Salt}
}
struct CXVariant{
    input: Vec<sim::Atom>,
    output: Vec<sim::Atom>,
    input_name: String,
    output_name: String,
}
fn make_variant(variant_id: usize, source: &Vec<sim::Atom>) -> CXVariant{
    let mut input_arr = [false;6];
    for idx in 0..6{
        input_arr[idx] = (variant_id>>idx) & 0x1 == 1;
    }
    let mut input = source.clone();
    let mut output = source.clone();

    let mut output_name = "".to_string();
    let input_name:String = input_arr.map(|b|if b {'F'} else {'S'}).iter().collect();
    for idx in 0..6{
        let output_bool = rule_cx(input_arr[(idx+5)%6],
                                  input_arr[idx],
                                  input_arr[(idx+1)%6]);
        let pos = sim::rot_to_pos(idx as i32);
        output_name += if output_bool {"F"} else {"S"};
        //println!("at {},{} : {},{},{} -> {}", pos.x, pos.y ,input_arr[(idx+5)%6],input_arr[(idx+0)%6],input_arr[(idx+1)%6],output_bool);
        for atom in &mut input{
            if atom.pos == pos{
                atom.atom_type = to_type(input_arr[idx]);
            }
        }
        for atom in &mut output{
            if atom.pos == pos{
                atom.atom_type = to_type(output_bool);
            }
        }
    }
    CXVariant{
        input, output, input_name, output_name
    }
}
#[wasm_bindgen]
pub fn evaluate_solution(solution: &[u8]){
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();

    let puzzle_baseline = include_bytes!("../OM2022_TransmutationCX.puzzle");
    let puzzle_read = &mut puzzle_baseline.as_slice();
    let mut puzzle = parser::parse_puzzle(puzzle_read).unwrap();
    //First input is data, atoms after 0 (1-6) are input
    //Only/first output is data, atoms after 0 (1-6) are output
    //But are differently ordered, so do it via checking the positions?
    let mut reader = solution;
    let sol = parser::parse_solution(&mut reader).unwrap();
    add_text(&format!("solution: {}",sol.solution_name));

    let mut any_failures = false;
    let mut worst_stats:Option<sim::SolutionStats> = None;
    let mut motions = sim::WorldStepInfo::new();
    'variants: for variant in 0..64{
        let variant_data = make_variant(variant, &puzzle.inputs[0][0]);
        puzzle.inputs[0][0] = variant_data.input;
        puzzle.outputs[0][0] = variant_data.output;
        let init = parser::puzzle_prep(&puzzle, &sol).unwrap();
        let mut world = sim::World::setup_sim(&init).unwrap();
        let fail_str = format!("ID {}, input {}, output {}",variant,variant_data.input_name,variant_data.output_name);
        while !world.is_complete() {
            if let Err(error_out) = world.run_step(true, &mut motions){
                any_failures = true;
                add_square(false);
                add_text(&format!("{}: Step {} error: {}", fail_str, world.timestep, error_out));
                continue 'variants;
            }
            if world.timestep > 10_000_000{
                any_failures = true;
                add_square(false);
                add_text(&format!("{}: Over 10 million steps simmed, assuming failure", fail_str));
                continue 'variants;
            }
        }
        add_square(true);
        let stats = world.get_stats();
        if let Some(worst) = &mut worst_stats{
            if stats.cycles > worst.cycles{
                worst.cycles = stats.cycles;
            }
            if stats.area > worst.area{
                worst.area = stats.area;
            }
        } else {
            worst_stats = Some(stats);
        }
    }
    if !any_failures{
        if let Some(worst) = worst_stats{
            add_text(&format!("All good! Slowest solve: {}, approximate area: {}",worst.cycles,worst.area));
            add_text(&format!("Cost: {}, Instructions: {}, wSum: {}",worst.cost,worst.instructions,(worst.cost/5)+worst.cycles+worst.area));
        }
    } else {
        add_text("Fire/Salt patterns are in counter-clockwise order, starting from the right");
    }
}

#[wasm_bindgen]
pub fn random_test(solution: &[u8], variants: &[usize]) -> bool{
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();

    let puzzle_baseline = include_bytes!("../OM2022_TransmutationCX.puzzle");
    let puzzle_read = &mut puzzle_baseline.as_slice();
    let mut puzzle = parser::parse_puzzle(puzzle_read).unwrap();
    let mut reader = solution;
    let sol = parser::parse_solution(&mut reader).unwrap();

    let mut motions = sim::WorldStepInfo::new();

    let mut new_inputs = sim::AtomPattern::new();
    let mut new_outputs = sim::AtomPattern::new();
    for variant in variants{
        let variant_data = make_variant(*variant, &puzzle.inputs[0][0]);
        new_inputs.push(variant_data.input);
        new_outputs.push(variant_data.output);
    }
    puzzle.inputs[0] = new_inputs;
    puzzle.outputs[0] = new_outputs;
    let init = parser::puzzle_prep(&puzzle, &sol).unwrap();
    let mut world = sim::World::setup_sim(&init).unwrap();
    let fail_str = format!("{:?}",variants);

    while !world.is_complete() {
        if let Err(error_out) = world.run_step(true, &mut motions){
            add_text(&format!("{}: Step {} error: {}", fail_str, world.timestep, error_out));
            return false;
        }
        if world.timestep > 10_000_000{
            add_text(&format!("{}: Over 10 million steps simmed, assuming failure", fail_str));
            return false;
        }
    }
    true
}