//! Balloon geometry: outlines, tails, think-trails, spikes, waves.
//! Codifies balloon.cpp/.h, bodycam.cpp/.h (tail placement), and the
//! balloon drawing in comic-chat-rs 0.3.17 src/app.rs.
//!
//! The original balloon is NOT a rounded rect. It is (balloon.cpp
//! CreateBalloonSpline + GetFilters/PermuteFilters + AddWavies):
//!   1. Per-line left/right filters from the galley rows.
//!   2. Control points = per-line edges (coarse polygon, one segment
//!      per edge / per-line step), NOT rect corners + midpoints.
//!   3. MS wavies (AddWavies) on the coarse control polygon, then
//!      closed CBeta spline (tension 5, bias 1) through those points.
//!   4. Style transform: Speech/Whisper/Thought = MS wavies (cloud);
//!      Whisper adds dashed outline at draw time; Thought adds trail;
//!      Shout = spiked, no wavies.
//!   5. Tail (Speech/Shout/Whisper) from balloon edge to speaker's head
//!      anchor, with a bend; Thought uses a trail, not a tail.
//!
//! Constants below are the values observed in the Microsoft source and
//! in 0.3.17 app.rs balloon_metrics. They are in device px at 96 DPI
//! (zoom 1.0). Multiply by zoom for other zooms — do NOT bake zoom into
//! the constants.

pub use comic_geometry::Point;
use comic_geometry::{bspline_closed, add_waves, add_spikes, tail_polygon, think_trail, Vector};

/// MS balloon.cpp, in device px at 96 DPI (twips / 15):
/// VWAVEINTERVAL 300 -> 20.0, VWAVEHEIGHT / HWAVEHEIGHT 70 -> 4.6667.
pub const WAVE_INTERVAL: f64 = 300.0 / 15.0;
pub const WAVE_HEIGHT: f64 = 70.0 / 15.0;
/// CBeta defaults from spline.cpp: tension 5.0, bias 1.0.
pub const BETA_TENSION: f64 = 5.0;
pub const BETA_BIAS: f64 = 1.0;

fn push_dedupe(ctrl: &mut Vec<Point>, pt: Point) {
    if ctrl.last().map(|q| q.distance(pt) > 1e-9).unwrap_or(true) { ctrl.push(pt); }
}

/// MS AddWavies (balloon.cpp:546), ported: along each control-polygon
/// edge, if the edge is >= 2 intervals long, insert alternating points;
/// every other inserted point is offset OUTWARD by wave_height (MS
/// normal (unit.y, -unit.x), which for a clockwise polygon already
/// points outward). The spline then smooths these into the scalloped
/// cloud edge. No Vector use: plain Point arithmetic only.
pub fn add_wavies_ms(poly: &[Point], wave_height: f64, wave_interval: f64) -> Vec<Point> {
    if poly.len() < 2 { return poly.to_vec(); }
    let n = poly.len();
    let mut out: Vec<Point> = Vec::new();
    for i in 0..n {
        let p1 = poly[i];
        let p2 = poly[(i + 1) % n];
        out.push(p1);
        let dist = p1.distance(p2);
        let n_waves = dist / wave_interval;
        if n_waves < 2.0 { continue; }
        let i_waves = n_waves as i32;
        if i_waves < 2 { continue; }
        let wave_len = dist / i_waves as f64;
        let ux = (p2.x - p1.x) / dist;
        let uy = (p2.y - p1.y) / dist;
        // MS normal (unit.y, -unit.x). For a clockwise polygon this already
        // points OUTWARD; do NOT flip toward the centroid (that inverts
        // the cloud bumps into dents).
        let (nx, ny) = (uy, -ux);
        let mut base = p1;
        for k in 0..(i_waves - 1) {
            base = Point::new(base.x + ux * wave_len, base.y + uy * wave_len);
            if k % 2 == 0 {
                out.push(Point::new(base.x + nx * wave_height, base.y + ny * wave_height));
            } else {
                out.push(base);
            }
        }
    }
    out
}

/// MS CBeta spline (spline.cpp SetMatrix, tension 5, bias 1):
/// matrix (1/17)*[[-2,16,-16,2],[6,-27,21,0],[-6,0,6,0],[2,13,2,0]],
/// rows are u^3, u^2, u, 1. Closed: segment i uses ctrl[i..i+3] wrapped.
pub fn beta_spline_closed(ctrl: &[Point], per_seg: usize) -> Vec<Point> {
    let n = ctrl.len();
    if n < 2 || per_seg == 0 { return Vec::new(); }
    let d = 1.0 / (BETA_TENSION + 2.0 * BETA_BIAS.powi(3) + 4.0 * (BETA_BIAS * BETA_BIAS + BETA_BIAS) + 2.0);
    let mut out = Vec::with_capacity(n * per_seg);
    for i in 0..n {
        let (p0, p1, p2, p3) = (ctrl[i], ctrl[(i + 1) % n], ctrl[(i + 2) % n], ctrl[(i + 3) % n]);
        for k in 0..per_seg {
            let u = k as f64 / per_seg as f64;
            let (u2, u3) = (u * u, u * u * u);
            let c0 = d * (u3 * -2.0 + u2 * 6.0 + u * -6.0 + 2.0);
            let c1 = d * (u3 * 16.0 + u2 * -27.0 + u * 0.0 + 13.0);
            let c2 = d * (u3 * -16.0 + u2 * 21.0 + u * 6.0 + 2.0);
            let c3 = d * (u3 * 2.0);
            out.push(Point::new(
                c0 * p0.x + c1 * p1.x + c2 * p2.x + c3 * p3.x,
                c0 * p0.y + c1 * p1.y + c2 * p2.y + c3 * p3.y,
            ));
        }
    }
    out
}

/// Balloon style (mirrors comic-text::BalloonStyle; kept separate so
/// comic-balloon does not depend on comic-text).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Style { Speech, Thought, Shout, Whisper }

/// Metrics for one balloon. All px at zoom 1.0.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    /// Horizontal padding inside balloon (text to outline), each side.
    pub pad_x: f64,
    /// Vertical padding, each side.
    pub pad_y: f64,
    /// Minimum balloon width / height (so a 1-char message still reads).
    pub min_w: f64, pub min_h: f64,
    /// Tail base half-width (half the width where tail meets balloon).
    pub tail_half_width: f64,
    /// Tail bend (px lateral offset at tip control point).
    pub tail_bend: f64,
    /// Whisper dash: on/off lengths for the dashed outline.
    pub dash_on: f64, pub dash_off: f64,
    /// Thought-trail base circle radius.
    pub trail_radius: f64,
}
impl Default for Metrics {
    fn default() -> Self {
        Self { pad_x: 12.0, pad_y: 8.0, min_w: 48.0, min_h: 32.0, tail_half_width: 10.0, tail_bend: 6.0, dash_on: 6.0, dash_off: 4.0, trail_radius: 8.0 }
    }
}

/// WARNING: rect simplification, NOT the original algorithm. The original
/// (balloon.cpp CreateBalloonSpline) and app.rs build the outline from
/// PER-LINE edges — see control_points_per_line(). Do NOT wire
/// draw_balloon to this; it would regress the hugging shape.
/// Text rect -> balloon control points: corners + edge midpoints,
/// in clockwise order starting at top-left corner.
/// Order matters: bspline_closed treats the list as a closed loop.
pub fn control_points(text_rect: (f64, f64, f64, f64), m: &Metrics) -> Vec<Point> {
    let (x, y, w, h) = text_rect;
    let (w, h) = (w.max(m.min_w - 2.0*m.pad_x), h.max(m.min_h - 2.0*m.pad_y));
    let x0 = x - m.pad_x; let y0 = y - m.pad_y;
    let x1 = x0 + w + 2.0*m.pad_x; let y1 = y0 + h + 2.0*m.pad_y;
    let mx = (x0+x1)/2.0; let my = (y0+y1)/2.0;
    vec![
        Point::new(x0, y0), Point::new(mx, y0), Point::new(x1, y0), Point::new(x1, my),
        Point::new(x1, y1), Point::new(mx, y1), Point::new(x0, y1), Point::new(x0, my),
    ]
}

/// Full balloon outline for a text rect + style.
/// Returns the closed polyline (last point connects back to first).
/// `per_seg` = B-spline subdivisions per control span (8 in 0.3.17).
pub fn outline(text_rect: (f64, f64, f64, f64), style: Style, m: &Metrics, per_seg: usize) -> Vec<Point> {
    // Same order as outline_per_line: wavies BEFORE spline, spikes AFTER.
    let mut ctrl = control_points(text_rect, m);
    match style {
        Style::Speech | Style::Whisper | Style::Thought => { ctrl = add_wavies_ms(&ctrl, WAVE_HEIGHT, WAVE_INTERVAL); }
        Style::Shout => {}
    }
    let mut pts = beta_spline_closed(&ctrl, per_seg);
    if style == Style::Shout { pts = add_spikes(&pts, 18.0, 8.0); }
    pts
}

/// Full per-line balloon outline: coarse control polygon + MS AddWavies
/// + closed CBeta spline. This is what draw_balloon wires to -
/// do NOT wire it to outline(), which uses the rect control_points().
/// Order is MS order: wavies BEFORE the spline (the spline smooths them);
/// spikes AFTER the spline (the spline would round them off).
/// `per_seg` = spline subdivisions per control span (8, as in outline()).
pub fn outline_per_line(
    line_widths: &[f64],
    row_tops: &[f64],
    row_bottoms: &[f64],
    text_w: f64,
    text_h: f64,
    pad_l: f64, pad_r: f64, pad_t: f64, pad_b: f64,
    balloon_w: f64, balloon_h: f64,
    style: Style, per_seg: usize,
) -> Vec<Point> {
    let mut ctrl = control_points_per_line(
        line_widths, row_tops, row_bottoms,
        text_w, text_h, pad_l, pad_r, pad_t, pad_b, balloon_w, balloon_h,
    );
    match style {
        Style::Speech | Style::Whisper | Style::Thought => { ctrl = add_wavies_ms(&ctrl, WAVE_HEIGHT * (pad_l / 12.0) * 0.5, WAVE_INTERVAL * (pad_l / 12.0)); }
        Style::Shout => {}
    }
    let mut pts = beta_spline_closed(&ctrl, per_seg);
    if style == Style::Shout { pts = add_spikes(&pts, 18.0, 8.0); }
    pts
}

/// Where the tail attaches: the point on the balloon outline closest
/// to the speaker's head anchor, offset along the outline so the tail
/// base straddles it. Returns (base_left, base_right) on the outline.
/// The base is perpendicular to the direction from balloon centre to head.
pub fn tail_base(outline_pts: &[Point], head: Point, m: &Metrics) -> Option<(Point, Point)> {
    if outline_pts.is_empty() { return None; }
    let mut best: Option<(f64, Point)> = None;
    for p in outline_pts { let d = p.distance(head); if best.as_ref().map(|(bd,_)| d < *bd).unwrap_or(true) { best = Some((d, *p)); } }
    let attach = best?.1;
    // Tangent at attach: from neighbours in the polyline.
    let idx = outline_pts.iter().position(|p| *p == attach).unwrap_or(0);
    let prev = outline_pts[(idx + outline_pts.len() - 1) % outline_pts.len()];
    let next = outline_pts[(idx + 1) % outline_pts.len()];
    let tangent = next.sub(prev).normalised();
    let left = attach.add(tangent.scale(-m.tail_half_width));
    let right = attach.add(tangent.scale(m.tail_half_width));
    Some((left, right))
}

/// Complete balloon: outline + tail polygon (or think-trail).
/// Returns (outline, tail_polyline_or_empty, trail_centres, trail_radii).
/// For Thought, tail is empty and trail is populated. For others,
/// trail is empty and tail is populated (may be empty if head is None).
pub fn balloon(text_rect: (f64, f64, f64, f64), style: Style, m: &Metrics, head: Option<Point>) -> (Vec<Point>, Vec<Point>, Vec<Point>, Vec<f64>) {
    let ol = outline(text_rect, style, m, 8);
    match style {
        Style::Thought => {
            if let Some(h) = head {
                let centre = ol.iter().fold(Point::origin(), |a, p| Point::new(a.x + p.x / ol.len() as f64, a.y + p.y / ol.len() as f64));
                let (c, r) = think_trail(centre, h, m.tail_bend, m.trail_radius);
                (ol, vec![], c, r)
            } else { (ol, vec![], vec![], vec![]) }
        }
        _ => {
            if let Some(h) = head {
                if let Some((l, r)) = tail_base(&ol, h, m) {
                    let tail = tail_polygon(l, r, h, m.tail_bend);
                    return (ol, tail, vec![], vec![]);
                }
            }
            (ol, vec![], vec![], vec![])
        }
    }
}

/// Balloon fill / outline colours (RGB). The original used the user's
/// chosen balloon colour for the speaker; default is white fill, black
/// outline. Whisper used a grey outline in some versions — keep black
/// unless the theme says otherwise.
pub fn colours_for(speaker_colour: Option<(u8,u8,u8)>) -> ((u8,u8,u8), (u8,u8,u8)) {
    (speaker_colour.unwrap_or((255,255,255)), (0,0,0))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn outline_closed() { let m = Metrics::default(); let ol = outline((0.0,0.0,100.0,40.0), Style::Speech, &m, 8); assert!(!ol.is_empty()); assert!(ol.len() > 64); assert_eq!(ol.len() % 8, 0); }
    #[test] fn shout_spikes_change_outline() { let m = Metrics::default(); let s = outline((0.0,0.0,100.0,40.0), Style::Speech, &m, 8); let sh = outline((0.0,0.0,100.0,40.0), Style::Shout, &m, 8); assert_ne!(s, sh); }
    #[test] fn thought_has_trail() { let m = Metrics::default(); let (_, tail, centres, _) = balloon((0.0,0.0,100.0,40.0), Style::Thought, &m, Some(Point::new(50.0,200.0))); assert!(tail.is_empty()); assert_eq!(centres.len(), 3); }
    #[test] fn speech_has_tail() { let m = Metrics::default(); let (_, tail, _, _) = balloon((0.0,0.0,100.0,40.0), Style::Speech, &m, Some(Point::new(50.0,200.0))); assert!(!tail.is_empty()); }
    #[test] fn min_size() { let m = Metrics::default(); let ol = outline((0.0,0.0,1.0,1.0), Style::Speech, &m, 8); let xs: Vec<f64> = ol.iter().map(|p| p.x).collect(); let w = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max) - xs.iter().cloned().fold(f64::INFINITY, f64::min); assert!(w >= m.min_w - 1.0); }
}

/// Per-line balloon control polygon — the ORIGINAL algorithm
/// (balloon.cpp CreateBalloonSpline + GetFilters/PermuteFilters +
/// AddWavies) and the one app.rs draw_balloon uses.
///
/// The crate's control_points() (single text rect, 8 points) is a
/// simplification, NOT the original: the original builds the outline
/// from per-line left/right edges so the balloon hugs short lines
/// instead of ballooning into a rect/amoeba. Do NOT wire draw_balloon
/// to control_points()/outline(); wire it to this.
///
/// Inputs are galley-local (origin at text top-left), exactly as
/// draw_balloon computes them:
///   line_widths: each row's text width
///   row_tops / row_bottoms: each row's y extents (galley-local)
///   text_w / text_h: galley size
///   pad_l/pad_r/pad_t/pad_b: scaled padding
///   balloon_w/balloon_h: final balloon size (after min + col clamp)
/// Returns the COARSE control points, clockwise from top-left, BEFORE
/// wavies/spline. MS CreateBalloonSpline works on this coarse polygon
/// (one segment per edge / per-line step); subdividing it would make
/// the AddWavies segments too short for any wavies to appear.
/// See outline_per_line(): wavies first, then CBeta spline, spikes last.
pub fn control_points_per_line(
    line_widths: &[f64],
    row_tops: &[f64],
    row_bottoms: &[f64],
    text_w: f64,
    text_h: f64,
    pad_l: f64, pad_r: f64, pad_t: f64, pad_b: f64,
    balloon_w: f64, balloon_h: f64,
) -> Vec<Point> {
    let n_lines = line_widths.len().max(1);
    // Step 18: per-line edges from row widths, centered in the galley.
    let mut left_e: Vec<f64> = Vec::new();
    let mut right_e: Vec<f64> = Vec::new();
    for w in line_widths {
        left_e.push((text_w - w) / 2.0 - pad_l);
        right_e.push((text_w + w) / 2.0 + pad_r);
    }
    if left_e.is_empty() { left_e.push(-pad_l); right_e.push(text_w + pad_r); }
    // Half-blend smoothing: raw edges give a sharp 90-degree step that the
    // CBeta spline turns into a pointy cusp on the 2nd line. Full min/max
    // erases hugging for 2 lines. Blend halfway to the neighbour's min/max:
    // the step is halved, so the spline rounds it into a flowing curve,
    // and the short line still hugs by half.
    // Full hug: MS Chat hugs short lines fully; CBeta handles the step without cusping.
    let left_s = left_e.clone();
    let right_s = right_e.clone();
    let extra_w = (balloon_w - (text_w + pad_l + pad_r)).max(0.0);
    let extra_h = (balloon_h - (text_h + pad_t + pad_b)).max(0.0);
    let bx0 = -pad_l - extra_w / 2.0;
    let bx1 = text_w + pad_r + extra_w / 2.0;
    let by0 = -pad_t - extra_h / 2.0;
    let by1 = text_h + pad_b + extra_h / 2.0;
    let mut ctrl: Vec<Point> = Vec::new();
    // MS coarse polygon: top edge is ONE segment between the first
    // line's edges (not full bx0..bx1), so a narrow first line hugs.
    let tl_x = (left_e[0] - extra_w / 2.0).clamp(bx0, bx1);
    let tr_x = (right_e[0] + extra_w / 2.0).clamp(bx0, bx1);
    push_dedupe(&mut ctrl, Point::new(tl_x, by0));
    push_dedupe(&mut ctrl, Point::new(tr_x, by0));
    // Right side: top/bottom per line (CBeta, unlike the B-spline,
    // does not cusp on the one-point-per-line polygon). Step 18 shift kept.
    for i in 0..n_lines {
        let (ry0, ry1) = if i < row_tops.len() { (row_tops[i], row_bottoms[i]) } else { (0.0, text_h) };
        let rx = (right_s[i] + extra_w / 2.0).clamp(bx0, bx1);
        push_dedupe(&mut ctrl, Point::new(rx, ry0));
        push_dedupe(&mut ctrl, Point::new(rx, ry1));
    }
    // Bottom edge: ONE segment, reversed, between the last line's edges.
    let br_x = (right_e[n_lines - 1] + extra_w / 2.0).clamp(bx0, bx1);
    let bl_x = (left_e[n_lines - 1] - extra_w / 2.0).clamp(bx0, bx1);
    push_dedupe(&mut ctrl, Point::new(br_x, by1));
    push_dedupe(&mut ctrl, Point::new(bl_x, by1));
    // Left side: bottom/top per line, bottom-up (polygon runs clockwise).
    for i in (0..n_lines).rev() {
        let (ly0, ly1) = if i < row_tops.len() { (row_tops[i], row_bottoms[i]) } else { (0.0, text_h) };
        let lx = (left_s[i] - extra_w / 2.0).clamp(bx0, bx1);
        push_dedupe(&mut ctrl, Point::new(lx, ly1));
        push_dedupe(&mut ctrl, Point::new(lx, ly0));
    }
    ctrl
}


#[cfg(test)]
mod per_line_tests {
    use super::*;
    fn approx(a: f64, b: f64) -> bool { (a - b).abs() < 1e-9 }
    #[test] fn five_lines_middle_hugs() {
        // Full hug: consecutive narrow lines share the same edge x, so
        // push_dedupe collapses the shared boundary point. 2 top + 8 right
        // + 2 bottom + 8 left = 20 (was 24 with half-blend, where the
        // blended x differed per line).
        let pts = control_points_per_line(
            &[100.0, 60.0, 60.0, 60.0, 100.0],
            &[0.0, 20.0, 40.0, 60.0, 80.0], &[20.0, 40.0, 60.0, 80.0, 100.0],
            100.0, 100.0, 14.0, 14.0, 10.0, 12.0, 128.0, 122.0,
        );
        assert_eq!(pts.len(), 20);
        // First point: top-left corner (bx0, by0) = (-14, -10).
        assert!(approx(pts[0].x, -14.0) && approx(pts[0].y, -10.0));
        // Right side, line 0: widest line lands on edge x=114.
        assert!(approx(pts[2].x, 114.0) && approx(pts[2].y, 0.0));
        assert!(approx(pts[3].x, 114.0) && approx(pts[3].y, 20.0));
        // Right side, middle line: hugs in at x=94.
        assert!(approx(pts[5].x, 94.0) && approx(pts[5].y, 40.0));
        assert!(approx(pts[6].x, 94.0) && approx(pts[6].y, 60.0));
        // Left side, middle line (bottom-up): hugs in at x=6.
        assert!(approx(pts[16].x, 6.0) && approx(pts[16].y, 40.0));
        assert!(approx(pts[17].x, 6.0) && approx(pts[17].y, 20.0));
        // Every point stays inside the balloon box.
        for p in &pts { assert!(p.x >= -14.0 - 1e-9 && p.x <= 114.0 + 1e-9); assert!(p.y >= -10.0 - 1e-9 && p.y <= 112.0 + 1e-9); }
    }
    #[test] fn single_line_is_rect_like() {
        let pts = control_points_per_line(
            &[80.0], &[0.0], &[20.0],
            80.0, 20.0, 14.0, 14.0, 10.0, 12.0, 108.0, 42.0,
        );
        assert_eq!(pts.len(), 8);
        for p in pts.iter().take(2) { assert!(approx(p.y, -10.0)); }
    }
    #[test] fn empty_lines_fallback_does_not_panic() {
        let pts = control_points_per_line(&[], &[], &[], 0.0, 0.0, 14.0, 14.0, 10.0, 12.0, 48.0, 32.0);
        assert_eq!(pts.len(), 6);
    }
    #[test] fn extra_width_spreads_evenly() {
        // Balloon wider than text+padding: extra width splits evenly
        // (bx0/bx1 shift out by extra_w/2 each side).
        let pts = control_points_per_line(
            &[80.0], &[0.0], &[20.0],
            80.0, 20.0, 14.0, 14.0, 10.0, 12.0, 148.0, 42.0,
        );
        // extra_w = 148 - (80+28) = 40 -> bx0 = -14-20 = -34, bx1 = 94+20 = 114.
        assert!(approx(pts[0].x, -34.0));
        assert!(approx(pts[1].x, 114.0));
    }
    #[test] fn per_line_outline_stays_in_box_for_speech() {
        // Speech applies no style transform, so the spline stays inside
        // the control points' box (same 5-line case as five_lines_middle_hugs).
        let pts = outline_per_line(
            &[100.0, 60.0, 60.0, 60.0, 100.0],
            &[0.0, 20.0, 40.0, 60.0, 80.0], &[20.0, 40.0, 60.0, 80.0, 100.0],
            100.0, 100.0, 14.0, 14.0, 10.0, 12.0, 128.0, 122.0,
            Style::Speech, 8,
        );
        assert!(!pts.is_empty());
        // Outward wavies bump up to WAVE_HEIGHT outside the control box.
        let tol = WAVE_HEIGHT + 1.0;
        for p in &pts { assert!(p.x >= -14.0 - tol && p.x <= 114.0 + tol); assert!(p.y >= -10.0 - tol && p.y <= 112.0 + tol); }
    }
    #[test] fn wavies_go_outward_on_long_edge() {
        // 100x50 rect: top edge gets 4 inserted points, alternating
        // offset/on-line; the offset one bumps OUTWARD (y < 0 on top edge,
        // MS normal as-is). Right/left edges (50px) get 1 each, bottom
        // gets 4: 4 + 4 + 1 + 4 + 1 = 14.
        let poly = vec![Point::new(0.0, 0.0), Point::new(100.0, 0.0), Point::new(100.0, 50.0), Point::new(0.0, 50.0)];
        let out = add_wavies_ms(&poly, WAVE_HEIGHT, WAVE_INTERVAL);
        assert_eq!(out.len(), 14);
        assert!(approx(out[1].x, 20.0) && out[1].y < 0.0);
        assert!(approx(out[2].x, 40.0) && approx(out[2].y, 0.0));
    }
    #[test] fn beta_spline_first_point_matches_matrix() {
        // Segment 0 at u=0: (2*P0 + 13*P1 + 2*P2)/17 for the unit square.
        let ctrl = vec![Point::new(0.0, 0.0), Point::new(100.0, 0.0), Point::new(100.0, 100.0), Point::new(0.0, 100.0)];
        let pts = beta_spline_closed(&ctrl, 8);
        assert_eq!(pts.len(), 32);
        assert!(approx(pts[0].x, 1500.0 / 17.0) && approx(pts[0].y, 200.0 / 17.0));
        for p in &pts { assert!(p.x >= -1e-9 && p.x <= 100.0 + 1e-9); assert!(p.y >= -1e-9 && p.y <= 100.0 + 1e-9); }
    }
}
