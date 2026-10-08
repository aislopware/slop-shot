//! Pixel snapping on the frozen screen, ported from SnapEngine.swift.
//!
//! Edges are found from the image itself: a fixed low threshold on the per-channel
//! difference between neighbouring pixels, then runs of edge pixels long enough to be
//! real lines. Selection edges snap to the best-covered line nearby, and hovering finds
//! the chain of nested boxes around the pointer whose four sides are all present.
//!
//! Public coordinates are logical points from the display's top-left; internally
//! everything is image pixels. A vertical boundary `b` lies between columns `b - 1`
//! and `b`, so a box from `L` to `R` covers columns `L..R`.
//!
//! macOS also snaps to window frames from CGWindowList. Wayland hides other windows'
//! geometry, so here everything comes from pixels.

use image::RgbaImage;

/// Low enough for faint borders such as a dark photo on a dark background; noise inside
/// photos is rejected later by the box checks, not here.
const THR: u8 = 16;

pub struct SnapEngine {
    w: usize,
    h: usize,
    /// Image pixels per point.
    scale: f32,
    v_edge: Vec<u8>,
    h_edge: Vec<u8>,
    v_start: Vec<u32>,
    v_a: Vec<i32>,
    v_b: Vec<i32>,
    h_start: Vec<u32>,
    h_a: Vec<i32>,
    h_b: Vec<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RectF {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

fn chan_diff(a: &[u8], b: &[u8]) -> u8 {
    let d = |i: usize| (a[i] as i32 - b[i] as i32).unsigned_abs();
    d(0).max(d(1)).max(d(2)).min(255) as u8
}

impl SnapEngine {
    /// Safe to call off the main thread; takes some tens of milliseconds on 4K.
    pub fn build(image: &RgbaImage, scale: f32) -> Option<Self> {
        let (w, h) = (image.width() as usize, image.height() as usize);
        if w < 8 || h < 8 || scale <= 0. || w * h > 40_000_000 {
            return None;
        }
        let px = image.as_raw();
        let mut v_edge = vec![0u8; w * h];
        let mut h_edge = vec![0u8; w * h];
        for y in 0..h {
            let row = y * w * 4;
            let base = y * w;
            for x in 1..w {
                v_edge[base + x] = chan_diff(&px[row + (x - 1) * 4..], &px[row + x * 4..]);
            }
            if y == 0 {
                continue;
            }
            let prev = (y - 1) * w * 4;
            for x in 0..w {
                h_edge[base + x] = chan_diff(&px[prev + x * 4..], &px[row + x * 4..]);
            }
        }

        let t = THR;

        // Runs of edge pixels, bridging gaps of up to 2 px, at least ~20 pt long.
        let min_line = 12.max((20. * scale).round() as i32);
        let mut v_start = vec![0u32; w + 1];
        let (mut v_a, mut v_b) = (Vec::with_capacity(4096), Vec::with_capacity(4096));
        for x in 0..w {
            v_start[x] = v_a.len() as u32;
            if x == 0 {
                continue;
            }
            let (mut run, mut last) = (-1i32, -1i32);
            for y in 0..h as i32 {
                if v_edge[y as usize * w + x] >= t {
                    if run < 0 {
                        run = y;
                    }
                    last = y;
                } else if run >= 0 && y - last > 2 {
                    if last - run + 1 >= min_line {
                        v_a.push(run);
                        v_b.push(last);
                    }
                    run = -1;
                }
            }
            if run >= 0 && last - run + 1 >= min_line {
                v_a.push(run);
                v_b.push(last);
            }
        }
        v_start[w] = v_a.len() as u32;

        let mut h_start = vec![0u32; h + 1];
        let (mut h_a, mut h_b) = (Vec::with_capacity(4096), Vec::with_capacity(4096));
        for y in 0..h {
            h_start[y] = h_a.len() as u32;
            if y == 0 {
                continue;
            }
            let base = y * w;
            let (mut run, mut last) = (-1i32, -1i32);
            for x in 0..w as i32 {
                if h_edge[base + x as usize] >= t {
                    if run < 0 {
                        run = x;
                    }
                    last = x;
                } else if run >= 0 && x - last > 2 {
                    if last - run + 1 >= min_line {
                        h_a.push(run);
                        h_b.push(last);
                    }
                    run = -1;
                }
            }
            if run >= 0 && last - run + 1 >= min_line {
                h_a.push(run);
                h_b.push(last);
            }
        }
        h_start[h] = h_a.len() as u32;

        Some(Self { w, h, scale, v_edge, h_edge, v_start, v_a, v_b, h_start, h_a, h_b })
    }

    fn px(&self, v: f32) -> i32 {
        (v * self.scale).round() as i32
    }

    fn pt(&self, v: i32) -> f32 {
        v as f32 / self.scale
    }

    /// Columns with a vertical line through `row`, nearest first, scanning from
    /// `from` towards `stop`. With `span`, lines covering at least half of it count
    /// too, so an edge that fades into the background right at the pointer is still
    /// found from the rest of it.
    fn v_lines(&self, from: i32, stop: i32, row: i32, span: Option<(i32, i32)>, limit: usize) -> Vec<i32> {
        lines(&self.v_start, &self.v_a, &self.v_b, self.w as i32, from, stop, row, span, limit)
    }

    fn h_lines(&self, from: i32, stop: i32, col: i32, span: Option<(i32, i32)>, limit: usize) -> Vec<i32> {
        lines(&self.h_start, &self.h_a, &self.h_b, self.h as i32, from, stop, col, span, limit)
    }

    fn v_coverage(&self, x: i32, y0: i32, y1: i32) -> f32 {
        if x < 1 || x >= self.w as i32 || y1 < y0 {
            return 0.;
        }
        let hit = (y0..=y1).filter(|&y| self.v_edge[y as usize * self.w + x as usize] >= THR).count();
        hit as f32 / (y1 - y0 + 1) as f32
    }

    fn h_coverage(&self, y: i32, x0: i32, x1: i32) -> f32 {
        if y < 1 || y >= self.h as i32 || x1 < x0 {
            return 0.;
        }
        let base = y as usize * self.w;
        let hit = (x0..=x1).filter(|&x| self.h_edge[base + x as usize] >= THR).count();
        hit as f32 / (x1 - x0 + 1) as f32
    }

    /// The vertical edge worth snapping to near `x`, scored by how much of the span
    /// `y0..y1` it covers; strength beats nearness.
    pub fn snap_x(&self, x: f32, y0: f32, y1: f32, radius: f32) -> Option<f32> {
        let (w, h) = (self.w as i32, self.h as i32);
        let center = self.px(x).clamp(0, w);
        let r = self.px(radius).max(1);
        let mut a = self.px(y0.min(y1)).clamp(0, h - 1);
        let mut b = (self.px(y0.max(y1)) - 1).clamp(0, h - 1);
        if b < a {
            std::mem::swap(&mut a, &mut b);
        }
        if b - a < 4 {
            b = (h - 1).min(a + 4);
        }
        let mut best = None;
        let mut best_score = 0f32;
        for cand in (center - r).max(1)..=(center + r).min(w - 1) {
            let cov = self.v_coverage(cand, a, b);
            if cov < 0.55 {
                continue;
            }
            let score = cov - 0.25 * (cand - center).abs() as f32 / r as f32;
            if score > best_score {
                best_score = score;
                best = Some(cand);
            }
        }
        best.map(|v| self.pt(v))
    }

    pub fn snap_y(&self, y: f32, x0: f32, x1: f32, radius: f32) -> Option<f32> {
        let (w, h) = (self.w as i32, self.h as i32);
        let center = self.px(y).clamp(0, h);
        let r = self.px(radius).max(1);
        let mut a = self.px(x0.min(x1)).clamp(0, w - 1);
        let mut b = (self.px(x0.max(x1)) - 1).clamp(0, w - 1);
        if b < a {
            std::mem::swap(&mut a, &mut b);
        }
        if b - a < 4 {
            b = (w - 1).min(a + 4);
        }
        let mut best = None;
        let mut best_score = 0f32;
        for cand in (center - r).max(1)..=(center + r).min(h - 1) {
            let cov = self.h_coverage(cand, a, b);
            if cov < 0.55 {
                continue;
            }
            let score = cov - 0.25 * (cand - center).abs() as f32 / r as f32;
            if score > best_score {
                best_score = score;
                best = Some(cand);
            }
        }
        best.map(|v| self.pt(v))
    }

    /// The nested boxes around `point` inside `limit`, smallest first (button, card,
    /// column…). A box equal to `limit` itself is left out.
    pub fn elements(&self, point: (f32, f32), limit: RectF) -> Vec<RectF> {
        let (w, h) = (self.w as i32, self.h as i32);
        let x0 = self.px(limit.x).clamp(0, w - 1);
        let x1 = self.px(limit.x + limit.w).clamp(1, w);
        let y0 = self.px(limit.y).clamp(0, h - 1);
        let y1 = self.px(limit.y + limit.h).clamp(1, h);
        if x1 - x0 <= 16 || y1 - y0 <= 16 {
            return Vec::new();
        }
        let cx = self.px(point.0).clamp(x0, x1 - 1);
        let cy = self.px(point.1).clamp(y0, y1 - 1);
        let min_size = 12.max(self.px(16.));
        let limit_n = 20;

        // A second pass also takes lines covering half the gap between the nearest
        // lines on the other axis.
        let first = |v: &[i32], or: i32| v.first().copied().unwrap_or(or);
        let x_span = (first(&self.v_lines(cx, x0, cy, None, 1), x0), first(&self.v_lines(cx, x1 - 1, cy, None, 1), x1) - 1);
        let y_span = (first(&self.h_lines(cy, y0, cx, None, 1), y0), first(&self.h_lines(cy, y1 - 1, cx, None, 1), y1) - 1);
        let mut lefts = self.v_lines(cx, x0, cy, Some(y_span), limit_n);
        let mut rights = self.v_lines(cx, x1 - 1, cy, Some(y_span), limit_n);
        let mut tops = self.h_lines(cy, y0, cx, Some(x_span), limit_n);
        let mut bottoms = self.h_lines(cy, y1 - 1, cx, Some(x_span), limit_n);
        // The search area's edges are known boundaries, so they always count as sides,
        // even on the screen edge where there are no pixels to measure.
        for (v, e) in [(&mut lefts, x0), (&mut rights, x1), (&mut tops, y0), (&mut bottoms, y1)] {
            if !v.contains(&e) {
                v.push(e);
            }
        }

        // Prefix sums make any side's coverage O(1), which keeps hovering smooth.
        let v_cands: Vec<i32> = lefts.iter().chain(&rights).copied().collect();
        let h_cands: Vec<i32> = tops.iter().chain(&bottoms).copied().collect();
        let col_pre: Vec<Vec<i32>> = v_cands.iter().map(|&x| self.column_prefix(x, y0, y1 - 1)).collect();
        let row_pre: Vec<Vec<i32>> = h_cands.iter().map(|&y| self.row_prefix(y, x0, x1 - 1)).collect();

        // Rounded corners: the straight part of each side starts one radius away from
        // the corner, the same distance on both sides that meet there. A box that took
        // a line from outside the item is off at both corners of that side. One odd
        // corner is allowed for timestamps or buttons sitting in a corner of an image.
        let cap = self.px(24.);
        let large = self.px(160.);
        let gap_v: Vec<Vec<i32>> = v_cands
            .iter()
            .map(|&x| {
                h_cands
                    .iter()
                    .enumerate()
                    .map(|(hi, &y)| if hi < tops.len() { self.v_gap(x, y, 1, cap) } else { self.v_gap(x, y - 1, -1, cap) })
                    .collect()
            })
            .collect();
        let gap_h: Vec<Vec<i32>> = h_cands
            .iter()
            .map(|&y| {
                v_cands
                    .iter()
                    .enumerate()
                    .map(|(vi, &x)| if vi < lefts.len() { self.h_gap(y, x, 1, cap) } else { self.h_gap(y, x - 1, -1, cap) })
                    .collect()
            })
            .collect();
        let px2 = self.px(2.);
        let px3 = self.px(3.);
        // None: the sides don't meet; Some(false): they meet at different radii.
        let corner = |gv: i32, gh: i32, v_fixed: bool, h_fixed: bool| -> Option<bool> {
            // A line meeting a straight window edge reaches it.
            if v_fixed {
                return (gh <= px2).then_some(true);
            }
            if h_fixed {
                return (gv <= px2).then_some(true);
            }
            if gv > cap || gh > cap {
                return None;
            }
            Some((gv - gh).abs() <= px3.max(gv.max(gh) / 3))
        };
        // Coverage of the straight part only; a straight part under a quarter of the
        // side is no side.
        let cov = |pre: &[i32], lo: i32, hi: i32, base: i32, g0: i32, g1: i32| -> f32 {
            let (a, b) = (lo + g0, hi - g1);
            if (b - a + 1) * 4 < hi - lo + 1 {
                return 0.;
            }
            (pre[(b - base + 1) as usize] - pre[(a - base) as usize]) as f32 / (b - a + 1) as f32
        };

        let mut boxes: Vec<Box4> = Vec::new();
        for (ti, &t) in tops.iter().enumerate() {
            for (bi, &b) in bottoms.iter().enumerate() {
                if b - t < min_size {
                    continue;
                }
                let bj = tops.len() + bi;
                for (li, &l) in lefts.iter().enumerate() {
                    for (ri, &r) in rights.iter().enumerate() {
                        if r - l < min_size {
                            continue;
                        }
                        let rj = lefts.len() + ri;
                        let (fl, fr, ft, fb) = (l == x0, r == x1, t == y0, b == y1);
                        if fl && fr && ft && fb {
                            continue;
                        }
                        let corners = [
                            corner(gap_v[li][ti], gap_h[ti][li], fl, ft),
                            corner(gap_v[rj][ti], gap_h[ti][rj], fr, ft),
                            corner(gap_v[li][bj], gap_h[bj][li], fl, fb),
                            corner(gap_v[rj][bj], gap_h[bj][rj], fr, fb),
                        ];
                        let missing = corners.iter().filter(|c| c.is_none()).count();
                        let skewed = corners.iter().filter(|c| **c == Some(false)).count();
                        // A large image on a dark background loses a corner and the
                        // sides near it where the image is dark too. Allow that for
                        // big boxes whose other corners agree, at stricter coverage.
                        let partial = missing == 1 && skewed == 0 && b - t >= large && r - l >= large;
                        if !(missing == 0 && skewed <= 1 || partial) {
                            continue;
                        }
                        let side = |fixed: bool, c: f32| if fixed { 1. } else { c };
                        let c_l = side(fl, cov(&col_pre[li], t, b - 1, y0, gap_v[li][ti], gap_v[li][bj]));
                        let c_r = side(fr, cov(&col_pre[rj], t, b - 1, y0, gap_v[rj][ti], gap_v[rj][bj]));
                        let c_t = side(ft, cov(&row_pre[ti], l, r - 1, x0, gap_h[ti][li], gap_h[ti][rj]));
                        let c_b = side(fb, cov(&row_pre[bj], l, r - 1, x0, gap_h[bj][li], gap_h[bj][rj]));
                        // Real UI boxes measure min ≥ 0.85 / mean ≥ 0.91; chance boxes in
                        // photos about 0.67 / 0.77.
                        let low = c_l.min(c_r).min(c_t.min(c_b));
                        let mean = (c_l + c_r + c_t + c_b) / 4.;
                        let pass = if partial { low >= 0.50 && mean >= 0.78 } else { low >= 0.80 && mean >= 0.86 };
                        if !pass || self.is_stack(l, t, r, b) {
                            continue;
                        }
                        let score = mean - 0.05 * skewed as f32 - if partial { 0.15 } else { 0. };
                        boxes.push(Box4 { l, t, r, b, score });
                    }
                }
            }
        }

        // Two crossing boxes can't both be items; usually one is pieced together from
        // text aligned with a card border. Keep the one with clearer sides.
        let slack = self.px(1.);
        let contains = |a: &Box4, b: &Box4| a.l <= b.l + slack && a.t <= b.t + slack && a.r >= b.r - slack && a.b >= b.b - slack;
        boxes.sort_by(|a, b| b.score.total_cmp(&a.score));
        let mut kept: Vec<Box4> = Vec::new();
        for b in boxes {
            if kept.iter().all(|k| contains(k, &b) || contains(&b, k)) {
                kept.push(b);
            }
        }

        // One nested chain. A box that only adds a thin strip on one or two sides (a
        // card's title bar) or 1-2 px all round (a double border) replaces the last
        // one as the same item; even padding all round (an image in a chat bubble)
        // is a new level.
        kept.sort_by_key(|b| (b.r - b.l) as i64 * (b.b - b.t) as i64);
        let mut chain: Vec<Box4> = Vec::new();
        // Size of the first box of the current item, so replacements can't drift out
        // to a bigger item.
        let mut anchor = (0, 0);
        for b in kept {
            let (bw, bh) = (b.r - b.l, b.b - b.t);
            let Some(last) = chain.last().copied() else {
                chain.push(b);
                anchor = (bw, bh);
                continue;
            };
            if !contains(&b, &last) {
                continue;
            }
            let grow = [last.l - b.l, last.t - b.t, b.r - last.r, b.b - last.b];
            let shared = grow.iter().filter(|&&g| g <= slack).count();
            let max_grow = grow.into_iter().max().unwrap();
            if bw - anchor.0 <= self.px(6.).max(anchor.0 / 8)
                && bh - anchor.1 <= self.px(6.).max(anchor.1 / 8)
                && (shared >= 2 || max_grow <= px2)
            {
                *chain.last_mut().unwrap() = b;
            } else {
                chain.push(b);
                anchor = (bw, bh);
            }
        }
        chain
            .into_iter()
            .map(|b| RectF { x: self.pt(b.l), y: self.pt(b.t), w: self.pt(b.r - b.l), h: self.pt(b.b - b.t) })
            .collect()
    }

    /// A box cut through its middle by a line nearly as strong as its border, running
    /// almost all the way across, is a stack of items (two list rows, two columns).
    /// A fainter cut (a grid inside a photo) or one near an edge (a title bar) is not.
    fn is_stack(&self, l: i32, t: i32, r: i32, b: i32) -> bool {
        let (hh, ww) = (b - t, r - l);
        let row_ref = self.row_strength(t, l, r - 1).min(self.row_strength(b, l, r - 1));
        if (t + hh * 3 / 10..=b - hh * 3 / 10).any(|y| {
            spans(&self.h_start, &self.h_a, &self.h_b, y, l, r - 1) && self.row_strength(y, l, r - 1) * 5 >= row_ref * 3
        }) {
            return true;
        }
        let col_ref = self.col_strength(l, t, b - 1).min(self.col_strength(r, t, b - 1));
        (l + ww * 3 / 10..=r - ww * 3 / 10).any(|x| {
            spans(&self.v_start, &self.v_a, &self.v_b, x, t, b - 1) && self.col_strength(x, t, b - 1) * 5 >= col_ref * 3
        })
    }

    /// Mean edge strength of row `y` over `x0..=x1`; the screen edge counts as full.
    fn row_strength(&self, y: i32, x0: i32, x1: i32) -> i32 {
        if y < 1 || y >= self.h as i32 {
            return 255;
        }
        let base = y as usize * self.w;
        let sum: i32 = (x0..=x1).map(|x| self.h_edge[base + x as usize] as i32).sum();
        sum / (x1 - x0 + 1)
    }

    fn col_strength(&self, x: i32, y0: i32, y1: i32) -> i32 {
        if x < 1 || x >= self.w as i32 {
            return 255;
        }
        let sum: i32 = (y0..=y1).map(|y| self.v_edge[y as usize * self.w + x as usize] as i32).sum();
        sum / (y1 - y0 + 1)
    }

    /// Pixels walked along vertical boundary `x` from row `y` before meeting an edge:
    /// about 0 at a square corner, the radius at a rounded one, `cap + 1` if none.
    fn v_gap(&self, x: i32, mut y: i32, step: i32, cap: i32) -> i32 {
        if x < 1 || x >= self.w as i32 {
            return cap + 1;
        }
        for k in 0..=cap {
            if y < 0 || y >= self.h as i32 {
                break;
            }
            if self.v_edge[y as usize * self.w + x as usize] >= THR {
                return k;
            }
            y += step;
        }
        cap + 1
    }

    fn h_gap(&self, y: i32, mut x: i32, step: i32, cap: i32) -> i32 {
        if y < 1 || y >= self.h as i32 {
            return cap + 1;
        }
        for k in 0..=cap {
            if x < 0 || x >= self.w as i32 {
                break;
            }
            if self.h_edge[y as usize * self.w + x as usize] >= THR {
                return k;
            }
            x += step;
        }
        cap + 1
    }

    fn column_prefix(&self, x: i32, y0: i32, y1: i32) -> Vec<i32> {
        let mut pre = vec![0; (y1 - y0 + 2) as usize];
        if x < 1 || x >= self.w as i32 {
            return pre;
        }
        let mut acc = 0;
        for (k, y) in (y0..=y1).enumerate() {
            if self.v_edge[y as usize * self.w + x as usize] >= THR {
                acc += 1;
            }
            pre[k + 1] = acc;
        }
        pre
    }

    fn row_prefix(&self, y: i32, x0: i32, x1: i32) -> Vec<i32> {
        let mut pre = vec![0; (x1 - x0 + 2) as usize];
        if y < 1 || y >= self.h as i32 {
            return pre;
        }
        let base = y as usize * self.w;
        let mut acc = 0;
        for (k, x) in (x0..=x1).enumerate() {
            if self.h_edge[base + x as usize] >= THR {
                acc += 1;
            }
            pre[k + 1] = acc;
        }
        pre
    }
}

#[derive(Clone, Copy)]
struct Box4 {
    l: i32,
    t: i32,
    r: i32,
    b: i32,
    score: f32,
}

/// Lines at boundaries `from` towards `stop`, nearest first; see `v_lines`.
#[allow(clippy::too_many_arguments)]
fn lines(start: &[u32], a: &[i32], b: &[i32], count: i32, from: i32, stop: i32, p: i32, span: Option<(i32, i32)>, limit: usize) -> Vec<i32> {
    // Total line length at boundary `i` if one of its lines passes `p` or they cover
    // half of `span`, else 0.
    let reach = |i: i32| -> i32 {
        if i < 1 || i >= count {
            return 0;
        }
        let (mut through, mut in_span, mut total) = (false, 0, 0);
        for k in start[i as usize] as usize..start[i as usize + 1] as usize {
            let (lo, hi) = (a[k], b[k]);
            total += hi - lo + 1;
            through |= lo - 2 <= p && p <= hi + 2;
            if let Some((s0, s1)) = span {
                in_span += (hi.min(s1) - lo.max(s0) + 1).max(0);
            }
        }
        let half = span.is_some_and(|(s0, s1)| in_span * 2 > s1 - s0);
        if through || half { total } else { 0 }
    };
    let mut out = Vec::new();
    let step = if stop > from { 1 } else { -1 };
    let mut i = from + step * 2;
    while out.len() < limit && i >= 1 && i < count && if step > 0 { i <= stop } else { i >= stop } {
        let mut best_len = reach(i);
        if best_len == 0 {
            i += step;
            continue;
        }
        // A thick or double border is one line; take its longest, since the first one
        // met may be a short piece next to the real edge.
        let mut best = i;
        for j in [i + step, i + 2 * step] {
            let len = reach(j);
            if len > best_len {
                best = j;
                best_len = len;
            }
        }
        out.push(best);
        i += step * 3;
    }
    out
}

/// Whether the lines at boundary `i` cover at least 90% of `lo..=hi`.
fn spans(start: &[u32], a: &[i32], b: &[i32], i: i32, lo: i32, hi: i32) -> bool {
    if i < 1 || i as usize + 1 >= start.len() || hi <= lo {
        return false;
    }
    let covered: i32 = (start[i as usize] as usize..start[i as usize + 1] as usize)
        .map(|k| (hi.min(b[k]) - lo.max(a[k]) + 1).max(0))
        .sum();
    covered * 10 >= (hi - lo + 1) * 9
}
