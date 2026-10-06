//! The video editor's data (VideoEditModel.swift and the lane rules of VideoEditStore.swift).
//!
//! Every time here is in seconds of the SOURCE clip, before cuts and speed changes:
//! the timeline draws on that axis, so an effect stays where it was dropped when
//! other stretches are cut. `TimeMap` converts between it and the edited clip's clock.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::annotate::Rgba;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Cut,
    Speed,
    Freeze,
    Zoom,
    Censor,
    Text,
}

impl Kind {
    pub const ALL: [Kind; 6] = [Kind::Cut, Kind::Speed, Kind::Freeze, Kind::Zoom, Kind::Censor, Kind::Text];
    pub const LANES: usize = 5;

    pub fn label(self) -> &'static str {
        match self {
            Kind::Cut => "Cut",
            Kind::Speed => "Speed",
            Kind::Freeze => "Freeze",
            Kind::Zoom => "Zoom",
            Kind::Censor => "Censor",
            Kind::Text => "Text",
        }
    }

    /// The effect draws a region on the frame.
    pub fn has_region(self) -> bool {
        matches!(self, Kind::Zoom | Kind::Censor | Kind::Text)
    }

    /// Speed and Freeze share a lane: speeding up a held frame means nothing.
    pub fn lane(self) -> usize {
        match self {
            Kind::Cut => 0,
            Kind::Speed | Kind::Freeze => 1,
            Kind::Zoom => 2,
            Kind::Censor => 3,
            Kind::Text => 4,
        }
    }

    pub fn min_duration(self) -> f64 {
        if self == Kind::Cut { 0.1 } else { 0.2 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CensorStyle {
    Blur,
    Pixelate,
}

impl CensorStyle {
    pub const ALL: [CensorStyle; 2] = [CensorStyle::Blur, CensorStyle::Pixelate];

    pub fn label(self) -> &'static str {
        match self {
            CensorStyle::Blur => "Blur",
            CensorStyle::Pixelate => "Pixelate",
        }
    }
}

/// Normalized to the frame, 0…1 from the top-left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl NRect {
    pub fn mid(self) -> (f64, f64) {
        (self.x + self.w / 2., self.y + self.h / 2.)
    }
}

pub const SPEEDS: [f64; 7] = [0.25, 0.5, 0.75, 2., 3., 5., 10.];

pub fn speed_label(f: f64) -> String {
    if f == f.round() { format!("{}×", f as i64) } else { format!("{f}×") }
}

/// One struct for all six kinds, so the timeline, undo and inspector handle one list.
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub id: u64,
    pub kind: Kind,
    /// For a freeze, `start` is the held frame and `end - start` how long it's held.
    pub start: f64,
    pub end: f64,
    pub zoom: f64,
    /// Normalized, top-left origin.
    pub center: (f64, f64),
    /// Seconds to ease in and out.
    pub fade: f64,
    pub speed: f64,
    pub censor_style: CensorStyle,
    /// 0…1, mapped to the blur radius or pixel size.
    pub strength: f64,
    pub rect: NRect,
    pub text: String,
    /// Of the frame height, so a full-resolution export keeps the proportions.
    pub font_scale: f64,
    pub text_color: Rgba,
    pub shadow: bool,
}

impl Segment {
    pub fn new(kind: Kind, start: f64, end: f64) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let rect = if kind == Kind::Text {
            NRect { x: 0.15, y: 0.72, w: 0.7, h: 0.14 }
        } else {
            NRect { x: 0.3, y: 0.3, w: 0.4, h: 0.2 }
        };
        Self {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            kind,
            start,
            end,
            zoom: 2.,
            center: (0.5, 0.5),
            fade: 0.35,
            speed: 2.,
            censor_style: CensorStyle::Blur,
            strength: 0.5,
            rect,
            text: "Text".into(),
            font_scale: 0.08,
            text_color: Rgba([255, 255, 255, 255]),
            shadow: true,
        }
    }

    pub fn duration(&self) -> f64 {
        (self.end - self.start).max(0.)
    }

    /// The short label on the timeline pill.
    pub fn badge(&self) -> String {
        match self.kind {
            Kind::Cut => "Cut".into(),
            Kind::Speed => speed_label(self.speed),
            Kind::Freeze => format!("{:.1}s", self.duration()),
            Kind::Zoom => format!("{:.1}×", self.zoom),
            Kind::Censor => self.censor_style.label().into(),
            Kind::Text if self.text.is_empty() => "Text".into(),
            Kind::Text => self.text.clone(),
        }
    }

    /// The region shown on the frame: a zoom is stored as center and level, drawn as
    /// the square it magnifies.
    pub fn region(&self) -> Option<NRect> {
        match self.kind {
            Kind::Zoom => {
                let side = 1. / self.zoom.max(1.001);
                let half = side / 2.;
                let cx = self.center.0.clamp(half, 1. - half);
                let cy = self.center.1.clamp(half, 1. - half);
                Some(NRect { x: cx - half, y: cy - half, w: side, h: side })
            }
            Kind::Censor | Kind::Text => Some(self.rect),
            _ => None,
        }
    }

    /// Stores a region drawn on the frame; a zoom turns it into center and level.
    pub fn set_region(&mut self, r: NRect) {
        let c = NRect { x: r.x.clamp(0., 1.), y: r.y.clamp(0., 1.), w: r.w.clamp(0.03, 1.), h: r.h.clamp(0.03, 1.) };
        match self.kind {
            Kind::Zoom => {
                self.center = c.mid();
                // The larger side, so all of the drawn box fits in the frame.
                self.zoom = (1. / c.w.max(c.h).max(0.0001)).clamp(1.2, 5.);
            }
            Kind::Censor | Kind::Text => self.rect = c,
            _ => {}
        }
    }
}

/// The source ranges left after trimming both ends and removing the cuts; sorted,
/// not overlapping.
pub fn kept_ranges(trim_start: f64, trim_end: f64, segments: &[Segment]) -> Vec<(f64, f64)> {
    if trim_end <= trim_start {
        return vec![];
    }
    let mut clipped: Vec<(f64, f64)> = segments
        .iter()
        .filter(|s| s.kind == Kind::Cut && s.end > s.start)
        .map(|s| (s.start.max(trim_start), s.end.min(trim_end)))
        .filter(|(a, b)| a < b)
        .collect();
    clipped.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut merged: Vec<(f64, f64)> = vec![];
    for c in clipped {
        match merged.last_mut() {
            Some(last) if c.0 <= last.1 + 0.001 => last.1 = last.1.max(c.1),
            _ => merged.push(c),
        }
    }
    let mut kept = vec![];
    let mut cursor = trim_start;
    for (cs, ce) in merged {
        if cs > cursor + 0.001 {
            kept.push((cursor, cs));
        }
        cursor = cursor.max(ce);
    }
    if cursor < trim_end - 0.001 {
        kept.push((cursor, trim_end));
    }
    kept
}

/// A continuous stretch of the edited clip:
/// `source = src_start + (comp - comp_start) × factor`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    pub comp_start: f64,
    pub comp_duration: f64,
    pub src_start: f64,
    pub src_duration: f64,
}

impl Piece {
    pub fn comp_end(&self) -> f64 {
        self.comp_start + self.comp_duration
    }

    pub fn src_end(&self) -> f64 {
        self.src_start + self.src_duration
    }

    /// 1 plays normally, the speed for a speed ramp, about 0 for a freeze.
    pub fn factor(&self) -> f64 {
        if self.comp_duration > 0. { self.src_duration / self.comp_duration } else { 1. }
    }

    pub fn is_freeze(&self) -> bool {
        self.src_duration <= FREEZE_SLICE + 1e-9 && self.comp_duration > self.src_duration * 2.
    }
}

/// The sliver of source a freeze holds: one frame, but not zero long.
pub const FREEZE_SLICE: f64 = 1. / 600.;

/// Edited clip ⇄ source clip.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TimeMap {
    pub pieces: Vec<Piece>,
}

impl TimeMap {
    pub fn build(kept: &[(f64, f64)], segments: &[Segment]) -> Self {
        let mut speeds: Vec<&Segment> = segments.iter().filter(|s| s.kind == Kind::Speed && s.end > s.start && s.speed > 0.).collect();
        speeds.sort_by(|a, b| a.start.total_cmp(&b.start));
        // A later overlapping ramp loses its overlap; the UI prevents it anyway.
        let mut clean: Vec<(f64, f64, f64)> = vec![];
        for s in speeds {
            let from = s.start.max(clean.last().map_or(f64::NEG_INFINITY, |c| c.1));
            if s.end > from {
                clean.push((from, s.end, s.speed));
            }
        }
        let mut stops: Vec<&Segment> = segments.iter().filter(|s| s.kind == Kind::Freeze && s.duration() > 0.).collect();
        stops.sort_by(|a, b| a.start.total_cmp(&b.start));

        let mut out = vec![];
        let mut comp = 0.;
        let mut emit = |src_start: f64, src_duration: f64, comp_duration: f64| {
            if comp_duration > 0.0005 {
                out.push(Piece { comp_start: comp, comp_duration, src_start, src_duration });
                comp += comp_duration;
            }
        };

        for &(range_start, range_end) in kept {
            if range_end <= range_start {
                continue;
            }
            let mut cursor = range_start;
            let emit_up_to = |limit: f64, cursor: &mut f64, emit: &mut dyn FnMut(f64, f64, f64)| {
                if limit <= *cursor {
                    return;
                }
                for &(ss, se, factor) in &clean {
                    if se <= *cursor {
                        continue;
                    }
                    if ss >= limit {
                        break;
                    }
                    let (a, b) = (ss.max(*cursor), se.min(limit));
                    if a > *cursor {
                        emit(*cursor, a - *cursor, a - *cursor);
                    }
                    if b > a {
                        emit(a, b - a, (b - a) / factor);
                    }
                    *cursor = cursor.max(b);
                }
                if *cursor < limit {
                    emit(*cursor, limit - *cursor, limit - *cursor);
                    *cursor = limit;
                }
            };
            for stop in stops.iter().filter(|s| s.start > range_start && s.start < range_end) {
                emit_up_to(stop.start, &mut cursor, &mut emit);
                let slice_start = stop.start.max(cursor);
                let slice = FREEZE_SLICE.min((range_end - slice_start).max(0.));
                if slice > 0. {
                    emit(slice_start, slice, stop.duration());
                    cursor = slice_start + slice;
                }
            }
            emit_up_to(range_end, &mut cursor, &mut emit);
        }
        Self { pieces: out }
    }

    pub fn duration(&self) -> f64 {
        self.pieces.last().map_or(0., |p| p.comp_end())
    }

    /// The piece playing at edited time `t`, clamped to the ends.
    pub fn piece_at(&self, t: f64) -> Option<usize> {
        if self.pieces.is_empty() {
            return None;
        }
        Some(self.pieces.iter().position(|p| t < p.comp_end()).unwrap_or(self.pieces.len() - 1))
    }

    pub fn source_time(&self, t: f64) -> f64 {
        let Some(first) = self.pieces.first() else { return 0. };
        if t <= first.comp_start {
            return first.src_start;
        }
        for p in &self.pieces {
            if t < p.comp_end() {
                if t < p.comp_start {
                    break;
                }
                return p.src_start + (t - p.comp_start) * p.factor();
            }
        }
        self.pieces.last().unwrap().src_end()
    }

    /// A time inside a cut maps to the start of the next kept stretch.
    pub fn comp_time(&self, t: f64) -> f64 {
        let Some(first) = self.pieces.first() else { return 0. };
        if t <= first.src_start {
            return first.comp_start;
        }
        for p in &self.pieces {
            if t < p.src_start {
                return p.comp_start;
            }
            if t <= p.src_end() {
                let f = p.factor();
                return if f > 0. { p.comp_start + (t - p.src_start) / f } else { p.comp_start };
            }
        }
        self.duration()
    }
}

/// The zoom level at source time `t`, eased in and out; 1 outside the segment.
pub fn zoom_level(seg: &Segment, t: f64) -> f64 {
    let dur = seg.end - seg.start;
    if t < seg.start || t > seg.end || dur <= 0. {
        return 1.;
    }
    let f = seg.fade.min((dur / 2. - 0.001).max(0.));
    if f <= 0. {
        return seg.zoom;
    }
    let ease = |x: f64| {
        let c = x.clamp(0., 1.);
        c * c * (3. - 2. * c)
    };
    let (into, to_end) = (t - seg.start, seg.end - t);
    if into < f {
        1. + (seg.zoom - 1.) * ease(into / f)
    } else if to_end < f {
        1. + (seg.zoom - 1.) * ease(to_end / f)
    } else {
        seg.zoom
    }
}

/// The trim and the effects: what the editor undoes and the renderer reads.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Edit {
    pub duration: f64,
    pub trim_start: f64,
    pub trim_end: f64,
    pub segments: Vec<Segment>,
}

impl Edit {
    pub fn new(duration: f64) -> Self {
        Self { duration, trim_start: 0., trim_end: duration, segments: vec![] }
    }

    pub fn time_map(&self) -> TimeMap {
        TimeMap::build(&kept_ranges(self.trim_start, self.trim_end, &self.segments), &self.segments)
    }

    pub fn get(&self, id: u64) -> Option<&Segment> {
        self.segments.iter().find(|s| s.id == id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Segment> {
        self.segments.iter_mut().find(|s| s.id == id)
    }

    // Two segments on one lane never overlap: every way to create, move or resize
    // goes through the functions below.

    fn lane_mates(&self, lane: usize, excluding: Option<u64>) -> Vec<&Segment> {
        let mut mates: Vec<&Segment> = self.segments.iter().filter(|s| s.kind.lane() == lane && Some(s.id) != excluding).collect();
        mates.sort_by(|a, b| a.start.total_cmp(&b.start));
        mates
    }

    fn gaps(&self, lane: usize, excluding: Option<u64>) -> Vec<(f64, f64)> {
        let mut out = vec![];
        let mut cursor = 0.;
        for m in self.lane_mates(lane, excluding) {
            if m.start > cursor {
                out.push((cursor, m.start));
            }
            cursor = f64::max(cursor, m.end);
        }
        if cursor < self.duration {
            out.push((cursor, self.duration));
        }
        out
    }

    /// The gap nearest `t` that holds `min_len`. Nearest, not widest: a drop on a
    /// full stretch lands right beside it rather than back at the start.
    fn nearest_gap(&self, lane: usize, t: f64, min_len: f64, excluding: Option<u64>) -> Option<(f64, f64)> {
        let distance = |g: &(f64, f64)| if t < g.0 { g.0 - t } else if t > g.1 { t - g.1 } else { 0. };
        self.gaps(lane, excluding)
            .into_iter()
            .filter(|g| g.1 - g.0 >= min_len)
            .min_by(|a, b| distance(a).total_cmp(&distance(b)))
    }

    /// Where a new segment fits, or None when its lane is full.
    pub fn fit(&self, kind: Kind, start: f64, end: f64) -> Option<(f64, f64)> {
        let (a, b) = (start.min(end), start.max(end));
        let min_len = kind.min_duration();
        let want = (b - a).max(min_len);
        // A gap that holds the whole length first; otherwise a drop inside an
        // existing segment squeezes out a 0.2 s sliver nobody meant.
        let g = self.nearest_gap(kind.lane(), a, want, None).or_else(|| self.nearest_gap(kind.lane(), a, min_len, None))?;
        let s = if a >= g.0 && a <= g.1 - min_len { a } else { a.max(g.0).min(g.1 - want.min(g.1 - g.0)) };
        Some((s, (s + want).min(g.1)))
    }

    /// The valid start nearest `desired` when moving a segment. The gap is chosen by
    /// the segment's middle, so a strong drag jumps past a neighbour.
    pub fn clamped_start(&self, id: u64, desired: f64) -> f64 {
        let Some(seg) = self.get(id) else { return desired };
        let len = seg.duration();
        let d = desired.clamp(0., (self.duration - len).max(0.));
        match self.nearest_gap(seg.kind.lane(), d + len / 2., len, Some(id)) {
            Some(g) => d.max(g.0).min(g.1 - len),
            None => seg.start,
        }
    }

    /// How far a segment's edges may go when resizing: up to its neighbours.
    pub fn resize_bounds(&self, id: u64) -> (f64, f64) {
        let Some(seg) = self.get(id) else { return (0., self.duration) };
        let mates = self.lane_mates(seg.kind.lane(), Some(id));
        let lo = mates.iter().filter(|m| m.end <= seg.start + 0.001).map(|m| m.end).fold(0., f64::max);
        let hi = mates.iter().filter(|m| m.start >= seg.end - 0.001).map(|m| m.start).fold(self.duration, f64::min);
        (lo, hi)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(kind: Kind, start: f64, end: f64) -> Segment {
        Segment::new(kind, start, end)
    }

    #[test]
    fn cuts_merge_and_clip_to_the_trim() {
        let cuts = [seg(Kind::Cut, 2., 3.), seg(Kind::Cut, 2.5, 4.), seg(Kind::Cut, 9., 12.), seg(Kind::Zoom, 0., 10.)];
        assert_eq!(kept_ranges(1., 10., &cuts), vec![(1., 2.), (4., 9.)]);
        assert_eq!(kept_ranges(5., 5., &cuts), vec![]);
    }

    #[test]
    fn speed_and_freeze_map_both_ways() {
        let mut freeze = seg(Kind::Freeze, 6., 8.);
        freeze.end = freeze.start + 2.;
        let mut fast = seg(Kind::Speed, 2., 4.);
        fast.speed = 2.;
        let map = TimeMap::build(&[(0., 10.)], &[fast, freeze]);
        // 0-2 normal, 2-4 in 1 s, 4-6 normal, 2 s held, the rest.
        assert!((map.duration() - (2. + 1. + 2. + 2. + 4.)).abs() < 0.01);
        assert!((map.source_time(2.5) - 3.).abs() < 1e-9);
        assert!((map.comp_time(3.) - 2.5).abs() < 1e-9);
        let held = map.source_time(6.);
        assert!((held - 6.).abs() < 0.01, "{held}");
        assert!(map.pieces.iter().any(Piece::is_freeze));
        assert!((map.source_time(map.duration()) - 10.).abs() < 0.01);
    }

    #[test]
    fn a_time_inside_a_cut_maps_to_the_next_kept_stretch() {
        let map = TimeMap::build(&[(0., 2.), (4., 6.)], &[]);
        assert_eq!(map.comp_time(3.), 2.);
        assert_eq!(map.source_time(2.), 4.);
    }

    #[test]
    fn zoom_eases_in_and_out() {
        let mut z = seg(Kind::Zoom, 1., 3.);
        z.zoom = 3.;
        z.fade = 0.5;
        assert_eq!(zoom_level(&z, 0.5), 1.);
        assert_eq!(zoom_level(&z, 2.), 3.);
        let half = zoom_level(&z, 1.25);
        assert!(half > 1.5 && half < 2.5, "{half}");
    }

    #[test]
    fn segments_on_one_lane_never_overlap() {
        let mut edit = Edit::new(10.);
        edit.segments.push(seg(Kind::Speed, 2., 4.));
        // A freeze shares the speed lane: dropped inside the ramp it moves beside it.
        let (s, e) = edit.fit(Kind::Freeze, 3., 4.).unwrap();
        assert!(s >= 4. || e <= 2., "{s}..{e}");
        // Other lanes don't care.
        assert_eq!(edit.fit(Kind::Zoom, 3., 5.), Some((3., 5.)));
        let mut cut = seg(Kind::Cut, 0., 10.);
        cut.id = 99;
        edit.segments.push(cut);
        assert_eq!(edit.fit(Kind::Cut, 3., 4.), None);
    }

    #[test]
    fn moving_and_resizing_stop_at_neighbours() {
        let mut edit = Edit::new(10.);
        let a = seg(Kind::Zoom, 1., 3.);
        let b = seg(Kind::Zoom, 5., 6.);
        let (a_id, b_id) = (a.id, b.id);
        edit.segments.extend([a, b]);
        assert_eq!(edit.resize_bounds(a_id), (0., 5.));
        assert_eq!(edit.resize_bounds(b_id), (3., 10.));
        // Dragged a little into its neighbour: pressed against it.
        assert_eq!(edit.clamped_start(b_id, 2.6), 3.);
        // Dragged past its middle: jumps to the other side.
        assert_eq!(edit.clamped_start(b_id, 0.2), 0.);
    }

    #[test]
    fn a_drawn_box_sets_the_zoom() {
        let mut z = seg(Kind::Zoom, 0., 1.);
        z.set_region(NRect { x: 0.5, y: 0.5, w: 0.25, h: 0.2 });
        assert_eq!(z.zoom, 4.);
        assert_eq!(z.center, (0.625, 0.6));
        let r = z.region().unwrap();
        assert!((r.w - 0.25).abs() < 1e-9);
    }
}
