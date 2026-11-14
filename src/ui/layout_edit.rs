//Edits to the layout (the parts and where they start), and how parts are named
use super::*;

//An edit to some parts of the layout: each moves by `delta`, then rotates by `rot` around `pivot`. Arms can also
//change length (each clamped to what its type allows) and type.
//A lone arm is turned in place by giving its own start as `pivot`: at later timesteps it may be grabbed far from
//where it starts.
#[derive(Clone, Debug, PartialEq)]
pub struct PartEdit{
    pub parts: Vec<PartId>,
    pub delta: Pos,
    pub rot: Rot,
    pub pivot: Pos,
    pub len: i32,
    pub arm_type: Option<ArmType>,
}
impl PartEdit{
    pub fn new(parts: Vec<PartId>) -> Self{
        PartEdit{parts, delta: Pos::zeros(), rot: 0, pivot: Pos::zeros(), len: 0, arm_type: None}
    }
    pub fn is_noop(&self) -> bool{
        self.delta == Pos::zeros() && self.rot == 0 && self.len == 0 && self.arm_type.is_none()
    }
}
//Moves, rotates and resizes one part in place
fn edit_part(layout: &mut InitialWorld, part: PartId, edit: &PartEdit){
    let place = |pos: Pos| rotate_around(pos + edit.delta, edit.rot, edit.pivot);
    match part{
        PartId::Glyph(i) => {
            let glyph = &mut layout.glyphs[i];
            glyph.pos = place(glyph.pos);
            if let GlyphType::Track(path) = &mut glyph.glyph_type{
                //Track rotation is ignored when loading (like omsim), so the path itself rotates
                for p in path{
                    *p = rotate(*p, edit.rot);
                }
            } else if edit.rot != 0{
                glyph.rot = normalize_dir(glyph.rot + edit.rot);
            }
        }
        PartId::Arm(i) => {
            let arm = &mut layout.arms[i];
            arm.pos = place(arm.pos);
            arm.rot = (arm.rot + edit.rot) % 6;
            if let Some(arm_type) = edit.arm_type{
                arm.arm_type = arm_type;
            }
            arm.len = arm.arm_type.clamp_len(arm.len + edit.len);
        }
    }
}
//A part that can be added from the palette (track is drawn instead)
#[derive(Clone, Debug, PartialEq)]
pub enum NewPart{
    Glyph(GlyphType),
    Arm(ArmType),
}
impl NewPart{
    pub fn name(&self) -> String{
        match self{
            NewPart::Glyph(glyph_type) => glyph_name(glyph_type),
            NewPart::Arm(arm_type) => arm_type_name(*arm_type).to_owned(),
        }
    }
}
//A change to the layout
#[derive(Clone, Debug, PartialEq)]
pub enum LayoutEdit{
    Part(PartEdit),
    //A new part at `pos`; arms start with an empty tape
    Add{part: NewPart, pos: Pos, rot: Rot, len: i32},
    //Copies of the edited parts (an arm's copy has the same tape), with the edit applied to the copies. The copies
    //go last, in the order of `parts`.
    Duplicate(PartEdit),
    Delete(BTreeSet<PartId>),
    //Takes the parts of this kind numbered `moved` (as indexes, in order) out, and puts them back in that order
    //starting at index `to` among the rest. Nothing changes position on the board.
    Reorder{kind: PartKind, moved: Vec<usize>, to: usize},
    //Gives a track this path (absolute hexes), or adds a new track if `glyph` is None
    SetTrack{glyph: Option<usize>, path: Vec<Pos>},
}
impl LayoutEdit{
    //The layout after the edit, and the parts it placed or changed (none for a deletion or a reorder)
    pub fn apply(&self, world: &World) -> (InitialWorld, Vec<PartId>){
        let mut layout = world.layout();
        let parts = match self{
            LayoutEdit::Part(edit) => {
                for &part in &edit.parts{
                    edit_part(&mut layout, part, edit);
                }
                edit.parts.clone()
            }
            LayoutEdit::Add{part, pos, rot, len} => vec![match part{
                NewPart::Glyph(glyph_type) => {
                    layout.glyphs.push(Glyph{pos: *pos, rot: normalize_dir(*rot), glyph_type: glyph_type.clone()});
                    PartId::Glyph(layout.glyphs.len()-1)
                }
                NewPart::Arm(arm_type) => {
                    let len = arm_type.clamp_len(*len);
                    layout.arms.push(Arm::new(*pos, *rot, len, *arm_type, Tape::new(0, Vec::new())));
                    PartId::Arm(layout.arms.len()-1)
                }
            }],
            LayoutEdit::Duplicate(edit) => {
                let copies: Vec<PartId> = edit.parts.iter().map(|&part| match part{
                    PartId::Glyph(i) => {
                        layout.glyphs.push(layout.glyphs[i].clone());
                        PartId::Glyph(layout.glyphs.len()-1)
                    }
                    PartId::Arm(i) => {
                        layout.arms.push(layout.arms[i].clone());
                        PartId::Arm(layout.arms.len()-1)
                    }
                }).collect();
                for &copy in &copies{
                    edit_part(&mut layout, copy, edit);
                }
                copies
            }
            LayoutEdit::Delete(parts) => {
                let (mut glyphs, mut arms) = (Vec::new(), Vec::new());
                for &part in parts{
                    match part {PartId::Glyph(i) => glyphs.push(i), PartId::Arm(i) => arms.push(i)}
                }
                remove_all(&mut layout.glyphs, glyphs);
                remove_all(&mut layout.arms, arms);
                Vec::new()
            }
            LayoutEdit::Reorder{kind: PartKind::Glyph, moved, to} => {
                move_block(&mut layout.glyphs, moved, *to);
                Vec::new()
            }
            LayoutEdit::Reorder{kind: PartKind::Arm, moved, to} => {
                move_block(&mut layout.arms, moved, *to);
                Vec::new()
            }
            LayoutEdit::SetTrack{glyph, path} => {
                //The track's position is its first hex
                let pos = path[0];
                let track = Glyph{pos, rot: 0, glyph_type: GlyphType::Track(path.iter().map(|&p| p - pos).collect())};
                let i = match glyph{
                    Some(i) => {
                        layout.glyphs[*i] = track;
                        *i
                    }
                    None => {
                        layout.glyphs.push(track);
                        layout.glyphs.len()-1
                    }
                };
                vec![PartId::Glyph(i)]
            }
        };
        (layout, parts)
    }
    //Whether it changes nothing, so it isn't worth a step of undo
    pub fn is_noop(&self) -> bool{
        match self{
            LayoutEdit::Part(edit) => edit.is_noop(),
            LayoutEdit::Delete(parts) => parts.is_empty(),
            LayoutEdit::Reorder{moved, ..} => moved.is_empty(),
            LayoutEdit::Add{..} | LayoutEdit::Duplicate(_) | LayoutEdit::SetTrack{..} => false,
        }
    }
    //Whether it adds or removes parts, so the parts it returns are selected instead of the selection kept
    pub fn is_structural(&self) -> bool{
        !matches!(self, LayoutEdit::Part(_) | LayoutEdit::SetTrack{glyph: Some(_), ..} | LayoutEdit::Reorder{..})
    }
    //Whether parts it keeps may have other numbers afterwards (see renumbered)
    pub fn renumbers(&self) -> bool{
        matches!(self, LayoutEdit::Reorder{..} | LayoutEdit::Delete(_))
    }
    //The number that a part numbered `part` before the edit has after it, or None if the edit deletes it
    pub fn renumbered(&self, part: PartId) -> Option<PartId>{
        let i = part.index();
        match self{
            LayoutEdit::Delete(parts) => {
                if parts.contains(&part){
                    return None;
                }
                let deleted_below = parts.iter().filter(|p| p.kind() == part.kind() && p.index() < i).count();
                Some(part.kind().part(i - deleted_below))
            }
            //As in apply: the block goes in at `to` among the rest, which keep their order
            LayoutEdit::Reorder{kind, moved, to} if *kind == part.kind() => {
                let now = match moved.iter().position(|&m| m == i){
                    Some(place) => to + place,
                    None => {
                        let among_rest = i - moved.iter().filter(|&&m| m < i).count();
                        if among_rest < *to {among_rest} else {among_rest + moved.len()}
                    }
                };
                Some(kind.part(now))
            }
            _ => Some(part),
        }
    }
}
//Removes the items at these indexes, which are in increasing order
fn remove_all<T>(items: &mut Vec<T>, indexes: Vec<usize>){
    //From the back, so each removal leaves the indexes still to go where they were
    for i in indexes.into_iter().rev(){
        items.remove(i);
    }
}
//Takes the items at `moved` (indexes in increasing order) out and puts them back in order, starting at index `to`
//among the rest
fn move_block<T>(items: &mut Vec<T>, moved: &[usize], to: usize){
    let mut taken: Vec<T> = moved.iter().rev().map(|&i| items.remove(i)).collect();
    taken.reverse();
    items.splice(to..to, taken);
}
//The `to` of a Reorder that puts the parts numbered `moved` (indexes in increasing order) directly after `anchor`
//(or before it, if not `after`), in their order, with the other parts keeping theirs. None if that changes nothing.
pub fn reorder_block(moved: &[usize], anchor: usize, after: bool) -> Option<usize>{
    if moved.is_empty() || moved.contains(&anchor){
        return None;
    }
    //Taking out the parts before the anchor shifts it down
    let anchor_among_rest = anchor - moved.iter().filter(|&&m| m < anchor).count();
    let to = if after {anchor_among_rest + 1} else {anchor_among_rest};
    //Only a run of consecutive numbers put back where it was stays the same
    let unchanged = moved.windows(2).all(|w| w[1] == w[0] + 1) && moved.first() == Some(&to);
    (!unchanged).then_some(to)
}
//A track's hexes, in global positions
pub fn track_path(world: &World, glyph: usize) -> Vec<Pos>{
    world.glyphs[glyph].footprint(None).into_vec()
}
//Inputs, outputs and conduits come with the puzzle, so they can't be added or removed
pub fn is_puzzle_part(glyph_type: &GlyphType) -> bool{
    use GlyphType::*;
    matches!(glyph_type, Input(..) | Output(..) | OutputRepeating(..) | Conduit(..))
}
pub const ALL_NEW_GLYPHS: [GlyphType; 16] = {
    use GlyphType::*;
    [Bonding, Unbonding, MultiBond, TriplexBond, Calcification, Duplication, Projection, Purification,
        Animismus, Disposal, Equilibrium, Dispersion, Unification, Rejection, Division, Proliferation]
};
pub const ALL_ARM_TYPES: [ArmType; 7] = [ArmType::PlainArm, ArmType::DoubleArm, ArmType::TripleArm, ArmType::HexArm,
    ArmType::Piston, ArmType::VanBerlo, ArmType::Ravari];
pub fn arm_type_name(arm_type: ArmType) -> &'static str{
    match arm_type{
        ArmType::PlainArm => "Arm",
        ArmType::DoubleArm => "Double arm",
        ArmType::TripleArm => "Triple arm",
        ArmType::HexArm => "Hex arm",
        ArmType::Piston => "Piston",
        ArmType::VanBerlo => "Van Berlo's wheel",
        ArmType::Ravari => "Ravari's wheel",
    }
}
fn glyph_name(glyph_type: &GlyphType) -> String{
    use GlyphType::*;
    match glyph_type{
        Calcification => "Calcification".to_owned(),
        Animismus => "Animismus".to_owned(),
        Projection => "Projection".to_owned(),
        Dispersion => "Dispersion".to_owned(),
        Purification => "Purification".to_owned(),
        Duplication => "Duplication".to_owned(),
        Unification => "Unification".to_owned(),
        Bonding => "Bonder".to_owned(),
        Unbonding => "Unbonder".to_owned(),
        TriplexBond => "Triplex bonder".to_owned(),
        MultiBond => "Multi-bonder".to_owned(),
        Disposal => "Disposal".to_owned(),
        Equilibrium => "Equilibrium".to_owned(),
        Rejection => "Rejection".to_owned(),
        Division => "Division".to_owned(),
        Proliferation => "Proliferation".to_owned(),
        Track(_) => "Track".to_owned(),
        Conduit(_, id) => format!("Conduit {id}"),
        Input(_, id) => format!("Input {id}"),
        Output(_, _, id) => format!("Output {id}"),
        OutputRepeating(_, _, id) => format!("Polymer output {id}"),
    }
}
pub fn part_label(world: &World, part: PartId) -> String{
    match part{
        PartId::Arm(i) => format!("Arm {} ({})", i+1, arm_type_name(world.arms[i].arm_type)),
        PartId::Glyph(i) => format!("{} (glyph {})", glyph_name(&world.glyphs[i].glyph_type), i+1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glyph(x: i32, y: i32, rot: Rot, glyph_type: GlyphType) -> Glyph {
        Glyph { pos: Pos::new(x, y), rot, glyph_type }
    }
    fn arm(x: i32, y: i32, len: i32, arm_type: ArmType) -> Arm {
        Arm::new(Pos::new(x, y), 0, len, arm_type, Tape::new(0, vec![Instr::RotateClockwise]))
    }
    fn world(glyphs: Vec<Glyph>, arms: Vec<Arm>) -> World {
        World::setup_sim(&InitialWorld { glyphs, arms, ..Default::default() }).unwrap()
    }
    fn edited(w: &World, edit: PartEdit) -> World {
        applied(w, LayoutEdit::Part(edit))
    }
    fn applied(w: &World, edit: LayoutEdit) -> World {
        World::setup_sim(&edit.apply(w).0).unwrap()
    }
    fn tape(w: &World, arm: usize) -> Vec<Instr> {
        w.arms[arm].tape().instructions.clone()
    }
    //Saves and loads the world, like the Save button and a reload
    fn round_trip(w: &World) -> World {
        let solution = parser::create_solution(w, String::from("puzzle"), String::from("solution"));
        let mut data = Vec::new();
        parser::write_solution(&mut data, &solution).unwrap();
        let parsed = parser::parse_solution(&mut data.as_slice()).unwrap();
        let puzzle = parser::FullPuzzle {
            puzzle_name: String::new(), creator_id: 0, allowed_bitfield: 0,
            inputs: vec![], outputs: vec![], output_multiplier: 1, chambers: Chambers::default(), conduits: vec![],
        };
        World::setup_sim(&parser::puzzle_prep(&puzzle, &parsed).unwrap()).unwrap()
    }

    #[test]
    fn rotating_while_dragging_keeps_the_held_hex() {
        let w = world(vec![glyph(0, 0, 0, GlyphType::Bonding)], vec![]);
        //Held by its second hex at (1,0), moved two hexes right, then rotated
        let held = Pos::new(3, 0);
        let edit = PartEdit { delta: Pos::new(2, 0), rot: 1, pivot: held, ..PartEdit::new(vec![PartId::Glyph(0)]) };
        let moved = edited(&w, edit);
        assert_eq!(moved.glyphs[0].rot, 1);
        assert!(moved.glyph_footprint(0).contains(&held));
        assert_eq!(moved.glyphs[0].pos, held - rotate(Pos::new(1, 0), 1));
    }

    #[test]
    fn tracks_rotate_their_path() {
        let path = vec![Pos::new(0, 0), Pos::new(1, 0), Pos::new(2, 0)];
        let w = world(vec![glyph(5, 5, 0, GlyphType::Track(path))], vec![]);
        let edit = PartEdit { rot: 2, pivot: Pos::new(5, 5), ..PartEdit::new(vec![PartId::Glyph(0)]) };
        let rotated = edited(&w, edit);
        assert_eq!(rotated.glyphs[0].rot, 0);
        let expected: Vec<Pos> = (0..3).map(|i| Pos::new(5, 5) + rotate(Pos::new(i, 0), 2)).collect();
        assert_eq!(rotated.glyph_footprint(0).to_vec(), expected);
        //The track moves arms along the new path
        assert_eq!(rotated.track_maps.plus.get(&Pos::new(5, 5)), Some(&rotate(Pos::new(1, 0), 2)));
    }

    #[test]
    fn arm_edits_clamp_length() {
        let w = world(vec![], vec![arm(0, 0, 2, ArmType::Piston), arm(4, 0, 1, ArmType::PlainArm)]);
        let longer = edited(&w, PartEdit { len: 5, ..PartEdit::new(vec![PartId::Arm(0)]) });
        assert_eq!(longer.arms[0].len, 3);
        let wheel = edited(&w, PartEdit { len: 1, arm_type: Some(ArmType::VanBerlo), ..PartEdit::new(vec![PartId::Arm(1)]) });
        assert_eq!((wheel.arms[1].len, wheel.arms[1].arm_type), (1, ArmType::VanBerlo));
        assert_eq!(wheel.cost, w.cost + 10, "a wheel costs more than a plain arm");
        assert_eq!(tape(&wheel, 0), tape(&w, 0));
    }

    #[test]
    fn edits_survive_saving() {
        let conduit_a = glyph(0, 0, 0, GlyphType::Conduit(vec![Pos::new(0, 0), Pos::new(1, 0)], 7));
        let conduit_b = glyph(6, 0, 0, GlyphType::Conduit(vec![Pos::new(0, 0), Pos::new(1, 0)], 7));
        let track = glyph(0, 4, 0, GlyphType::Track(vec![Pos::new(0, 0), Pos::new(1, 0)]));
        let w = world(vec![conduit_a, conduit_b, track, glyph(3, 3, 0, GlyphType::Unification)],
            vec![arm(0, 4, 1, ArmType::PlainArm)]);
        let edits = [
            PartEdit { delta: Pos::new(0, -3), rot: 1, pivot: Pos::new(0, -3), ..PartEdit::new(vec![PartId::Glyph(0)]) },
            PartEdit { delta: Pos::new(1, 1), rot: -2, pivot: Pos::new(1, 5), ..PartEdit::new(vec![PartId::Glyph(2)]) },
            PartEdit { rot: 3, pivot: Pos::new(4, 3), ..PartEdit::new(vec![PartId::Glyph(3)]) },
            PartEdit { delta: Pos::new(2, 2), rot: -1, pivot: Pos::new(2, 6), len: 1, ..PartEdit::new(vec![PartId::Arm(0)]) },
        ];
        let mut w = w;
        for edit in edits {
            w = edited(&w, edit);
        }
        let loaded = round_trip(&w);
        for i in 0..w.glyphs.len() {
            assert_eq!(loaded.glyph_footprint(i), w.glyph_footprint(i), "glyph {i}");
        }
        assert_eq!(loaded.conduit_pairs, w.conduit_pairs);
        assert_eq!(loaded.track_maps.plus, w.track_maps.plus);
        let a = (&loaded.arms[0], &w.arms[0]);
        assert_eq!((a.0.pos, a.0.rot, a.0.len), (a.1.pos, a.1.rot, a.1.len));
    }

    #[test]
    fn added_parts_go_last() {
        let w = world(vec![glyph(0, 0, 0, GlyphType::Bonding)], vec![arm(5, 5, 1, ArmType::PlainArm)]);
        let edit = LayoutEdit::Add{part: NewPart::Glyph(GlyphType::Calcification), pos: Pos::new(3, 1), rot: 2, len: 1};
        assert_eq!(edit.apply(&w).1, vec![PartId::Glyph(1)]);
        let with_glyph = round_trip(&applied(&w, edit));
        assert_eq!((with_glyph.glyphs[1].pos, with_glyph.glyphs[1].rot), (Pos::new(3, 1), 2));

        let edit = LayoutEdit::Add{part: NewPart::Arm(ArmType::Piston), pos: Pos::new(-2, 0), rot: -1, len: 7};
        assert_eq!(edit.apply(&w).1, vec![PartId::Arm(1)]);
        let with_arm = round_trip(&applied(&w, edit));
        let arm = &with_arm.arms[1];
        assert_eq!((arm.pos, arm.rot, arm.len, arm.arm_type), (Pos::new(-2, 0), -1, 3, ArmType::Piston));
        assert!(arm.tape().instructions.is_empty());
        assert_eq!(tape(&with_arm, 0), tape(&w, 0));

        let wheel = applied(&w, LayoutEdit::Add{part: NewPart::Arm(ArmType::VanBerlo), pos: Pos::new(9, 0), rot: 0, len: 3});
        assert_eq!(wheel.arms[1].len, 1);
    }

    #[test]
    fn deleting_renumbers_later_parts() {
        let w = world(vec![glyph(0, 0, 0, GlyphType::Bonding), glyph(4, 0, 0, GlyphType::Calcification)], vec![
            Arm::new(Pos::new(0, 3), 0, 1, ArmType::PlainArm, Tape::new(0, vec![Instr::Grab])),
            Arm::new(Pos::new(3, 3), 0, 1, ArmType::PlainArm, Tape::new(0, vec![Instr::Drop])),
        ]);
        assert_eq!(LayoutEdit::Delete([PartId::Arm(0)].into()).apply(&w).1, vec![]);
        let no_arm = round_trip(&applied(&w, LayoutEdit::Delete([PartId::Arm(0)].into())));
        assert_eq!(no_arm.arms.len(), 1);
        assert_eq!((no_arm.arms[0].pos, tape(&no_arm, 0)), (Pos::new(3, 3), vec![Instr::Drop]));
        let no_glyph = applied(&w, LayoutEdit::Delete([PartId::Glyph(0)].into()));
        assert_eq!(no_glyph.glyphs.len(), 1);
        assert_eq!(no_glyph.glyphs[0].glyph_type, GlyphType::Calcification);
    }

    #[test]
    fn duplicates_are_moved_copies() {
        let w = world(vec![glyph(0, 0, 0, GlyphType::Bonding)], vec![
            Arm::new(Pos::new(0, 3), 1, 2, ArmType::Piston, Tape::new(0, vec![Instr::Grab, Instr::Extend])),
        ]);
        let edit = LayoutEdit::Duplicate(PartEdit{delta: Pos::new(4, 0), ..PartEdit::new(vec![PartId::Arm(0)])});
        assert_eq!(edit.apply(&w).1, vec![PartId::Arm(1)]);
        let copied = round_trip(&applied(&w, edit));
        assert_eq!(copied.arms[0].pos, Pos::new(0, 3), "the original stays");
        let copy = &copied.arms[1];
        assert_eq!((copy.pos, copy.rot, copy.len, copy.arm_type), (Pos::new(4, 3), 1, 2, ArmType::Piston));
        assert_eq!(tape(&copied, 1), tape(&w, 0));

        let edit = LayoutEdit::Duplicate(PartEdit{rot: 1, pivot: Pos::new(0, 0), ..PartEdit::new(vec![PartId::Glyph(0)])});
        let copied = applied(&w, edit);
        assert_eq!((copied.glyphs[0].rot, copied.glyphs[1].rot), (0, 1));
    }

    #[test]
    fn a_group_turns_together_about_the_pivot() {
        let w = world(vec![glyph(0, 0, 0, GlyphType::Bonding), glyph(5, 5, 0, GlyphType::Calcification)],
            vec![arm(2, 0, 1, ArmType::PlainArm)]);
        let group = vec![PartId::Glyph(0), PartId::Arm(0)];
        let edit = PartEdit{delta: Pos::new(0, 1), rot: 1, pivot: Pos::new(0, 1), ..PartEdit::new(group.clone())};
        let turned = edited(&w, edit.clone());
        assert_eq!((turned.glyphs[0].pos, turned.glyphs[0].rot), (Pos::new(0, 1), 1));
        assert_eq!((turned.arms[0].pos, turned.arms[0].rot), (Pos::new(0, 1) + rotate(Pos::new(2, 0), 1), 1));
        assert_eq!(turned.glyphs[1].pos, Pos::new(5, 5), "only the group moves");

        let copied = applied(&w, LayoutEdit::Duplicate(edit));
        assert_eq!((copied.glyphs.len(), copied.arms.len()), (3, 2));
        assert_eq!((copied.glyphs[2].pos, copied.arms[1].pos), (turned.glyphs[0].pos, turned.arms[0].pos));
        assert_eq!(tape(&copied, 1), tape(&w, 0));
        assert_eq!((copied.glyphs[0].pos, copied.arms[0].pos), (Pos::new(0, 0), Pos::new(2, 0)), "the originals stay");
    }

    #[test]
    fn reordered_arms_take_their_tapes() {
        let arms = (0..3).map(|i| Arm::new(Pos::new(i*2, 0), 0, 1, ArmType::PlainArm,
            Tape::new(i as usize, vec![Instr::Grab]))).collect();
        let w = world(vec![], arms);
        let edit = LayoutEdit::Reorder{kind: PartKind::Arm, moved: vec![0], to: 2};
        let moved = round_trip(&applied(&w, edit));
        let order: Vec<(i32, usize)> = moved.arms.iter().map(|a| (a.pos.x, a.tape().first)).collect();
        assert_eq!(order, vec![(2, 1), (4, 2), (0, 0)]);
    }

    #[test]
    fn reordered_glyphs_are_saved_in_their_new_order() {
        let w = world(vec![glyph(0, 0, 0, GlyphType::Bonding), glyph(0, 0, 0, GlyphType::Calcification),
            glyph(0, 0, 0, GlyphType::Disposal)], vec![]);
        let edit = LayoutEdit::Reorder{kind: PartKind::Glyph, moved: vec![2], to: 0};
        let reordered = round_trip(&applied(&w, edit));
        let types: Vec<GlyphType> = reordered.glyphs.iter().map(|g| g.glyph_type.clone()).collect();
        assert_eq!(types, vec![GlyphType::Disposal, GlyphType::Bonding, GlyphType::Calcification]);
    }

    #[test]
    fn renumbered_parts_are_where_the_edit_puts_them() {
        //Each part has its own position, to tell them apart
        let glyphs = (0..5).map(|i| glyph(i * 3, 0, 0, GlyphType::Bonding)).collect();
        let arms = (0..5).map(|i| arm(i * 3, 5, 1, ArmType::PlainArm)).collect();
        let w = world(glyphs, arms);
        let pos = |layout: &InitialWorld, part| match part {
            PartId::Glyph(i) => layout.glyphs[i].pos,
            PartId::Arm(i) => layout.arms[i].pos,
        };
        let parts: Vec<PartId> = (0..5).map(PartId::Glyph).chain((0..5).map(PartId::Arm)).collect();
        use LayoutEdit::*;
        let reorder = |kind, moved: &[usize], to| Reorder{kind, moved: moved.to_vec(), to};
        for edit in [reorder(PartKind::Glyph, &[0], 2), reorder(PartKind::Glyph, &[3], 1),
            reorder(PartKind::Arm, &[1], 3), reorder(PartKind::Arm, &[2], 0), reorder(PartKind::Glyph, &[0, 2, 3], 1),
            reorder(PartKind::Arm, &[1, 4], 0), reorder(PartKind::Arm, &[0, 1], 3),
            Delete([PartId::Glyph(1)].into()), Delete([PartId::Arm(2)].into()),
            Delete([PartId::Glyph(3), PartId::Arm(0), PartId::Glyph(1), PartId::Arm(4)].into())]
        {
            let after = edit.apply(&w).0;
            for &part in &parts {
                match edit.renumbered(part) {
                    Some(now) => assert_eq!(pos(&after, now), pos(&w.layout(), part), "{edit:?} moves {part:?}"),
                    None => assert!(matches!(&edit, Delete(deleted) if deleted.contains(&part)), "only a deleted part is gone"),
                }
            }
        }
    }

    #[test]
    fn reordering_overlapping_tracks_changes_which_one_wins() {
        //They cross at (1,0): the first goes right from there, the second up
        let across = glyph(0, 0, 0, GlyphType::Track(vec![Pos::new(0, 0), Pos::new(1, 0), Pos::new(2, 0)]));
        let up = glyph(1, -1, 0, GlyphType::Track(vec![Pos::new(0, 0), Pos::new(0, 1), Pos::new(0, 2)]));
        let w = world(vec![across, up], vec![]);
        let crossing = Pos::new(1, 0);
        assert_eq!(w.track_maps.plus.get(&crossing), Some(&Pos::new(1, 0)));
        let reordered = applied(&w, LayoutEdit::Reorder{kind: PartKind::Glyph, moved: vec![1], to: 0});
        assert_eq!(reordered.track_maps.plus.get(&crossing), Some(&Pos::new(0, 1)));
    }

    #[test]
    fn reorder_block_puts_the_parts_next_to_the_anchor() {
        //Glyphs 0..6, told apart by x; each case is the order of the glyphs (by their old numbers) afterwards
        let w = world((0..6).map(|i| glyph(i * 3, 0, 0, GlyphType::Bonding)).collect(), vec![]);
        let order = |moved: &[usize], anchor: usize, after: bool| {
            let to = reorder_block(moved, anchor, after)?;
            let edit = LayoutEdit::Reorder{kind: PartKind::Glyph, moved: moved.to_vec(), to};
            Some(edit.apply(&w).0.glyphs.iter().map(|g| (g.pos.x / 3) as usize).collect::<Vec<_>>())
        };
        assert_eq!(order(&[1], 4, true), Some(vec![0, 2, 3, 4, 1, 5]));
        assert_eq!(order(&[1], 4, false), Some(vec![0, 2, 3, 1, 4, 5]));
        assert_eq!(order(&[4], 1, true), Some(vec![0, 1, 4, 2, 3, 5]));
        assert_eq!(order(&[4], 1, false), Some(vec![0, 4, 1, 2, 3, 5]));
        assert_eq!(order(&[5], 0, false), Some(vec![5, 0, 1, 2, 3, 4]));
        assert_eq!(order(&[0], 5, true), Some(vec![1, 2, 3, 4, 5, 0]));
        assert_eq!(order(&[0, 3], 5, false), Some(vec![1, 2, 4, 0, 3, 5]), "a block keeps its order");
        assert_eq!(order(&[1, 4], 2, true), Some(vec![0, 2, 1, 4, 3, 5]), "from either side of the anchor");
        assert_eq!(order(&[2, 4], 3, true), Some(vec![0, 1, 3, 2, 4, 5]), "gathered after a part between them");
        assert_eq!(order(&[], 2, true), None, "nothing to move");
        assert_eq!(order(&[2], 2, true), None, "beside itself");
        assert_eq!(order(&[2, 4], 4, true), None, "beside one of the block");
        assert_eq!(order(&[2], 3, false), None, "already just before it");
        assert_eq!(order(&[2], 1, true), None, "already just after it");
        assert_eq!(order(&[2, 3], 4, false), None, "a run already just before it");
    }

    #[test]
    fn set_track_starts_at_its_first_hex() {
        let w = world(vec![glyph(1, 1, 0, GlyphType::Track(vec![Pos::new(-1, 0), Pos::new(0, 0)]))], vec![]);
        let path = vec![Pos::new(5, 5), Pos::new(6, 5), Pos::new(6, 6), Pos::new(9, 9)];
        let reshaped = round_trip(&applied(&w, LayoutEdit::SetTrack{glyph: Some(0), path: path.clone()}));
        assert_eq!(reshaped.glyphs[0].pos, Pos::new(5, 5));
        assert_eq!(track_path(&reshaped, 0), path);
        assert_eq!(reshaped.track_maps.plus.get(&Pos::new(6, 6)), Some(&Pos::new(3, 3)));

        let edit = LayoutEdit::SetTrack{glyph: None, path: vec![Pos::new(-3, 0)]};
        assert_eq!(edit.apply(&w).1, vec![PartId::Glyph(1)]);
        let added = round_trip(&applied(&w, edit));
        assert_eq!(track_path(&added, 1), vec![Pos::new(-3, 0)]);
        assert_eq!(added.cost, w.cost + 5);
    }
}
