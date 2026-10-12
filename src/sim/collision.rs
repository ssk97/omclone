//Collision detection and area marking during the motion phase.
//Ported from omsim's collision.c (non-chain mode), including its float math, so borderline
//collisions and swept area behave like the game.
use super::*;

//Based on the game's hex tile image dimensions. Note this isn't a perfectly regular hex grid.
const HEX_X: f32 = 82.;
const HEX_Y: f32 = 71.;
const ATOM_RADIUS: f32 = 29.;
const PRODUCED_ATOM_RADIUS: f32 = 15.;
const ARM_BASE_RADIUS: f32 = 20.;
//Spatial grid cell size; must be at least the largest collision distance (2*ATOM_RADIUS)
const GRID_CELL: f32 = 64.;

#[derive(Copy, Clone, Debug)]
struct Xy {
    x: f32,
    y: f32,
}
fn to_xy(p: Pos) -> Xy {
    Xy { x: HEX_X * (p.x as f32 + 0.5 * p.y as f32), y: HEX_Y * p.y as f32 }
}
fn from_xy(xy: Xy) -> Pos {
    let u = xy.x / HEX_X - 0.5 * xy.y / HEX_Y;
    let v = xy.y / HEX_Y;
    round_axial(u, v)
}
fn xy_len(xy: Xy) -> f32 {
    (xy.x * xy.x + xy.y * xy.y).sqrt()
}
fn xy_dist(a: Xy, b: Xy) -> f32 {
    xy_len(Xy { x: a.x - b.x, y: a.y - b.y })
}
//Matches C's (float)cos(double) on a float argument
fn cos(a: f32) -> f32 {
    (a as f64).cos() as f32
}
fn sin(a: f32) -> f32 {
    (a as f64).sin() as f32
}
//Converts to the coordinates used by SimError/rendering (hex centers line up exactly)
fn to_sim_xy(xy: Xy) -> XYPos {
    XYPos::new(xy.x / (HEX_X / 2.), xy.y / HEX_Y * f32::sqrt(3.))
}
fn hexicab_length(p: Pos) -> i32 {
    (p.x.abs() + p.y.abs() + (p.x + p.y).abs()) / 2
}
fn apply_movement(pos: Pos, movement: Movement) -> Pos {
    match movement {
        Movement::Linear(offset) => pos + offset,
        Movement::Rotation(r, pivot) => rotate_around(pos, r, pivot),
        Movement::HeldStill => pos,
    }
}

//Hexes whose center is closer than `distance` to the point
fn hexes_near(center: Xy, distance: f32) -> impl Iterator<Item = Pos> {
    let p = from_xy(center);
    let neighbors = [(0, 0), (1, 0), (0, 1), (-1, 1), (-1, 0), (0, -1), (1, -1)];
    neighbors.into_iter()
        .map(move |(du, dv)| p + Pos::new(du, dv))
        .filter(move |&hex| xy_dist(center, to_xy(hex)) < distance)
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum ColliderKind {
    Atom,
    ArmBase,
    ProducedAtom,
}
#[derive(Copy, Clone)]
struct Collider {
    center: Xy,
    radius: f32,
    kind: ColliderKind,
}
fn collision_message(a: ColliderKind, b: ColliderKind) -> &'static str {
    use ColliderKind::*;
    match (a, b) {
        (ArmBase, ArmBase) => "Arm-Arm collision!",
        (ProducedAtom, _) | (_, ProducedAtom) => "atom/spawning atom collision!",
        (ArmBase, _) | (_, ArmBase) => "atom/arm collision!",
        (Atom, Atom) => "atom/atom collision!",
    }
}

//Colliders are only checked against colliders before the cursor (so atoms of the same movement
//don't collide with each other), and against stationary atoms on the board.
struct ColliderList<'a> {
    colliders: Vec<Collider>,
    cursor: usize,
    grid: FxHashMap<(i32, i32), SmallVec<[usize; 4]>>,
    board: &'a WorldAtoms,
    motion: &'a WorldStepInfo,
    area: &'a mut FxHashSet<Pos>,
}
impl<'a> ColliderList<'a> {
    fn cell(xy: Xy) -> (i32, i32) {
        ((xy.x / GRID_CELL).floor() as i32, (xy.y / GRID_CELL).floor() as i32)
    }
    fn add(&mut self, collider: Collider) -> SimResult<()> {
        //Mark area and check against the board (atoms that aren't moving)
        for hex in hexes_near(collider.center, collider.radius + ATOM_RADIUS) {
            self.area.insert(hex);
            //Hexes aren't stacked during motion, so the visible atom is the only one
            if let Some(key) = self.board.visible(hex) {
                let moving = matches!(self.motion.atoms.get(key), Some(m) if *m != Movement::HeldStill);
                if !moving {
                    let error_str = collision_message(collider.kind, ColliderKind::Atom);
                    return Err(SimError { error_str, location: to_sim_xy(to_xy(hex)) });
                }
            }
        }
        //Check against earlier colliders
        let (cx, cy) = Self::cell(collider.center);
        for dx in -1..=1 {
            for dy in -1..=1 {
                let Some(indices) = self.grid.get(&(cx + dx, cy + dy)) else {continue};
                for &i in indices {
                    let other = self.colliders[i];
                    if xy_dist(other.center, collider.center) < other.radius + collider.radius {
                        let error_str = collision_message(other.kind, collider.kind);
                        let middle = Xy { x: (other.center.x + collider.center.x) / 2., y: (other.center.y + collider.center.y) / 2. };
                        return Err(SimError { error_str, location: to_sim_xy(middle) });
                    }
                }
            }
        }
        self.colliders.push(collider);
        Ok(())
    }
    //Makes every collider added so far visible to future checks
    fn commit(&mut self) {
        for i in self.cursor..self.colliders.len() {
            let cell = Self::cell(self.colliders[i].center);
            self.grid.entry(cell).or_default().push(i);
        }
        self.cursor = self.colliders.len();
    }
}

impl World {
    //How many substeps the motion phase is split into for collision checks (omsim's increment)
    pub fn substep_count(&self, motion: &WorldStepInfo) -> usize {
        let mut max_distance = 1;
        for (atom_key, movement) in &motion.atoms {
            if let Movement::Rotation(_, center) = movement {
                let distance = hexicab_length(self.atoms.atom_map[atom_key].pos - center);
                max_distance = max_distance.max(distance);
            }
        }
        let mut increment = 0.25 / 2f64.powf(((max_distance as f64).ln() / 2f64.ln()).round());
        if !(increment <= 0.125) {
            increment = 0.125;
        }
        (1. / increment).round() as usize
    }

    //Checks collisions and marks area at progress substep/substep_count of the motion phase.
    //Must be called between prepare_step and finalize_step, for substep in 1..substep_count.
    pub fn collide_substep(&mut self, motion: &WorldStepInfo, substep: usize, substep_count: usize) -> SimResult<()> {
        let progress = substep as f32 / substep_count as f32;
        let remaining = 1. - progress;
        for (arm, &arm_motion) in self.arms.iter().zip(&motion.arms) {
            mark_arm_sweep(&mut self.area_touched, arm, arm_motion, remaining);
        }
        let mut list = ColliderList {
            colliders: Vec::new(),
            cursor: 0,
            grid: Default::default(),
            board: &self.atoms,
            motion,
            area: &mut self.area_touched,
        };
        //Fixed colliders: stationary arm bases, then atoms being produced. Each can collide with the others of its
        //kind (two products can land on one hex).
        let mut moving_arms = Vec::new();
        for (arm, arm_motion) in self.arms.iter().zip(&motion.arms) {
            if let ArmMovement::Move(Movement::Linear(translation)) = arm_motion {
                moving_arms.push((arm.pos + translation, *translation));
                continue;
            }
            list.add(Collider { center: to_xy(arm.pos), radius: ARM_BASE_RADIUS, kind: ColliderKind::ArmBase })?;
            list.commit();
        }
        for atom in motion.products.iter().flat_map(|p| &p.atoms) {
            list.add(Collider { center: to_xy(atom.pos), radius: PRODUCED_ATOM_RADIUS, kind: ColliderKind::ProducedAtom })?;
            list.commit();
        }
        //Arm bases moving along tracks
        for (final_pos, translation) in moving_arms {
            let mut xy = to_xy(final_pos);
            let tr = to_xy(translation);
            xy.x -= tr.x * remaining;
            xy.y -= tr.y * remaining;
            list.add(Collider { center: xy, radius: ARM_BASE_RADIUS, kind: ColliderKind::ArmBase })?;
            list.commit();
        }
        //Moving atoms, positioned relative to the moving grabber like the game does
        for (grab, moved_atoms) in motion.grabs.iter().zip(&motion.grab_atoms) {
            if moved_atoms.is_empty() || grab.movement == Movement::HeldStill {
                continue;
            }
            let arm = &self.arms[grab.arm];
            let mut base = arm.pos;
            let mut base_rotation = arm.rot;
            let mut len = arm.len;
            let mut rotation = 0;
            let mut swing = false;
            let mut track = None;
            let mut piston_extension = 0;
            match motion.arms[grab.arm] {
                ArmMovement::Move(Movement::Rotation(r, _)) => {
                    rotation = r;
                    swing = true;
                    base_rotation = (base_rotation + r) % 6;
                }
                ArmMovement::Pivot(r) => rotation = r,
                ArmMovement::Move(Movement::Linear(translation)) => {
                    base += translation;
                    track = Some(translation);
                }
                ArmMovement::LengthAdjust(a) => {
                    piston_extension = a;
                    len += a;
                }
                ArmMovement::Move(Movement::HeldStill) => continue,
            }
            let mut v = to_xy(base);
            let rotation_rad = rot_to_angle(rotation);
            let arm_rotation_rad = if swing {rotation_rad} else {0.};
            let r = (0. - rotation_rad) * remaining;
            if let Some(translation) = track {
                let tr = to_xy(translation);
                v.x -= tr.x * remaining;
                v.y -= tr.y * remaining;
            }
            let base_rotation_rad = rot_to_angle(base_rotation);
            let mut g = to_xy(rot_to_pos(grab.grabber as Rot) * len);
            if arm.arm_type == ArmType::Piston {
                let g_len = xy_len(g);
                let new_len = xy_len(g) - (piston_extension as f32) * HEX_X * remaining;
                g.x = new_len / g_len * g.x;
                g.y = new_len / g_len * g.y;
            }
            let grabber_rotation = base_rotation_rad - arm_rotation_rad * remaining;
            let (grx, gry) = (cos(grabber_rotation), sin(grabber_rotation));
            v.x += g.x * grx - g.y * gry;
            v.y += g.x * gry + g.y * grx;
            let (rx, ry) = (cos(r), sin(r));
            let final_grab = apply_movement(self.atoms.atom_map[grab.atom].pos, grab.movement);
            for &atom_key in moved_atoms {
                let final_pos = apply_movement(self.atoms.atom_map[atom_key].pos, grab.movement);
                let xy = to_xy(final_pos - final_grab);
                let center = Xy { x: v.x + (xy.x * rx - xy.y * ry), y: v.y + (xy.x * ry + xy.y * rx) };
                list.add(Collider { center, radius: ATOM_RADIUS, kind: ColliderKind::Atom })?;
            }
            list.commit();
        }
        Ok(())
    }

    //Marks every hex covered by each arm, from grabber to base (omsim's mark_arm_area)
    pub(super) fn mark_arm_area(&mut self) {
        for arm in &self.arms {
            for r in (0..6).step_by(Arm::angles_between_arm(arm.arm_type) as usize) {
                for l in 0..=arm.len {
                    self.area_touched.insert(arm.pos + rot_to_pos(arm.rot + r) * l);
                }
            }
        }
    }
}

//Marks the area of a moving arm partway through its motion, treating the grabber and each hex-length
//point along the arm like an atom. For the arm lengths that exist (1 to 3), sweeping this over a rotation
//gives exactly the hexes of omsim's record_swing_area, while showing the area as the arm moves.
fn mark_arm_sweep(area: &mut FxHashSet<Pos>, arm: &Arm, arm_motion: ArmMovement, remaining: f32) {
    let mut base = to_xy(arm.pos);
    let mut final_rotation = arm.rot;
    let mut swing = 0.;
    let mut len = arm.len as f32;
    match arm_motion {
        ArmMovement::Move(Movement::Rotation(r, _)) => {
            final_rotation = (final_rotation + r) % 6;
            swing = rot_to_angle(r);
        }
        ArmMovement::Move(Movement::Linear(translation)) => {
            let final_base = to_xy(arm.pos + translation);
            let tr = to_xy(translation);
            base = Xy { x: final_base.x - tr.x * remaining, y: final_base.y - tr.y * remaining };
        }
        ArmMovement::LengthAdjust(a) => len += a as f32 * (1. - remaining),
        ArmMovement::Move(Movement::HeldStill) | ArmMovement::Pivot(_) => return,
    }
    let rotation = rot_to_angle(final_rotation) - swing * remaining;
    let (c, s) = (cos(rotation), sin(rotation));
    let mut distances: SmallVec<[f32; 4]> = (1..=len.floor() as i32).map(|d| d as f32).collect();
    if len.fract() != 0. {
        distances.push(len);
    }
    for g in (0..6).step_by(Arm::angles_between_arm(arm.arm_type) as usize) {
        let dir = to_xy(rot_to_pos(g as Rot));
        for &d in &distances {
            let point = Xy {
                x: base.x + (dir.x * c - dir.y * s) * d,
                y: base.y + (dir.x * s + dir.y * c) * d,
            };
            area.extend(hexes_near(point, ATOM_RADIUS + ATOM_RADIUS));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    //omsim's record_swing_area: the hexes a long arm's grabber sweeps when rotating from `position`
    fn omsim_swing_area(base: Pos, position: Pos, rotation: Rot, output: &mut FxHashSet<Pos>) {
        let mut offset = position - base;
        if rotation == -1 {
            offset = Pos::new(offset.x + offset.y, -offset.x);
        }
        let arm_length = offset.x.abs() | offset.y.abs();
        let o = Pos::new(offset.x.signum(), offset.y.signum());
        let mut mark = |du: i32, dv: i32, ou: i32, ov: i32| {
            output.insert(base + Pos::new(du * o.x - dv * o.y, ou * o.x + ov * o.y));
        };
        if arm_length >= 3 {
            mark(3, 1, 1, 4);
            mark(2, 2, 2, 4);
            mark(1, 3, 3, 4);
        }
        if arm_length >= 2 {
            mark(2, 1, 1, 3);
            mark(1, 1, 1, 2);
            mark(1, 2, 2, 3);
        }
    }

    #[test]
    fn arm_sweep_matches_omsim_swing_area() {
        for arm_type in [ArmType::PlainArm, ArmType::TripleArm] {
            for len in 1..=3 {
                for rot in -6..6 {
                    for rotation in [1, -1] {
                        let base = Pos::new(2, -1);
                        let arm = Arm::new(base, rot, len, arm_type, Tape::new(0, Vec::new()));
                        let grabbers: Vec<Rot> = (0..6).step_by(Arm::angles_between_arm(arm_type) as usize).collect();
                        //Arm hexes before and after the rotation are marked at the start and end of the step
                        let mut endpoints = FxHashSet::default();
                        for r in [rot, rot + rotation] {
                            for &g in &grabbers {
                                for l in 0..=len {
                                    endpoints.insert(base + rot_to_pos(r + g) * l);
                                }
                            }
                        }
                        let mut expected = endpoints.clone();
                        for &g in &grabbers {
                            omsim_swing_area(base, base + rot_to_pos(rot + g) * len, rotation, &mut expected);
                        }
                        for substep_count in [8, 16, 32, 64, 256] {
                            let mut swept = endpoints.clone();
                            for substep in 1..substep_count {
                                let remaining = 1. - substep as f32 / substep_count as f32;
                                mark_arm_sweep(&mut swept, &arm, ArmMovement::Move(Movement::Rotation(rotation, base)), remaining);
                            }
                            assert_eq!(swept, expected, "{arm_type:?} len {len} rot {rot} rotation {rotation} substeps {substep_count}");
                        }
                    }
                }
            }
        }
    }
}
