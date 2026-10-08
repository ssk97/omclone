//Hex positions and rotations, and conversions between hex coordinates and world coordinates.
//Hex: axial Pos, +x is to the right and +y is up and to the right.
//World: XYPos with 2 units between adjacent hex centers (a hex's inner radius is 1), y up.
//Angles: radians, counterclockwise. A Rot of 1 is 60 degrees counterclockwise.
//(collision.rs works in omsim's pixel units internally, but uses the same angles)
use super::{Pos, Rot};
use nalgebra::{Point2, Vector2};

pub const SQRT3: f32 = 1.7320508;
pub type XYPos = Point2<f32>;
pub type XYVec = Vector2<f32>;
//The editor can't place anything further out than this, so positions stay far from overflowing
pub const MAX_COORD: i32 = 1 << 20;

pub fn pos_to_xy(input: Pos) -> XYPos {
    XYPos::origin() + pos_to_xy_vec(input)
}
pub fn pos_to_xy_vec(input: Pos) -> XYVec {
    let a = input.x as f32;
    let b = input.y as f32;
    XYVec::new(a * 2. + b, b * SQRT3)
}
//Written to match omsim's float math, so collision results line up with the game
pub fn rot_to_angle(r: Rot) -> f32 {
    (r * 60) as f32 * (std::f32::consts::PI / 180.)
}
//Rounds fractional axial coordinates to the nearest hex
pub fn round_axial(u: f32, v: f32) -> Pos {
    let w = 0. - u - v;
    let mut ui = u.round() as i32;
    let mut vi = v.round() as i32;
    let wi = w.round() as i32;
    let uf = (u - ui as f32).abs();
    let vf = (v - vi as f32).abs();
    let wf = (w - wi as f32).abs();
    if uf > vf && uf > wf {
        ui = -vi - wi;
    } else if vf > wf {
        vi = -ui - wi;
    }
    Pos::new(ui, vi)
}
//The hex containing a world position
pub fn xy_to_pos(input: XYPos) -> Pos {
    let v = input.y / SQRT3;
    let u = (input.x - v) / 2.;
    round_axial(u, v)
}
//How many steps apart two hexes are
pub fn hex_distance(a: Pos, b: Pos) -> i32 {
    let d = b - a;
    (d.x.abs() + d.y.abs() + (d.x + d.y).abs()) / 2
}
//The hexes along a straight line from `a` to `b`, both included, each next to the one before
pub fn hex_line(a: Pos, b: Pos) -> Vec<Pos> {
    let n = hex_distance(a, b);
    let (start, end) = (pos_to_xy(a), pos_to_xy(b));
    //Nudged off the edges between hexes, so ties round the same way all along the line
    let nudge = XYVec::new(1e-3, 2e-3);
    (0..=n).map(|i| {
        let t = if n == 0 {0.} else {i as f32 / n as f32};
        xy_to_pos(start + (end - start) * t + nudge)
    }).collect()
}

pub fn normalize_dir(r: Rot) -> Rot {
    return r.rem_euclid(6);
}

pub fn pos_to_rot(input: Pos) -> Option<Rot> {
    match (input.x, input.y) {
        ( 1, 0) => Some(0),
        ( 0, 1) => Some(1),
        (-1, 1) => Some(2),
        (-1, 0) => Some(3),
        ( 0,-1) => Some(4),
        ( 1,-1) => Some(5),
        _ => None,
    }
}
pub fn rot_dist_to_pos(n: i32, angle: Rot) -> Pos {
    match normalize_dir(angle) {
        0 => Pos::new( n, 0),
        1 => Pos::new( 0, n),
        2 => Pos::new(-n, n),
        3 => Pos::new(-n, 0),
        4 => Pos::new( 0,-n),
        5 => Pos::new( n,-n),
        _ => panic!("Invalid Rotation"),
    }
}
pub fn rot_to_pos(angle: Rot) -> Pos {
    rot_dist_to_pos(1, angle)
}
pub fn rotate(pos: Pos, angle: Rot) -> Pos {
    match normalize_dir(angle) {
        0 => pos, //  ( pos.x      ,       pos.y)
        1 => Pos::new(      -pos.y , pos.x+pos.y),
        2 => Pos::new(-pos.x-pos.y , pos.x      ),
        3 => Pos::new(-pos.x       ,      -pos.y),
        4 => Pos::new(       pos.y ,-pos.x-pos.y),
        5 => Pos::new( pos.x+pos.y ,-pos.x      ),
        _ => panic!("Invalid Rotation"),
    }
}

pub fn rotate_around(pos: Pos, angle: Rot, pivot: Pos) -> Pos {
    rotate(pos - pivot, angle) + pivot
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xy_round_trip() {
        //Hex corners are 2/sqrt(3) from the center, edges are 1 away
        let nudges = (0..12).map(|i| {
            let angle = (i as f32) * std::f32::consts::PI / 6.;
            XYVec::new(angle.cos(), angle.sin()) * 0.99
        });
        let nudges: Vec<XYVec> = nudges.chain([XYVec::zeros()]).collect();
        for x in -5..=5 {
            for y in -5..=5 {
                let p = Pos::new(x, y);
                for nudge in &nudges {
                    assert_eq!(xy_to_pos(pos_to_xy(p) + nudge), p, "{p:?} nudged by {nudge:?}");
                }
            }
        }
    }

    #[test]
    fn hex_lines_step_to_neighbors() {
        for (a, b) in [((0, 0), (0, 0)), ((0, 0), (5, 0)), ((2, -1), (-3, 4)), ((0, 0), (3, 2)), ((1, 1), (-4, -2))] {
            let (a, b) = (Pos::new(a.0, a.1), Pos::new(b.0, b.1));
            let line = hex_line(a, b);
            assert_eq!((line[0], *line.last().unwrap()), (a, b));
            assert_eq!(line.len() as i32, hex_distance(a, b) + 1);
            for pair in line.windows(2) {
                assert_eq!(hex_distance(pair[0], pair[1]), 1, "{a:?} to {b:?}: {line:?}");
            }
        }
    }

    #[test]
    fn angles_match_rotate() {
        for x in -3..=3 {
            for y in -3..=3 {
                let p = Pos::new(x, y);
                for r in -6..=6 {
                    let rotated = nalgebra::Rotation2::new(rot_to_angle(r)) * pos_to_xy_vec(p);
                    let expected = pos_to_xy_vec(rotate(p, r));
                    assert!((rotated - expected).norm() < 1e-4, "{p:?} rotated by {r}: {rotated:?} != {expected:?}");
                }
            }
        }
    }

    #[test]
    fn rotation_tests() {
        for r in 0..6 {
            assert_eq!(rot_dist_to_pos(1, r), rotate(Pos::new(1, 0), r));
            assert_eq!(rot_dist_to_pos(2, r), rotate(Pos::new(2, 0), r));
            assert_eq!(rotate(Pos::new(3, 1), r + 1), rotate(Pos::new(-1, 4), r));
        }
    }
}
