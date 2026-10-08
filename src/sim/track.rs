//Track movement, following the game's handling of overlapping and self-intersecting track
//(as decoded by omsim's decode.c, which builds it backwards with later writes winning).
use super::*;
use std::iter::Peekable;

pub type TrackMap = FxHashMap<Pos, Pos>;

//One track part's hexes, in global positions
#[derive(Debug, Clone)]
pub struct TrackPath {
    pub hexes: Vec<Pos>,
    pub looping: bool,
}

#[derive(Debug, Clone, Default)]
pub struct TrackMaps {
    //Offset moved by Forward (plus) and Back (minus) from each hex; the arm stays put on missing hexes
    pub plus: TrackMap,
    pub minus: TrackMap,
    //Each track's own path, which Reset follows
    pub paths: Vec<TrackPath>,
}

//Insert unless the hex was already claimed
fn claim(map: &mut TrackMap, hex: Pos, offset: Pos) {
    map.entry(hex).or_insert(offset);
}

impl TrackMaps {
    //Earlier tracks win where tracks overlap
    pub fn build<'a>(glyphs: impl IntoIterator<Item=&'a Glyph>) -> Result<Self> {
        let mut maps = TrackMaps::default();
        for g in glyphs {
            if let GlyphType::Track(hexes) = &g.glyph_type {
                maps.add(hexes.iter().map(|&p| g.reposition(p)).collect())?;
            }
        }
        //Zero offsets only served to stop later track from claiming the ends of non-looping track
        let blocked = Pos::zeros();
        maps.plus.retain(|_, v| *v != blocked);
        maps.minus.retain(|_, v| *v != blocked);
        Ok(maps)
    }

    fn add(&mut self, hexes: Vec<Pos>) -> Result<()> {
        ensure!(!hexes.is_empty(), "Track of length 0!");
        let (first, last) = (hexes[0], hexes[hexes.len() - 1]);
        let looping = path_loops(&hexes);
        let (closing_plus, closing_minus) = if looping {
            (first - last, last - first)
        } else {
            (Pos::zeros(), Pos::zeros())
        };
        //Every insert is first-wins, so earlier tracks keep their hexes. Within a track, earlier
        //visits to a hex win, except the start's backward move, which beats later visits.
        claim(&mut self.minus, first, closing_minus);
        for pair in hexes.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            claim(&mut self.plus, a, b - a);
            claim(&mut self.minus, b, a - b);
        }
        claim(&mut self.plus, last, closing_plus);
        self.paths.push(TrackPath { hexes, looping });
        Ok(())
    }

    //Reset follows the arm along the track it started on (the first track listing its hex), by
    //that track's own order, even where overlapping track actually moves the arm elsewhere
    pub fn start_position(&self, hex: Pos) -> Option<TrackPosition> {
        self.paths.iter().enumerate().find_map(|(path, p)| {
            let index = p.hexes.iter().position(|&h| h == hex)?;
            Some(TrackPosition { path, index, steps: 0 })
        })
    }
}

//Where an arm is along its starting track, and its net steps from the start
#[derive(Debug, Clone, Copy)]
pub struct TrackPosition {
    path: usize,
    index: usize,
    steps: i32,
}

impl TrackPosition {
    //Steps count only if they move along the track (not off a non-looping end)
    pub fn step(&mut self, maps: &TrackMaps, forward: bool) {
        let path = &maps.paths[self.path];
        let len = path.hexes.len();
        let next = match (forward, self.index) {
            (true, i) if i + 1 < len => i + 1,
            (true, _) if path.looping => 0,
            (false, 0) if path.looping => len - 1,
            (false, i) if i > 0 => i - 1,
            _ => return,
        };
        self.index = next;
        self.steps += if forward { 1 } else { -1 };
    }

    //Net forward steps to undo, going the shorter way round a loop (ties undo the steps taken)
    pub fn steps_to_return(&self, maps: &TrackMaps) -> i32 {
        let path = &maps.paths[self.path];
        let mut steps = self.steps;
        if path.looping {
            let loop_len = path.hexes.len() as i32;
            while steps > loop_len / 2 { steps -= loop_len; }
            while steps < -loop_len / 2 { steps += loop_len; }
        }
        steps
    }
}

//Two-hex tracks can't loop, and longer ones only loop if their ends are adjacent
pub fn path_loops(hexes: &[Pos]) -> bool {
    hexes.len() > 2 && pos_to_rot(hexes[0] - hexes[hexes.len() - 1]).is_some()
}

//Path text: one letter per step to a neighboring hex, from the keys around S (w e / a d / z x), indexed by Rot.
//Any other step is a jump, written (dx,dy).
const STEP_LETTERS: [char; 6] = ['d', 'e', 'w', 'a', 'z', 'x'];

pub fn path_to_text(hexes: &[Pos]) -> String {
    let mut text = String::new();
    for pair in hexes.windows(2) {
        let step = pair[1] - pair[0];
        match pos_to_rot(step) {
            Some(r) => text.push(STEP_LETTERS[r as usize]),
            None => text += &format!("({},{})", step.x, step.y),
        }
    }
    text
}

//Path text can't reach hexes further out than this, so positions stay far from overflowing
const MAX_PATH_COORD: i32 = 1 << 20;
//Reads path text into a path starting at `first`, keeping everything up to the first part it can't read.
//Letters can be either case, and whitespace is skipped.
pub fn parse_path_text(text: &str, first: Pos) -> Vec<Pos> {
    let mut chars = text.chars().filter(|c| !c.is_whitespace()).peekable();
    let mut path = vec![first];
    let mut at = first;
    while let Some(c) = chars.next() {
        let step = if c == '(' {
            match parse_jump(&mut chars) {
                Some(step) => step,
                None => break,
            }
        } else {
            match STEP_LETTERS.iter().position(|&l| l == c.to_ascii_lowercase()) {
                Some(r) => rot_to_pos(r as Rot),
                None => break,
            }
        };
        let next = at + step;
        if next.x.abs() > MAX_PATH_COORD || next.y.abs() > MAX_PATH_COORD {
            break;
        }
        at = next;
        path.push(at);
    }
    path
}

//The rest of a "(dx,dy)" jump after its "(". A jump to the same hex isn't a step.
fn parse_jump(chars: &mut Peekable<impl Iterator<Item=char>>) -> Option<Pos> {
    let dx = parse_int(chars)?;
    if chars.next() != Some(',') {return None;}
    let dy = parse_int(chars)?;
    if chars.next() != Some(')') {return None;}
    let step = Pos::new(dx, dy);
    (step != Pos::zeros()).then_some(step)
}
fn parse_int(chars: &mut Peekable<impl Iterator<Item=char>>) -> Option<i32> {
    let mut s = String::new();
    if let Some(sign) = chars.next_if(|&c| c == '-' || c == '+') {
        s.push(sign);
    }
    while let Some(d) = chars.next_if(|c| c.is_ascii_digit()) {
        s.push(d);
    }
    s.parse().ok().filter(|n: &i32| n.abs() <= MAX_PATH_COORD)
}

//How many steps in the path aren't to a neighboring hex
pub fn gap_count(hexes: &[Pos]) -> usize {
    hexes.windows(2).filter(|pair| pos_to_rot(pair[1] - pair[0]).is_none()).count()
}

//The ways a track's path can be reshaped on the board, a hex at a time
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TrackTool {
    AddFront, AddBack, Remove,
}

//Applies `tool` to the hex the cursor entered during a stroke over the board, returning whether the path changed.
//`added` counts the hexes this stroke added, which going back over undoes. A path keeps at least one hex.
//Without `allow_gaps` only neighboring hexes are added and only end hexes removed;
//without `allow_overlap` a hex already on the path isn't added again.
pub fn stroke_hex(path: &mut Vec<Pos>, added: &mut usize, tool: TrackTool, hex: Pos,
    allow_gaps: bool, allow_overlap: bool) -> std::result::Result<bool, &'static str>
{
    const NOT_NEXT: &str = "Not next to that end of the track (see \"Allow gaps\")";
    const OVERLAP: &str = "The track already covers that hex (see \"Allow overlap\")";
    let len = path.len();
    match tool {
        TrackTool::AddFront | TrackTool::AddBack => {
            let front = tool == TrackTool::AddFront;
            let (end, prev) = if front {(0, 1)} else {(len - 1, len.wrapping_sub(2))};
            if hex == path[end] {
                return Ok(false);
            }
            if *added > 0 && len >= 2 && hex == path[prev] {
                path.remove(end);
                *added -= 1;
                return Ok(true);
            }
            if pos_to_rot(hex - path[end]).is_none() && !allow_gaps {
                return Err(NOT_NEXT);
            }
            if path.contains(&hex) && !allow_overlap {
                return Err(OVERLAP);
            }
            if front {path.insert(0, hex)} else {path.push(hex)}
            *added += 1;
            Ok(true)
        }
        TrackTool::Remove => {
            if !path.contains(&hex) {
                return Ok(false);
            }
            if len == 1 {
                return Err("A track keeps at least one hex (delete it instead)");
            }
            let index = if path[len - 1] == hex {
                len - 1
            } else if path[0] == hex {
                0
            } else if allow_gaps {
                path.iter().position(|&h| h == hex).unwrap()
            } else {
                return Err("Removing a middle hex leaves a gap (see \"Allow gaps\")");
            };
            path.remove(index);
            Ok(true)
        }
    }
}
