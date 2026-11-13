//! Transitions: values that follow a target over a short time, keyed by an ImGui id.

use std::collections::HashMap;

use dear_imgui_rs::sys;

use crate::ui::Cx;

/// The duration of a transition that names none: `.25s`, the stylesheet's usual.
pub const SMOOTH_SECONDS: f32 = 0.25;

/// A value moving linearly towards a target ([`Cx::smooth_value`]).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Linear {
    value: f32,
    last_frame: i32,
}

/// A CSS transition between 0 and 1: a change of target restarts it from the value shown.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Transition {
    from: f32,
    to: f32,
    shown: f32,
    start: f64,
}

impl Transition {
    /// A transition resting at `target`: the first use shows no animation from nothing.
    fn resting(target: f32, now: f64) -> Self {
        Self {
            from: target,
            to: target,
            shown: target,
            start: now,
        }
    }

    /// Moves on to time `now` towards `target`, `seconds` after each change of it. Returns the value to show
    /// and whether the transition is still under way.
    fn advance(&mut self, target: f32, now: f64, seconds: f32) -> (f32, bool) {
        if target != self.to {
            self.from = self.shown;
            self.to = target;
            self.start = now;
        }
        let progress = if seconds <= 0.0 {
            1.0
        } else {
            (((now - self.start) / f64::from(seconds)) as f32).clamp(0.0, 1.0)
        };
        self.shown = self.from + (self.to - self.from) * css_ease(progress);
        (self.shown, progress < 1.0)
    }
}

/// The transitions under way (the C++ `g_anims` / `g_transitions` tables), in [`UiState`](crate::ui::UiState).
#[derive(Debug, Default)]
pub struct Animations {
    linear: HashMap<sys::ImGuiID, Linear>,
    transitions: HashMap<sys::ImGuiID, Transition>,
    /// The last frame a transition was still moving in.
    animating_frame: Option<i32>,
}

/// CSS `ease`: `cubic-bezier(0.25, 0.1, 0.25, 1)`, solved for x by Newton's method.
#[must_use]
pub fn css_ease(x: f32) -> f32 {
    let bezier = |t: f32, p1: f32, p2: f32| {
        let u = 1.0 - t;
        3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t
    };
    let mut t = x;
    for _ in 0..8 {
        // dx/dt of the x curve (both x control points 0.25).
        let u = 1.0 - t;
        let slope = 0.75 * u * u + 2.25 * t * t;
        if slope < 1e-6 {
            break;
        }
        t = (t - (bezier(t, 0.25, 0.25) - x) / slope).clamp(0.0, 1.0);
    }
    bezier(t, 0.1, 1.0)
}

/// CSS `ease-in-out` as a quadratic: slow, fast, slow over `t` in 0..=1.
#[must_use]
pub fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        2.0 * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
    }
}

impl Cx<'_> {
    /// The CSS transitions (`ease` over [`SMOOTH_SECONDS`]) between 0 and 1: a change of `on` restarts the
    /// transition from the value shown. Keyed by `id` in the current id scope; call every frame it is needed.
    pub fn smooth(&mut self, id: &str, on: bool) -> f32 {
        self.smooth_over(id, on, SMOOTH_SECONDS)
    }

    /// [`Self::smooth`] over `seconds`.
    pub fn smooth_over(&mut self, id: &str, on: bool, seconds: f32) -> f32 {
        let key = self.id(id);
        let target = if on { 1.0 } else { 0.0 };
        let now = self.time();
        let transition = self
            .state
            .animations
            .transitions
            .entry(key)
            .or_insert_with(|| Transition::resting(target, now));
        let (shown, moving) = transition.advance(target, now, seconds);
        if moving {
            self.keep_animating();
        }
        shown
    }

    /// A value that moves towards `target` at a constant rate, a full unit taking [`SMOOTH_SECONDS`]. Starts
    /// at the target on first use.
    pub fn smooth_value(&mut self, id: &str, target: f32) -> f32 {
        self.smooth_value_over(id, target, SMOOTH_SECONDS)
    }

    /// [`Self::smooth_value`] with a full unit taking `seconds`.
    pub fn smooth_value_over(&mut self, id: &str, target: f32, seconds: f32) -> f32 {
        let key = self.id(id);
        let frame = self.frame_count();
        let delta_time = self.delta_time();
        let state = self.state.animations.linear.entry(key).or_insert(Linear {
            value: target, // first use: no animation from nothing
            last_frame: frame,
        });
        if state.last_frame != frame {
            let step = if seconds <= 0.0 {
                1.0
            } else {
                delta_time / seconds
            };
            if state.value < target {
                state.value = target.min(state.value + step);
            } else if state.value > target {
                state.value = target.max(state.value - step);
            }
        }
        state.last_frame = frame;
        let value = state.value;
        if value != target {
            self.keep_animating();
        }
        value
    }

    /// Whether a transition was still under way in the last frame.
    #[must_use]
    pub fn animating(&self) -> bool {
        self.state
            .animations
            .animating_frame
            .is_some_and(|frame| frame >= self.frame_count() - 1)
    }

    /// Keeps the main loop drawing at the display rate for the next frame instead of waiting for input: for
    /// an animation that is not a [`Self::smooth`] transition (the toasts' slides, a progress bar fed from
    /// another thread).
    pub fn keep_animating(&mut self) {
        self.state.animations.animating_frame = Some(self.frame_count());
        self.shell.request_animation();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easing_curves() {
        assert_eq!(css_ease(0.0), 0.0);
        assert!((css_ease(1.0) - 1.0).abs() < 1e-6);
        // CSS `ease` is ahead of linear: 0.8024 at the half-way point.
        assert!((css_ease(0.5) - 0.8024).abs() < 1e-3);
        assert!((css_ease(0.25) - 0.4094).abs() < 1e-3);
        for step in 0..20 {
            let (a, b) = (step as f32 / 20.0, (step + 1) as f32 / 20.0);
            assert!(css_ease(a) < css_ease(b));
        }
        assert_eq!(ease_in_out(0.0), 0.0);
        assert_eq!(ease_in_out(0.5), 0.5);
        assert_eq!(ease_in_out(1.0), 1.0);
        assert_eq!(ease_in_out(0.25), 0.125);
        assert_eq!(ease_in_out(-1.0), 0.0);
        assert_eq!(ease_in_out(2.0), 1.0);
    }

    #[test]
    fn transitions_restart_from_the_value_shown() {
        let mut transition = Transition::resting(0.0, 10.0);
        assert_eq!(transition.advance(0.0, 10.5, 0.25), (0.0, false));
        // Turned on: starts moving.
        let (start, moving) = transition.advance(1.0, 11.0, 0.25);
        assert_eq!((start, moving), (0.0, true));
        let (half, moving) = transition.advance(1.0, 11.125, 0.25);
        assert!(moving && (half - css_ease(0.5)).abs() < 1e-6);
        // Turned off half-way: goes back from where it is.
        let (back, moving) = transition.advance(0.0, 11.125, 0.25);
        assert!(moving && (back - half).abs() < 1e-6);
        let (later, _) = transition.advance(0.0, 11.25, 0.25);
        assert!(later < back && later > 0.0);
        let (done, moving) = transition.advance(0.0, 12.0, 0.25);
        assert_eq!((done, moving), (0.0, false));
        // No duration: there at once.
        assert_eq!(transition.advance(1.0, 12.0, 0.0), (1.0, false));
    }
}
