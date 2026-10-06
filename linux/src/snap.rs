//! Pixel snapping on the frozen screen, ported from SnapEngine.swift.
//!
//! Edges are found from the image itself: an adaptive threshold on the per-channel
//! difference between neighbouring pixels, then runs of edge pixels long enough to be
//! real lines. Selection edges snap to the best-covered line nearby, and hovering finds
//! the tightest box whose four sides are all present around the pointer.
//!
//! Public coordinates are logical points from the display's top-left; internally
//! everything is image pixels. A vertical boundary `b` lies between columns `b - 1`
//! and `b`, so a box from `L` to `R` covers columns `L..R`.
//!
//! macOS also snaps to window frames from CGWindowList. Wayland hides other windows'
//! geometry, so here everything comes from pixels.

use image::RgbaImage;

pub struct SnapEngine {
    w: usize,
    h: usize,
    /// Image pixels per point.
    scale: f32,
    v_edge: Vec<u8>,
    h_edge: Vec<u8>,
    thr: u8,
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
        let mut hist = [0usize; 256];
        for y in 0..h {
            let row = y * w * 4;
            let base = y * w;
            for x in 1..w {
                let d = chan_diff(&px[row + (x - 1) * 4..], &px[row + x * 4..]);
                v_edge[base + x] = d;
                hist[d as usize] += 1;
            }
            if y == 0 {
                continue;
            }
            let prev = (y - 1) * w * 4;
            for x in 0..w {
                let d = chan_diff(&px[prev + x * 4..], &px[row + x * 4..]);
                h_edge[base + x] = d;
                hist[d as usize] += 1;
            }
        }

        // The strongest ~1.5% of samples set the bar, kept within [16, 64]: flat UI
        // drops it to catch faint borders, busy photos raise it above their noise.
        let total: usize = hist.iter().sum();
        let mut cut = (total * 15 / 1000).max(1) as isize;
        let mut thr = 64usize;
        for v in (1..=255).rev() {
            cut -= hist[v] as isize;
            if cut <= 0 {
                thr = v;
                break;
            }
        }
        let t = thr.clamp(16, 64) as u8;

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

        Some(Self { w, h, scale, v_edge, h_edge, thr: t, v_start, v_a, v_b, h_start, h_a, h_b })
    }

    fn px(&self, v: f32) -> i32 {
        (v * self.scale).round() as i32
    }

    fn pt(&self, v: i32) -> f32 {
        v as f32 / self.scale
    }

    /// Columns with a vertical line through `row`, nearest first, scanning from
    /// `from` towards `stop`.
    fn v_lines(&self, from: i32, stop: i32, row: i32, limit: usize) -> Vec<i32> {
        let mut out = Vec::new();
        let step = if stop > from { 1 } else { -1 };
        let mut x = from + step * 2;
        while out.len() < limit && x >= 1 && x < self.w as i32 && if step > 0 { x <= stop } else { x >= stop } {
            let (s, e) = (self.v_start[x as usize] as usize, self.v_start[x as usize + 1] as usize);
            let hit = (s..e).any(|i| self.v_a[i] - 2 <= row && row <= self.v_b[i] + 2);
            if hit {
                out.push(x);
                // Skip the rest of a border a few pixels thick.
                x += step * 3;
            } else {
                x += step;
            }
        }
        out
    }

    fn h_lines(&self, from: i32, stop: i32, col: i32, limit: usize) -> Vec<i32> {
        let mut out = Vec::new();
        let step = if stop > from { 1 } else { -1 };
        let mut y = from + step * 2;
        while out.len() < limit && y >= 1 && y < self.h as i32 && if step > 0 { y <= stop } else { y >= stop } {
            let (s, e) = (self.h_start[y as usize] as usize, self.h_start[y as usize + 1] as usize);
            let hit = (s..e).any(|i| self.h_a[i] - 2 <= col && col <= self.h_b[i] + 2);
            if hit {
                out.push(y);
                y += step * 3;
            } else {
                y += step;
            }
        }
        out
    }

    fn v_coverage(&self, x: i32, y0: i32, y1: i32) -> f32 {
        if x < 1 || x >= self.w as i32 || y1 < y0 {
            return 0.;
        }
        let hit = (y0..=y1).filter(|&y| self.v_edge[y as usize * self.w + x as usize] >= self.thr).count();
        hit as f32 / (y1 - y0 + 1) as f32
    }

    fn h_coverage(&self, y: i32, x0: i32, x1: i32) -> f32 {
        if y < 1 || y >= self.h as i32 || x1 < x0 {
            return 0.;
        }
        let base = y as usize * self.w;
        let hit = (x0..=x1).filter(|&x| self.h_edge[base + x as usize] >= self.thr).count();
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

    /// The smallest box around `point` inside `limit` whose four sides are lines.
    pub fn element(&self, point: (f32, f32), limit: RectF) -> Option<RectF> {
        let (w, h) = (self.w as i32, self.h as i32);
        let x0 = self.px(limit.x).clamp(0, w - 1);
        let x1 = self.px(limit.x + limit.w).clamp(1, w);
        let y0 = self.px(limit.y).clamp(0, h - 1);
        let y1 = self.px(limit.y + limit.h).clamp(1, h);
        if x1 - x0 <= 16 || y1 - y0 <= 16 {
            return None;
        }
        let cx = self.px(point.0).clamp(x0, x1 - 1);
        let cy = self.px(point.1).clamp(y0, y1 - 1);
        let min_size = 12.max(self.px(16.));
        let limit_n = 20;

        let mut lefts = self.v_lines(cx, x0, cy, limit_n);
        let mut rights = self.v_lines(cx, x1 - 1, cy, limit_n);
        let mut tops = self.h_lines(cy, y0, cx, limit_n);
        let mut bottoms = self.h_lines(cy, y1 - 1, cx, limit_n);
        // The search area's own edges are the fallback sides.
        if x0 >= 1 && !lefts.contains(&x0) {
            lefts.push(x0);
        }
        if x1 < w && !rights.contains(&x1) {
            rights.push(x1);
        }
        if y0 >= 1 && !tops.contains(&y0) {
            tops.push(y0);
        }
        if y1 < h && !bottoms.contains(&y1) {
            bottoms.push(y1);
        }
        if lefts.is_empty() || rights.is_empty() || tops.is_empty() || bottoms.is_empty() {
            return None;
        }

        // Prefix sums make any side's coverage O(1), which keeps hovering smooth.
        let col_pre: Vec<Vec<i32>> =
            lefts.iter().chain(&rights).map(|&x| self.column_prefix(x, y0, y1 - 1)).collect();
        let row_pre: Vec<Vec<i32>> =
            tops.iter().chain(&bottoms).map(|&y| self.row_prefix(y, x0, x1 - 1)).collect();
        let cov = |pre: &[i32], lo: i32, hi: i32, base: i32| -> f32 {
            if hi < lo {
                return 0.;
            }
            (pre[(hi - base + 1) as usize] - pre[(lo - base) as usize]) as f32 / (hi - lo + 1) as f32
        };

        // Candidates are sorted by distance, so areas only grow along each list and a
        // branch can stop at the first box larger than the best one.
        let min_h = (bottoms[0] - tops[0]).max(1);
        let mut best = None;
        let mut best_area = i64::MAX;
        for (li, &l) in lefts.iter().enumerate() {
            if ((rights[0] - l) as i64) * min_h as i64 >= best_area {
                break;
            }
            for (ri, &r) in rights.iter().enumerate() {
                if ((r - l) as i64) * min_h as i64 >= best_area {
                    break;
                }
                if r - l < min_size {
                    continue;
                }
                for (ti, &t) in tops.iter().enumerate() {
                    if ((r - l) as i64) * ((bottoms[0] - t) as i64) >= best_area {
                        break;
                    }
                    for (bi, &b) in bottoms.iter().enumerate() {
                        let area = ((r - l) as i64) * ((b - t) as i64);
                        if area >= best_area {
                            break;
                        }
                        if b - t < min_size {
                            continue;
                        }
                        let c_l = cov(&col_pre[li], t, b - 1, y0);
                        let c_r = cov(&col_pre[lefts.len() + ri], t, b - 1, y0);
                        let c_t = cov(&row_pre[ti], l, r - 1, x0);
                        let c_b = cov(&row_pre[tops.len() + bi], l, r - 1, x0);
                        // Real UI boxes measure min ≥ 0.85 / mean ≥ 0.91; chance boxes in
                        // photos about 0.67 / 0.77.
                        if c_l.min(c_r).min(c_t.min(c_b)) < 0.80 || (c_l + c_r + c_t + c_b) / 4. < 0.86 {
                            continue;
                        }
                        best_area = area;
                        best = Some(RectF { x: self.pt(l), y: self.pt(t), w: self.pt(r - l), h: self.pt(b - t) });
                    }
                }
            }
        }
        best
    }

    fn column_prefix(&self, x: i32, y0: i32, y1: i32) -> Vec<i32> {
        let mut pre = vec![0; (y1 - y0 + 2) as usize];
        if x < 1 || x >= self.w as i32 {
            return pre;
        }
        let mut acc = 0;
        for (k, y) in (y0..=y1).enumerate() {
            if self.v_edge[y as usize * self.w + x as usize] >= self.thr {
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
            if self.h_edge[base + x as usize] >= self.thr {
                acc += 1;
            }
            pre[k + 1] = acc;
        }
        pre
    }
}
