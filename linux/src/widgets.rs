// Derived from zeron (MIT, github.com/zeronsh/zeron @ 75b7260), see THIRD_PARTY_NOTICES.md
//! The settings vocabulary Slopty ships: a centered page column, large title, small
//! section labels over filled blocks of hairline-split rows, pill switches, selects,
//! sliders and one family of action buttons, so every window reads as one product.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    AnyElement, Bounds, Context, Div, ElementId, FontWeight, Hsla, Pixels, SharedString, Stateful, Window, div,
    prelude::*, px,
};

use crate::icons::{self, icon};
use crate::popover;
use crate::theme::{self, Theme, flatten};

pub const ROW_TITLE_SIZE: f32 = 13.0;
pub const ROW_DESCRIPTION_SIZE: f32 = 12.0;

/// The page column's outer max width and side padding.
const PAGE_MAX_WIDTH: f32 = 760.0;
const PAGE_PAD_X: f32 = 40.0;

/// Centered page column under the titlebar strip.
pub fn page_column() -> Div {
    div()
        .w_full()
        .max_w(px(PAGE_MAX_WIDTH))
        .mx_auto()
        .px(px(PAGE_PAD_X))
        // Titlebar clearance lives inside the scroll so content scrolls up to the edge.
        .pt(px(Theme::TITLEBAR_HEIGHT + 16.0))
        .pb(px(48.0))
        .flex()
        .flex_col()
}

/// Page headline row: a large title, optionally followed by a muted count on the same
/// baseline. Inset to line up with the section labels.
pub fn page_header(theme: &Theme, title: impl Into<SharedString>, count: Option<usize>) -> Div {
    div()
        .px(px(SECTION_LABEL_INSET))
        .flex()
        .flex_row()
        .items_baseline()
        .gap(px(10.0))
        .child(
            div()
                .text_size(px(20.0))
                .line_height(px(26.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text)
                .child(title.into()),
        )
        .when_some(count, |el, count| {
            el.child(div().text_size(px(13.0)).text_color(theme.text_muted).child(SharedString::from(count.to_string())))
        })
}

/// Subtitle under the headline: `mt-1 text-[13px] text-muted-foreground`.
pub fn page_subtitle(theme: &Theme, copy: impl Into<SharedString>) -> Div {
    div()
        .mt(px(4.0))
        .px(px(SECTION_LABEL_INSET))
        .min_w_0()
        .text_size(px(13.0))
        .line_height(px(17.0))
        .text_color(theme.text_muted)
        .child(copy.into())
}

/// Section labels and page titles sit a little inside the blocks' edge.
const SECTION_LABEL_INSET: f32 = 8.0;
/// Air between one section and the next.
const SECTION_GAP: f32 = 32.0;

/// The small plain label above a block: sentence case, muted, never an eyebrow.
pub fn section_label(theme: &Theme, label: impl Into<SharedString>) -> Div {
    div()
        .px(px(SECTION_LABEL_INSET))
        .text_size(px(13.0))
        .line_height(px(17.0))
        .text_color(theme.text_muted)
        .child(label.into())
}

/// A labeled section: the label, then its block tucked close underneath.
pub fn section(theme: &Theme, label: impl Into<SharedString>, block: impl IntoElement) -> Div {
    div().mt(px(SECTION_GAP)).flex().flex_col().gap(px(8.0)).child(section_label(theme, label)).child(block)
}

/// The block's fill: a faint solid tint of the theme's ink, no outline.
pub fn block_fill(theme: &Theme) -> Hsla {
    theme.wash(0.045)
}

/// The hairline between rows inside a block.
pub fn row_divider(theme: &Theme) -> Hsla {
    theme.border.opacity(0.6)
}

/// A filled, rounded block of related rows.
pub fn section_card(theme: &Theme) -> Div {
    div().rounded(px(12.0)).bg(block_fill(theme)).overflow_hidden().flex().flex_col()
}

/// One row in a [`section_card`]: title and muted description on the left, the control
/// on the right, split from the previous row by a hairline inset to the text edge.
pub fn card_row(theme: &Theme, first: bool) -> Div {
    div()
        .mx(px(16.0))
        .py(px(12.0))
        .min_h(px(60.0))
        .when(!first, |el| el.border_t_1().border_color(row_divider(theme)))
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap(px(16.0))
}

/// Bare leading glyph for rows whose icon carries identity.
pub fn row_tile(theme: &Theme, icon_path: &'static str) -> Div {
    div()
        .flex_none()
        .w(px(24.0))
        .h(px(32.0))
        .flex()
        .items_center()
        .justify_center()
        .child(icon(icon_path).size(px(16.0)).text_color(theme.text_muted))
}

pub fn row_title(theme: &Theme, title: impl Into<SharedString>) -> Div {
    div()
        .min_w_0()
        .text_size(px(ROW_TITLE_SIZE))
        .line_height(px(17.0))
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme.text)
        .child(title.into())
}

/// The quiet meta line under a row title: fragments joined by dots.
pub fn meta_line(theme: &Theme, fragments: Vec<AnyElement>) -> Div {
    let mut line = div()
        .mt(px(Theme::TEXT_STACK_GAP))
        .min_w_0()
        .max_w_full()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap_x(px(8.0))
        .gap_y(px(2.0))
        .text_size(px(ROW_DESCRIPTION_SIZE))
        .line_height(px(16.0))
        .text_color(theme.text_muted);
    for (i, fragment) in fragments.into_iter().enumerate() {
        if i > 0 {
            line = line.child(div().text_color(theme.text_muted.opacity(0.3)).child("·"));
        }
        line = line.child(div().min_w_0().max_w_full().child(fragment));
    }
    line
}

/// A row's text column: [`row_title`] over an optional one-fragment [`meta_line`].
pub fn row_text(theme: &Theme, title: impl Into<SharedString>, description: Option<SharedString>) -> Div {
    div().flex_1().min_w(px(200.0)).child(row_title(theme, title)).when_some(description, |el, description| {
        el.child(meta_line(theme, vec![div().child(description).into_any_element()]))
    })
}

/// Right-anchored badge pill: a filled wash, no outline.
pub fn badge(theme: &Theme, label: impl Into<SharedString>) -> Div {
    div()
        .flex_none()
        .px(px(8.0))
        .py(px(2.0))
        .rounded_full()
        .bg(theme.wash(0.08))
        .text_size(px(10.5))
        .text_color(theme.text_muted)
        .child(label.into())
}

/// Emerald status pill.
pub const SWITCH_WIDTH: f32 = 44.8;
const SWITCH_HEIGHT: f32 = 28.8;
const SWITCH_TRACK_HEIGHT: f32 = 20.8;
const SWITCH_SIDE_INSET: f32 = 1.6;
const SWITCH_THUMB_WIDTH: f32 = 24.0;
const SWITCH_THUMB_HEIGHT: f32 = SWITCH_TRACK_HEIGHT - 2.0 * SWITCH_SIDE_INSET;
const SWITCH_MARK_SIZE: f32 = 7.2;

/// A pill switch with the on/off marks nested beneath a sliding thumb. The caller owns
/// activation; only the thumb interpolates.
pub fn toggle_switch(theme: &Theme, on: bool, key: impl Into<SharedString>) -> Div {
    let key: SharedString = key.into();
    div().flex_none().w(px(SWITCH_WIDTH)).h(px(SWITCH_HEIGHT)).child(SwitchVisual {
        theme: theme.clone(),
        on,
        key: format!("settings-switch-{key}").into(),
    })
}

#[derive(IntoElement)]
struct SwitchVisual {
    theme: Theme,
    on: bool,
    key: SharedString,
}

struct SwitchTravel {
    from: f32,
    target: f32,
    started: std::time::Instant,
}

impl SwitchTravel {
    fn value(&self, now: std::time::Instant) -> f32 {
        let t = (now.saturating_duration_since(self.started).as_secs_f32() / 0.18).min(1.0);
        self.from + (self.target - self.from) * (1.0 - (1.0 - t).powi(3))
    }
}

fn switch_track_color(theme: &Theme, on: bool) -> Hsla {
    if on {
        // Keep the accent saturated and opaque, with more depth against the dark surface.
        flatten(gpui::black().opacity(0.14), theme.accent_strong)
    } else {
        flatten(theme.ink(0.18), theme.surface)
    }
}

fn switch_thumb_color(theme: &Theme) -> Hsla {
    flatten(gpui::white().opacity(0.96), theme.surface)
}

impl RenderOnce for SwitchVisual {
    fn render(self, window: &mut Window, cx: &mut gpui::App) -> impl IntoElement {
        let now = std::time::Instant::now();
        let target = if self.on { 1.0 } else { 0.0 };
        let reduced = crate::motion::reduced_motion(cx);
        let position = window.with_global_id(self.key.into(), |id, window| {
            window.with_element_state(id, |previous: Option<SwitchTravel>, _| {
                let mut travel = previous.unwrap_or(SwitchTravel { from: target, target, started: now });
                let current = travel.value(now);
                if travel.target != target {
                    travel = SwitchTravel { from: current, target, started: now };
                }
                if reduced {
                    travel.from = target;
                    travel.target = target;
                }
                (travel.value(now), travel)
            })
        });
        if (position - target).abs() > 0.001 {
            window.request_animation_frame();
        }
        let track = switch_track_color(&self.theme, self.on);
        let thumb = switch_thumb_color(&self.theme);
        let empty_width = SWITCH_WIDTH - SWITCH_THUMB_WIDTH - SWITCH_SIDE_INSET;
        let mark_padding = (empty_width - SWITCH_MARK_SIZE) / 2.0;
        let thumb_left = SWITCH_SIDE_INSET + (SWITCH_WIDTH - SWITCH_THUMB_WIDTH - 2.0 * SWITCH_SIDE_INSET) * position;
        let mark = || div().size(px(SWITCH_MARK_SIZE)).flex().items_center().justify_center();
        let track_element = div()
            .absolute()
            .top(px((SWITCH_HEIGHT - SWITCH_TRACK_HEIGHT) / 2.0))
            .left_0()
            .w(px(SWITCH_WIDTH))
            .h(px(SWITCH_TRACK_HEIGHT))
            .rounded_full()
            .bg(track)
            .border_1()
            .border_color(if self.on {
                flatten(gpui::white().opacity(0.12), track)
            } else {
                flatten(self.theme.border, track)
            })
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .px(px(mark_padding))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        mark()
                            .opacity(position)
                            .child(div().w(px(1.2)).h(px(7.2)).rounded_full().bg(gpui::white().opacity(0.96))),
                    )
                    .child(
                        mark().opacity(1.0 - position).child(
                            div().size(px(6.4)).rounded_full().border(px(1.0)).border_color(gpui::white().opacity(0.92)),
                        ),
                    ),
            );
        let thumb_element = div()
            .absolute()
            .top(px((SWITCH_HEIGHT - SWITCH_THUMB_HEIGHT) / 2.0))
            .left(px(thumb_left))
            .w(px(SWITCH_THUMB_WIDTH))
            .h(px(SWITCH_THUMB_HEIGHT))
            .rounded_full()
            .bg(thumb)
            .border_1()
            .border_color(flatten(gpui::black().opacity(0.10), thumb));
        div().relative().w(px(SWITCH_WIDTH)).h(px(SWITCH_HEIGHT)).child(track_element).child(thumb_element)
    }
}

struct TabSelectionTravel {
    from: f32,
    target: f32,
    started: std::time::Instant,
}

impl TabSelectionTravel {
    fn value(&self, now: std::time::Instant) -> f32 {
        let seconds = crate::motion::TAB_SLIDE.total().as_secs_f32() * crate::motion::speed_scale();
        let elapsed = now.saturating_duration_since(self.started).as_secs_f32();
        let progress = crate::motion::TAB_SLIDE.progress((elapsed / seconds).min(1.0));
        self.from + (self.target - self.from) * progress
    }
}

/// Retargetable selected-state fade for a tab. First paint and reduced-motion changes
/// settle immediately instead of flashing an entrance.
pub fn tab_selection_t(window: &mut Window, key: impl Into<SharedString>, selected: bool, reduced_motion: bool) -> f32 {
    let now = std::time::Instant::now();
    let target = if selected { 1.0 } else { 0.0 };
    let value = window.with_global_id(key.into().into(), |id, window| {
        window.with_element_state(id, |previous: Option<TabSelectionTravel>, _| {
            let mut travel = previous.unwrap_or(TabSelectionTravel { from: target, target, started: now });
            let current = travel.value(now);
            if travel.target != target {
                travel = TabSelectionTravel { from: current, target, started: now };
            }
            if reduced_motion {
                travel.from = target;
                travel.target = target;
            }
            (travel.value(now), travel)
        })
    });
    if (value - target).abs() > 0.001 {
        window.request_animation_frame();
    }
    value
}

/// One treatment for section tabs: the selected wash and text ease over the tab
/// timing, while hover keeps the sidebar color fade.
pub fn section_tab(
    theme: &Theme,
    selected: bool,
    selection_t: f32,
    id: impl Into<ElementId>,
    hover_key: impl Into<SharedString>,
) -> Stateful<Div> {
    let hover_key = hover_key.into();
    let base_bg = crate::motion::mix(theme::wash(0.0), theme::glass_selected_bg(), selection_t);
    let base_text = crate::motion::mix(theme.text_muted, theme.text, selection_t);
    let hover_bg = if selected { base_bg } else { theme.glass_hover() };
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .rounded(px(8.0))
        .px(px(Theme::SPACE_SM))
        .py(px(6.0))
        .min_h(px(32.0))
        .flex_shrink_0()
        .text_size(px(13.0))
        .when(selected, |el| el.font_weight(FontWeight::MEDIUM))
        .text_color(crate::motion::hover_blend(&hover_key, base_text, theme.text))
        .bg(crate::motion::hover_blend(&hover_key, base_bg, hover_bg))
        .cursor_pointer()
        .id(id.into())
        .on_hover(crate::motion::hover_listener(hover_key))
}

/// Height of every select trigger: centered in a 60px card row.
pub const SELECT_HEIGHT: f32 = 32.0;
/// Menus never shrink below this, so short values still open a readable list.
const SELECT_MENU_MIN_WIDTH: f32 = 160.0;

/// A select's fill on a block: a translucent wash one step above [`block_fill`],
/// lifting while hovered or open.
pub fn select_fill(theme: &Theme, lifted: bool) -> Hsla {
    theme.wash(if lifted { 0.10 } else { 0.06 })
}

/// The trigger of every dropdown: a borderless filled pill. The transparent edge is
/// held for the focus ring, so focusing never shifts the label.
pub fn select_trigger(theme: &Theme, id: impl Into<SharedString>, open: bool) -> Stateful<Div> {
    let id: SharedString = id.into();
    let hover_key: SharedString = format!("{id}-hover").into();
    let lifted = select_fill(theme, true);
    div()
        .id(id)
        .relative()
        .flex_none()
        .max_w_full()
        .h(px(SELECT_HEIGHT))
        .pl(px(10.0))
        .pr(px(8.0))
        .rounded(px(8.0))
        .border_1()
        .border_color(gpui::transparent_black())
        .bg(if open { lifted } else { crate::motion::hover_blend(&hover_key, select_fill(theme, false), lifted) })
        .on_hover(crate::motion::hover_listener(hover_key))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .cursor_pointer()
        .text_size(px(12.5))
        .text_color(theme.text)
        .tab_index(0)
}

/// The trigger's current value, truncating before the chevron.
pub fn select_value(label: impl Into<SharedString>) -> Div {
    div().flex_1().min_w_0().truncate().child(label.into())
}

pub fn select_chevron(theme: &Theme, open: bool) -> gpui::Svg {
    icon(icons::ALT_ARROW_DOWN).size(px(14.0)).flex_none().text_color(if open { theme.text } else { theme.text_muted })
}

/// The open menu: the shared popover card.
pub fn select_menu(theme: &Theme) -> Div {
    popover::popover_card(theme).flex().flex_col()
}

/// The trailing check slot, kept on every row so labels align.
pub fn select_check(theme: &Theme, selected: bool) -> Div {
    div()
        .w(px(18.0))
        .flex_none()
        .flex()
        .justify_end()
        .when(selected, |slot| slot.child(icon(icons::CHECK).size(px(14.0)).text_color(theme.accent)))
}

/// Where dropdowns may open: the window below the titlebar strip, inset 8px.
fn dropdown_limits(viewport: gpui::Size<Pixels>) -> Bounds<Pixels> {
    let top = px(Theme::TITLEBAR_HEIGHT).min(viewport.height);
    Bounds::new(
        gpui::point(px(8.0), top),
        gpui::size((viewport.width - px(16.0)).max(px(1.0)), (viewport.height - top - px(8.0)).max(px(1.0))),
    )
}

#[derive(IntoElement)]
struct Dropdown {
    id: SharedString,
    content: Div,
    closing: Option<std::time::Instant>,
    trigger_height: f32,
}

impl RenderOnce for Dropdown {
    fn render(self, window: &mut Window, _: &mut gpui::App) -> impl IntoElement {
        let limits = dropdown_limits(window.viewport_size());
        popover::contained_menu(self.id, self.content, self.closing, self.trigger_height, limits)
    }
}

/// Scroll only a dropdown's options, keeping its frame fixed.
fn dropdown_rows(id: SharedString, rows: Vec<AnyElement>, max_height: f32) -> Stateful<Div> {
    div().id(id).max_h(px(max_height)).overflow_y_scroll().flex().flex_col().gap(px(2.0)).children(rows)
}

fn dropdown_list_height(viewport: gpui::Size<Pixels>, trigger_height: f32, chrome_height: f32) -> f32 {
    let limits = dropdown_limits(viewport);
    let card = ((f32::from(limits.size.height) - trigger_height) / 2.0 - 6.0).clamp(1.0, 320.0);
    (card - chrome_height).max(1.0)
}

/// Open state for one [`select`], owned by its view: the popup with its exit phase,
/// and the keyboard highlight.
#[derive(Default)]
pub struct SelectState {
    menu: popover::Popup<()>,
    highlighted: usize,
}

impl SelectState {
    fn open(&mut self, highlighted: usize) {
        self.highlighted = highlighted;
        self.menu.open(());
    }
}

/// One option in a [`select`] menu.
pub struct SelectOption {
    label: SharedString,
}

impl SelectOption {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self { label: label.into() }
    }
}

type SelectReach<V> = Rc<dyn Fn(&mut V) -> &mut SelectState>;
type SelectHandler<V> = Rc<dyn Fn(&mut V, usize, &mut Window, &mut Context<V>)>;

/// The dropdown: a [`select_trigger`] opening a [`select_menu`] of options with a check
/// on the current one. Pointer: the trigger toggles and an outside press dismisses.
/// Keyboard: ↑/↓ open and move the highlight, Home/End jump, Enter commits, Escape
/// closes. `reach` re-borrows the view's [`SelectState`] inside listeners.
pub fn select<V: 'static>(
    id: impl Into<SharedString>,
    theme: &Theme,
    reach: impl Fn(&mut V) -> &mut SelectState + 'static,
) -> Select<V> {
    Select {
        id: id.into(),
        theme: theme.clone(),
        options: Vec::new(),
        selected: 0,
        width: None,
        reach: Rc::new(reach),
        on_select: None,
    }
}

pub struct Select<V: 'static> {
    id: SharedString,
    theme: Theme,
    options: Vec<SelectOption>,
    selected: usize,
    width: Option<f32>,
    reach: SelectReach<V>,
    on_select: Option<SelectHandler<V>>,
}

fn close_select<V: 'static>(view: &mut V, reach: &SelectReach<V>, cx: &mut Context<V>) {
    if reach(view).menu.begin_close() {
        let reach = reach.clone();
        popover::reap_popup(cx, move |view: &mut V| &mut reach(view).menu);
    }
    cx.notify();
}

fn commit_select<V: 'static>(
    view: &mut V,
    ix: usize,
    reach: &SelectReach<V>,
    on_select: &Option<SelectHandler<V>>,
    window: &mut Window,
    cx: &mut Context<V>,
) {
    close_select(view, reach, cx);
    if let Some(on_select) = on_select {
        on_select(view, ix, window, cx);
    }
}

impl<V: 'static> Select<V> {
    pub fn options(mut self, options: impl IntoIterator<Item = SelectOption>, selected: usize) -> Self {
        self.options = options.into_iter().collect();
        self.selected = selected;
        self
    }

    pub fn on_select(mut self, on_select: impl Fn(&mut V, usize, &mut Window, &mut Context<V>) + 'static) -> Self {
        self.on_select = Some(Rc::new(on_select));
        self
    }

    /// Fixed trigger width, which the menu matches. Without one the trigger fits its value.
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    pub fn render(self, state: &SelectState, window: &Window, cx: &mut Context<V>) -> Stateful<Div> {
        let Self { id, theme, options, selected, width, reach, on_select } = self;
        let count = options.len();
        let open = state.menu.is_open();
        let value = options.get(selected).map(|option| option.label.clone()).unwrap_or_default();

        let mut trigger = select_trigger(&theme, id.clone(), open)
            .when_some(width, |el, width| el.w(px(width)))
            .on_mouse_down(gpui::MouseButton::Left, {
                let reach = reach.clone();
                cx.listener(move |view, _, _, _| reach(view).menu.note_trigger_press())
            })
            .on_click({
                let reach = reach.clone();
                cx.listener(move |view, _: &gpui::ClickEvent, _, cx| {
                    let state = reach(view);
                    if state.menu.take_press_was_open() || state.menu.is_open() {
                        close_select(view, &reach, cx);
                    } else {
                        state.open(selected);
                        cx.notify();
                    }
                })
            })
            .on_key_down({
                let reach = reach.clone();
                let on_select = on_select.clone();
                cx.listener(move |view, event: &gpui::KeyDownEvent, window, cx| {
                    let key = event.keystroke.key.as_str();
                    let menu_key = popover::classify_key(key, event.keystroke.modifiers.control);
                    let state = reach(view);
                    if !state.menu.is_open() {
                        if matches!(menu_key, popover::MenuKey::Up | popover::MenuKey::Down) {
                            state.open(selected);
                            cx.stop_propagation();
                            cx.notify();
                        }
                        return;
                    }
                    let current = Some(state.highlighted.min(count.saturating_sub(1)));
                    state.highlighted = match (key, menu_key) {
                        (_, popover::MenuKey::Up) => popover::menu_step(current, count, -1).unwrap_or_default(),
                        (_, popover::MenuKey::Down) => popover::menu_step(current, count, 1).unwrap_or_default(),
                        ("home", _) => 0,
                        ("end", _) => count.saturating_sub(1),
                        (_, popover::MenuKey::Enter) => {
                            let ix = state.highlighted;
                            cx.stop_propagation();
                            commit_select(view, ix, &reach, &on_select, window, cx);
                            return;
                        }
                        (_, popover::MenuKey::Escape) => {
                            cx.stop_propagation();
                            close_select(view, &reach, cx);
                            return;
                        }
                        _ => return,
                    };
                    cx.stop_propagation();
                    cx.notify();
                })
            })
            .child(select_value(value))
            .child(select_chevron(&theme, open));

        if state.menu.get().is_some() {
            let highlighted = state.highlighted;
            let rows = options
                .into_iter()
                .enumerate()
                .map(|(ix, option)| {
                    let row_id: SharedString = format!("{id}-option-{ix}").into();
                    let reach = reach.clone();
                    let on_select = on_select.clone();
                    popover::menu_row_nav(&theme, ix == selected, ix == highlighted, row_id.clone())
                        .id(row_id)
                        .on_click(cx.listener(move |view, _, window, cx| {
                            cx.stop_propagation();
                            commit_select(view, ix, &reach, &on_select, window, cx);
                        }))
                        .child(div().flex_1().min_w_0().truncate().child(option.label))
                        .child(select_check(&theme, ix == selected))
                        .into_any_element()
                })
                .collect();
            let menu = select_menu(&theme)
                .w(px(width.unwrap_or(0.0).max(SELECT_MENU_MIN_WIDTH)))
                .on_mouse_down_out({
                    let reach = reach.clone();
                    cx.listener(move |view, _, _, cx| close_select(view, &reach, cx))
                })
                .child(dropdown_rows(
                    format!("{id}-list").into(),
                    rows,
                    dropdown_list_height(window.viewport_size(), SELECT_HEIGHT, 8.0),
                ));
            trigger = trigger.child(Dropdown {
                id: format!("{id}-menu").into(),
                content: menu,
                closing: state.menu.closing_since(),
                trigger_height: SELECT_HEIGHT,
            });
        }
        trigger
    }
}

/// Actions share one size, corner radius and hover language: quiet for secondary
/// actions, outlined for selectors, filled for row actions on a block, solid for
/// commits.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ActionTone {
    Quiet,
    Filled,
    Solid,
}

pub fn action_button(theme: &Theme, tone: ActionTone) -> Div {
    let button = div()
        .flex()
        .flex_row()
        .flex_none()
        .items_center()
        .justify_center()
        .gap(px(6.0))
        .rounded(px(8.0))
        .min_h(px(32.0))
        .px(px(10.0))
        .py(px(5.0))
        .text_size(px(12.5))
        .cursor_pointer();
    match tone {
        ActionTone::Quiet => {
            button.text_color(theme.text_muted).hover(|s| s.bg(theme.glass_hover()).text_color(theme.text))
        }
        ActionTone::Filled => {
            let lifted = select_fill(theme, true);
            button.bg(select_fill(theme, false)).text_color(theme.text).hover(move |s| s.bg(lifted))
        }
        ActionTone::Solid => button
            .bg(theme.solid)
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme.on_solid)
            .hover(|s| s.opacity(0.9)),
    }
}

pub fn text_action(theme: &Theme, tone: ActionTone, label: impl Into<SharedString>) -> Div {
    action_button(theme, tone).child(label.into())
}

/// The red error strip with a leading danger triangle.
pub fn error_strip(theme: &Theme, message: impl Into<SharedString>) -> Div {
    strip(theme.danger, theme.danger_muted, 12.5, 16.0, message.into()).mt(px(16.0)).py(px(12.0))
}

fn strip(tone: Hsla, text: Hsla, text_size: f32, glyph: f32, message: SharedString) -> Div {
    div()
        .px(px(16.0))
        .rounded(px(12.0))
        .border_1()
        .border_color(tone.opacity(0.2))
        .bg(tone.opacity(0.06))
        .text_size(px(text_size))
        .text_color(text.opacity(0.9))
        .flex()
        .flex_row()
        .items_start()
        .gap(px(8.0))
        .child(
            div().flex_none().mt(px(2.0)).child(icon(icons::DANGER_TRIANGLE).size(px(glyph)).text_color(text.opacity(0.9))),
        )
        .child(div().min_w_0().child(message))
}

/// A one-line hover note for controls (icon-only actions), in the popover chip.
pub struct TextTooltip {
    text: SharedString,
}

impl Render for TextTooltip {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        div()
            .max_w(px(320.0))
            .px(px(9.0))
            .py(px(6.0))
            .rounded(px(6.0))
            .border_1()
            .border_color(theme.border)
            .bg(popover::surface_bg(theme))
            .font_family(theme.font_sans.clone())
            .text_size(px(11.0))
            .text_color(theme.text_muted)
            .child(self.text.clone())
    }
}

/// `.tooltip(...)` builder for a [`TextTooltip`].
pub fn text_tooltip(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut gpui::App) -> gpui::AnyView + 'static {
    let text: SharedString = text.into();
    move |_, cx| cx.new(|_| TextTooltip { text: text.clone() }).into()
}

/// What a [`slider`] reports: a value while the thumb moves, and the release.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SlideEvent {
    Change(f32),
    Release,
}

#[derive(Clone)]
struct SliderDrag(ElementId);

impl Render for SliderDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

const SLIDER_HEIGHT: f32 = 28.0;
const SLIDER_THUMB: f32 = 14.0;

/// A track with an accent fill and a round thumb at `fraction` (0..1). The view keeps
/// the value; `on_slide` gets the fraction under the pointer while it is pressed or
/// dragged, then [`SlideEvent::Release`]. Size the width at the call site.
pub fn slider<V: 'static>(
    id: impl Into<ElementId>,
    theme: &Theme,
    fraction: f32,
    cx: &mut Context<V>,
    on_slide: impl Fn(&mut V, SlideEvent, &mut Window, &mut Context<V>) + 'static,
) -> Stateful<Div> {
    let id: ElementId = id.into();
    let fraction = fraction.clamp(0.0, 1.0);
    let bounds: Rc<Cell<Option<Bounds<Pixels>>>> = Rc::default();
    let on_slide = Rc::new(on_slide);
    let at = {
        let bounds = bounds.clone();
        move |x: Pixels| {
            let bounds = bounds.get()?;
            let travel = (f32::from(bounds.size.width) - SLIDER_THUMB).max(1.0);
            Some(((f32::from(x - bounds.left()) - SLIDER_THUMB / 2.0) / travel).clamp(0.0, 1.0))
        }
    };
    let release = {
        let on_slide = on_slide.clone();
        cx.listener(move |view, _: &gpui::MouseUpEvent, window, cx| on_slide(view, SlideEvent::Release, window, cx))
    };
    let release_out = {
        let on_slide = on_slide.clone();
        cx.listener(move |view, _: &gpui::MouseUpEvent, window, cx| on_slide(view, SlideEvent::Release, window, cx))
    };
    div()
        .id(id.clone())
        .relative()
        .h(px(SLIDER_HEIGHT))
        .min_w(px(40.0))
        .cursor_pointer()
        .on_mouse_down(gpui::MouseButton::Left, {
            let on_slide = on_slide.clone();
            let at = at.clone();
            cx.listener(move |view, event: &gpui::MouseDownEvent, window, cx| {
                cx.stop_propagation();
                if let Some(value) = at(event.position.x) {
                    on_slide(view, SlideEvent::Change(value), window, cx);
                }
            })
        })
        .on_drag(SliderDrag(id.clone()), |drag, _, _, cx| {
            cx.stop_propagation();
            cx.new(|_| drag.clone())
        })
        .on_drag_move(cx.listener(move |view, event: &gpui::DragMoveEvent<SliderDrag>, window, cx| {
            if event.drag(cx).0 != id {
                return;
            }
            if let Some(value) = at(event.event.position.x) {
                on_slide(view, SlideEvent::Change(value), window, cx);
            }
        }))
        .on_mouse_up(gpui::MouseButton::Left, release)
        .on_mouse_up_out(gpui::MouseButton::Left, release_out)
        .child(gpui::canvas(move |rect, _, _| bounds.set(Some(rect)), |_, _, _, _| {}).absolute().size_full())
        .child(
            div()
                .absolute()
                .left(px(SLIDER_THUMB / 2.0))
                .right(px(SLIDER_THUMB / 2.0))
                .top(px((SLIDER_HEIGHT - 4.0) / 2.0))
                .h(px(4.0))
                .rounded_full()
                .bg(theme.border)
                .child(div().h_full().w(gpui::relative(fraction)).rounded_full().bg(theme.accent))
                .child(
                    div()
                        .absolute()
                        .left(gpui::relative(fraction))
                        .ml(px(-SLIDER_THUMB / 2.0))
                        .top(px(-(SLIDER_THUMB - 4.0) / 2.0))
                        .size(px(SLIDER_THUMB))
                        .rounded_full()
                        .bg(theme.accent),
                ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switch_fills_are_opaque_and_the_on_track_deepens_the_accent() {
        let theme = Theme::dark();
        let on = switch_track_color(&theme, true);
        assert_eq!(on.a, 1.0);
        assert!(on.l < theme.accent_strong.l);
        assert_eq!(switch_track_color(&theme, false).a, 1.0);
        assert_eq!(switch_thumb_color(&theme).a, 1.0);
    }

    #[test]
    fn switch_reversal_keeps_current_position_and_settles() {
        use std::time::{Duration, Instant};
        let now = Instant::now();
        let forward = SwitchTravel { from: 0.0, target: 1.0, started: now };
        let halfway = now + Duration::from_millis(90);
        let current = forward.value(halfway);
        let reverse = SwitchTravel { from: current, target: 0.0, started: halfway };
        assert_eq!(reverse.value(halfway), current);
        assert_eq!(reverse.value(halfway + Duration::from_millis(180)), 0.0);
        assert_eq!(forward.value(now + Duration::from_millis(180)), 1.0);
    }

    #[test]
    fn dropdowns_stay_below_the_titlebar() {
        let viewport = gpui::size(px(600.0), px(500.0));
        let limits = dropdown_limits(viewport);
        assert!(limits.top() >= px(Theme::TITLEBAR_HEIGHT));
        assert!(limits.right() <= viewport.width && limits.bottom() <= viewport.height);
    }
}
