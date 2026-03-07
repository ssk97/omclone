use crate::sim::*;

#[cfg(feature = "color_eyre")]
use color_eyre::{
    eyre::{bail, ensure, eyre},
    Result,
};
#[cfg(not(feature = "color_eyre"))]
use simple_eyre::{
    eyre::{bail, ensure, eyre},
    Result,
};

use num_traits::FromPrimitive;
use std::collections::HashMap;
use std::io::{Read, Write};
use smallvec::smallvec;

type AllowedParts = u64;
/*use bitflags::bitflags;
bitflags! {
    #[repr(transparent)]
    struct AllowedParts: u64 {
        const ALLOW_ARM                 = 1<<0 ;
        const ALLOW_MULTIARM            = 1<<1 ;
        const ALLOW_PISTON              = 1<<2 ;
        const ALLOW_TRACK               = 1<<3 ;
        const ALLOW_BONDER              = 1<<7 ;
        const ALLOW_UNBONDER            = 1<<8 ;
        const ALLOW_MULTIBONDER         = 1<<9 ;
        const ALLOW_TRIPLEX_BONDER      = 1<<10;
        const ALLOW_CALCIFICATION       = 1<<11;
        const ALLOW_DUPLICATION         = 1<<12;
        const ALLOW_PROJECTION          = 1<<13;
        const ALLOW_PURIFICATION        = 1<<14;
        const ALLOW_ANIMISMUS           = 1<<15;
        const ALLOW_DISPOSAL            = 1<<16;
        const ALLOW_QUINTESSENCE_GLYPHS = 1<<17;
        const ALLOW_GRAB_AND_ROTATION   = 1<<22;
        const ALLOW_DROP                = 1<<23;
        const ALLOW_RESET               = 1<<24;
        const ALLOW_REPEAT              = 1<<25;
        const ALLOW_PIVOT               = 1<<26;
        const ALLOW_BERLOS_WHEEL        = 1<<28;
    }
}*/

fn expect_arr<const N: usize>(f: &mut impl Read, expected: [u8; N]) -> Result<()> {
    let mut dat = [0u8; N];
    f.read_exact(&mut dat)?;
    ensure!(
        dat == expected,
        "Expected value {:?} but read {:?}",
        expected,
        dat
    );
    Ok(())
}
fn expect_byte(f: &mut impl Read, expected: u8) -> Result<()> {
    expect_arr(f, expected.to_le_bytes())
}
fn expect_int(f: &mut impl Read, expected: i32) -> Result<()> {
    expect_arr(f, expected.to_le_bytes())
}

fn parse_byte(f: &mut impl Read) -> Result<u8> {
    let mut dat = [0u8; 1];
    f.read_exact(&mut dat)?;
    Ok(dat[0])
}
fn parse_int(f: &mut impl Read) -> Result<i32> {
    let mut dat = [0u8; 4];
    f.read_exact(&mut dat)?;
    Ok(i32::from_le_bytes(dat))
}
fn parse_long(f: &mut impl Read) -> Result<u64> {
    let mut dat = [0u8; 8];
    f.read_exact(&mut dat)?;
    Ok(u64::from_le_bytes(dat))
}
fn parse_pos(f: &mut impl Read) -> Result<Pos> {
    let x = parse_int(f)?;
    let y = parse_int(f)?;
    Ok(Pos::new(x, y))
}
fn parse_bytepos(f: &mut impl Read) -> Result<Pos> {
    let x = (parse_byte(f)? as i8) as i32;
    let y = (parse_byte(f)? as i8) as i32;
    Ok(Pos::new(x, y))
}

fn parse_var_int(f: &mut impl Read) -> Result<usize> {
    let mut acc = 0;
    loop{
        let dat = parse_byte(f)?;
        acc *= 0x80;
        acc += (dat&0x7F) as usize;
        if dat&0x80 == 0{
            return Ok(acc)
        }
    }
}
fn parse_str(f: &mut impl Read) -> Result<String> {
    let length = parse_var_int(f)?;
    let mut dat = vec![0; length as usize];
    f.read_exact(&mut dat)?;
    //Have to re-wrap to convert to anyhow error
    Ok(String::from_utf8(dat)?)
}

fn write_byte(f: &mut impl Write, n: u8) -> Result<()>{
    f.write_all(&n.to_le_bytes())?;
    Ok(())
}
fn write_int(f: &mut impl Write, n: i32) -> Result<()>{
    f.write_all(&n.to_le_bytes())?;
    Ok(())
}
fn write_pos(f: &mut impl Write, p: Pos) -> Result<()>{
    write_int(f, p.x)?;
    write_int(f, p.y)
}
fn write_str(f: &mut impl Write, s: &str) -> Result<()> {
    //Instead of writing a byte, should write a var_int
    ensure!(s.len() < 128, "Write string {:?} too long {:?} > 127", s, s.len());
    write_byte(f, s.len() as u8)?;
    f.write_all(s.as_bytes())?;
    Ok(())
}

pub struct FullSolution {
    pub puzzle_name: String,
    pub solution_name: String,
    pub stats: Option<SolutionStats>,
    part_list: Vec<Part>,
}

fn parse_stats(f: &mut impl Read) -> Result<Option<SolutionStats>> {
    let finished = parse_int(f)?;
    if finished == 0 {
        Ok(None)
    } else {
        expect_int(f, 0)?;
        let cycles = parse_int(f)?;
        expect_int(f, 1)?;
        let cost = parse_int(f)?;
        expect_int(f, 2)?;
        let area = parse_int(f)?;
        expect_int(f, 3)?;
        let instructions = parse_int(f)?;
        Ok(Some(SolutionStats { cycles, cost, area, instructions }))
    }
}

fn write_stats(f: &mut impl Write, stats: &Option<SolutionStats>) -> Result<()> {
    match stats{
        None => write_int(f, 0)?,
        Some(stats) => {
            write_int(f, 4)?;
            write_int(f, 0)?;
            write_int(f, stats.cycles)?;
            write_int(f, 1)?;
            write_int(f, stats.cost)?;
            write_int(f, 2)?;
            write_int(f, stats.area)?;
            write_int(f, 3)?;
            write_int(f, stats.instructions)?;
        }
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
enum PartType {
    TGlyph(GlyphType),
    TArm(ArmType),
    TTrack,
    TInput,
    TOutput,
    TOutputRep,
    TConduit,
}
use PartType::*;
struct Part {
    part_type: PartType,
    pos: Pos,
    arm_size: i32,
    rot: i32,
    input_output_index: i32,
    instructions: Vec<(i32, Instr)>,
    tracks: Option<Vec<Pos>>,
    arm_index: i32,
    conduit: Option<(i32, Vec<Pos>)>,
}
fn name_to_part_type(name: String) -> Result<PartType> {
    use ArmType::*;
    use GlyphType::*;
    Ok(match name.as_str() {
        "glyph-calcification"  => TGlyph(Calcification),
        "glyph-life-and-death" => TGlyph(Animismus),
        "glyph-projection"     => TGlyph(Projection),
        "glyph-dispersion"     => TGlyph(Dispersion),
        "glyph-purification"   => TGlyph(Purification),
        "glyph-duplication"    => TGlyph(Duplication),
        "glyph-unification"    => TGlyph(Unification),
        "bonder"               => TGlyph(Bonding),
        "unbonder"             => TGlyph(Unbonding),
        "bonder-prisma"        => TGlyph(TriplexBond),
        "bonder-speed"         => TGlyph(MultiBond),
        "glyph-disposal"       => TGlyph(Disposal),
        "glyph-marker"         => TGlyph(Equilibrium),
        "glyph-rejection"      => TGlyph(Rejection),
        "glyph-division"       => TGlyph(Division),
        "glyph-proliferation"  => TGlyph(Proliferation),
        "arm1"                 => TArm(PlainArm),
        "arm2"                 => TArm(DoubleArm),
        "arm3"                 => TArm(TripleArm),
        "arm6"                 => TArm(HexArm),
        "piston"               => TArm(Piston),
        "baron"                => TArm(VanBerlo),
        "ravari"               => TArm(Ravari),
        "track"                => TTrack,
        "input"                => TInput,
        "out-std"              => TOutput,
        "out-rep"              => TOutputRep,
        "pipe"                 => TConduit,
        _                      => bail!("Arm/Glyph {:?} not recognized", name),
    })
}
fn part_type_to_name(part: &PartType) -> Result<&'static str> {
    use ArmType::*;
    use GlyphType::*;
    Ok(match part {
        TGlyph(Calcification) => "glyph-calcification",
        TGlyph(Animismus)     => "glyph-life-and-death",
        TGlyph(Projection)    => "glyph-projection",
        TGlyph(Dispersion)    => "glyph-dispersion",
        TGlyph(Purification)  => "glyph-purification",
        TGlyph(Duplication)   => "glyph-duplication",
        TGlyph(Unification)   => "glyph-unification",
        TGlyph(Bonding)       => "bonder",
        TGlyph(Unbonding)     => "unbonder",
        TGlyph(TriplexBond)   => "bonder-prisma",
        TGlyph(MultiBond)     => "bonder-speed",
        TGlyph(Disposal)      => "glyph-disposal",
        TGlyph(Equilibrium)   => "glyph-marker",
        TGlyph(Rejection)     => "glyph-rejection",
        TGlyph(Division)      => "glyph-division",
        TGlyph(Proliferation) => "glyph-proliferation",
        TArm(PlainArm)        => "arm1",
        TArm(DoubleArm)       => "arm2",
        TArm(TripleArm)       => "arm3",
        TArm(HexArm)          => "arm6",
        TArm(Piston)          => "piston",
        TArm(VanBerlo)        => "baron",
        TArm(Ravari)          => "ravari",
        TTrack                => "track",
        TInput                => "input",
        TOutput               => "out-std",
        TOutputRep            => "out-rep",
        TConduit              => "pipe",
        _                     => bail!("Illegal part type {:?} in write attempt!", part),
    })
}
pub fn parse_solution(f: &mut impl Read) -> Result<FullSolution> {
    expect_int(f, 7)?;
    let puzzle_name = parse_str(f)?;
    let solution_name = parse_str(f)?;
    let stats = parse_stats(f)?;
    let mut solution_output = FullSolution {
        puzzle_name,
        solution_name,
        stats,
        part_list: Vec::new(),
    };
    for _ in 0..parse_int(f)? {
        let part_type = name_to_part_type(parse_str(f)?)?;
        expect_byte(f, 1)?;
        let pos = parse_pos(f)?;
        let arm_size = parse_int(f)?;
        let rot = parse_int(f)?;
        let input_output_index = parse_int(f)?;
        let instruction_count = parse_int(f)?;
        let mut instructions = Vec::new();
        for _ in 0..instruction_count {
            let instr_pos = parse_int(f)? as i32;
            let action = parse_byte(f)?;
            instructions.push((instr_pos, Instr::from_byte(action).ok_or(eyre!("invalid instruction"))?));
        }
        let tracks = if part_type == PartType::TTrack {
            let track_count = parse_int(f)?;
            let mut inner_tracks = Vec::new();
            for _ in 0..track_count {
                inner_tracks.push(parse_pos(f)?);
            }
            Some(inner_tracks)
        } else {
            None
        };
        let arm_index = parse_int(f)?;
        let conduit = if part_type == PartType::TConduit{
            let conduit_id = parse_int(f)?;
            let conduit_count = parse_int(f)?;
            let mut inner_conduits = Vec::new();
            for _ in 0..conduit_count {
                inner_conduits.push(parse_pos(f)?);
            }
            Some((conduit_id,inner_conduits))
        } else {
            None
        };
        solution_output.part_list.push(Part {
            part_type, pos, arm_size, rot, input_output_index,
            instructions, tracks, arm_index, conduit
        });
    }
    Ok(solution_output)
}
pub fn create_solution(world: &World, puzzle_name: String, solution_name: String) -> FullSolution{
    let stats = None;
    let mut solution_output = FullSolution {
        puzzle_name,
        solution_name,
        stats,
        part_list: Vec::new(),
    };
    let part_list = &mut solution_output.part_list;
    for glyph in &world.glyphs{
        let rot = glyph.rot;
        let pos = glyph.pos;
        let arm_size = 0;
        let arm_index = 0;
        let instructions = Vec::new();
        //Clone and return points to relative position (absolute position done in setup_sim/reposition_glyph)
        let (part_type,input_output_index,tracks,conduit ) = match &glyph.glyph_type{
            GlyphType::Track(track) => {
                (TTrack, 0, Some(track.iter().map(|x|rotate(x - pos, -rot)).collect()), None)
            }
            GlyphType::Input(_,id) => {
                (TInput, *id, None, None)
            }
            GlyphType::Output(_, _,id) => {
                (TOutput, *id, None, None)
            }
            GlyphType::OutputRepeating(_, _,id) => {
                (TOutputRep, *id, None, None)
            }
            GlyphType::Conduit(conduit,id) => {
                (TConduit, *id, None, Some((*id, conduit.iter().map(|x|rotate(x - pos, -rot)).collect())))
            }
            normal_glyph_type => {
                (TGlyph(normal_glyph_type.clone()), 0, None, None)
            }
        };
        part_list.push(Part {
            part_type, pos, arm_size, rot, input_output_index,
            instructions, tracks, arm_index, conduit
        });
    }
    let mut found_first = false;
    let loop_len = world.repeat_length;
    for (id, arm) in world.arms.iter().enumerate(){
        let part_type = TArm(arm.arm_type);
        let pos = arm.pos;
        let arm_size = arm.len;
        let rot = arm.rot;
        let input_output_index = 0;

        let mut instructions = Vec::new();
        let tape = &arm.instruction_tape;
        let mut step = tape.first as i32;
        for &instr in &tape.instructions{
            if instr != Instr::Empty{
                instructions.push((step,instr));
            }
            step += 1;
        }
        let target_loop = (loop_len+tape.first) as i32;
        if !found_first && step < target_loop{
            instructions.push((target_loop-1,Instr::Noop));
            found_first = true;
        }
        let tracks = None;
        let arm_index = id as _;
        let conduit = None;
        part_list.push(Part {
            part_type, pos, arm_size, rot, input_output_index,
            instructions, tracks, arm_index, conduit,
        });
    }
    solution_output
}
pub fn write_solution(f: &mut impl Write, sol: &FullSolution) -> Result<()>{
    write_int(f, 7)?;
    write_str(f, &sol.puzzle_name)?;
    write_str(f, &sol.solution_name)?;
    write_stats(f, &sol.stats)?;
    write_int(f, sol.part_list.len() as i32)?;
    for part in &sol.part_list {
        write_str(f, part_type_to_name(&part.part_type)?)?;
        write_byte(f, 1)?;
        write_pos(f, part.pos)?;
        write_int(f, part.arm_size)?;
        write_int(f, part.rot)?;
        write_int(f, part.input_output_index)?;
        write_int(f, part.instructions.len() as i32)?;
        for (instr_pos, instr) in &part.instructions {
            write_int(f, *instr_pos)?;
            write_byte(f, instr.to_byte())?;
        }
        if let Some(tracks) = &part.tracks {
            ensure!(part.part_type == PartType::TTrack, "Tracks on non-track piece");
            write_int(f, tracks.len() as i32)?;
            for t in tracks {
                write_pos(f, *t)?;
            }
        }
        write_int(f, part.arm_index)?;
        if let Some((id, conduits)) = &part.conduit{
            ensure!(part.part_type == PartType::TConduit, "Conduits on non-conduit piece");
            write_int(f, *id)?;
            write_int(f, conduits.len() as i32)?;
            for c in conduits {
                write_pos(f, *c)?;
            }
        }
    }
    Ok(())
}

pub struct FullPuzzle {
    pub puzzle_name: String,
    pub creator_id: u64,
    pub allowed_bitfield: AllowedParts,
    pub inputs: Vec<AtomPattern>,
    pub outputs: Vec<AtomPattern>,
    pub output_multiplier: i32,
    pub production: bool,
    //TODO: production data here
}
fn parse_molecule(f: &mut impl Read) -> Result<AtomPattern> {
    let mut atoms = Vec::new();
    let mut atom_locs: HashMap<Pos, usize> = HashMap::new();
    for i in 0..(parse_int(f)? as usize) {
        let atom_type = AtomType::from_u8(parse_byte(f)?).ok_or(eyre!("Illegal atom type"))?;
        let pos = parse_bytepos(f)?;
        atoms.push(Atom::new(pos, atom_type));
        let check = atom_locs.insert(pos, i);
        ensure!(check == None, "Multiple atoms in same location!");
    }
    //bonds
    for _ in 0..parse_int(f)? {
        let bond_type = Bonds::from_bits(parse_byte(f)?).ok_or(eyre!("Illegal bond type"))?;
        let from_pos = parse_bytepos(f)?;
        let to_pos = parse_bytepos(f)?;

        let atom1 = *atom_locs
            .get(&from_pos)
            .ok_or(eyre!("bond1 to nonatom position"))?;
        let atom2 = *atom_locs
            .get(&to_pos)
            .ok_or(eyre!("bond2 to nonatom position"))?;
        let rot = pos_to_rot(atoms[atom2].pos - atoms[atom1].pos).ok_or(eyre!("nonadjacent bond"))?;
        atoms[atom1].connections[rot as usize] = bond_type;
        atoms[atom2].connections[normalize_dir(rot + 3) as usize] = bond_type;
    }
    let final_output = smallvec![atoms];
    Ok(final_output)
}
fn process_repeats(input: &Vec<Atom>, reps: i32) -> Result<AtomPattern> {
    let mut rep_offset = None;
    for atom in input {
        if atom.atom_type == AtomType::RepeatingOutputMarker {
            ensure!(rep_offset == None, "Multiple repeating atom markers!");
            rep_offset = Some(atom.pos);
        }
    }
    let rep_offset = rep_offset.ok_or(eyre!("Repeating atom marker expected but not found!"))?;
    let mut output = Vec::with_capacity(input.len() * (reps as usize));
    for atom in input {
        if atom.atom_type == AtomType::RepeatingOutputMarker {
            output.push(Atom {
                pos: atom.pos + ((reps-1) * rep_offset),
                ..*atom
            });
        } else {
            for i in 0..reps {
                output.push(Atom {
                    pos: atom.pos + (i * rep_offset),
                    ..*atom
                });
            }
        }
    }
    Ok(smallvec![output])
}

pub fn parse_puzzle(f: &mut impl Read) -> Result<FullPuzzle> {
    expect_int(f, 3)?;
    let puzzle_name = parse_str(f)?;
    let creator_id = parse_long(f)?;
    let bitfield_base = parse_long(f)?;
    let allowed_bitfield = bitfield_base; /*AllowedParts::from_bits(bitfield_base)
                                          .ok_or(eyre!("allowed parts bitfield error: {:b}",bitfield_base))?;*/
    let mut inputs = Vec::new();
    for _ in 0..parse_int(f)? {
        inputs.push(parse_molecule(f)?)
    }
    let mut outputs = Vec::new();
    for _ in 0..parse_int(f)? {
        outputs.push(parse_molecule(f)?)
    }
    let output_multiplier = parse_int(f)?;
    let production = parse_byte(f)? != 0;
    //TODO: if production is true, read their stuff
    let puzzle_output = FullPuzzle {
        puzzle_name,
        creator_id,
        allowed_bitfield,
        inputs,
        outputs,
        output_multiplier,
        production,
    };
    Ok(puzzle_output)
}
fn process_instructions(input: &[(i32, Instr)]) -> Result<Tape> {
    let mut first = usize::MAX; //A giant value
    let mut instructions = Vec::new();
    for &(pos, instr) in input {
        ensure!(instr != Instr::Empty, "Empty instruction found in file!");
        let pos = pos as usize;
        if first > pos {
            if first != usize::MAX {
                let splice_source = std::iter::repeat(Instr::Empty).take(first - pos);
                instructions.splice(..0, splice_source);
            }
            first = pos
        }
        if pos >= first + instructions.len() {
            instructions.resize(pos - first + 1, Instr::Empty);
        }
        instructions[pos - first] = instr;
    }
    if first == usize::MAX{
        first = 0;
    }
    Ok(Tape {
        first,
        instructions,
    })
}
pub fn puzzle_prep(puzzle: &FullPuzzle, soln: &FullSolution) -> Result<InitialWorld> {
    //Convert from generic parts to glyphs, arms, etc.
    let mut glyphs = Vec::new();
    let mut arms = Vec::new();
    for p in &soln.part_list {
        match &p.part_type {
            TGlyph(gtype) => {
                glyphs.push(Glyph {
                    glyph_type: gtype.clone(),
                    pos: p.pos,
                    rot: p.rot,
                });
            }
            TArm(atype) => {
                let instr = process_instructions(&p.instructions)?;
                arms.push((Arm::new(p.pos, p.rot, p.arm_size, *atype, instr),p.arm_index));
            }
            TInput => {
                let id = p.input_output_index;
                let molecule = puzzle.inputs.get(id as usize).ok_or(eyre!(
                    "Input ID {} not found (max {})",
                    id,
                    puzzle.inputs.len()
                ))?;
                let input_glyph = GlyphType::Input(molecule.clone(), p.input_output_index);
                glyphs.push(Glyph {
                    glyph_type: input_glyph,
                    pos: p.pos,
                    rot: p.rot,
                });
            }
            TOutput => {
                let id = p.input_output_index;
                let molecule = puzzle.outputs.get(id as usize)
                    .ok_or(eyre!("Output  ID {} not found (max {})", id, puzzle.outputs.len()))?;
                let output_glyph = GlyphType::Output(molecule.clone(), 6 * puzzle.output_multiplier, p.input_output_index);
                glyphs.push(Glyph {glyph_type: output_glyph, pos: p.pos, rot: p.rot });
            }
            TOutputRep => {
                let id = p.input_output_index;
                let molecule = puzzle.outputs.get(id as usize)
                    .ok_or(eyre!("Output(rep) ID {} not found (max {})",id,puzzle.outputs.len()))?;
                let repeated_molecule = process_repeats(&molecule[0], 6)?;
                let output_glyph =
                    GlyphType::OutputRepeating(repeated_molecule, 6 * puzzle.output_multiplier, p.input_output_index);
                glyphs.push(Glyph {glyph_type: output_glyph, pos: p.pos, rot: p.rot });
            }
            TTrack => {
                let tracks = p.tracks.clone()
                    .ok_or(eyre!("Track data not found on track glyph"))?;
                glyphs.push(Glyph {glyph_type: GlyphType::Track(tracks), pos: p.pos, rot: p.rot });
            }
            TConduit => {
                let conduits = p.conduit.clone()
                    .ok_or(eyre!("Conduit data not found on conduit glyph"))?;
                glyphs.push(Glyph {glyph_type: GlyphType::Conduit(conduits.1, conduits.0), pos: p.pos, rot: p.rot });
            }
        }
    }
    arms.sort_by(|a, b| a.1.cmp(&b.1));
    let arms = arms.into_iter().map(|a| a.0).collect();
    Ok(InitialWorld { glyphs, arms })
}
