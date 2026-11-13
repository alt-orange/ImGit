//! SVG path data to coverage: the path parser, the flattening of its curves and a scanline rasteriser.
//!
//! This is the C++ app's own rasteriser, ported as it is (16 sub-scanlines per pixel row with exact horizontal
//! coverage, non-zero winding or even-odd) rather than replaced by a crate, so the icons come out with the
//! same pixels.

use crate::ui::geom::{Vec2, vec2};

/// A closed polygon in viewBox units.
pub type Contour = Vec<Vec2>;

/// Line segments a cubic Bézier is flattened into.
const CUBIC_STEPS: u32 = 16;
/// Line segments a quadratic Bézier is flattened into.
const QUADRATIC_STEPS: u32 = 12;
/// Sub-scanlines per pixel row.
const SUB_SCANLINES: u32 = 16;

/// SVG path data to polygons (viewBox units).
struct PathParser<'d> {
    data: &'d [u8],
    pos: usize,
    contours: Vec<Contour>,
    current: Contour,
    point: Vec2,
    start: Vec2,
    last_control: Vec2,
    /// The last command run, in upper case.
    last_command: u8,
}

/// The polygons of the SVG path data `data` (the `d` of a `<path>`), curves flattened. Malformed data yields
/// what was read up to the fault.
#[must_use]
pub fn parse_path(data: &str) -> Vec<Contour> {
    PathParser {
        data: data.as_bytes(),
        pos: 0,
        contours: Vec::new(),
        current: Contour::new(),
        point: Vec2::ZERO,
        start: Vec2::ZERO,
        last_control: Vec2::ZERO,
        last_command: 0,
    }
    .parse()
}

impl PathParser<'_> {
    fn parse(mut self) -> Vec<Contour> {
        let mut command = 0u8;
        loop {
            self.skip_separators();
            let Some(&c) = self.data.get(self.pos) else {
                break;
            };
            let letter = c.is_ascii_alphabetic();
            if letter {
                command = c;
                self.pos += 1;
            } else if command == 0 {
                break; // malformed
            }
            let before = self.pos;
            if !self.run(command) {
                break;
            }
            if !letter && self.pos == before {
                break; // a number after a command that takes none (Z): malformed, do not spin on it
            }
            // An implicit repeat of M is L.
            if command == b'M' {
                command = b'L';
            } else if command == b'm' {
                command = b'l';
            }
        }
        self.close();
        self.contours
    }

    fn peek(&self) -> Option<u8> {
        self.data.get(self.pos).copied()
    }

    fn skip_separators(&mut self) {
        while matches!(self.peek(), Some(b' ' | b',' | b'\n' | b'\t' | b'\r')) {
            self.pos += 1;
        }
    }

    fn number(&mut self) -> Option<f32> {
        self.skip_separators();
        let digit_at = |at: usize| self.data.get(at).is_some_and(u8::is_ascii_digit);
        let begin = self.pos;
        let mut i = self.pos;
        if matches!(self.data.get(i), Some(b'-' | b'+')) {
            i += 1;
        }
        let mut digits = false;
        let mut dot = false;
        while let Some(&c) = self.data.get(i) {
            if c.is_ascii_digit() {
                digits = true;
            } else if c == b'.' && !dot {
                dot = true;
            } else {
                break;
            }
            i += 1;
        }
        if !digits {
            return None;
        }
        if matches!(self.data.get(i), Some(b'e' | b'E')) {
            let mut j = i + 1;
            if matches!(self.data.get(j), Some(b'-' | b'+')) {
                j += 1;
            }
            if digit_at(j) {
                i = j;
                while digit_at(i) {
                    i += 1;
                }
            }
        }
        // Signs, digits, a dot and an exponent: ASCII, and a form `f32::from_str` reads like `strtof` (a
        // leading '+' and a bare trailing dot included).
        let text = std::str::from_utf8(&self.data[begin..i]).ok()?;
        let value = text.parse::<f32>().ok()?;
        self.pos = i;
        Some(value)
    }

    /// The numbers of a command, each offset by nothing.
    fn numbers<const N: usize>(&mut self) -> Option<[f32; N]> {
        let mut values = [0.0; N];
        for value in &mut values {
            *value = self.number()?;
        }
        Some(values)
    }

    /// An arc's flag: a single `0` or `1`, which need not be separated from what follows.
    fn flag(&mut self) -> Option<bool> {
        self.skip_separators();
        let flag = match self.peek()? {
            b'0' => false,
            b'1' => true,
            _ => return None,
        };
        self.pos += 1;
        Some(flag)
    }

    fn line_to(&mut self, p: Vec2) {
        if self.current.is_empty() {
            self.current.push(self.point);
        }
        self.current.push(p);
        self.point = p;
    }

    fn close(&mut self) {
        let contour = std::mem::take(&mut self.current);
        if contour.len() >= 3 {
            self.contours.push(contour);
        }
    }

    fn cubic(&mut self, c1: Vec2, c2: Vec2, end: Vec2) {
        let p0 = self.point;
        for i in 1..=CUBIC_STEPS {
            let t = i as f32 / CUBIC_STEPS as f32;
            let u = 1.0 - t;
            let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            self.line_to(vec2(
                a * p0.x + b * c1.x + c * c2.x + d * end.x,
                a * p0.y + b * c1.y + c * c2.y + d * end.y,
            ));
        }
    }

    fn quadratic(&mut self, c1: Vec2, end: Vec2) {
        let p0 = self.point;
        for i in 1..=QUADRATIC_STEPS {
            let t = i as f32 / QUADRATIC_STEPS as f32;
            let u = 1.0 - t;
            self.line_to(vec2(
                u * u * p0.x + 2.0 * u * t * c1.x + t * t * end.x,
                u * u * p0.y + 2.0 * u * t * c1.y + t * t * end.y,
            ));
        }
    }

    /// SVG arc (endpoint parameterisation, SVG 1.1 F.6.5).
    fn arc(
        &mut self,
        mut rx: f32,
        mut ry: f32,
        rotation_deg: f32,
        large: bool,
        sweep: bool,
        end: Vec2,
    ) {
        use std::f32::consts::{PI, TAU as TWO_PI};
        let p0 = self.point;
        if rx == 0.0 || ry == 0.0 || p0 == end {
            self.line_to(end);
            return;
        }
        rx = rx.abs();
        ry = ry.abs();
        let phi = rotation_deg * PI / 180.0;
        let (sin_phi, cos_phi) = phi.sin_cos();
        let dx = (p0.x - end.x) / 2.0;
        let dy = (p0.y - end.y) / 2.0;
        let x1 = cos_phi * dx + sin_phi * dy;
        let y1 = -sin_phi * dx + cos_phi * dy;
        let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
        if lambda > 1.0 {
            rx *= lambda.sqrt();
            ry *= lambda.sqrt();
        }
        let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
        let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
        let mut coef = if den == 0.0 {
            0.0
        } else {
            (num / den).max(0.0).sqrt()
        };
        if large == sweep {
            coef = -coef;
        }
        let cx1 = coef * (rx * y1 / ry);
        let cy1 = coef * -(ry * x1 / rx);
        let cx = cos_phi * cx1 - sin_phi * cy1 + (p0.x + end.x) / 2.0;
        let cy = sin_phi * cx1 + cos_phi * cy1 + (p0.y + end.y) / 2.0;
        let angle =
            |ux: f32, uy: f32, vx: f32, vy: f32| (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
        let theta1 = angle(1.0, 0.0, (x1 - cx1) / rx, (y1 - cy1) / ry);
        let mut delta = angle(
            (x1 - cx1) / rx,
            (y1 - cy1) / ry,
            (-x1 - cx1) / rx,
            (-y1 - cy1) / ry,
        );
        if !sweep && delta > 0.0 {
            delta -= TWO_PI;
        } else if sweep && delta < 0.0 {
            delta += TWO_PI;
        }
        let steps = ((delta.abs() / (TWO_PI / 48.0)).ceil() as i32).max(4);
        for i in 1..=steps {
            let t = theta1 + delta * i as f32 / steps as f32;
            let ex = rx * t.cos();
            let ey = ry * t.sin();
            self.line_to(if i == steps {
                end
            } else {
                vec2(
                    cos_phi * ex - sin_phi * ey + cx,
                    sin_phi * ex + cos_phi * ey + cy,
                )
            });
        }
    }

    /// Runs one `command` with the numbers that follow it. `false` when they are missing or the command is
    /// unknown.
    fn run(&mut self, command: u8) -> bool {
        self.run_command(command).is_some()
    }

    fn run_command(&mut self, command: u8) -> Option<()> {
        let base = if command.is_ascii_lowercase() {
            self.point
        } else {
            Vec2::ZERO
        };
        let upper = command.to_ascii_uppercase();
        match upper {
            b'M' => {
                let [x, y] = self.numbers()?;
                self.close();
                self.point = vec2(base.x + x, base.y + y);
                self.start = self.point;
            }
            b'L' => {
                let [x, y] = self.numbers()?;
                self.line_to(vec2(base.x + x, base.y + y));
            }
            b'H' => {
                let [x] = self.numbers()?;
                self.line_to(vec2(base.x + x, self.point.y));
            }
            b'V' => {
                let [y] = self.numbers()?;
                self.line_to(vec2(self.point.x, base.y + y));
            }
            b'C' => {
                let v: [f32; 6] = self.numbers()?;
                self.last_control = vec2(base.x + v[2], base.y + v[3]);
                self.cubic(
                    vec2(base.x + v[0], base.y + v[1]),
                    self.last_control,
                    vec2(base.x + v[4], base.y + v[5]),
                );
            }
            b'S' => {
                let v: [f32; 4] = self.numbers()?;
                let smooth = matches!(self.last_command, b'C' | b'S');
                let c1 = if smooth {
                    vec2(
                        2.0 * self.point.x - self.last_control.x,
                        2.0 * self.point.y - self.last_control.y,
                    )
                } else {
                    self.point
                };
                self.last_control = vec2(base.x + v[0], base.y + v[1]);
                self.cubic(c1, self.last_control, vec2(base.x + v[2], base.y + v[3]));
            }
            b'Q' => {
                let v: [f32; 4] = self.numbers()?;
                self.last_control = vec2(base.x + v[0], base.y + v[1]);
                self.quadratic(self.last_control, vec2(base.x + v[2], base.y + v[3]));
            }
            b'T' => {
                let [x, y] = self.numbers()?;
                let smooth = matches!(self.last_command, b'Q' | b'T');
                self.last_control = if smooth {
                    vec2(
                        2.0 * self.point.x - self.last_control.x,
                        2.0 * self.point.y - self.last_control.y,
                    )
                } else {
                    self.point
                };
                self.quadratic(self.last_control, vec2(base.x + x, base.y + y));
            }
            b'A' => {
                let [rx, ry, rotation] = self.numbers()?;
                let large = self.flag()?;
                let sweep = self.flag()?;
                let [x, y] = self.numbers()?;
                self.arc(rx, ry, rotation, large, sweep, vec2(base.x + x, base.y + y));
            }
            b'Z' => {
                if !self.current.is_empty() {
                    self.line_to(self.start);
                }
                self.close();
                self.point = self.start;
            }
            _ => return None,
        }
        self.last_command = upper;
        Some(())
    }
}

/// Turns `contours` by `degrees` (clockwise on screen) about `pivot`.
pub fn rotate(contours: &mut [Contour], pivot: Vec2, degrees: f32) {
    if degrees == 0.0 {
        return;
    }
    let radians = degrees * std::f32::consts::PI / 180.0;
    let (sin, cos) = radians.sin_cos();
    for point in contours.iter_mut().flatten() {
        let dx = point.x - pivot.x;
        let dy = point.y - pivot.y;
        *point = vec2(pivot.x + dx * cos - dy * sin, pivot.y + dx * sin + dy * cos);
    }
}

/// Polygons to coverage: non-zero winding (or even-odd), exact horizontal coverage, 16 sub-scanlines per row.
///
/// The contours are scaled by `scale` and moved by `offset` pixels, then composited at `opacity` over what
/// `coverage` (`width * height` values in 0..=1, rows top to bottom) already holds.
#[allow(clippy::too_many_arguments)]
pub fn rasterize(
    contours: &[Contour],
    width: usize,
    height: usize,
    scale: f32,
    offset: Vec2,
    opacity: f32,
    coverage: &mut [f32],
    even_odd: bool,
) {
    struct Edge {
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        dir: i32,
    }
    debug_assert_eq!(coverage.len(), width * height);
    if width == 0 || height == 0 {
        return;
    }
    let mut edges = Vec::new();
    for contour in contours {
        for (index, a) in contour.iter().enumerate() {
            let b = contour[(index + 1) % contour.len()];
            let (ax, ay) = (a.x * scale + offset.x, a.y * scale + offset.y);
            let (bx, by) = (b.x * scale + offset.x, b.y * scale + offset.y);
            if ay == by {
                continue;
            }
            edges.push(if ay < by {
                Edge {
                    x0: ax,
                    y0: ay,
                    x1: bx,
                    y1: by,
                    dir: 1,
                }
            } else {
                Edge {
                    x0: bx,
                    y0: by,
                    x1: ax,
                    y1: ay,
                    dir: -1,
                }
            });
        }
    }
    let sub = SUB_SCANLINES as f32;
    let mut row = vec![0.0f32; width];
    let mut crossings: Vec<(f32, i32)> = Vec::new();
    for y in 0..height {
        row.fill(0.0);
        for s in 0..SUB_SCANLINES {
            let sy = y as f32 + (s as f32 + 0.5) / sub;
            crossings.clear();
            crossings.extend(
                edges
                    .iter()
                    .filter(|edge| sy >= edge.y0 && sy < edge.y1)
                    .map(|edge| {
                        let t = (sy - edge.y0) / (edge.y1 - edge.y0);
                        (edge.x0 + t * (edge.x1 - edge.x0), edge.dir)
                    }),
            );
            crossings.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut winding = 0;
            for pair in crossings.windows(2) {
                winding += pair[0].1;
                let outside = if even_odd {
                    winding & 1 == 0
                } else {
                    winding == 0
                };
                if outside {
                    continue;
                }
                let xa = pair[0].0.clamp(0.0, width as f32);
                let xb = pair[1].0.clamp(0.0, width as f32);
                if xb <= xa {
                    continue;
                }
                let first = xa as usize;
                let last = (xb as usize).min(width - 1);
                for (x, cell) in row.iter_mut().enumerate().take(last + 1).skip(first) {
                    let lo = xa.max(x as f32);
                    let hi = xb.min((x + 1) as f32);
                    if hi > lo {
                        *cell += (hi - lo) / sub;
                    }
                }
            }
        }
        for (dst, covered) in coverage[y * width..(y + 1) * width].iter_mut().zip(&row) {
            let src = covered.min(1.0) * opacity;
            *dst = src + *dst * (1.0 - src);
        }
    }
}

/// How a path is placed in its raster.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// Pixels per viewBox unit.
    pub scale: f32,
    /// Where the viewBox origin lands, in pixels.
    pub offset: Vec2,
    /// The point (viewBox units) the path is turned about ...
    pub pivot: Vec2,
    /// ... by this many degrees, clockwise.
    pub degrees: f32,
}

impl Placement {
    /// Scaled and moved, not turned.
    #[must_use]
    pub const fn new(scale: f32, offset: Vec2) -> Self {
        Self {
            scale,
            offset,
            pivot: Vec2::ZERO,
            degrees: 0.0,
        }
    }
}

/// Rasterises `primary` (and under it, at 40% opacity, `secondary`: a duotone's second path) to
/// `width * height` white RGBA8 texels whose alpha is the coverage. A leading `!` on `primary` selects
/// fill-rule evenodd.
#[must_use]
pub fn rasterize_paths(
    primary: &str,
    secondary: &str,
    width: usize,
    height: usize,
    placement: Placement,
) -> Vec<u8> {
    let mut coverage = vec![0.0f32; width * height];
    let mut layer = |path: &str, opacity: f32, even_odd: bool| {
        let mut contours = parse_path(path);
        rotate(&mut contours, placement.pivot, placement.degrees);
        rasterize(
            &contours,
            width,
            height,
            placement.scale,
            placement.offset,
            opacity,
            &mut coverage,
            even_odd,
        );
    };
    if !secondary.is_empty() {
        layer(secondary, 0.4, false);
    }
    match primary.strip_prefix('!') {
        Some(path) => layer(path, 1.0, true),
        None => layer(primary, 1.0, false),
    }
    coverage
        .iter()
        .flat_map(|&alpha| [255, 255, 255, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha(rgba: &[u8], width: usize, x: usize, y: usize) -> u8 {
        rgba[(y * width + x) * 4 + 3]
    }

    #[test]
    fn parses_lines_and_closes_contours() {
        let contours = parse_path("M0 0H10V10H0Z M20,20 l5 0 0 5 -5 0 z");
        assert_eq!(contours.len(), 2);
        assert_eq!(
            contours[0],
            [
                vec2(0.0, 0.0),
                vec2(10.0, 0.0),
                vec2(10.0, 10.0),
                vec2(0.0, 10.0),
                vec2(0.0, 0.0)
            ]
        );
        // Relative, with the implicit repeats of `l`.
        assert_eq!(contours[1][1], vec2(25.0, 20.0));
        assert_eq!(contours[1][3], vec2(20.0, 25.0));
        // An unclosed contour still counts once it has three points; an implicit repeat of M is L.
        assert_eq!(parse_path("M0 0 10 0 10 10").len(), 1);
        assert!(parse_path("M0 0 L10 0").is_empty());
    }

    #[test]
    fn parses_numbers_like_svg_writes_them() {
        // No separators: signs and second dots start the next number.
        let contours = parse_path("M1.5.5-2-3L4e1 5E-1 .25+6z");
        assert_eq!(
            contours[0][..3],
            [vec2(1.5, 0.5), vec2(-2.0, -3.0), vec2(40.0, 0.5)]
        );
        assert_eq!(contours[0][3], vec2(0.25, 6.0));
        // An arc's flags may run into the next number.
        let arc = parse_path("M0 0a5 5 0 1110 0z");
        assert_eq!(arc.len(), 1);
        assert_eq!(arc[0].last().copied(), Some(vec2(0.0, 0.0)));
        assert!(arc[0].contains(&vec2(10.0, 0.0)));
    }

    #[test]
    fn stops_at_malformed_data() {
        assert!(parse_path("").is_empty());
        assert!(parse_path("10 10").is_empty());
        assert!(parse_path("M0 0 X").is_empty());
        // A number after Z does not spin.
        assert_eq!(parse_path("M0 0H10V10Z 5 5").len(), 1);
        // What was read before the fault is kept.
        assert_eq!(parse_path("M0 0H10V10H0Z L").len(), 1);
    }

    #[test]
    fn flattens_curves() {
        let cubic = parse_path("M0 0C0 10 10 10 10 0Z");
        // The start, 16 steps and the closing point.
        assert_eq!(cubic[0].len(), 1 + 16 + 1);
        assert_eq!(cubic[0][16], vec2(10.0, 0.0));
        let peak = cubic[0][8];
        assert!((peak.x - 5.0).abs() < 1e-4 && (peak.y - 7.5).abs() < 1e-4);
        let quadratic = parse_path("M0 0Q5 10 10 0T20 0Z");
        assert_eq!(quadratic[0].len(), 1 + 12 + 12 + 1);
        // The reflected control point of T mirrors the curve.
        let dip = quadratic[0][12 + 6];
        assert!((dip.x - 15.0).abs() < 1e-4 && (dip.y + 5.0).abs() < 1e-4);
        // A full circle from two half arcs stays on its radius.
        let circle = parse_path("M10 0A10 10 0 1 0 -10 0A10 10 0 1 0 10 0Z");
        for point in &circle[0] {
            assert!((point.length() - 10.0).abs() < 1e-3, "{point:?}");
        }
    }

    #[test]
    fn rasterises_exact_coverage() {
        // A square from 1.5 to 3 in a 4x4 raster: half-covered at its left and top edges.
        let rgba = rasterize_paths(
            "M1.5 1.5H3V3H1.5Z",
            "",
            4,
            4,
            Placement::new(1.0, Vec2::ZERO),
        );
        assert_eq!(rgba.len(), 4 * 4 * 4);
        assert_eq!(&rgba[..3], [255, 255, 255]);
        assert_eq!(alpha(&rgba, 4, 0, 0), 0);
        assert_eq!(alpha(&rgba, 4, 2, 2), 255);
        assert_eq!(alpha(&rgba, 4, 1, 2), 128);
        assert_eq!(alpha(&rgba, 4, 2, 1), 128);
        assert_eq!(alpha(&rgba, 4, 1, 1), 64);
        assert_eq!(alpha(&rgba, 4, 3, 3), 0);
        // Scaled by two and moved by one pixel: the same square over 4..7.
        let scaled = rasterize_paths(
            "M1.5 1.5H3V3H1.5Z",
            "",
            8,
            8,
            Placement::new(2.0, vec2(1.0, 1.0)),
        );
        assert_eq!(alpha(&scaled, 8, 4, 4), 255);
        assert_eq!(alpha(&scaled, 8, 3, 4), 0);
        assert_eq!(alpha(&scaled, 8, 6, 6), 255);
        assert_eq!(alpha(&scaled, 8, 7, 7), 0);
    }

    #[test]
    fn fill_rules_and_layers() {
        // A square with a hole wound the same way: filled by non-zero, cut out by even-odd.
        let path = "M0 0H6V6H0Z M2 2H4V4H2Z";
        let nonzero = rasterize_paths(path, "", 6, 6, Placement::new(1.0, Vec2::ZERO));
        assert_eq!(alpha(&nonzero, 6, 3, 3), 255);
        let even_odd = rasterize_paths(
            &format!("!{path}"),
            "",
            6,
            6,
            Placement::new(1.0, Vec2::ZERO),
        );
        assert_eq!(alpha(&even_odd, 6, 3, 3), 0);
        assert_eq!(alpha(&even_odd, 6, 1, 1), 255);
        // Wound the other way, the hole is a hole for both.
        let reversed = "M0 0H6V6H0Z M2 2V4H4V2Z";
        let hole = rasterize_paths(reversed, "", 6, 6, Placement::new(1.0, Vec2::ZERO));
        assert_eq!(alpha(&hole, 6, 3, 3), 0);
        // The secondary path lies under the primary at 40%.
        let duotone = rasterize_paths(
            "M0 0H2V2H0Z",
            "M0 0H4V4H0Z",
            4,
            4,
            Placement::new(1.0, Vec2::ZERO),
        );
        assert_eq!(alpha(&duotone, 4, 0, 0), 255);
        assert_eq!(alpha(&duotone, 4, 3, 3), 102);
    }

    #[test]
    fn rotates_about_a_pivot() {
        let mut contours = parse_path("M2 0H4V1H2Z");
        rotate(&mut contours, vec2(2.0, 2.0), 90.0);
        // (2, 0) is straight above the pivot: a quarter turn clockwise puts it to the right.
        let first = contours[0][0];
        assert!((first.x - 4.0).abs() < 1e-5 && (first.y - 2.0).abs() < 1e-5);
        let unchanged = parse_path("M2 0H4V1H2Z");
        let mut same = unchanged.clone();
        rotate(&mut same, vec2(9.0, 9.0), 0.0);
        assert_eq!(same, unchanged);
    }
}
