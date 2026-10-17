//Arm instructions, and the tapes that hold them, a cell per timestep

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Instr {
    RotateClockwise, RotateCounterClockwise,
    Extend, Retract,
    Grab, Drop,
    PivotClockwise, PivotCounterClockwise,
    Forward, Back,
    Repeat, Reset,
    Noop, Halt, Empty,
    //Only in decoded tapes (see Arm::decode): a written instruction where a Repeat or Reset runs, which crashes there
    Clash,
}
impl Instr {
    pub fn to_byte(&self) -> u8 {
        use Instr::*;
        match self {
            RotateClockwise         => b'R',
            RotateCounterClockwise  => b'r',
            Extend                  => b'E',
            Retract                 => b'e',
            Grab                    => b'G',
            Drop                    => b'g',
            PivotClockwise          => b'P',
            PivotCounterClockwise   => b'p',
            Forward                 => b'A',
            Back                    => b'a',
            Repeat                  => b'C',
            Reset                   => b'X',
            Noop                    => b'O',
            Halt                   => b'B',
            Empty                   => b' ',
            Clash                   => unreachable!("only decoded tapes have clashes, and they aren't saved"),
        }
    }
    pub fn from_byte(input: u8) -> Option<Self> {
        use Instr::*;
        match input {
            b'R' => Some(RotateClockwise),
            b'r' => Some(RotateCounterClockwise),
            b'E' => Some(Extend),
            b'e' => Some(Retract),
            b'G' => Some(Grab),
            b'g' => Some(Drop),
            b'P' => Some(PivotClockwise),
            b'p' => Some(PivotCounterClockwise),
            b'A' => Some(Forward),
            b'a' => Some(Back),
            b'C' => Some(Repeat),
            b'X' => Some(Reset),
            b'O' => Some(Noop),
            b'B' => Some(Halt),
            b' ' => Some(Empty),
            _ => None,
        }
    }
    pub fn to_char(&self) -> char {
        use Instr::*;
        match self {
            RotateClockwise         => 'd',
            RotateCounterClockwise  => 'a',
            Extend                  => 'w',
            Retract                 => 's',
            Grab                    => 'f',
            Drop                    => 'r',
            PivotClockwise          => 'e',
            PivotCounterClockwise   => 'q',
            Forward                 => 'g',
            Back                    => 't',
            Repeat                  => 'C',
            Reset                   => 'X',
            Noop                    => 'O',
            Halt                    => 'B',
            Empty                   => ' ',
            Clash                   => '!',
        }
    }
    pub fn from_char(input: char) -> Option<Self> {
        use Instr::*;
        match input {
             'd' =>  Some(RotateClockwise       ),
             'a' =>  Some(RotateCounterClockwise),
             'w' =>  Some(Extend                ),
             's' =>  Some(Retract               ),
             'f' =>  Some(Grab                  ),
             'r' =>  Some(Drop                  ),
             'e' =>  Some(PivotClockwise        ),
             'q' =>  Some(PivotCounterClockwise ),
             'g' =>  Some(Forward               ),
             't' =>  Some(Back                  ),
             'c' =>  Some(Repeat                ),
             'x' =>  Some(Reset                 ),
             'o' =>  Some(Noop                  ),
             'b' =>  Some(Halt                  ),
             ' ' =>  Some(Empty                 ),
             _ => None
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Tape {
    pub first: usize,
    pub instructions: Vec<Instr>,
    //Cached, since it's needed every step. The methods below keep it up to date; anything else that adds or removes a
    //Halt or Clash in `instructions` must call update_runs_once.
    runs_once: bool,
}
impl Tape {
    pub fn new(first: usize, instructions: Vec<Instr>) -> Self {
        let mut tape = Tape { first, instructions, runs_once: false };
        tape.update_runs_once();
        tape
    }
    pub fn update_runs_once(&mut self) {
        self.runs_once = self.instructions.iter().any(|&i| matches!(i, Instr::Halt | Instr::Clash));
    }
    //A tape with a Halt or a Clash runs once; all others loop. A Clash comes on the first pass, so the run crashes
    //before such a tape could start over anyway.
    pub fn runs_once(&self) -> bool {
        self.runs_once
    }
    //The timestep just after the last instruction, before the tape loops
    pub fn end(&self) -> usize {
        self.first + self.instructions.len()
    }
    //The instruction at an absolute timestep, before the tape loops
    pub fn cell(&self, step: usize) -> Instr {
        step.checked_sub(self.first).and_then(|i| self.instructions.get(i)).copied().unwrap_or(Instr::Empty)
    }
    //The first timestep at which two versions of a tape can act differently, if they differ, given the loop length
    pub fn first_difference(&self, other: &Tape, loop_len: usize) -> Option<usize> {
        if self.first == other.first && self.instructions == other.instructions {
            return None;
        }
        let (end_a, end_b) = (self.end(), other.end());
        let start = self.first.min(other.first);
        //Tapes that only differ in blanks at the end act the same, so that's where they differ
        let mut step = (start..end_a.max(end_b)).find(|&i| self.cell(i) != other.cell(i)).unwrap_or(end_a.min(end_b));
        //A tape that runs once goes on past the loop, instead of starting over
        if self.runs_once() != other.runs_once() {
            step = step.min(start + loop_len);
        }
        Some(step)
    }
    pub fn get(&self, timestep: usize, loop_len: usize) -> Instr {
        use Instr::Empty;
        if timestep >= self.first{
            let after_first = timestep - self.first;
            if self.runs_once{
                return *self
                    .instructions
                    .get(after_first)
                    .unwrap_or(&Empty)
            } else if loop_len > 0{
                return *self
                    .instructions
                    .get(after_first % loop_len)
                    .unwrap_or(&Empty)
            }
        }
        return Empty
    }
    pub fn to_string(&self) -> String{
        let mut output = " ".repeat(self.first);
        for i in &self.instructions{
            output.push(i.to_char());
        }
        output
    }
    //Drops blanks after the last instruction. They don't change what runs, but they'd keep the loop from
    //being made shorter.
    fn trim_end(&mut self) {
        while self.instructions.last() == Some(&Instr::Empty) {
            self.instructions.pop();
        }
    }
    //Moves leading empty instructions into `first`. A tape with no instructions starts at 0,
    //like omsim does for arms without instructions.
    fn trim_start(&mut self) {
        let leading = self.instructions.iter().take_while(|&&i| i == Instr::Empty).count();
        self.instructions.drain(..leading);
        self.first += leading;
        if self.instructions.is_empty() {
            self.first = 0;
        }
    }
    //Inserts instructions at the absolute position `index`, or writes over existing ones if `overwrite`
    pub fn insert(&mut self, index: usize, new: &[Instr], overwrite: bool) {
        if new.is_empty() {return;}
        if index < self.first {
            let gap = self.first - index;
            self.instructions.splice(0..0, std::iter::repeat(Instr::Empty).take(gap));
            self.first = index;
        }
        let start = index - self.first;
        if self.instructions.len() < start {
            self.instructions.resize(start, Instr::Empty);
        }
        if overwrite {
            let end = start + new.len();
            if self.instructions.len() < end {
                self.instructions.resize(end, Instr::Empty);
            }
            self.instructions[start..end].copy_from_slice(new);
        } else {
            self.instructions.splice(start..start, new.iter().copied());
        }
        self.trim_end();
        self.trim_start();
        self.update_runs_once();
    }
    //Deletes the absolute positions in `range`, or blanks them if `overwrite`
    pub fn delete(&mut self, range: std::ops::Range<usize>, overwrite: bool) {
        let old_first = self.first;
        let old_end = old_first + self.instructions.len();
        let start = range.start.clamp(old_first, old_end) - old_first;
        let end = range.end.clamp(old_first, old_end) - old_first;
        if overwrite {
            for i in start..end {
                self.instructions[i] = Instr::Empty;
            }
        } else {
            self.instructions.drain(start..end);
            //Deleted blank space before the first instruction shifts it earlier
            let blanks_before = range.end.min(old_first).saturating_sub(range.start);
            self.first -= blanks_before;
        }
        self.trim_end();
        self.trim_start();
        self.update_runs_once();
    }
}

//A tape as it runs, with each Repeat and Reset replaced by what it stands for (see Arm::decode)
#[derive(Debug, Clone, Default)]
pub struct DecodedTape {
    pub tape: Tape,
    //Each Repeat's and Reset's timesteps, in order: from its own cell to the end of what it runs
    pub spans: Vec<std::ops::Range<usize>>,
}
impl DecodedTape {
    //The span covering a timestep, if any
    pub fn span_at(&self, step: usize) -> Option<&std::ops::Range<usize>> {
        let i = self.spans.partition_point(|s| s.end <= step);
        self.spans.get(i).filter(|s| s.start <= step)
    }
}
