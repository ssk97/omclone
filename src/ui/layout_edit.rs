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
pub(super) fn edited_layout(world: &World, edit: &PartEdit) -> InitialWorld{
    let mut layout = world.layout();
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
    layout
}
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
pub(super) fn part_label(world: &World, part: PartId) -> String{
    use GlyphType::*;
    match part{
        PartId::Arm(i) => format!("Arm {} ({})", i+1, arm_type_name(world.arms[i].arm_type)),
        PartId::Glyph(i) => {
            let name = match &world.glyphs[i].glyph_type{
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
            };
            format!("{name} (glyph {})", i+1)
        }
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
        World::setup_sim(&edited_layout(w, &edit)).unwrap()
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
}
