//A verifier that works like omsim's (verifier.h, which the verify crate exports), measuring with omclone's sim. It
//follows omsim's verifier.c branch for branch, including when it sets and clears its error. omclone has no loop
//detection, so it can't measure the metrics of a solution running forever (throughput, steady state): those are errors.
use crate::parser::{self, FullPuzzle, FullSolution, PartType};
use crate::rules;
use crate::sim::*;
use std::borrow::Cow;
use std::ops::Range;
use std::path::Path;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ErrorSource {
    PuzzleFile, SolutionFile, Metric, Simulation,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifierError {
    pub description: Cow<'static, str>,
    pub source: ErrorSource,
    //For a crash: the cycle it happened in (counting from 1) and its hex. 0 otherwise.
    pub cycle: i32,
    pub location: Pos,
}
impl VerifierError {
    fn new(description: impl Into<Cow<'static, str>>, source: ErrorSource) -> Self {
        VerifierError{description: description.into(), source, cycle: 0, location: Pos::zeros()}
    }
    fn metric(description: &'static str) -> Self {
        Self::new(description, ErrorSource::Metric)
    }
    fn simulation(description: &'static str) -> Self {
        Self::new(description, ErrorSource::Simulation)
    }
}
//For the @∞ metrics, and the output intervals that need the same run
fn no_loop_detection() -> VerifierError {
    VerifierError::metric("omclone can't measure a solution running forever (it has no loop detection)")
}

//What omsim measures at the end of a run (its per_cycle_measurements). -1 is unknown.
#[derive(Clone, Debug)]
struct Measurements {
    cycles: i32,
    area: i32,
    //At 0, 60 and 120 degrees
    heights: [i32; 3],
    widths2: [i32; 3],
    minimum_hexagon: i32,
    executed_instructions: i32,
    //By HOTKEYS
    instruction_executions: [i32; 10],
    //By atom type, numbered as in a puzzle file minus 1
    atom_grabs: [i32; 16],
    atoms: [i32; 16],
}
const UNSET: Measurements = Measurements{
    cycles: -1, area: -1, heights: [-1; 3], widths2: [-1; 3], minimum_hexagon: -1, executed_instructions: -1,
    instruction_executions: [-1; 10], atom_grabs: [-1; 16], atoms: [-1; 16],
};
//The instructions omsim counts executions of, in its order
const HOTKEYS: [u8; 10] = *b"dawsrfeqgt";
fn hotkey_index(instr: Instr) -> Option<usize> {
    use Instr::*;
    Some(match instr {
        RotateClockwise => 0, RotateCounterClockwise => 1, Extend => 2, Retract => 3, Drop => 4,
        Grab => 5, PivotClockwise => 6, PivotCounterClockwise => 7, Forward => 8, Back => 9,
        _ => return None,
    })
}
//Atom names by type number minus 1 (the repetition marker has none)
const ATOM_NAMES: [&str; 16] = ["salt", "air", "earth", "fire", "water", "quicksilver", "gold", "silver", "copper",
    "iron", "tin", "lead", "vitae", "mors", "", "quintessence"];
fn atom_index(name: &str) -> Option<usize> {
    if name.is_empty() {None} else {ATOM_NAMES.iter().position(|&n| n == name)}
}

//The smallest of the known (non-negative) values, as omsim picks a height or width
fn smallest_known(values: [i32; 3]) -> i32 {
    values[1..].iter().fold(values[0], |best, &v| if best < 0 || (v >= 0 && v < best) {v} else {best})
}
fn sum(values: &[i32]) -> i32 {
    values.iter().fold(0, |total: i32, &v| total.wrapping_add(v))
}
//omsim's lookup_per_cycle_metric. Unknown metrics are errors whatever the measurements are.
fn lookup(m: &Measurements, metric: &str) -> Result<i32, VerifierError> {
    Ok(match metric {
        "cycles" => m.cycles,
        "area (approximate)" | "area" => m.area,
        "height at 0 degrees" => m.heights[0],
        "height at 60 degrees" => m.heights[1],
        "height at 120 degrees" => m.heights[2],
        "width*2 at 0 degrees" => m.widths2[0],
        "width*2 at 60 degrees" => m.widths2[1],
        "width*2 at 120 degrees" => m.widths2[2],
        "height" => smallest_known(m.heights),
        "width*2" => smallest_known(m.widths2),
        "minimum hexagon" => m.minimum_hexagon,
        "executed instructions" => m.executed_instructions,
        "instruction executions" => sum(&m.instruction_executions),
        "atoms grabbed" => sum(&m.atom_grabs),
        "number of atoms" => sum(&m.atoms),
        _ => if let Some(hotkeys) = metric.strip_prefix("instruction executions with hotkey ") {
            if hotkeys.is_empty() {
                return Err(VerifierError::metric("no hotkeys specified in 'instruction executions with hotkey' metric"));
            }
            let mut total: i32 = 0;
            for c in hotkeys.bytes() {
                let i = HOTKEYS.iter().position(|&h| h == c.to_ascii_lowercase())
                    .ok_or(VerifierError::metric("invalid instruction hotkey in 'instruction executions with hotkey' metric"))?;
                total = total.wrapping_add(m.instruction_executions[i]);
            }
            total
        } else if let Some(name) = metric.strip_prefix("atoms grabbed of type ") {
            m.atom_grabs[atom_index(name).ok_or(VerifierError::metric("unknown atom type"))?]
        } else if let Some(name) = metric.strip_prefix("number of atoms of type ") {
            m.atoms[atom_index(name).ok_or(VerifierError::metric("unknown atom type"))?]
        } else {
            return Err(VerifierError::metric("unknown metric"));
        }
    })
}

//How a run went: the world when it stopped, and where it started
struct Run {
    world: World,
    start: u64,
    crash: Option<SimError>,
    complete: bool,
}
impl Run {
    fn new(world: World) -> Self {
        Run{start: world.timestep, world, crash: None, complete: false}
    }
    fn cycles(&self) -> u64 {
        self.world.timestep - self.start
    }
    //Runs steps while `more` says to, stopping at a crash
    fn run_while(&mut self, mut more: impl FnMut(&Run) -> bool) {
        let mut motion = WorldStepInfo::new();
        while self.crash.is_none() && more(self) {
            match self.world.run_step(true, &mut motion) {
                Ok(()) => self.complete = self.world.is_complete(),
                Err(err) => self.crash = Some(err),
            }
        }
    }
    //omsim's measure_at_current_cycle
    fn measure(&self) -> Result<Measurements, VerifierError> {
        if let Some(crash) = self.crash {
            //omsim finishes the cycle a crash happens in, so its cycle count includes it
            let cycle = i32::try_from(self.cycles() + 1).unwrap_or(i32::MAX);
            return Err(VerifierError{cycle, location: xy_to_pos(crash.location), ..VerifierError::simulation(crash.error_str)});
        }
        if !self.complete {
            return Err(VerifierError::simulation("solution did not complete within cycle limit"));
        }
        let cycles = i32::try_from(self.cycles())
            .map_err(|_| VerifierError::simulation("solution did not complete within cycle integer limit"))?;
        let world = &self.world;
        //Over every hex counted as area, which is what omsim's board holds. Heights at 0, 60 and 120 degrees,
        //then the doubled widths.
        let mut ranges = [(i64::MAX, i64::MIN); 6];
        for p in &world.area_touched {
            let (u, v) = (p.x as i64, p.y as i64);
            for (range, value) in ranges.iter_mut().zip([v, u, -u - v, -u - 2 * v, -2 * u - v, u - v]) {
                *range = (range.0.min(value), range.1.max(value));
                if range.1 - range.0 + 2 > i32::MAX as i64 {
                    return Err(VerifierError::simulation("solution bounding box is too large to compute width and height"));
                }
            }
        }
        let (mut heights, mut widths2, mut minimum_hexagon) = ([0; 3], [0; 3], 0);
        if !world.area_touched.is_empty() {
            let span = |i: usize, extra: i64| (ranges[i].1 - ranges[i].0 + extra) as i32;
            heights = [span(0, 1), span(1, 1), span(2, 1)];
            widths2 = [span(3, 2), span(4, 2), span(5, 2)];
            let maxs = ((ranges[0].1 + ranges[1].1 + ranges[2].1 + 2) / 3) as i32;
            let mins = ((-ranges[0].0 - ranges[1].0 - ranges[2].0 + 2) / 3) as i32;
            minimum_hexagon = heights.iter().map(|h| h / 2).chain([maxs, mins]).max().unwrap_or(0) + 1;
        }
        let mut atoms = [0; 16];
        for atom in world.atoms.atom_map.values().filter(|a| !a.immutable_wheel) {
            if let Some(count) = (world.atoms.type_of(atom) as usize).checked_sub(1).and_then(|i| atoms.get_mut(i)) {
                *count += 1;
            }
        }
        let (executed_instructions, instruction_executions) = self.instructions_run(cycles as i64);
        Ok(Measurements{
            cycles, area: world.area_touched.len() as i32, heights, widths2, minimum_hexagon,
            executed_instructions, instruction_executions,
            atom_grabs: world.atom_grabs.map(|n| i32::try_from(n).unwrap_or(i32::MAX)), atoms,
        })
    }
    //omsim's counts of the instructions run by `cycle`: each tape cell once if it's been reached (without looping),
    //and how many times each kind of instruction has run. A tape stops counting at its first halt (or clash).
    fn instructions_run(&self, cycle: i64) -> (i32, [i32; 10]) {
        let period = self.world.repeat_length as i64;
        let mut executed: i64 = 0;
        let mut executions = [0i64; 10];
        for arm in &self.world.arms {
            let tape = &arm.decoded().tape;
            let start = tape.first as i64 - self.start as i64;
            if cycle < start || tape.instructions.is_empty() {
                continue;
            }
            let elapsed = cycle - start;
            let (times, progress) = match tape.runs_once() {
                true => (0, elapsed),
                false if period > 0 => (elapsed / period, elapsed % period),
                false => continue,
            };
            for (j, &instr) in tape.instructions.iter().enumerate() {
                if matches!(instr, Instr::Halt | Instr::Clash) {
                    break;
                }
                let Some(index) = hotkey_index(instr) else {continue};
                let j = j as i64;
                if j < elapsed {
                    executed += 1;
                }
                executions[index] += if j < progress {times + 1} else {times};
            }
        }
        (executed as i32, executions.map(|n| n as i32))
    }
}

//The character omsim's decoded tape has for an instruction, which "instructions with hotkey" counts. Blanks have none.
fn tape_char(instr: Instr) -> Option<u8> {
    match instr {
        Instr::Halt => Some(b'b'),
        instr => hotkey_index(instr).map(|i| HOTKEYS[i]),
    }
}

//Reads the count in "<count> <metric>" as omsim does with strtol: optional leading whitespace and sign, then digits.
//Returns the count and what follows it, or None if there are no digits.
fn parse_count(text: &str) -> Option<(i64, &str)> {
    let trimmed = text.trim_start_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r']);
    let unsigned = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
    let digits = unsigned.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let magnitude = unsigned[..digits].bytes().fold(0i64, |n, d| n.saturating_mul(10).saturating_add((d - b'0') as i64));
    let count = if trimmed.starts_with('-') {-magnitude} else {magnitude};
    Some((count, &unsigned[digits..]))
}

//The puzzle and solution, both read. Without a puzzle, omsim doesn't read the solution.
struct Files {
    puzzle: FullPuzzle,
    solution: FullSolution,
}
//The world an output crashed in on a wrong product
struct WrongOutput {
    //The output's index in the puzzle
    index: i32,
    glyph: usize,
    world: World,
}
pub struct Verifier {
    files: Option<Files>,
    cycle_limit: u64,
    disable_limits: bool,
    wrong_output_checks: WrongOutputChecks,
    wrong_output: Option<WrongOutput>,
    //The measurements at completion, kept once made
    completion: Option<Result<Measurements, VerifierError>>,
    error: Option<VerifierError>,
}

//Parses a solution as omsim reads it, which fails past its reader's limits
fn read_solution(bytes: &[u8]) -> Option<FullSolution> {
    let solution = parser::parse_solution(&mut &bytes[..]).ok()?;
    rules::too_big(&solution).is_empty().then_some(solution)
}
//Where the puzzle's name is in a solution file, if the solution can be read
pub fn find_puzzle_name(solution: &[u8]) -> Option<Range<usize>> {
    let parsed = read_solution(solution)?;
    //After the version (4 bytes) comes the name's length, 7 bits a byte, each but the last with its top bit set
    let length_bytes = solution.get(4..)?.iter().position(|b| b & 0x80 == 0)? + 1;
    let start = 4 + length_bytes;
    Some(start..start + parsed.puzzle_name.len())
}

impl Verifier {
    pub fn from_bytes(puzzle: &[u8], solution: &[u8]) -> Self {
        Self::read(Some(puzzle), || Some(solution.to_vec()))
    }
    pub fn from_files(puzzle: &Path, solution: &Path) -> Self {
        Self::read(std::fs::read(puzzle).ok().as_deref(), || std::fs::read(solution).ok())
    }
    //From files already read
    pub fn new(puzzle: FullPuzzle, solution: FullSolution) -> Self {
        let mut verifier = Self::empty();
        if rules::too_big(&solution).is_empty() {
            verifier.files = Some(Files{puzzle, solution});
        } else {
            verifier.error = Some(VerifierError::new("invalid solution file", ErrorSource::SolutionFile));
        }
        verifier
    }
    fn empty() -> Self {
        Verifier{
            files: None, cycle_limit: 150000, disable_limits: false, wrong_output_checks: Default::default(),
            wrong_output: None, completion: None, error: None,
        }
    }
    //Without either file, the error says it's invalid
    fn read(puzzle: Option<&[u8]>, solution: impl FnOnce() -> Option<Vec<u8>>) -> Self {
        let Some(puzzle) = puzzle.and_then(|bytes| parser::parse_puzzle(&mut &bytes[..]).ok()) else {
            let mut verifier = Self::empty();
            verifier.error = Some(VerifierError::new("invalid puzzle file", ErrorSource::PuzzleFile));
            return verifier;
        };
        match solution().and_then(|bytes| parser::parse_solution(&mut &bytes[..]).ok()) {
            Some(solution) => Self::new(puzzle, solution),
            None => {
                let mut verifier = Self::empty();
                verifier.error = Some(VerifierError::new("invalid solution file", ErrorSource::SolutionFile));
                verifier
            }
        }
    }

    pub fn set_cycle_limit(&mut self, cycle_limit: i32) {
        self.cycle_limit = cycle_limit.max(0) as u64;
    }
    pub fn disable_limits(&mut self) {
        self.disable_limits = true;
    }
    pub fn set_fails_on_wrong_output(&mut self, output_index: i32, fails: bool) {
        set_bit(&mut self.wrong_output_checks.atoms, output_index, fails);
    }
    pub fn set_fails_on_wrong_output_bonds(&mut self, output_index: i32, fails: bool) {
        set_bit(&mut self.wrong_output_checks.bonds, output_index, fails);
    }
    //The puzzle index of the output that crashed on a wrong product, or -1
    pub fn wrong_output_index(&self) -> i32 {
        self.wrong_output.as_ref().map_or(-1, |w| w.index)
    }
    //The type of the atom (numbered as in a puzzle file) at an offset from the output that crashed on a wrong product,
    //given as in the puzzle file, or -1 if there isn't one
    pub fn wrong_output_atom(&self, u: i32, v: i32) -> i32 {
        let Some(wrong) = &self.wrong_output else {return -1};
        let atoms = &wrong.world.atoms;
        let hex = wrong.world.glyphs[wrong.glyph].reposition(Pos::new(u, v));
        match atoms.visible(hex).map(|key| atoms.type_of(&atoms.atom_map[key]) as i32) {
            Some(atom_type) if atom_type >= 1 => atom_type,
            _ => -1,
        }
    }
    pub fn wrong_output_clear(&mut self) {
        self.wrong_output = None;
    }
    pub fn error(&self) -> Option<&VerifierError> {
        self.error.as_ref()
    }
    pub fn error_clear(&mut self) {
        self.error = None;
    }

    //omsim measures the intervals between outputs on the run that finds where the solution repeats, so they're
    //unknown: `unknown` (what omsim returns without a solution) and, if there is one, an error
    fn output_intervals(&mut self, unknown: i32) -> i32 {
        if self.files.is_some() {
            self.error = Some(no_loop_detection());
        }
        unknown
    }
    pub fn number_of_output_intervals(&mut self) -> i32 {
        self.output_intervals(0)
    }
    pub fn output_interval(&mut self, _which_interval: i32) -> i32 {
        self.output_intervals(-1)
    }
    pub fn output_intervals_repeat_after(&mut self) -> i32 {
        self.output_intervals(-1)
    }

    pub fn evaluate_approximate_metric(&mut self, metric: &str) -> f64 {
        if self.files.is_none() {
            return -1.;
        }
        match metric {
            "per repetition^2 area" => {
                self.error = Some(no_loop_detection());
                -1.
            }
            "width" => self.evaluate_metric("width*2") as f64 / 2.,
            "steady state width" => self.evaluate_metric("steady state width*2") as f64 / 2.,
            _ => self.evaluate_metric(metric) as f64,
        }
    }

    pub fn evaluate_metric(&mut self, metric: &str) -> i32 {
        match self.measure_metric(metric) {
            Ok(value) => value,
            Err((value, error)) => {
                self.error = Some(error);
                value
            }
        }
    }
    //The metric's value, or the value omsim returns with the error it sets. The run's measurements also set the error
    //(see look_up).
    fn measure_metric(&mut self, metric: &str) -> Result<i32, (i32, VerifierError)> {
        let failed = |error| (-1, error);
        let Some(files) = &self.files else {return Ok(-1)};
        if let Some(result) = file_metric(files, metric) {
            return result;
        }
        if matches!(metric, "reaches steady state" | "per repetition cycles" | "throughput cycles"
            | "per repetition outputs" | "throughput outputs" | "per repetition area" | "throughput waste"
            | "visual loop start cycle" | "visual loop end cycle")
        {
            return Err(failed(no_loop_detection()));
        }
        let world = setup(files, self.wrong_output_checks).map_err(failed)?;
        if let Some(result) = decoded_metric(&world, &files.puzzle, metric) {
            return result.map_err(failed);
        }
        if metric == "overlap" {
            return Ok(i32::try_from(world.overlap_count()).unwrap_or(i32::MAX));
        }

        let limit = if self.disable_limits {u64::MAX} else {self.cycle_limit};
        if let Some(rest) = metric.strip_prefix("product ") {
            let (count, metric) = counted(rest, "invalid product count", "product count must be followed by a metric")
                .map_err(failed)?;
            let mut world = world;
            world.set_output_target(count.clamp(0, i32::MAX as i64) as i32);
            let mut run = Run::new(world);
            run.complete = count <= 0;
            run.run_while(|run| !run.complete && run.cycles() < limit);
            let measured = run.measure();
            self.keep_wrong_output(run.world);
            return Ok(self.look_up(&measured, metric));
        }
        if let Some(rest) = metric.strip_prefix("cycle ") {
            let (count, metric) = counted(rest, "invalid cycle count", "cycle count must be followed by a metric")
                .map_err(failed)?;
            //Like omsim, this ignores the cycle limit
            let mut run = Run::new(world);
            run.run_while(|run| (run.cycles() as i64) < count);
            run.complete = true;
            let measured = run.measure();
            self.keep_wrong_output(run.world);
            return Ok(self.look_up(&measured, metric));
        }
        if metric.starts_with("steady state ") {
            return Err(failed(no_loop_detection()));
        }
        //Checks the metric's name before measuring anything
        lookup(&UNSET, metric).map_err(failed)?;
        let measured = match self.completion.take() {
            Some(measured) => measured,
            None => {
                let mut run = Run::new(world);
                run.run_while(|run| !run.complete && run.cycles() < limit);
                let measured = run.measure();
                self.keep_wrong_output(run.world);
                measured
            }
        };
        let value = self.look_up(&measured, metric);
        self.completion = Some(measured);
        Ok(value)
    }
    //Sets the error from the measurements, unless the metric is unknown
    fn look_up(&mut self, measured: &Result<Measurements, VerifierError>, metric: &str) -> i32 {
        self.error = measured.as_ref().err().cloned();
        match lookup(measured.as_ref().unwrap_or(&UNSET), metric) {
            Ok(value) => value,
            Err(error) => {
                self.error = Some(error);
                -1
            }
        }
    }
    //Keeps the first world an output crashed in on a wrong product
    fn keep_wrong_output(&mut self, world: World) {
        if self.wrong_output.is_some() {
            return;
        }
        let Some(glyph) = world.wrong_output else {return};
        let GlyphType::Output(_, _, index) = world.glyphs[glyph].glyph_type else {return};
        self.wrong_output = Some(WrongOutput{index, glyph, world});
    }
}
//The world as omsim decodes and sets it up, or why it can't be
fn setup(files: &Files, wrong_output_checks: WrongOutputChecks) -> Result<World, VerifierError> {
    let solution_error = |description: String| VerifierError::new(description, ErrorSource::SolutionFile);
    if let Some(rejection) = rules::rejections(&files.puzzle, &files.solution).into_iter().next() {
        return Err(solution_error(rejection.to_string()));
    }
    let init = parser::puzzle_prep(&files.puzzle, &files.solution).map_err(|e| solution_error(e.to_string()))?;
    let mut world = World::setup_sim(&init).map_err(|e| solution_error(e.to_string()))?;
    world.wrong_output_checks = wrong_output_checks;
    Ok(world)
}
//Reads "<count> <metric>"
fn counted<'a>(text: &'a str, invalid: &'static str, no_metric: &'static str) -> Result<(i64, &'a str), VerifierError> {
    let (count, rest) = parse_count(text).filter(|&(count, _)| count >= 0).ok_or(VerifierError::metric(invalid))?;
    let metric = rest.strip_prefix(' ').ok_or(VerifierError::metric(no_metric))?;
    Ok((count, metric))
}
fn set_bit(mask: &mut u64, index: i32, on: bool) {
    let Some(bit) = u32::try_from(index).ok().and_then(|i| 1u64.checked_shl(i)) else {return};
    if on {*mask |= bit} else {*mask &= !bit}
}

//A metric of the solution file alone: its value, or its value and an error
fn file_metric(files: &Files, metric: &str) -> Option<Result<i32, (i32, VerifierError)>> {
    use PartType::*;
    let parts = files.solution.parts();
    let stats = files.solution.stats.clone().unwrap_or(SolutionStats{cycles: 0, cost: 0, area: 0, instructions: 0});
    let capped = |n: i64| i32::try_from(n).unwrap_or(i32::MAX);
    Some(Ok(match metric {
        "parsed cycles" => stats.cycles,
        "parsed cost" => stats.cost,
        "parsed area" => stats.area,
        "parsed instructions" => stats.instructions,
        "number of track segments" => parts.iter().map(|p| p.tracks.as_ref().map_or(0, |t| t.len())).sum::<usize>() as i32,
        "cost" => parts.iter().map(|p| match p.name().unwrap_or("") {
            "glyph-calcification" | "bonder" | "unbonder" => 10,
            "glyph-life-and-death" | "glyph-projection" | "glyph-dispersion" | "glyph-purification"
                | "glyph-duplication" | "glyph-unification" | "bonder-prisma" | "glyph-rejection" | "glyph-division"
                | "arm1" => 20,
            "bonder-speed" | "arm2" | "arm3" | "arm6" | "baron" | "ravari" => 30,
            "piston" | "glyph-proliferation" => 40,
            "track" => 5 * p.tracks.as_ref().map_or(0, |t| t.len() as i64),
            _ => 0,
        }).sum::<i64>() as i32,
        "duplicate reagents" | "duplicate products" => {
            let reagents = metric == "duplicate reagents";
            let n = if reagents {files.puzzle.inputs.len()} else {files.puzzle.outputs.len()};
            let mut seen = vec![false; n];
            let mut duplicates = 0;
            for p in parts {
                let counted = if reagents {p.part_type == TInput} else {matches!(p.part_type, TOutput | TOutputRep)};
                if !counted {
                    continue;
                }
                //A negative index is a huge one to omsim
                let Some(seen) = seen.get_mut(p.input_output_index as u32 as usize) else {
                    let error = VerifierError::new("solution refers to a reagent or product that doesn't exist in the puzzle",
                        ErrorSource::SolutionFile);
                    return Some(Err((-1, error)));
                };
                duplicates += *seen as i32;
                *seen = true;
            }
            duplicates
        }
        "maximum track gap^2" => capped(parts.iter().flat_map(|p| p.tracks.iter().flat_map(|t| t.windows(2))).map(|pair| {
            let (du, dv) = ((pair[1].x - pair[0].x) as i64, (pair[1].y - pair[0].y) as i64);
            du * du + du * dv + dv * dv
        }).max().unwrap_or(0)),
        "maximum absolute part coordinate" => capped(parts.iter().map(|p| {
            let (u, v) = (p.pos.x as i64, p.pos.y as i64);
            u.abs().max(v.abs()).max((u + v).abs())
        }).max().unwrap_or(0)),
        _ => match metric.strip_prefix("parts of type ") {
            Some(name) => parts.iter().filter(|p| p.name() == Some(name)).count() as i32,
            None => return None,
        },
    }))
}

//A metric of the solution as set up, before it runs
fn decoded_metric(world: &World, puzzle: &FullPuzzle, metric: &str) -> Option<Result<i32, VerifierError>> {
    let tracks = || world.glyphs.iter().filter_map(|g| match &g.glyph_type {
        GlyphType::Track(hexes) => Some(hexes),
        _ => None,
    });
    let violations = || world.cabinet_violations(&puzzle.conduits);
    Some(Ok(match metric {
        "instructions" => world.instruction_count(),
        "instruction tape period" => world.repeat_length as i32,
        "number of arms" => world.arms.len() as i32,
        "cabinet violations" => !violations().is_empty() as i32,
        "conduit violations" => violations().intersects(CabinetViolations::CONDUIT_ALTERED | CabinetViolations::CONDUIT_IN_FREESPACE) as i32,
        "track self-overlap" => tracks().map(|hexes| {
            (0..hexes.len()).map(|j| hexes[j + 1..].iter().filter(|&&h| h == hexes[j]).count()).sum::<usize>()
        }).sum::<usize>() as i32,
        "track loops" => tracks().filter(|hexes| path_loops(hexes)).count() as i32,
        _ => {
            let hotkeys = metric.strip_prefix("instructions with hotkey ")?;
            if hotkeys.is_empty() {
                return Some(Err(VerifierError::metric("no hotkeys specified in 'instructions with hotkey' metric")));
            }
            let mut count = 0;
            for c in hotkeys.bytes().map(|c| c.to_ascii_lowercase()) {
                if !b"adefgqrstwb".contains(&c) {
                    return Some(Err(VerifierError::metric("invalid instruction hotkey in 'instructions with hotkey' metric")));
                }
                count += world.arms.iter().flat_map(|a| &a.decoded().tape.instructions)
                    .filter(|&&instr| tape_char(instr) == Some(c)).count() as i32;
            }
            count
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use smallvec::smallvec;
    use AtomType::*;
    use Instr::*;

    fn atom(x: i32, y: i32, atom_type: AtomType) -> Atom {
        Atom::new(Pos::new(x, y), atom_type)
    }
    //Two atoms side by side
    fn pair(atom_type: AtomType, bonded: bool) -> Vec<Atom> {
        let mut atoms = vec![atom(0, 0, atom_type), atom(1, 0, atom_type)];
        if bonded {
            atoms[0].connections[0] = Bonds::NORMAL;
            atoms[1].connections[3] = Bonds::NORMAL;
        }
        atoms
    }
    //An arm at (-1,0) carries what the input at (0,0) gives it to the output at (-1,1), taking 4 cycles each time:
    //grab, turn, drop, turn back
    fn mover(input: Vec<Atom>, output: Vec<Atom>) -> Verifier {
        let world = World::setup_sim(&InitialWorld{
            glyphs: vec![
                Glyph{pos: Pos::new(0, 0), rot: 0, glyph_type: GlyphType::Input(smallvec![input.clone()], 0)},
                Glyph{pos: Pos::new(-1, 1), rot: 1, glyph_type: GlyphType::Output(smallvec![output.clone()], 6, 0)},
            ],
            arms: vec![Arm::new(Pos::new(-1, 0), 0, 1, ArmType::PlainArm,
                Tape::new(0, vec![Grab, RotateCounterClockwise, Drop, RotateClockwise]))],
            ..Default::default()
        }).unwrap();
        let solution = parser::create_solution(&world, String::from("test"), String::from("mover"));
        let puzzle = FullPuzzle{puzzle_name: String::from("test"), creator_id: 0, allowed_bitfield: u64::MAX,
            inputs: vec![smallvec![input]], outputs: vec![smallvec![output]], output_multiplier: 1,
            chambers: Chambers::default(), conduits: vec![]};
        Verifier::new(puzzle, solution)
    }
    fn salt_mover() -> Verifier {
        mover(vec![atom(0, 0, Salt)], vec![atom(0, 0, Salt)])
    }
    fn error(v: &Verifier) -> Option<(&str, ErrorSource)> {
        v.error().map(|e| (&*e.description, e.source))
    }

    #[test]
    fn measures_a_finished_solution() {
        let mut v = salt_mover();
        //Each product takes 4 cycles, and the last one 3
        assert_eq!(v.evaluate_metric("cycles"), 23);
        assert_eq!(v.error(), None);
        let metrics = ["cost", "instructions", "area", "height", "width*2", "minimum hexagon", "atoms grabbed",
            "atoms grabbed of type salt", "atoms grabbed of type air", "number of atoms", "executed instructions",
            "instruction executions", "instruction executions with hotkey fd", "instructions with hotkey f"];
        let values: Vec<i32> = metrics.iter().map(|m| v.evaluate_metric(m)).collect();
        //The arm and the two glyphs cover three hexes in a triangle. Six grabs, and the next salt waits on the
        //input. The tape ran 5 times and then 3 cycles more.
        assert_eq!(values, [20, 4, 3, 2, 4, 2, 6, 6, 0, 1, 4, 23, 11, 1]);
        assert_eq!(v.evaluate_approximate_metric("width"), 2.);
    }

    #[test]
    fn product_and_cycle_metrics_stop_early_or_late() {
        let mut v = salt_mover();
        let values: Vec<i32> = ["product 1 cycles", "product 2 cycles", "product 0 cycles", "product 7 cycles",
            "cycle 0 area", "cycle 2 atoms grabbed", "cycle 5 atoms grabbed", "cycle 30 cycles"]
            .iter().map(|m| v.evaluate_metric(m)).collect();
        assert_eq!(values, [3, 7, 0, 27, 3, 1, 2, 30]);
        assert_eq!(v.error(), None);
    }

    #[test]
    fn bad_metrics_are_errors() {
        let mut v = salt_mover();
        let cases = [
            ("bogus", "unknown metric"),
            ("atoms grabbed of type unobtainium", "unknown atom type"),
            ("instruction executions with hotkey ", "no hotkeys specified in 'instruction executions with hotkey' metric"),
            ("instructions with hotkey z", "invalid instruction hotkey in 'instructions with hotkey' metric"),
            ("product x cycles", "invalid product count"),
            ("product -1 cycles", "invalid product count"),
            ("product 2", "product count must be followed by a metric"),
            ("cycle 3cycles", "cycle count must be followed by a metric"),
        ];
        for (metric, description) in cases {
            v.error_clear();
            assert_eq!((v.evaluate_metric(metric), error(&v)), (-1, Some((description, ErrorSource::Metric))), "{metric}");
        }
        //A measured metric clears the error
        assert_eq!((v.evaluate_metric("cycles"), v.error()), (23, None));
    }

    #[test]
    fn metrics_at_infinity_need_loop_detection() {
        let mut v = salt_mover();
        for metric in ["per repetition cycles", "throughput outputs", "reaches steady state", "steady state area"] {
            assert_eq!(v.evaluate_metric(metric), -1, "{metric}");
            assert_eq!(v.error().map(|e| e.source), Some(ErrorSource::Metric));
        }
        assert_eq!(v.evaluate_approximate_metric("per repetition^2 area"), -1.);
        assert_eq!(v.number_of_output_intervals(), 0);
        assert!(v.error().is_some());
    }

    #[test]
    fn unfinished_and_unreadable_solutions_are_errors() {
        let mut v = salt_mover();
        v.set_cycle_limit(10);
        assert_eq!(v.evaluate_metric("cycles"), -1);
        assert_eq!(error(&v), Some(("solution did not complete within cycle limit", ErrorSource::Simulation)));

        let mut v = Verifier::from_bytes(b"not a puzzle", b"not a solution");
        assert_eq!(error(&v), Some(("invalid puzzle file", ErrorSource::PuzzleFile)));
        assert_eq!(v.evaluate_metric("cost"), -1);
    }

    #[test]
    fn wrong_products_crash_only_where_asked() {
        //Each wrong product is dropped on the output in the third cycle and left there (until the next one hits it)
        let air_for_salt = || mover(vec![atom(0, 0, Air)], vec![atom(0, 0, Salt)]);
        let mut v = air_for_salt();
        assert_eq!((v.evaluate_metric("cycle 4 cycles"), v.error()), (4, None));
        assert_eq!(v.wrong_output_index(), -1);

        let mut v = air_for_salt();
        v.set_fails_on_wrong_output(0, true);
        assert_eq!(v.evaluate_metric("cycle 4 cycles"), -1);
        let e = v.error().unwrap();
        //Reported on the product's first atom
        assert_eq!((&*e.description, e.source, e.cycle, e.location),
            ("output didn't match", ErrorSource::Simulation, 3, Pos::new(-1, 1)));
        assert_eq!((v.wrong_output_index(), v.wrong_output_atom(0, 0), v.wrong_output_atom(1, 0)), (0, Air as i32, -1));
        v.wrong_output_clear();
        assert_eq!(v.wrong_output_index(), -1);

        //Unbonded salts are the product's atoms but not its bonds
        let unbonded = || mover(pair(Salt, false), pair(Salt, true));
        let mut v = unbonded();
        v.set_fails_on_wrong_output(0, true);
        assert_eq!((v.evaluate_metric("cycle 4 cycles"), v.error()), (4, None));
        let mut v = unbonded();
        v.set_fails_on_wrong_output_bonds(0, true);
        assert_eq!(v.evaluate_metric("cycle 4 cycles"), -1);
        assert_eq!(error(&v), Some(("output bonds didn't match", ErrorSource::Simulation)));
        assert_eq!(v.wrong_output_atom(1, 0), Salt as i32);
    }

    #[test]
    fn overlap_counts_like_omsim() {
        use GlyphType::*;
        let glyph = |x: i32, glyph_type: GlyphType| Glyph{pos: Pos::new(x, 0), rot: 0, glyph_type};
        let arm = |x: i32| Arm::new(Pos::new(x, 0), 0, 1, ArmType::PlainArm, Tape::new(0, vec![]));
        let count = |glyphs: Vec<Glyph>, arms: Vec<Arm>, chambers: Chambers|
            World::setup_sim(&InitialWorld{glyphs, arms, chambers}).unwrap().overlap_count();
        let none = Chambers::default;
        assert_eq!(count(vec![glyph(0, Bonding), glyph(1, Bonding)], vec![], none()), 1);
        assert_eq!(count(vec![glyph(0, Calcification)], vec![arm(0), arm(0)], none()), 2);
        let track = |hexes: &[i32]| glyph(0, Track(hexes.iter().map(|&x| Pos::new(x, 0)).collect()));
        assert_eq!(count(vec![track(&[0, 1])], vec![arm(0)], none()), 0, "an arm may sit on track");
        assert_eq!(count(vec![track(&[0, 1, 0])], vec![], none()), 1, "track covering a hex twice");
        //Two chambers sharing the wall at (0,0)
        let chambers = || Chambers::new(&[(Pos::new(-2, 0), ChamberShape::Small), (Pos::new(2, 0), ChamberShape::Small)], &[], false);
        assert_eq!(count(vec![], vec![], chambers()), 0, "walls don't count against each other");
        assert_eq!(count(vec![glyph(0, Bonding)], vec![], chambers()), 1, "a part on a wall counts");
    }

    #[test]
    fn finds_the_puzzle_name() {
        let world = World::setup_sim(&InitialWorld::default()).unwrap();
        //A name of 128 bytes or more has a two-byte length
        for name in [String::from("P008"), "x".repeat(200)] {
            let mut data = Vec::new();
            parser::write_solution(&mut data, &parser::create_solution(&world, name.clone(), String::from("s"))).unwrap();
            let range = find_puzzle_name(&data).unwrap();
            assert_eq!(&data[range], name.as_bytes());
        }
        assert_eq!(find_puzzle_name(b"not a solution"), None);
    }
}
