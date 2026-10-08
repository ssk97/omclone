//Edits to the layout (the parts and where they start), and how parts are named
use super::*;

//An edit to one part of the layout: it moves by `delta`, then rotates by `rot` around `pivot`.
//Arms can also change length and type.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PartEdit{
    pub(super) part: PartId,
    pub(super) delta: Pos,
    pub(super) rot: Rot,
    pub(super) pivot: Pos,
    pub(super) len: i32,
    pub(super) arm_type: Option<ArmType>,
}
impl PartEdit{
    pub(super) fn new(part: PartId) -> Self{
        PartEdit{part, delta: Pos::zeros(), rot: 0, pivot: Pos::zeros(), len: 0, arm_type: None}
    }
    pub(super) fn is_noop(&self) -> bool{
        self.delta == Pos::zeros() && self.rot == 0 && self.len == 0 && self.arm_type.is_none()
    }
}
//Moves, rotates and resizes the part in place
fn edit_part(layout: &mut InitialWorld, edit: &PartEdit){
    let place = |pos: Pos| rotate_around(pos + edit.delta, edit.rot, edit.pivot);
    match edit.part{
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
            arm.len = if arm.arm_type.is_wheel() {1} else {(arm.len + edit.len).clamp(1, 3)};
        }
    }
}
//A part that can be added from the palette (track is drawn instead)
#[derive(Clone, Debug, PartialEq)]
pub(super) enum NewPart{
    Glyph(GlyphType),
    Arm(ArmType),
}
impl NewPart{
    pub(super) fn name(&self) -> String{
        match self{
            NewPart::Glyph(glyph_type) => glyph_name(glyph_type),
            NewPart::Arm(arm_type) => arm_type_name(*arm_type).to_owned(),
        }
    }
}
//A change to the layout
#[derive(Clone, Debug, PartialEq)]
pub(super) enum LayoutEdit{
    Part(PartEdit),
    //A new part at `pos`; arms start with an empty tape
    Add{part: NewPart, pos: Pos, rot: Rot, len: i32},
    //A copy of the edited part (an arm's copy has the same tape), with the edit applied to the copy
    Duplicate(PartEdit),
    Delete(PartId),
    MoveArm{from: usize, to: usize},
    //Gives a track this path (absolute hexes), or adds a new track if `glyph` is None
    SetTrack{glyph: Option<usize>, path: Vec<Pos>},
}
impl LayoutEdit{
    //The layout after the edit, and the part it placed or changed (none for a deletion)
    pub(super) fn apply(&self, world: &World) -> (InitialWorld, Option<PartId>){
        let mut layout = world.layout();
        let part = match self{
            LayoutEdit::Part(edit) => {
                edit_part(&mut layout, edit);
                Some(edit.part)
            }
            LayoutEdit::Add{part, pos, rot, len} => Some(match part{
                NewPart::Glyph(glyph_type) => {
                    layout.glyphs.push(Glyph{pos: *pos, rot: normalize_dir(*rot), glyph_type: glyph_type.clone()});
                    PartId::Glyph(layout.glyphs.len()-1)
                }
                NewPart::Arm(arm_type) => {
                    let len = if arm_type.is_wheel() {1} else {(*len).clamp(1, 3)};
                    layout.arms.push(Arm::new(*pos, *rot, len, *arm_type, Tape::new(0, Vec::new())));
                    PartId::Arm(layout.arms.len()-1)
                }
            }),
            LayoutEdit::Duplicate(edit) => {
                let copy = match edit.part{
                    PartId::Glyph(i) => {
                        layout.glyphs.push(layout.glyphs[i].clone());
                        PartId::Glyph(layout.glyphs.len()-1)
                    }
                    PartId::Arm(i) => {
                        layout.arms.push(layout.arms[i].clone());
                        PartId::Arm(layout.arms.len()-1)
                    }
                };
                edit_part(&mut layout, &PartEdit{part: copy, ..*edit});
                Some(copy)
            }
            LayoutEdit::Delete(part) => {
                match *part{
                    PartId::Glyph(i) => {layout.glyphs.remove(i);}
                    PartId::Arm(i) => {layout.arms.remove(i);}
                }
                None
            }
            LayoutEdit::MoveArm{from, to} => {
                let arm = layout.arms.remove(*from);
                layout.arms.insert(*to, arm);
                Some(PartId::Arm(*to))
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
                Some(PartId::Glyph(i))
            }
        };
        (layout, part)
    }
    //Whether parts can be renumbered or added, so the stack under the cursor may be out of date
    pub(super) fn is_structural(&self) -> bool{
        !matches!(self, LayoutEdit::Part(_) | LayoutEdit::SetTrack{glyph: Some(_), ..})
    }
}
//A track's hexes, in global positions
pub(super) fn track_path(world: &World, glyph: usize) -> Vec<Pos>{
    let g = &world.glyphs[glyph];
    match &g.glyph_type{
        GlyphType::Track(path) => path.iter().map(|&p| g.reposition(p)).collect(),
        _ => Vec::new(),
    }
}
//Inputs, outputs and conduits come with the puzzle, so they can't be added or removed
pub(super) fn is_puzzle_part(glyph_type: &GlyphType) -> bool{
    use GlyphType::*;
    matches!(glyph_type, Input(..) | Output(..) | OutputRepeating(..) | Conduit(..))
}
pub(super) const ALL_NEW_GLYPHS: [GlyphType; 16] = {
    use GlyphType::*;
    [Bonding, Unbonding, MultiBond, TriplexBond, Calcification, Duplication, Projection, Purification,
        Animismus, Disposal, Equilibrium, Dispersion, Unification, Rejection, Division, Proliferation]
};
pub(super) const ALL_ARM_TYPES: [ArmType; 7] = [ArmType::PlainArm, ArmType::DoubleArm, ArmType::TripleArm, ArmType::HexArm,
    ArmType::Piston, ArmType::VanBerlo, ArmType::Ravari];
pub(super) fn arm_type_name(arm_type: ArmType) -> &'static str{
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
pub(super) fn glyph_name(glyph_type: &GlyphType) -> String{
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
pub(super) fn part_label(world: &World, part: PartId) -> String{
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
        World::setup_sim(&InitialWorld { glyphs, arms }).unwrap()
    }
    fn edited(w: &World, edit: PartEdit) -> World {
        applied(w, LayoutEdit::Part(edit))
    }
    fn applied(w: &World, edit: LayoutEdit) -> World {
        World::setup_sim(&edit.apply(w).0).unwrap()
    }
    fn tape(w: &World, arm: usize) -> Vec<Instr> {
        w.arms[arm].instruction_tape.instructions.clone()
    }
    //Saves and loads the world, like the Save button and a reload
    fn round_trip(w: &World) -> World {
        let solution = parser::create_solution(w, String::from("puzzle"), String::from("solution"));
        let mut data = Vec::new();
        parser::write_solution(&mut data, &solution).unwrap();
        let parsed = parser::parse_solution(&mut data.as_slice()).unwrap();
        let puzzle = parser::FullPuzzle {
            puzzle_name: String::new(), creator_id: 0, allowed_bitfield: 0,
            inputs: vec![], outputs: vec![], output_multiplier: 1, production: false,
        };
        World::setup_sim(&parser::puzzle_prep(&puzzle, &parsed).unwrap()).unwrap()
    }

    #[test]
    fn rotating_while_dragging_keeps_the_held_hex() {
        let w = world(vec![glyph(0, 0, 0, GlyphType::Bonding)], vec![]);
        //Held by its second hex at (1,0), moved two hexes right, then rotated
        let held = Pos::new(3, 0);
        let edit = PartEdit { delta: Pos::new(2, 0), rot: 1, pivot: held, ..PartEdit::new(PartId::Glyph(0)) };
        let moved = edited(&w, edit);
        assert_eq!(moved.glyphs[0].rot, 1);
        assert!(moved.glyph_footprint(0).contains(&held));
        assert_eq!(moved.glyphs[0].pos, held - rotate(Pos::new(1, 0), 1));
    }

    #[test]
    fn tracks_rotate_their_path() {
        let path = vec![Pos::new(0, 0), Pos::new(1, 0), Pos::new(2, 0)];
        let w = world(vec![glyph(5, 5, 0, GlyphType::Track(path))], vec![]);
        let edit = PartEdit { rot: 2, pivot: Pos::new(5, 5), ..PartEdit::new(PartId::Glyph(0)) };
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
        let longer = edited(&w, PartEdit { len: 5, ..PartEdit::new(PartId::Arm(0)) });
        assert_eq!(longer.arms[0].len, 3);
        let wheel = edited(&w, PartEdit { len: 1, arm_type: Some(ArmType::VanBerlo), ..PartEdit::new(PartId::Arm(1)) });
        assert_eq!((wheel.arms[1].len, wheel.arms[1].arm_type), (1, ArmType::VanBerlo));
        assert_eq!(wheel.cost, w.cost + 10, "a wheel costs more than a plain arm");
        assert_eq!(wheel.arms[0].instruction_tape.instructions, w.arms[0].instruction_tape.instructions);
    }

    #[test]
    fn edits_survive_saving() {
        let conduit_a = glyph(0, 0, 0, GlyphType::Conduit(vec![Pos::new(0, 0), Pos::new(1, 0)], 7));
        let conduit_b = glyph(6, 0, 0, GlyphType::Conduit(vec![Pos::new(0, 0), Pos::new(1, 0)], 7));
        let track = glyph(0, 4, 0, GlyphType::Track(vec![Pos::new(0, 0), Pos::new(1, 0)]));
        let w = world(vec![conduit_a, conduit_b, track, glyph(3, 3, 0, GlyphType::Unification)],
            vec![arm(0, 4, 1, ArmType::PlainArm)]);
        let edits = [
            PartEdit { delta: Pos::new(0, -3), rot: 1, pivot: Pos::new(0, -3), ..PartEdit::new(PartId::Glyph(0)) },
            PartEdit { delta: Pos::new(1, 1), rot: -2, pivot: Pos::new(1, 5), ..PartEdit::new(PartId::Glyph(2)) },
            PartEdit { rot: 3, pivot: Pos::new(4, 3), ..PartEdit::new(PartId::Glyph(3)) },
            PartEdit { delta: Pos::new(2, 2), rot: -1, pivot: Pos::new(2, 6), len: 1, ..PartEdit::new(PartId::Arm(0)) },
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
        assert_eq!(edit.apply(&w).1, Some(PartId::Glyph(1)));
        let with_glyph = round_trip(&applied(&w, edit));
        assert_eq!((with_glyph.glyphs[1].pos, with_glyph.glyphs[1].rot), (Pos::new(3, 1), 2));

        let edit = LayoutEdit::Add{part: NewPart::Arm(ArmType::Piston), pos: Pos::new(-2, 0), rot: -1, len: 7};
        assert_eq!(edit.apply(&w).1, Some(PartId::Arm(1)));
        let with_arm = round_trip(&applied(&w, edit));
        let arm = &with_arm.arms[1];
        assert_eq!((arm.pos, arm.rot, arm.len, arm.arm_type), (Pos::new(-2, 0), -1, 3, ArmType::Piston));
        assert!(arm.instruction_tape.instructions.is_empty());
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
        assert_eq!(LayoutEdit::Delete(PartId::Arm(0)).apply(&w).1, None);
        let no_arm = round_trip(&applied(&w, LayoutEdit::Delete(PartId::Arm(0))));
        assert_eq!(no_arm.arms.len(), 1);
        assert_eq!((no_arm.arms[0].pos, tape(&no_arm, 0)), (Pos::new(3, 3), vec![Instr::Drop]));
        let no_glyph = applied(&w, LayoutEdit::Delete(PartId::Glyph(0)));
        assert_eq!(no_glyph.glyphs.len(), 1);
        assert_eq!(no_glyph.glyphs[0].glyph_type, GlyphType::Calcification);
    }

    #[test]
    fn duplicates_are_moved_copies() {
        let w = world(vec![glyph(0, 0, 0, GlyphType::Bonding)], vec![
            Arm::new(Pos::new(0, 3), 1, 2, ArmType::Piston, Tape::new(0, vec![Instr::Grab, Instr::Extend])),
        ]);
        let edit = LayoutEdit::Duplicate(PartEdit{delta: Pos::new(4, 0), ..PartEdit::new(PartId::Arm(0))});
        assert_eq!(edit.apply(&w).1, Some(PartId::Arm(1)));
        let copied = round_trip(&applied(&w, edit));
        assert_eq!(copied.arms[0].pos, Pos::new(0, 3), "the original stays");
        let copy = &copied.arms[1];
        assert_eq!((copy.pos, copy.rot, copy.len, copy.arm_type), (Pos::new(4, 3), 1, 2, ArmType::Piston));
        assert_eq!(tape(&copied, 1), tape(&w, 0));

        let edit = LayoutEdit::Duplicate(PartEdit{rot: 1, pivot: Pos::new(0, 0), ..PartEdit::new(PartId::Glyph(0))});
        let copied = applied(&w, edit);
        assert_eq!((copied.glyphs[0].rot, copied.glyphs[1].rot), (0, 1));
    }

    #[test]
    fn moved_arms_take_their_tapes() {
        let arms = (0..3).map(|i| Arm::new(Pos::new(i*2, 0), 0, 1, ArmType::PlainArm,
            Tape::new(i as usize, vec![Instr::Grab]))).collect();
        let w = world(vec![], arms);
        let edit = LayoutEdit::MoveArm{from: 0, to: 2};
        assert_eq!(edit.apply(&w).1, Some(PartId::Arm(2)));
        let moved = round_trip(&applied(&w, edit));
        let order: Vec<(i32, usize)> = moved.arms.iter().map(|a| (a.pos.x, a.instruction_tape.first)).collect();
        assert_eq!(order, vec![(2, 1), (4, 2), (0, 0)]);
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
        assert_eq!(edit.apply(&w).1, Some(PartId::Glyph(1)));
        let added = round_trip(&applied(&w, edit));
        assert_eq!(track_path(&added, 1), vec![Pos::new(-3, 0)]);
        assert_eq!(added.cost, w.cost + 5);
    }
}
