//! Points and rectangles in ImGui's coordinates.

use std::ops::{Add, AddAssign, Div, Mul, MulAssign, Neg, Sub, SubAssign};

use dear_imgui_rs::sys;

/// A point or an extent: ImGui's `ImVec2` with the arithmetic the layout code is written in. Converts from and
/// into `[f32; 2]` (what `dear_imgui_rs::Ui` takes and returns) and `sys::ImVec2`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[repr(C)] // laid out like `ImVec2`: slices of points are handed to ImGui as they are
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

/// Shorthand for [`Vec2::new`].
#[must_use]
pub const fn vec2(x: f32, y: f32) -> Vec2 {
    Vec2 { x, y }
}

impl Vec2 {
    pub const ZERO: Self = Self::new(0.0, 0.0);

    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Both components the same.
    #[must_use]
    pub const fn splat(value: f32) -> Self {
        Self::new(value, value)
    }

    #[must_use]
    pub fn floor(self) -> Self {
        Self::new(self.x.floor(), self.y.floor())
    }

    #[must_use]
    pub fn round(self) -> Self {
        Self::new(self.x.round(), self.y.round())
    }

    #[must_use]
    pub fn length(self) -> f32 {
        self.x.hypot(self.y)
    }

    /// The component-wise minimum.
    #[must_use]
    pub fn min(self, other: Self) -> Self {
        Self::new(self.x.min(other.x), self.y.min(other.y))
    }

    /// The component-wise maximum.
    #[must_use]
    pub fn max(self, other: Self) -> Self {
        Self::new(self.x.max(other.x), self.y.max(other.y))
    }

    #[must_use]
    pub const fn to_array(self) -> [f32; 2] {
        [self.x, self.y]
    }
}

impl From<[f32; 2]> for Vec2 {
    fn from([x, y]: [f32; 2]) -> Self {
        Self { x, y }
    }
}

impl From<(f32, f32)> for Vec2 {
    fn from((x, y): (f32, f32)) -> Self {
        Self { x, y }
    }
}

impl From<Vec2> for [f32; 2] {
    fn from(v: Vec2) -> Self {
        [v.x, v.y]
    }
}

impl From<sys::ImVec2> for Vec2 {
    fn from(v: sys::ImVec2) -> Self {
        Self { x: v.x, y: v.y }
    }
}

impl From<Vec2> for sys::ImVec2 {
    fn from(v: Vec2) -> Self {
        Self { x: v.x, y: v.y }
    }
}

impl Add for Vec2 {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y)
    }
}

impl Sub for Vec2 {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y)
    }
}

impl Mul<f32> for Vec2 {
    type Output = Self;
    fn mul(self, factor: f32) -> Self {
        Self::new(self.x * factor, self.y * factor)
    }
}

impl Mul<Vec2> for f32 {
    type Output = Vec2;
    fn mul(self, v: Vec2) -> Vec2 {
        v * self
    }
}

/// Component-wise.
impl Mul for Vec2 {
    type Output = Self;
    fn mul(self, other: Self) -> Self {
        Self::new(self.x * other.x, self.y * other.y)
    }
}

impl Div<f32> for Vec2 {
    type Output = Self;
    fn div(self, divisor: f32) -> Self {
        Self::new(self.x / divisor, self.y / divisor)
    }
}

impl Neg for Vec2 {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y)
    }
}

impl AddAssign for Vec2 {
    fn add_assign(&mut self, other: Self) {
        *self = *self + other;
    }
}

impl SubAssign for Vec2 {
    fn sub_assign(&mut self, other: Self) {
        *self = *self - other;
    }
}

impl MulAssign<f32> for Vec2 {
    fn mul_assign(&mut self, factor: f32) {
        *self = *self * factor;
    }
}

/// An axis-aligned rectangle `[min, max)`: ImGui's `ImRect`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub min: Vec2,
    pub max: Vec2,
}

impl Rect {
    #[must_use]
    pub const fn new(min: Vec2, max: Vec2) -> Self {
        Self { min, max }
    }

    /// The rectangle of `size` with its top-left corner at `pos`.
    #[must_use]
    pub fn from_pos_size(pos: Vec2, size: Vec2) -> Self {
        Self::new(pos, pos + size)
    }

    #[must_use]
    pub fn width(self) -> f32 {
        self.max.x - self.min.x
    }

    #[must_use]
    pub fn height(self) -> f32 {
        self.max.y - self.min.y
    }

    #[must_use]
    pub fn size(self) -> Vec2 {
        self.max - self.min
    }

    #[must_use]
    pub fn center(self) -> Vec2 {
        (self.min + self.max) / 2.0
    }

    /// Whether `point` is inside: at or after `min`, before `max`.
    #[must_use]
    pub fn contains(self, point: Vec2) -> bool {
        point.x >= self.min.x
            && point.x < self.max.x
            && point.y >= self.min.y
            && point.y < self.max.y
    }

    /// Grown by `amount` on every side (shrunk by a negative one).
    #[must_use]
    pub fn expand(self, amount: f32) -> Self {
        Self::new(
            self.min - Vec2::splat(amount),
            self.max + Vec2::splat(amount),
        )
    }

    /// Moved by `offset`.
    #[must_use]
    pub fn translate(self, offset: Vec2) -> Self {
        Self::new(self.min + offset, self.max + offset)
    }
}

impl From<Rect> for sys::ImRect {
    fn from(rect: Rect) -> Self {
        Self {
            Min: rect.min.into(),
            Max: rect.max.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vector_arithmetic() {
        let a = vec2(1.0, 2.0);
        let b = Vec2::from([3.0, 5.0]);
        assert_eq!(a + b, vec2(4.0, 7.0));
        assert_eq!(b - a, vec2(2.0, 3.0));
        assert_eq!(a * 2.0, vec2(2.0, 4.0));
        assert_eq!(2.0 * a, vec2(2.0, 4.0));
        assert_eq!(a * b, vec2(3.0, 10.0));
        assert_eq!(b / 2.0, vec2(1.5, 2.5));
        assert_eq!(-a, vec2(-1.0, -2.0));
        let mut c = a;
        c += b;
        c -= vec2(1.0, 1.0);
        c *= 2.0;
        assert_eq!(c, vec2(6.0, 12.0));
        assert_eq!(vec2(1.4, 1.6).floor(), vec2(1.0, 1.0));
        assert_eq!(vec2(1.4, 1.6).round(), vec2(1.0, 2.0));
        assert_eq!(vec2(3.0, 4.0).length(), 5.0);
        assert_eq!(a.min(vec2(0.0, 9.0)), vec2(0.0, 2.0));
        assert_eq!(a.max(vec2(0.0, 9.0)), vec2(1.0, 9.0));
        assert_eq!(<[f32; 2]>::from(a), [1.0, 2.0]);
        assert_eq!(Vec2::from((1.0, 2.0)), a);
        let raw: sys::ImVec2 = a.into();
        assert_eq!(Vec2::from(raw), a);
    }

    #[test]
    fn rectangles() {
        let rect = Rect::from_pos_size(vec2(10.0, 20.0), vec2(30.0, 40.0));
        assert_eq!(rect.max, vec2(40.0, 60.0));
        assert_eq!((rect.width(), rect.height()), (30.0, 40.0));
        assert_eq!(rect.center(), vec2(25.0, 40.0));
        assert!(rect.contains(vec2(10.0, 20.0)));
        assert!(!rect.contains(vec2(40.0, 30.0)));
        assert_eq!(rect.expand(1.0).min, vec2(9.0, 19.0));
        assert_eq!(rect.translate(vec2(1.0, 1.0)).min, vec2(11.0, 21.0));
    }
}
