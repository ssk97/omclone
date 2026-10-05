mod parser;
mod sim;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(inline_js="
    export function add_text(str){
        document.getElementById('text_out').textContent += str;
    }")]
extern "C" {
    fn add_text(s: &str);
}


static PARSED_PUZZLE: std::sync::Mutex<Option<parser::FullPuzzle>> = std::sync::Mutex::new(None);
static PARSED_SOLUTION: std::sync::Mutex<Option<parser::FullSolution>> = std::sync::Mutex::new(None);
#[wasm_bindgen(start)]
pub fn init_puzzle() {
    console_error_panic_hook::set_once();
    let puzzle_baseline = include_bytes!("../OM2023_W8_HabitabilityDetector.puzzle");
    let puzzle_read = &mut puzzle_baseline.as_slice();
    let puzzle = parser::parse_puzzle(puzzle_read).unwrap();
    *PARSED_PUZZLE.lock().unwrap() = Some(puzzle);
}
#[wasm_bindgen]
pub fn init_sol(solution: &[u8]){
    let mut reader = solution;
    let sol = parser::parse_solution(&mut reader).unwrap();
    *PARSED_SOLUTION.lock().unwrap() = Some(sol);
}

fn to_output(a: bool) -> sim::Atom{
    let atom_type = if a{ sim::AtomType::Gold } else { sim::AtomType::Salt };
    sim::Atom{atom_type, pos: sim::Pos::new(0,0),connections:[sim::Bonds::NO_BOND;6],is_berlo:false}
}
fn to_element(a: u8) -> sim::AtomType{
    match a{
        0 => sim::AtomType::Fire,
        1 => sim::AtomType::Water,
        2 => sim::AtomType::Earth,
        3 => sim::AtomType::Air,
        _ => panic!("Illegal element type {:?}",a)
    }
}
fn element_to_char(a: sim::AtomType) -> char{
    match a{
        sim::AtomType::Air => '🜁',
        sim::AtomType::Water => '🜄',
        sim::AtomType::Fire => '🜂',
        sim::AtomType::Earth => '🜃',
        _ => panic!("Illegal element type {:?}",a)
    }
}
struct AtomsData{
    atoms: Vec<sim::Atom>,
    name: String,
}
fn make_variant(variant_in1: usize, input: &Vec<sim::Atom>) -> AtomsData{
    let mut input = input.clone();
    input.sort_by(|a,b|a.pos.x.cmp(&b.pos.x));
    for (idx, atom) in &mut input.iter_mut().enumerate(){
        let pos = atom.pos;
        //println!("at {},{} : {}", pos.x, pos.y ,element_to_char(atom.atom_type));
        atom.atom_type = to_element(((variant_in1>>(idx*2)) & 0x3) as u8);
    }
    let input_name:String = input.iter().map(|b|element_to_char(b.atom_type)).collect();
    AtomsData{
        atoms:input, name:input_name
    }
}

#[wasm_bindgen]
#[repr(C)]
pub struct OutputStats{
    pub completed: bool,
    pub cycles: i32,
    pub cost: i32,
    pub area: i32
}

#[wasm_bindgen]
pub fn setup_stats() -> OutputStats{
    return OutputStats { completed: true, cycles: 0, cost: 0, area: 0 };
}
#[wasm_bindgen]
pub fn run_test(first: usize, variants: &[usize], test_area: bool) -> OutputStats{

    let fail_stats = OutputStats{completed: false, cycles:0, cost:0, area:0};
    let mut puzzle_bind = PARSED_PUZZLE.lock().unwrap();
    let puzzle = puzzle_bind.as_mut().unwrap();
    //Note that this changes the internal puzzle each time
    //But that's fine since it'll be changed next time anyway
    let sol_bind = PARSED_SOLUTION.lock().unwrap();
    let sol = sol_bind.as_ref().unwrap();
    let input_small = &puzzle.inputs[0][0];
    let input_small = make_variant(first, input_small);
    puzzle.inputs[0][0] = input_small.atoms;

    let mut new_inputs = sim::AtomPattern::new();
    let mut new_outputs = sim::AtomPattern::new();
    let mut fail_str = input_small.name.clone();
    fail_str.push('|');
    for variant in variants{
        let variant_data = make_variant(*variant, &puzzle.inputs[1][0]);
        let output = to_output(variant_data.name.contains(&input_small.name));
        fail_str.push_str(&variant_data.name);
        fail_str.push('|');
        new_inputs.push(variant_data.atoms);
        new_outputs.push(vec!(output));
    }
    puzzle.inputs[1] = new_inputs;
    puzzle.outputs[0] = new_outputs;
    let init = parser::puzzle_prep(&puzzle, &sol).unwrap();
    let mut world = sim::World::setup_sim(&init).unwrap();

    let mut motions = sim::WorldStepInfo::new();
    while !world.is_complete() {
        if let Err(error_out) = world.run_step(test_area, &mut motions){
            add_text(&format!("{}: Step {} error: {}\n", fail_str, world.timestep, error_out));
            return fail_stats;
        }
        if world.timestep > 100_000{
            add_text(&format!("{}: Over 100k steps simmed, assuming failure\n", fail_str));
            return fail_stats;
        }
    }
    let stats = world.get_stats();
    return OutputStats{completed: true, cycles: stats.cycles, cost: stats.cost, area: stats.area};
}