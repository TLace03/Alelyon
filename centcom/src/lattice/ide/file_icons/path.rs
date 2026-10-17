//! SVG path data (the `d` attribute) as canvas strokes: every command (`M L H V C S Q T A Z`, absolute and
//! relative), numbers as a minifier writes them (`1.5.5` is two numbers, `-1-2` is two, `1e-3` is one), arc flags
//! written without separators, and arcs turned into cubic Béziers.

use std::f32::consts::{FRAC_PI_2, TAU};

use iced::Point;
use iced::widget::canvas::path::Builder;

/// One drawing step, in the icon's own coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    Move([f32; 2]),
    Line([f32; 2]),
    Cubic([f32; 2], [f32; 2], [f32; 2]),
    Quad([f32; 2], [f32; 2]),
    Close,
}

struct Reader<'a> {
    s: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn skip(&mut self) {
        while self.s.get(self.at).is_some_and(|c| matches!(c, b' ' | b'\t' | b'\n' | b'\r' | b',')) {
            self.at += 1;
        }
    }

    fn command(&mut self) -> Option<u8> {
        self.skip();
        let c = *self.s.get(self.at)?;
        c.is_ascii_alphabetic().then(|| {
            self.at += 1;
            c
        })
    }

    /// Whether a number comes next: the command before it repeats.
    fn more(&mut self) -> bool {
        self.skip();
        self.s.get(self.at).is_some_and(|c| matches!(c, b'0'..=b'9' | b'.' | b'-' | b'+'))
    }

    fn number(&mut self) -> Result<f32, String> {
        self.skip();
        let (s, start) = (self.s, self.at);
        let mut i = start;
        if s.get(i).is_some_and(|c| matches!(c, b'+' | b'-')) {
            i += 1;
        }
        let mut digits = false;
        while s.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
            digits = true;
        }
        if s.get(i) == Some(&b'.') {
            i += 1;
            while s.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
                digits = true;
            }
        }
        if !digits {
            return Err(format!("no number at byte {start}"));
        }
        if s.get(i).is_some_and(|c| matches!(c, b'e' | b'E')) {
            let mut j = i + 1;
            if s.get(j).is_some_and(|c| matches!(c, b'+' | b'-')) {
                j += 1;
            }
            if s.get(j).is_some_and(u8::is_ascii_digit) {
                while s.get(j).is_some_and(u8::is_ascii_digit) {
                    j += 1;
                }
                i = j;
            }
        }
        self.at = i;
        std::str::from_utf8(&s[start..i]).ok().and_then(|t| t.parse().ok()).ok_or_else(|| format!("no number at byte {start}"))
    }

    fn pair(&mut self) -> Result<[f32; 2], String> {
        Ok([self.number()?, self.number()?])
    }

    /// An arc's flag: one character, `0` or `1`, which a minifier writes with no separator after it.
    fn flag(&mut self) -> Result<bool, String> {
        self.skip();
        let flag = match self.s.get(self.at) {
            Some(b'0') => false,
            Some(b'1') => true,
            _ => return Err(format!("no arc flag at byte {}", self.at)),
        };
        self.at += 1;
        Ok(flag)
    }
}

/// The steps `d` draws, or why it cannot be read.
pub fn parse(d: &str) -> Result<Vec<Seg>, String> {
    let mut r = Reader { s: d.as_bytes(), at: 0 };
    let mut out = Vec::new();
    let mut cmd = match r.command() {
        Some(c @ (b'M' | b'm')) => c,
        None if r.at >= r.s.len() => return Ok(out),
        _ => return Err("a path starts with M".into()),
    };
    let (mut cur, mut start) = ([0.0f32; 2], [0.0f32; 2]);
    // The last curve's control point, which S and T reflect.
    let (mut last_cubic, mut last_quad): (Option<[f32; 2]>, Option<[f32; 2]>) = (None, None);
    loop {
        let rel = cmd.is_ascii_lowercase();
        let abs = |p: [f32; 2], cur: [f32; 2]| if rel { [p[0] + cur[0], p[1] + cur[1]] } else { p };
        let (mut cubic, mut quad) = (None, None);
        match cmd.to_ascii_uppercase() {
            b'M' => {
                let p = abs(r.pair()?, cur);
                out.push(Seg::Move(p));
                (cur, start) = (p, p);
                // Pairs after a move are lines.
                cmd = if rel { b'l' } else { b'L' };
            }
            b'L' => {
                cur = abs(r.pair()?, cur);
                out.push(Seg::Line(cur));
            }
            b'H' => {
                let x = r.number()?;
                cur = [if rel { cur[0] + x } else { x }, cur[1]];
                out.push(Seg::Line(cur));
            }
            b'V' => {
                let y = r.number()?;
                cur = [cur[0], if rel { cur[1] + y } else { y }];
                out.push(Seg::Line(cur));
            }
            b'C' | b'S' => {
                let c1 = if cmd.eq_ignore_ascii_case(&b'C') { abs(r.pair()?, cur) } else { reflect(last_cubic, cur) };
                let c2 = abs(r.pair()?, cur);
                let p = abs(r.pair()?, cur);
                out.push(Seg::Cubic(c1, c2, p));
                (cubic, cur) = (Some(c2), p);
            }
            b'Q' | b'T' => {
                let c = if cmd.eq_ignore_ascii_case(&b'Q') { abs(r.pair()?, cur) } else { reflect(last_quad, cur) };
                let p = abs(r.pair()?, cur);
                out.push(Seg::Quad(c, p));
                (quad, cur) = (Some(c), p);
            }
            b'A' => {
                let (rx, ry, rotation) = (r.number()?, r.number()?, r.number()?);
                let (large, sweep) = (r.flag()?, r.flag()?);
                let p = abs(r.pair()?, cur);
                arc(&mut out, cur, [rx, ry], rotation, large, sweep, p);
                cur = p;
            }
            b'Z' => {
                out.push(Seg::Close);
                cur = start;
            }
            other => return Err(format!("the command {}", other as char)),
        }
        (last_cubic, last_quad) = (cubic, quad);
        if !cmd.eq_ignore_ascii_case(&b'Z') && r.more() {
            continue;
        }
        match r.command() {
            Some(c) => cmd = c,
            None => break,
        }
    }
    r.skip();
    if r.at < r.s.len() {
        return Err(format!("unread path data at byte {}", r.at));
    }
    Ok(out)
}

/// The control point S or T starts with: the last one reflected through the current point, or the point itself
/// when the step before was not the same kind of curve.
fn reflect(last: Option<[f32; 2]>, cur: [f32; 2]) -> [f32; 2] {
    last.map_or(cur, |c| [2.0 * cur[0] - c[0], 2.0 * cur[1] - c[1]])
}

/// An elliptical arc from `from` to `to` as cubic Béziers of at most a quarter turn each (SVG 1.1, F.6.5 and F.6.6:
/// out-of-range radii are scaled up; a zero radius is a line).
fn arc(out: &mut Vec<Seg>, from: [f32; 2], radii: [f32; 2], rotation: f32, large: bool, sweep: bool, to: [f32; 2]) {
    if from == to {
        return;
    }
    let (mut rx, mut ry) = (radii[0].abs(), radii[1].abs());
    if rx == 0.0 || ry == 0.0 {
        out.push(Seg::Line(to));
        return;
    }
    let (sin, cos) = rotation.to_radians().sin_cos();
    let (hx, hy) = ((from[0] - to[0]) / 2.0, (from[1] - to[1]) / 2.0);
    let (x1, y1) = (cos * hx + sin * hy, -sin * hx + cos * hy);
    let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lambda > 1.0 {
        (rx, ry) = (rx * lambda.sqrt(), ry * lambda.sqrt());
    }
    let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut coef = if den == 0.0 { 0.0 } else { (num / den).max(0.0).sqrt() };
    if large == sweep {
        coef = -coef;
    }
    let (cxp, cyp) = (coef * rx * y1 / ry, -coef * ry * x1 / rx);
    let (cx, cy) = (cos * cxp - sin * cyp + (from[0] + to[0]) / 2.0, sin * cxp + cos * cyp + (from[1] + to[1]) / 2.0);
    let angle = |u: [f32; 2], v: [f32; 2]| {
        let len = (u[0].hypot(u[1]) * v[0].hypot(v[1])).max(f32::MIN_POSITIVE);
        let a = ((u[0] * v[0] + u[1] * v[1]) / len).clamp(-1.0, 1.0).acos();
        if u[0] * v[1] - u[1] * v[0] < 0.0 { -a } else { a }
    };
    let u = [(x1 - cxp) / rx, (y1 - cyp) / ry];
    let v = [(-x1 - cxp) / rx, (-y1 - cyp) / ry];
    let theta = angle([1.0, 0.0], u);
    let mut delta = angle(u, v);
    if !sweep && delta > 0.0 {
        delta -= TAU;
    } else if sweep && delta < 0.0 {
        delta += TAU;
    }
    let pieces = (delta.abs() / FRAC_PI_2).ceil().max(1.0) as usize;
    let step = delta / pieces as f32;
    let k = 4.0 / 3.0 * (step / 4.0).tan();
    let point = |t: f32| {
        let (s, c) = t.sin_cos();
        [cx + rx * c * cos - ry * s * sin, cy + rx * c * sin + ry * s * cos]
    };
    let tangent = |t: f32| {
        let (s, c) = t.sin_cos();
        [-rx * s * cos - ry * c * sin, -rx * s * sin + ry * c * cos]
    };
    let mut t = theta;
    for piece in 0..pieces {
        let next = t + step;
        let (p0, d0, d1) = (point(t), tangent(t), tangent(next));
        // The last piece ends exactly where the arc was asked to.
        let p1 = if piece + 1 == pieces { to } else { point(next) };
        out.push(Seg::Cubic([p0[0] + k * d0[0], p0[1] + k * d0[1]], [p1[0] - k * d1[0], p1[1] - k * d1[1]], p1));
        t = next;
    }
}

/// Draw `segments` into `b`, each point placed by `at`.
pub fn trace(b: &mut Builder, segments: &[Seg], at: &impl Fn([f32; 2]) -> Point) {
    for segment in segments {
        match *segment {
            Seg::Move(p) => b.move_to(at(p)),
            Seg::Line(p) => b.line_to(at(p)),
            Seg::Cubic(c1, c2, p) => b.bezier_curve_to(at(c1), at(c2), at(p)),
            Seg::Quad(c, p) => b.quadratic_curve_to(at(c), at(p)),
            Seg::Close => b.close(),
        }
    }
}

/// Every point `segments` names, control points included.
#[cfg(test)]
pub fn points(segments: &[Seg]) -> impl Iterator<Item = [f32; 2]> + '_ {
    segments.iter().flat_map(|s| match *s {
        Seg::Move(p) | Seg::Line(p) => vec![p],
        Seg::Cubic(a, b, p) => vec![a, b, p],
        Seg::Quad(c, p) => vec![c, p],
        Seg::Close => vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3
    }

    #[test]
    fn lines_and_their_shorthands_are_read_absolute_and_relative() {
        assert_eq!(
            parse("M1 2L3 4h2v-1H0V0zm1 1 2 2").unwrap(),
            vec![
                Seg::Move([1.0, 2.0]),
                Seg::Line([3.0, 4.0]),
                Seg::Line([5.0, 4.0]),
                Seg::Line([5.0, 3.0]),
                Seg::Line([0.0, 3.0]),
                Seg::Line([0.0, 0.0]),
                Seg::Close,
                Seg::Move([2.0, 3.0]),
                Seg::Line([4.0, 5.0]),
            ]
        );
    }

    #[test]
    fn numbers_are_read_as_a_minifier_writes_them() {
        assert_eq!(parse("m1.5.5-1-2l1e1.5").unwrap(), vec![Seg::Move([1.5, 0.5]), Seg::Line([0.5, -1.5]), Seg::Line([10.5, -1.0])]);
    }

    #[test]
    fn smooth_curves_reflect_the_last_control_point() {
        let s = parse("M0 0C1 1 2 1 3 0S5-1 6 0").unwrap();
        assert_eq!(s[2], Seg::Cubic([4.0, -1.0], [5.0, -1.0], [6.0, 0.0]));
        let t = parse("M0 0Q1 2 2 0T4 0").unwrap();
        assert_eq!(t[2], Seg::Quad([3.0, -2.0], [4.0, 0.0]));
        // A smooth curve after a line starts at the current point.
        let after_line = parse("M0 0L1 0S2 1 3 0").unwrap();
        assert_eq!(after_line[2], Seg::Cubic([1.0, 0.0], [2.0, 1.0], [3.0, 0.0]));
    }

    #[test]
    fn arcs_end_where_asked_and_stay_on_their_circle() {
        // Two half circles of radius 5 around (5, 5), with the flags written as a minifier writes them.
        let s = parse("M0 5a5 5 0 1010 0 5 5 0 10-10 0").unwrap();
        let ends: Vec<[f32; 2]> = s.iter().filter_map(|x| if let Seg::Cubic(_, _, p) = x { Some(*p) } else { None }).collect();
        assert!(close(*ends.last().unwrap(), [0.0, 5.0]), "{ends:?}");
        assert!(ends.iter().any(|p| close(*p, [10.0, 5.0])), "{ends:?}");
        for p in &ends {
            assert!(((p[0] - 5.0).hypot(p[1] - 5.0) - 5.0).abs() < 1e-3, "{p:?} is off the circle");
        }
        // A zero radius is a line; an arc to where it starts draws nothing.
        assert_eq!(parse("M0 0A0 4 0 0 1 3 3").unwrap()[1], Seg::Line([3.0, 3.0]));
        assert_eq!(parse("M1 1A4 4 0 0 1 1 1").unwrap(), vec![Seg::Move([1.0, 1.0])]);
    }

    #[test]
    fn what_is_not_path_data_is_refused() {
        assert!(parse("L1 1").is_err(), "a path starts with a move");
        assert!(parse("M1 1X2 2").is_err());
        assert!(parse("M1 1 2").is_err(), "half a pair");
        assert!(parse("M0 0A1 1 0 2 0 1 1").is_err(), "an arc flag is 0 or 1");
        assert_eq!(parse("").unwrap(), vec![]);
    }
}
