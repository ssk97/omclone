//Track movement, following the game's handling of overlapping and self-intersecting track
//(as decoded by omsim's decode.c, which builds it backwards with later writes winning).
use super::*;

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
        //Two-hex tracks can't loop, and longer ones only loop if their ends are adjacent
        let looping = hexes.len() > 2 && pos_to_rot(first - last).is_some();
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
