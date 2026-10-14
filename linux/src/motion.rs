// Derived from zeron (MIT, github.com/zeronsh/zeron @ 053ce2e), see THIRD_PARTY_NOTICES.md
//! The motion catalog SlopShot uses, as helpers over gpui [`Animation`]: popover
//! entrances and exits, and CSS `transition-colors` hover fades.
//!
//! Custom easing is a closure over gpui's `Fn(f32) -> f32` easing shape; CSS
//! `cubic-bezier()` is evaluated exactly by [`CubicBezier`].
//!
//! translateY is a relative-position `top` inset: taffy applies relative insets after
//! layout, so, like a CSS transform, siblings never move.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use gpui::{
    Animation, AnimationElement, AnimationExt, App, ElementId, Hsla, IntoElement, Rgba, SharedString, Styled,
    Window, px,
};

/// A CSS `cubic-bezier(x1, y1, x2, y2)` timing function (endpoints fixed at (0,0) and
/// (1,1)). Evaluation solves x(t) = input by Newton iteration with a bisection
/// fallback, the standard UnitBezier approach.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CubicBezier {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}

impl CubicBezier {
    pub const fn new(x1: f32, y1: f32, x2: f32, y2: f32) -> Self {
        Self { x1, y1, x2, y2 }
    }

    fn coefficients(a: f32, b: f32) -> (f32, f32, f32) {
        let c = 3.0 * a;
        let bb = 3.0 * (b - a) - c;
        let aa = 1.0 - c - bb;
        (aa, bb, c)
    }

    fn sample_x(&self, t: f32) -> f32 {
        let (a, b, c) = Self::coefficients(self.x1, self.x2);
        ((a * t + b) * t + c) * t
    }

    fn sample_y(&self, t: f32) -> f32 {
        let (a, b, c) = Self::coefficients(self.y1, self.y2);
        ((a * t + b) * t + c) * t
    }

    fn sample_x_derivative(&self, t: f32) -> f32 {
        let (a, b, c) = Self::coefficients(self.x1, self.x2);
        (3.0 * a * t + 2.0 * b) * t + c
    }

    /// Curve parameter `t` for a given progress `x` (both 0..1).
    fn solve_t_for_x(&self, x: f32) -> f32 {
        let mut t = x;
        for _ in 0..8 {
            let err = self.sample_x(t) - x;
            if err.abs() < 1e-6 {
                return t;
            }
            let d = self.sample_x_derivative(t);
            if d.abs() < 1e-6 {
                break;
            }
            t -= err / d;
        }
        // x(t) is monotonic for valid CSS beziers.
        let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
        for _ in 0..32 {
            let mid = (lo + hi) / 2.0;
            if self.sample_x(mid) < x { lo = mid } else { hi = mid }
        }
        (lo + hi) / 2.0
    }

    /// Eased output for input progress `x ∈ [0,1]` (clamped).
    pub fn eval(&self, x: f32) -> f32 {
        if x <= 0.0 {
            return 0.0;
        }
        if x >= 1.0 {
            return 1.0;
        }
        // f32 rounding can push sample_y a hair past 1.0, and gpui's animation element
        // asserts `delta ∈ [0,1]`.
        self.sample_y(self.solve_t_for_x(x)).clamp(0.0, 1.0)
    }
}

/// CSS `ease-out`.
pub const EASE_OUT: CubicBezier = CubicBezier::new(0.0, 0.0, 0.58, 1.0);
/// CSS `ease`: quick fades, menu pops.
pub const EASE: CubicBezier = CubicBezier::new(0.25, 0.1, 0.25, 1.0);
/// Tailwind's default transition curve, CSS `cubic-bezier(0.4, 0, 0.2, 1)`.
pub const EASE_TAILWIND: CubicBezier = CubicBezier::new(0.4, 0.0, 0.2, 1.0);

/// One catalog entry: duration + optional delay + curve. The delay is folded into the
/// gpui animation timeline: the animation runs for `delay + duration` and
/// [`progress`](Self::progress) holds 0 until the delay has elapsed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionSpec {
    pub duration_ms: u64,
    pub delay_ms: u64,
    pub curve: CubicBezier,
}

impl MotionSpec {
    pub const fn new(duration_ms: u64, curve: CubicBezier) -> Self {
        Self { duration_ms, delay_ms: 0, curve }
    }

    /// Wall-clock span of the whole timeline (delay + duration).
    pub fn total(&self) -> Duration {
        Duration::from_millis(self.delay_ms + self.duration_ms)
    }

    /// Eased progress (0..1) for a raw timeline delta (0..1 across [`total`](Self::total)).
    pub fn progress(&self, raw_delta: f32) -> f32 {
        let total = (self.delay_ms + self.duration_ms) as f32;
        if total <= 0.0 || self.duration_ms == 0 {
            return 1.0;
        }
        let t = (raw_delta.clamp(0.0, 1.0) * total - self.delay_ms as f32) / self.duration_ms as f32;
        self.curve.eval(t.clamp(0.0, 1.0))
    }

    /// A oneshot gpui [`Animation`] for this spec (delay folded in).
    pub fn animation(&self) -> Animation {
        let spec = *self;
        Animation::new(spec.total().mul_f32(speed_scale())).with_easing(move |d| spec.progress(d))
    }
}

/// Popover-in: 0.14s (zeron's scale 0.96 approximated, translateY −2).
pub const MENU_IN: MotionSpec = MotionSpec::new(140, EASE);
/// Popover-out: 0.1s, quicker than the entrance so exits get out of the way.
pub const MENU_OUT: MotionSpec = MotionSpec::new(100, EASE);
/// Sliding selection indicators: 150ms.
pub const TAB_SLIDE: MotionSpec = MotionSpec::new(150, EASE_OUT);
/// CSS `transition-colors` default: 150ms over [`EASE_TAILWIND`].
pub const HOVER_FADE: MotionSpec = MotionSpec::new(150, EASE_TAILWIND);

/// Popover entrance travelling from the trigger's side: `from` is the signed starting
/// offset (negative above the resting place for dropdowns, positive below it for
/// menus that open upward).
pub fn menu_in_from<E>(id: impl Into<ElementId>, from: f32, element: E) -> AnimationElement<E>
where
    E: Styled + IntoElement + 'static,
{
    element.with_animation(id, MENU_IN.animation(), move |el, t| {
        el.relative().opacity(0.3 + 0.7 * t).top(px(from * (1.0 - t)))
    })
}

/// Popover exit retreating toward the trigger by half the entrance travel. The eased
/// progress `t` comes from the caller (computed off the closing instant at render
/// time): `with_animation`'s clock replays from 0 on remount, and a replay mid-exit
/// is a full-opacity flash. The wrapper only pumps frames for the exit's span.
pub fn menu_out_toward<E>(id: impl Into<ElementId>, toward: f32, t: f32, element: E) -> AnimationElement<E>
where
    E: Styled + IntoElement + 'static,
{
    element.with_animation(id, MENU_OUT.animation(), move |el, _| {
        el.relative().opacity(1.0 - t).top(px(toward * 0.5 * t))
    })
}

pub fn lerp(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t
}

// gpui `.hover()` styles snap; zeron fades every interactive wash over 150ms. This is
// the manual-drive tween for that: a per-element-key hover progress advanced from
// wall time, with each window's render requesting frames while a fade is in flight.
//
// The store is a main-thread `thread_local` so free-function element builders can
// blend colors without threading `cx` through every signature. SlopShot keeps several
// windows open, so entries and frame counts are per window: [`window_frame`], called
// first in each root render, names the window the builders that follow draw into.
//
// An element that unmounts mid-hover never gets its leave event, so entries are
// stamped with their window's frame on every read and pruned when a whole frame of
// that window passes without a read.

/// One element's hover fade: progress runs `origin → target` over [`HOVER_FADE`],
/// re-anchored at `origin` whenever the pointer flips direction mid-flight.
#[derive(Debug, Clone, Copy)]
struct FadeEntry {
    origin: f32,
    target: f32,
    started: Instant,
    seen: u64,
}

impl FadeEntry {
    fn value(&self, now: Instant, duration: Duration) -> f32 {
        let elapsed = now.saturating_duration_since(self.started);
        if duration.is_zero() || elapsed >= duration {
            return self.target;
        }
        let raw = elapsed.as_secs_f32() / duration.as_secs_f32();
        lerp(self.origin, self.target, HOVER_FADE.curve.eval(raw))
    }

    fn settled(&self, now: Instant, duration: Duration) -> bool {
        self.origin == self.target || now.saturating_duration_since(self.started) >= duration
    }
}

#[derive(Default)]
struct HoverFades {
    entries: HashMap<(u64, String), FadeEntry>,
    frames: HashMap<u64, u64>,
}

impl HoverFades {
    fn duration() -> Duration {
        HOVER_FADE.total().mul_f32(speed_scale())
    }

    fn frame(&self, window: u64) -> u64 {
        self.frames.get(&window).copied().unwrap_or(0)
    }

    fn set_at(&mut self, window: u64, key: &str, hovered: bool, reduced: bool, now: Instant) {
        let target = if hovered { 1.0 } else { 0.0 };
        let id = (window, key.to_string());
        let current = self.entries.get(&id).map(|e| e.value(now, Self::duration())).unwrap_or(0.0);
        if target == 0.0 && !self.entries.contains_key(&id) {
            return;
        }
        let origin = if reduced { target } else { current };
        let seen = self.frame(window);
        self.entries.insert(id, FadeEntry { origin, target, started: now, seen });
    }

    fn value_at(&mut self, window: u64, key: &str, now: Instant) -> f32 {
        let frame = self.frame(window);
        match self.entries.get_mut(&(window, key.to_string())) {
            Some(entry) => {
                entry.seen = frame;
                entry.value(now, Self::duration())
            }
            None => 0.0,
        }
    }

    /// Advance `window`'s frame, prune its entries settled at rest or unread for a
    /// whole frame, and report whether any of its fades is still mid-flight.
    fn tick_at(&mut self, window: u64, now: Instant) -> bool {
        let frame = self.frames.entry(window).or_default();
        *frame += 1;
        let frame = *frame;
        let duration = Self::duration();
        let mut active = false;
        self.entries.retain(|(owner, _), entry| {
            if *owner != window {
                return true;
            }
            if entry.seen + 1 < frame {
                return false;
            }
            let settled = entry.settled(now, duration);
            if !settled {
                active = true;
            }
            !(settled && entry.target == 0.0)
        });
        active
    }
}

thread_local! {
    static HOVER_FADES: RefCell<HoverFades> = RefCell::new(HoverFades::default());
    static CURRENT_WINDOW: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

fn window_key(window: &Window) -> u64 {
    window.window_handle().window_id().as_u64()
}

/// Call first in every root view's render: names the window the hover blends that
/// follow draw into, and keeps frames coming while one of its fades is mid-flight.
pub fn window_frame(window: &mut Window) {
    let key = window_key(window);
    CURRENT_WINDOW.with(|current| current.set(key));
    if HOVER_FADES.with(|fades| fades.borrow_mut().tick_at(key, Instant::now())) {
        window.request_animation_frame();
    }
}

/// Hover progress (0..1) for `key` this frame.
pub fn hover_t(key: &str) -> f32 {
    let window = CURRENT_WINDOW.with(|current| current.get());
    HOVER_FADES.with(|fades| fades.borrow_mut().value_at(window, key, Instant::now()))
}

/// An `.on_hover` listener driving the fade for `key`; pair with
/// [`hover_t`]/[`hover_blend`] reads of the same key in the same element.
pub fn hover_listener(key: impl Into<SharedString>) -> impl Fn(&bool, &mut Window, &mut App) + 'static {
    let key = key.into();
    move |hovered, window, cx| {
        let reduced = reduced_motion(cx);
        HOVER_FADES.with(|fades| fades.borrow_mut().set_at(window_key(window), &key, *hovered, reduced, Instant::now()));
        // `request_animation_frame` is draw-phase-only; the root render keeps frames
        // coming while the fade is mid-flight.
        window.refresh();
    }
}

/// Blend two colors by `t` the way browsers transition them: sRGB with premultiplied
/// alpha, so a wash fading in from transparent brightens without passing through grey.
pub fn mix(from: Hsla, to: Hsla, t: f32) -> Hsla {
    let t = t.clamp(0.0, 1.0);
    if t <= 0.0 {
        return from;
    }
    if t >= 1.0 {
        return to;
    }
    let (f, g) = (Rgba::from(from), Rgba::from(to));
    let a = lerp(f.a, g.a, t);
    if a <= f32::EPSILON {
        return Hsla::from(Rgba { a: 0.0, ..g });
    }
    Hsla::from(Rgba {
        r: lerp(f.r * f.a, g.r * g.a, t) / a,
        g: lerp(f.g * f.a, g.g * g.a, t) / a,
        b: lerp(f.b * f.a, g.b * g.a, t) / a,
        a,
    })
}

/// The standard hover blend: rest → hover color at `key`'s current progress.
pub fn hover_blend(key: &str, rest: Hsla, hover: Hsla) -> Hsla {
    mix(rest, hover, hover_t(key))
}

/// Measurement knob (`SLOPSHOT_MOTION_SCALE`, default 1): stretches every timeline.
pub fn speed_scale() -> f32 {
    static SCALE: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *SCALE.get_or_init(|| {
        std::env::var("SLOPSHOT_MOTION_SCALE")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .filter(|s| s.is_finite())
            .map(|s| s.clamp(0.01, 100.0))
            .unwrap_or(1.0)
    })
}

pub fn reduced_motion(cx: &App) -> bool {
    cx.reduce_motion()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bezier_endpoints_and_clamp() {
        for curve in [EASE, EASE_OUT, EASE_TAILWIND] {
            assert_eq!(curve.eval(0.0), 0.0);
            assert_eq!(curve.eval(1.0), 1.0);
            let mid = curve.eval(0.5);
            assert!(mid > 0.0 && mid < 1.0);
        }
    }

    #[test]
    fn hover_fade_ramps_and_reverses_continuously() {
        let mut fades = HoverFades::default();
        let t0 = Instant::now();
        let ms = |m: u64| t0 + Duration::from_millis(m);

        fades.set_at(1, "pill", true, false, t0);
        assert_eq!(fades.value_at(1, "pill", t0), 0.0);
        let mid = fades.value_at(1, "pill", ms(75));
        assert!(mid > 0.0 && mid < 1.0, "mid-flight enter: {mid}");
        assert_eq!(fades.value_at(1, "pill", ms(150)), 1.0);

        fades.set_at(1, "pill", true, false, t0);
        let at_flip = fades.value_at(1, "pill", ms(75));
        fades.set_at(1, "pill", false, false, ms(75));
        let after_flip = fades.value_at(1, "pill", ms(75));
        assert!((after_flip - at_flip).abs() < 1e-4, "continuity: {at_flip} vs {after_flip}");
        assert_eq!(fades.value_at(1, "pill", ms(225)), 0.0, "lands at rest");
    }

    #[test]
    fn unread_fades_are_pruned_per_window() {
        let mut fades = HoverFades::default();
        let t0 = Instant::now();
        fades.set_at(1, "row", true, false, t0);
        fades.set_at(2, "row", true, false, t0);
        fades.tick_at(1, t0);
        fades.tick_at(1, t0);
        assert_eq!(fades.entries.len(), 1, "another window's frames leave this one's fade alone");
        assert_eq!(fades.value_at(2, "row", t0 + Duration::from_secs(1)), 1.0);
    }
}
