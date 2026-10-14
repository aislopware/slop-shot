// Derived from zeron (MIT, github.com/zeronsh/zeron @ 75b7260), see THIRD_PARTY_NOTICES.md
//! Popover and menu primitives: a floating card with the `menu-in` animation and an
//! exit phase, outside-press dismissal, and the keyboard reducers the selects share.
//!
//! The trigger element conditionally children a deferred layer: deferred paints on a
//! floating layer above everything, positioned against the trigger.

mod contained;
pub use contained::contained_menu;

use std::time::Instant;

use gpui::{AnyElement, Div, Hsla, IntoElement, SharedString, div, prelude::*, px};

use crate::motion;
use crate::theme::{self, Theme, ink};

/// Popup state with an exit phase. gpui unmounts an element the frame its state drops,
/// so a closing animation needs the state held alive while the exit plays: `open` →
/// `begin_close` (render keeps mounting, with the out animation and dead hit-testing)
/// → [`reap_popup`]'s timer `finish_close`es after [`motion::MENU_OUT`]. Use
/// [`Self::is_open`] for logic (a closing popup already reads as closed) and
/// [`Self::get`] / [`Self::closing_since`] for rendering.
pub struct Popup<T> {
    inner: Option<(T, Option<Instant>)>,
    /// Whether the popup was still mounted when the current trigger press began; see
    /// [`Self::note_trigger_press`].
    pressed_while_open: bool,
}

impl<T> Default for Popup<T> {
    fn default() -> Self {
        Self { inner: None, pressed_while_open: false }
    }
}

impl<T> Popup<T> {
    pub fn open(&mut self, value: T) {
        self.inner = Some((value, None));
    }

    /// Open and interactive (not closing).
    pub fn is_open(&self) -> bool {
        matches!(self.inner, Some((_, None)))
    }

    /// When the exit phase began.
    pub fn closing_since(&self) -> Option<Instant> {
        match &self.inner {
            Some((_, Some(since))) => Some(*since),
            _ => None,
        }
    }

    /// The state while mounted, open or playing the exit animation.
    pub fn get(&self) -> Option<&T> {
        self.inner.as_ref().map(|(value, _)| value)
    }

    /// Enter the exit phase. Returns `true` when this call started it (the caller then
    /// schedules [`reap_popup`]).
    pub fn begin_close(&mut self) -> bool {
        match &mut self.inner {
            Some((_, closing @ None)) => {
                *closing = Some(Instant::now());
                true
            }
            _ => false,
        }
    }

    /// Record, from the trigger's `on_mouse_down`, whether this popup is still mounted.
    /// The card's `on_mouse_down_out` fires on that same press and begins the close, so
    /// by click time the popup already reads as closed; without the note a toggle
    /// would close and reopen.
    pub fn note_trigger_press(&mut self) {
        self.pressed_while_open = self.inner.is_some();
    }

    /// Consume the press note: `true` when the press behind the current click found the
    /// popup mounted, so the click should leave it closed.
    pub fn take_press_was_open(&mut self) -> bool {
        std::mem::take(&mut self.pressed_while_open)
    }

    /// Drop the state once the exit phase has run its course. A popup reopened since
    /// the matching [`Self::begin_close`] is left alone.
    pub fn finish_close(&mut self) {
        if let Some((_, Some(since))) = &self.inner
            && since.elapsed() >= motion::MENU_OUT.total().mul_f32(motion::speed_scale())
        {
            self.inner = None;
        }
    }
}

/// Schedule the reap for a [`Popup::begin_close`]: after the exit animation, drop the
/// popup state and repaint. `popup` re-borrows the field from the view.
pub fn reap_popup<V: 'static, T: 'static>(cx: &mut gpui::Context<V>, popup: impl Fn(&mut V) -> &mut Popup<T> + 'static) {
    cx.spawn(async move |view, cx| {
        let span = motion::MENU_OUT.total().mul_f32(motion::speed_scale());
        cx.background_executor().timer(span.saturating_add(std::time::Duration::from_millis(20))).await;
        view.update(cx, |view, cx| {
            popup(view).finish_close();
            cx.notify();
        })
        .ok();
    })
    .detach();
}

/// Step the active row of a menu: wraps at both ends; `None` enters at the edge
/// matching the direction. Empty menus stay `None`.
pub fn menu_step(active: Option<usize>, count: usize, delta: isize) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let count = count as isize;
    let next = match active {
        None if delta >= 0 => 0,
        None => count - 1,
        Some(at) => (at as isize + delta).rem_euclid(count),
    };
    Some(next as usize)
}

/// Keys the menus care about, classified from a raw keystroke.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuKey {
    Up,
    Down,
    Enter,
    Escape,
    Other,
}

pub fn classify_key(key: &str, ctrl: bool) -> MenuKey {
    match key {
        "up" => MenuKey::Up,
        "down" => MenuKey::Down,
        // Readline motion: ctrl-n/ctrl-p mirror ↓/↑.
        "n" if ctrl => MenuKey::Down,
        "p" if ctrl => MenuKey::Up,
        "enter" => MenuKey::Enter,
        "escape" => MenuKey::Escape,
        _ => MenuKey::Other,
    }
}

pub const CARD_RADIUS: f32 = 12.0;
pub const MENU_GAP: f32 = 2.0;
pub const CARD_INSET: f32 = 4.0;
/// Concentric corners: rows sit inside both the card's 1px border and padding.
pub const MENU_ITEM_RADIUS: f32 = CARD_RADIUS - 1.0 - CARD_INSET;

pub fn surface_bg(theme: &Theme) -> Hsla {
    theme.input_glass_bg()
}

/// The floating surface shared by dropdowns, menus and tooltips.
pub fn popover_card(theme: &Theme) -> Div {
    div()
        .border_1()
        .border_color(theme.border)
        .rounded(px(CARD_RADIUS))
        .when(!theme.is_frost(), |el| el.shadow_lg())
        .bg(surface_bg(theme))
        .p(px(CARD_INSET))
        .gap(px(MENU_GAP))
        .overflow_hidden()
        .text_size(px(13.0))
        .text_color(theme.text)
}

/// Eased exit progress (0..=1) for a [`Popup`] closing instant, from the wall clock at
/// render time, so it can never replay from 0 mid-exit.
fn exit_progress(since: Instant) -> f32 {
    let total = motion::MENU_OUT.total().mul_f32(motion::speed_scale()).as_secs_f32();
    let raw = if total <= 0.0 { 1.0 } else { (since.elapsed().as_secs_f32() / total).clamp(0.0, 1.0) };
    motion::MENU_OUT.progress(raw)
}

/// The press guard around a popover layer. Outside-dismiss listeners run during
/// capture; consuming the same press during bubble, after dismissal, keeps content
/// behind the menu from acting on it too.
fn guarded_menu(content: AnyElement) -> AnyElement {
    let guard = gpui::canvas(
        |_, _, _| (),
        |bounds, _, window, _| {
            window.on_mouse_event(move |event: &gpui::MouseDownEvent, phase, _, cx| {
                if phase == gpui::DispatchPhase::Bubble && !bounds.contains(&event.position) {
                    cx.stop_propagation();
                }
            });
        },
    )
    .absolute()
    .inset_0();
    div().relative().child(guard).child(content).into_any_element()
}

/// Entrance or exit motion for a popover layer. While exiting, the content plays the
/// exit under a fresh animation id (reusing the id would inherit the entrance's
/// finished clock) beneath an occluding overlay, so the dying menu's rows take no
/// clicks.
fn menu_motion(id: SharedString, exit: Option<f32>, inner: Div) -> AnyElement {
    const FROM: f32 = -2.0;
    if let Some(t) = exit {
        let inner = inner.relative().child(div().absolute().inset_0().occlude());
        motion::menu_out_toward(SharedString::from(format!("{id}-out")), FROM, t, inner).into_any_element()
    } else {
        motion::menu_in_from(id, FROM, inner).into_any_element()
    }
}

/// One menu row: `gap-2.5 rounded-lg px-2 py-1.5 text-[13px]`, active on the selected
/// wash, hover wash fading over `transition-colors`. The caller adds the id and click
/// listener; `fade_key` must be unique app-wide and stable across frames.
pub fn menu_row(theme: &Theme, active: bool, fade_key: impl Into<SharedString>) -> Div {
    let row = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .px(px(8.0))
        .py(px(6.0))
        .rounded(px(MENU_ITEM_RADIUS))
        .text_size(px(13.0))
        .cursor_pointer();
    if active {
        row.bg(theme::card_selected_bg()).text_color(theme.text)
    } else {
        let fade_key = fade_key.into();
        let mut row = row
            .text_color(motion::hover_blend(&fade_key, theme.text.opacity(0.9), theme.text))
            .bg(motion::hover_blend(&fade_key, theme::wash(0.0), theme::card_selected_bg()));
        row.interactivity().on_hover(motion::hover_listener(fade_key));
        row
    }
}

/// [`menu_row`] with a keyboard-navigation highlight distinct from the selection.
pub fn menu_row_nav(theme: &Theme, selected: bool, highlighted: bool, fade_key: impl Into<SharedString>) -> Div {
    let row = menu_row(theme, selected, fade_key);
    if !selected && highlighted { row.bg(theme::card_selected_bg()).text_color(theme.text) } else { row }
}

/// Dialog title: `text-[15px] font-semibold`.
pub fn dialog_title(theme: &Theme, title: impl Into<SharedString>) -> Div {
    div().text_size(px(15.0)).font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme.text).child(title.into())
}

/// Dialog body copy: `text-[13px] leading-relaxed text-muted-foreground`.
pub fn dialog_body(theme: &Theme, copy: impl Into<SharedString>) -> Div {
    div().text_size(px(13.0)).line_height(px(19.0)).text_color(theme.text_muted).child(copy.into())
}

/// Ghost button: quiet text, hover wash fading over `transition-colors`. Caller adds
/// id and click; `fade_key` as in [`menu_row`].
pub fn btn_ghost(theme: &Theme, label: impl Into<SharedString>, fade_key: impl Into<SharedString>) -> Div {
    let fade_key = fade_key.into();
    let mut btn = div()
        .px(px(12.0))
        .py(px(6.0))
        .rounded(px(8.0))
        .text_size(px(13.0))
        .text_color(motion::hover_blend(&fade_key, theme.text_muted, theme.text))
        .bg(motion::hover_blend(&fade_key, theme::wash(0.0), ink(0.06)))
        .cursor_pointer()
        .child(label.into());
    btn.interactivity().on_hover(motion::hover_listener(fade_key));
    btn
}

/// Primary button: white fill, near-black text.
pub fn btn_primary(theme: &Theme, label: impl Into<SharedString>) -> Div {
    div()
        .px(px(12.0))
        .py(px(6.0))
        .rounded(px(8.0))
        .bg(theme.text)
        .text_size(px(13.0))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(theme.on_solid)
        .cursor_pointer()
        .hover(|s| s.opacity(0.9))
        .child(label.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_step_wraps_and_enters_at_the_edge() {
        assert_eq!(menu_step(None, 3, 1), Some(0));
        assert_eq!(menu_step(None, 3, -1), Some(2));
        assert_eq!(menu_step(Some(2), 3, 1), Some(0));
        assert_eq!(menu_step(Some(0), 3, -1), Some(2));
        assert_eq!(menu_step(Some(0), 0, 1), None);
    }

    #[test]
    fn a_trigger_press_on_an_open_popup_keeps_it_closed() {
        let mut popup = Popup::default();
        popup.open(());
        popup.note_trigger_press();
        assert!(popup.begin_close());
        assert!(popup.take_press_was_open());
        assert!(!popup.take_press_was_open());
    }
}
