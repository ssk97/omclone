//The rules omsim checks when it loads a solution, most of which the game enforces while editing. The verifier rejects a
//solution that breaks them, as omsim does; the editor only notes them.
use crate::parser::{FullPuzzle, FullSolution, Part, PartType};
use crate::sim::Instr;
use std::fmt;

//Why omsim rejects a solution, in the order it checks. Arms are numbered as the editor shows them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejection {
    //Over one of the limits on omsim's file reader, which then can't read the file at all
    TooBig(&'static str),
    EmptyReagent,
    EmptyProduct,
    //The file's name for the part
    DisabledPart(String),
    MissingInput,
    MissingOutput,
    UnplacedProducts,
    TapeTooLong{arm: usize},
    TapeIndexOverflow{arm: usize},
    SameIndex{arm: usize},
    DisabledInstruction{arm: usize, byte: u8},
}
//omsim's message
impl fmt::Display for Rejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use Rejection::*;
        f.write_str(match self {
            TooBig(_) => "invalid solution file",
            EmptyReagent => "puzzle file contains a reagent with no atoms",
            EmptyProduct => "puzzle file contains a product with no atoms",
            DisabledPart(_) => "solution contains a part that has been disabled in the puzzle file",
            MissingInput => "solution refers to an input that doesn't exist in the puzzle file",
            MissingOutput => "solution refers to an output that doesn't exist in the puzzle file",
            UnplacedProducts => "all products in the puzzle must be placed in the solution",
            TapeTooLong{..} => "solution has an arm with an instruction tape that's too long",
            TapeIndexOverflow{..} => "solution has an arm with a potential instruction tape index overflow",
            SameIndex{..} => "solution contains an arm with two instructions that have the same index",
            DisabledInstruction{..} => "solution contains an instruction that has been disabled in the puzzle file",
        })
    }
}
impl Rejection {
    //What the editor says about it
    pub fn note(&self) -> String {
        use Rejection::*;
        match self {
            TooBig(what) => format!("omsim can't read a solution with {what}"),
            DisabledPart(name) => format!("omsim would reject this: the puzzle disables {name}"),
            TapeTooLong{arm} => format!("omsim would reject this: arm {arm}'s tape spans more than 99,999 cycles"),
            DisabledInstruction{arm, byte} => {
                let instruction = crate::parser::file_instruction(*byte).to_char();
                format!("omsim would reject this: arm {arm} uses '{instruction}', which the puzzle disables")
            }
            SameIndex{arm} | TapeIndexOverflow{arm} => format!("omsim would reject this ({self}, arm {arm})"),
            EmptyReagent | EmptyProduct | MissingInput | MissingOutput | UnplacedProducts =>
                format!("omsim would reject this: {self}"),
        }
    }
}

//The number the editor shows for each arm (1 runs first), by part index
fn arm_numbers(parts: &[Part]) -> Vec<usize> {
    let mut arms: Vec<(i32, usize)> = parts.iter().enumerate()
        .filter(|(_, p)| matches!(p.part_type, PartType::TArm(_))).map(|(i, p)| (p.arm_index, i)).collect();
    //Stable, as puzzle_prep's sort is
    arms.sort_by_key(|&(arm_index, _)| arm_index);
    let mut numbers = vec![0; parts.len()];
    for (n, &(_, i)) in arms.iter().enumerate() {
        numbers[i] = n + 1;
    }
    numbers
}

//The bits of a puzzle's allowed-parts field that a part needs (omsim's parts_available_bits_for_part_name)
fn part_bits(name: &str) -> u64 {
    1 << match name {
        "arm1" => 0,
        "arm2" | "arm3" | "arm6" | "glyph-marker" => 1,
        "piston" => 2,
        "track" => 3,
        "bonder" => 8,
        "unbonder" => 9,
        "bonder-speed" => 10,
        "bonder-prisma" => 11,
        "glyph-calcification" => 12,
        "glyph-duplication" => 13,
        "glyph-projection" => 14,
        "glyph-purification" => 15,
        "glyph-life-and-death" => 16,
        "glyph-disposal" => 17,
        "glyph-dispersion" | "glyph-unification" => 18,
        "glyph-rejection" => 19,
        "glyph-division" => 20,
        "glyph-proliferation" => 21,
        "baron" => 28,
        "ravari" => 29,
        _ => return 0,
    }
}
//The same for an instruction byte (omsim's parts_available_bits_for_instruction)
fn instruction_bits(byte: u8) -> u64 {
    1 << match byte {
        b'E' | b'e' => 2,
        b'A' | b'a' => 3,
        b'R' | b'r' | b'g' => 22,
        b'G' => 23,
        b'X' => 24,
        b'C' | b'O' => 25,
        b'P' | b'p' => 26,
        _ => return 0,
    }
}

//The limits on omsim's file reader that the solution is over, each as a TooBig
pub fn too_big(solution: &FullSolution) -> Vec<Rejection> {
    let parts = solution.parts();
    let mut found = Vec::new();
    let mut add = |what| if !found.contains(&Rejection::TooBig(what)) {found.push(Rejection::TooBig(what))};
    if parts.len() > 9999 {
        add("more than 9,999 parts");
    }
    for p in parts {
        if p.instructions.len() > 99999 {
            add("an arm with more than 99,999 instructions");
        }
        if p.part_type == PartType::TTrack && p.tracks.as_ref().is_some_and(|t| t.len() > 9999) {
            add("a track of more than 9,999 hexes");
        }
        if p.part_type == PartType::TConduit && p.conduit.as_ref().is_some_and(|c| c.1.len() > 9999) {
            add("a conduit of more than 9,999 hexes");
        }
    }
    found
}

//Everything omsim would reject the solution for, in the order it checks (so the first is what it reports), each once.
//omsim also rejects malformed infinite products and unpaired conduits, which setting up the world reports.
pub fn rejections(puzzle: &FullPuzzle, solution: &FullSolution) -> Vec<Rejection> {
    use PartType::*;
    use Rejection::*;
    let parts = solution.parts();
    let mut found = too_big(solution);
    let mut add = |rejection: Rejection| if !found.contains(&rejection) {found.push(rejection)};

    if puzzle.inputs.iter().any(|m| m.iter().all(|atoms| atoms.is_empty())) {
        add(EmptyReagent);
    }
    if puzzle.outputs.iter().any(|m| m.iter().all(|atoms| atoms.is_empty())) {
        add(EmptyProduct);
    }
    //A negative index is a huge one to omsim
    let index = |p: &Part| p.input_output_index as u32 as usize;
    let mut unplaced = puzzle.outputs.len() as i64;
    for p in parts {
        let name = p.name().unwrap_or("");
        let bits = part_bits(name);
        if puzzle.allowed_bitfield & bits != bits {
            add(DisabledPart(name.to_owned()));
        }
        match p.part_type {
            TInput if index(p) >= puzzle.inputs.len() => add(MissingInput),
            TOutput | TOutputRep if index(p) >= puzzle.outputs.len() => add(MissingOutput),
            TOutput | TOutputRep => unplaced -= 1,
            _ => {}
        }
    }
    //Only the count matters: one product placed twice makes up for another left out
    if unplaced != 0 {
        add(UnplacedProducts);
    }

    let numbers = arm_numbers(parts);
    for (i, p) in parts.iter().enumerate() {
        if !matches!(p.part_type, TArm(_)) || p.instructions.is_empty() {
            continue;
        }
        let arm = numbers[i];
        let mut sorted = p.instructions.clone();
        sorted.sort_by_key(|&(index, _)| index);
        let (first, last) = (sorted[0].0 as i64, sorted[sorted.len() - 1].0 as i64);
        if last - first > 99999 {
            add(TapeTooLong{arm});
            continue;
        }
        if first + (last - first + 1) * 2 > i32::MAX as i64 {
            add(TapeIndexOverflow{arm});
            continue;
        }
        for (j, &(index, byte)) in sorted.iter().enumerate() {
            if j > 0 && index == sorted[j - 1].0 {
                add(SameIndex{arm});
            }
            let bits = instruction_bits(byte);
            if puzzle.allowed_bitfield & bits != bits {
                add(DisabledInstruction{arm, byte});
            }
        }
    }
    found
}

//A way loading changes a solution from what its file says. Saving writes the solution as loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Adjustment {
    ArmLengthClamped{arm: usize, from: i32, to: i32},
    UnknownPart(String),
    //Bytes that aren't instructions, which load as noops
    NoopBytes{arm: usize, count: usize},
    //Two instructions at one timestep, where the later in the file is kept
    SameIndex{arm: usize, index: i32},
}
impl fmt::Display for Adjustment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use Adjustment::*;
        match self {
            ArmLengthClamped{arm, from, to} => write!(f, "Arm {arm}'s length of {from} was changed to {to}"),
            UnknownPart(name) => write!(f, "The unknown part \"{name}\" was left out"),
            NoopBytes{arm, count} => write!(f, "Arm {arm} had {count} unknown instructions, which became noops"),
            SameIndex{arm, index} => write!(f, "Arm {arm} had two instructions at timestep {index}; the later one in the file was kept"),
        }
    }
}
pub fn adjustments(solution: &FullSolution) -> Vec<Adjustment> {
    use Adjustment::*;
    let parts = solution.parts();
    let numbers = arm_numbers(parts);
    let mut found = Vec::new();
    for (i, p) in parts.iter().enumerate() {
        let arm = numbers[i];
        match &p.part_type {
            PartType::TUnknown(name) => found.push(UnknownPart(name.clone())),
            PartType::TArm(_) => {
                let to = p.arm_size.clamp(1, 3);
                if to != p.arm_size {
                    found.push(ArmLengthClamped{arm, from: p.arm_size, to});
                }
                let count = p.instructions.iter()
                    .filter(|&&(_, byte)| !matches!(Instr::from_byte(byte), Some(instr) if instr != Instr::Empty)).count();
                if count > 0 {
                    found.push(NoopBytes{arm, count});
                }
                let mut indexes: Vec<i32> = p.instructions.iter().map(|&(index, _)| index).collect();
                indexes.sort();
                for pair in indexes.windows(2) {
                    let repeat = SameIndex{arm, index: pair[0]};
                    if pair[0] == pair[1] && !found.contains(&repeat) {
                        found.push(repeat);
                    }
                }
            }
            _ => {}
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{puzzle_prep, PartType::*};
    use crate::sim::*;
    use smallvec::smallvec;

    fn puzzle(allowed_bitfield: u64, outputs: usize) -> FullPuzzle {
        let salt = || smallvec![vec![Atom::new(Pos::new(0, 0), AtomType::Salt)]];
        FullPuzzle{puzzle_name: String::new(), creator_id: 0, allowed_bitfield, inputs: vec![salt()],
            outputs: (0..outputs).map(|_| salt()).collect(), output_multiplier: 1, chambers: Chambers::default(),
            conduits: vec![]}
    }
    fn part(part_type: PartType, input_output_index: i32) -> Part {
        Part{part_type, pos: Pos::new(0, 0), arm_size: 1, rot: 0, input_output_index, instructions: vec![],
            tracks: None, arm_index: 0, conduit: None}
    }
    fn arm(arm_index: i32, arm_size: i32, instructions: &[(i32, u8)]) -> Part {
        Part{arm_size, arm_index, instructions: instructions.to_vec(), pos: Pos::new(arm_index * 4, 3),
            ..part(TArm(ArmType::PlainArm), 0)}
    }
    fn solution(part_list: Vec<Part>) -> FullSolution {
        FullSolution{puzzle_name: String::new(), solution_name: String::new(), stats: None, part_list}
    }
    const ALL: u64 = u64::MAX;
    const BONDER: u64 = 1 << 8;
    const PIVOT: u64 = 1 << 26;

    #[test]
    fn rejections_come_in_omsims_order() {
        use Rejection::*;
        let bonder = || part(TGlyph(GlyphType::Bonding), 0);
        let output = |index| part(TOutput, index);
        assert_eq!(rejections(&puzzle(ALL, 1), &solution(vec![bonder(), output(0)])), []);
        //omsim checks the parts before counting the products, so it reports the bonder
        let found = rejections(&puzzle(ALL & !BONDER, 1), &solution(vec![bonder()]));
        assert_eq!(found, [DisabledPart(String::from("bonder")), UnplacedProducts]);
        assert_eq!(found[0].to_string(), "solution contains a part that has been disabled in the puzzle file");
        //Only the count of products matters
        assert_eq!(rejections(&puzzle(ALL, 2), &solution(vec![output(0), output(0)])), []);
        assert_eq!(rejections(&puzzle(ALL, 1), &solution(vec![output(0), part(TInput, 5)])), [MissingInput]);
        assert_eq!(rejections(&puzzle(ALL, 1), &solution(vec![output(3)])), [MissingOutput, UnplacedProducts]);
    }

    #[test]
    fn tapes_are_checked_per_arm() {
        use Rejection::*;
        let check = |allowed, arms: Vec<Part>| {
            let mut parts = vec![part(TOutput, 0)];
            parts.extend(arms);
            rejections(&puzzle(allowed, 1), &solution(parts))
        };
        //Tapes are checked in file order, but arms are numbered by their order to run
        let arms = vec![arm(5, 1, &[(0, b'G'), (1, b'P')]), arm(2, 1, &[(0, b'G'), (0, b'g')])];
        assert_eq!(check(ALL & !PIVOT, arms), [DisabledInstruction{arm: 2, byte: b'P'}, SameIndex{arm: 1}]);
        assert_eq!(check(ALL, vec![arm(0, 1, &[(0, b'G'), (100000, b'g')])]), [TapeTooLong{arm: 1}]);
        let huge: Vec<(i32, u8)> = (0..100_000).map(|i| (i, b'G')).collect();
        assert_eq!(check(ALL, vec![arm(0, 1, &huge)])[0], TooBig("an arm with more than 99,999 instructions"));
    }

    #[test]
    fn loading_adjusts_what_omsim_accepts() {
        use Adjustment::*;
        let solution = solution(vec![part(TOutput, 0), arm(0, 5, &[(0, b'G'), (1, b'Z'), (1, b'g')]),
            part(TUnknown(String::from("glyph-mystery")), 0)]);
        assert_eq!(adjustments(&solution), [ArmLengthClamped{arm: 1, from: 5, to: 3}, NoopBytes{arm: 1, count: 1},
            SameIndex{arm: 1, index: 1}, UnknownPart(String::from("glyph-mystery"))]);
        //omsim reads it, apart from the two instructions at one index
        assert_eq!(rejections(&puzzle(ALL, 1), &solution), [Rejection::SameIndex{arm: 1}]);
        let init = puzzle_prep(&puzzle(ALL, 1), &solution).unwrap();
        assert_eq!((init.glyphs.len(), init.arms[0].len), (1, 3));
        assert_eq!(init.arms[0].instruction_tape.instructions, [Instr::Grab, Instr::Drop], "the later instruction is kept");
    }
}
