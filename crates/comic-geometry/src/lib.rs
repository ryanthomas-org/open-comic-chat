//! Geometry for Comic Chat panel rendering.
//! Codifies vector2d.cpp/.h, traj.cpp, arc.cpp, spline.cpp, bbox.h,
//! and balloon-geometry helpers from app.rs in comic-chat-rs 0.3.17
//! (bspline_closed, add_waves, add_spikes, tail_polygon, think_trail_centre).
//!
//! Conventions (Microsoft source, preserved exactly):
//! * Points are device points in panel space. Original used CPoint/DPOINT
//!   (long/int). This crate uses f64 internally, rounds only at rasteriser.
//! * Y grows DOWN (GDI/screen). Angles in degrees, 0 = +X (east),
//!   increasing clockwise in screen space (toward +Y). Do not "fix" to
//!   maths convention or tails/arcs flip vertically.
//! * Angles normalised to [0,360) on input to arc/angle helpers.
use std::f64::consts::PI;
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point { pub x: f64, pub y: f64 }
impl Point {
    pub fn new(x: f64, y: f64) -> Self { Self { x, y } }
    pub fn origin() -> Self { Self { x: 0.0, y: 0.0 } }
    pub fn add(self, v: Vector) -> Self { Self::new(self.x + v.dx, self.y + v.dy) }
    pub fn sub(self, o: Self) -> Vector { Vector::new(self.x - o.x, self.y - o.y) }
    pub fn distance(self, o: Self) -> f64 { self.sub(o).magnitude() }
    pub fn round(self) -> (i32, i32) { (self.x.round() as i32, self.y.round() as i32) }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vector { pub dx: f64, pub dy: f64 }
impl Vector {
    pub fn new(dx: f64, dy: f64) -> Self { Self { dx, dy } }
    pub fn zero() -> Self { Self::new(0.0, 0.0) }
    pub fn add(self, o: Self) -> Self { Self::new(self.dx + o.dx, self.dy + o.dy) }
    pub fn sub(self, o: Self) -> Self { Self::new(self.dx - o.dx, self.dy - o.dy) }
    pub fn scale(self, s: f64) -> Self { Self::new(self.dx * s, self.dy * s) }
    pub fn magnitude(self) -> f64 { (self.dx * self.dx + self.dy * self.dy).sqrt() }
    pub fn normalised(self) -> Self { let m = self.magnitude(); if m == 0.0 || !m.is_finite() { Self::zero() } else { self.scale(1.0 / m) } }
    pub fn dot(self, o: Self) -> f64 { self.dx * o.dx + self.dy * o.dy }
    pub fn cross(self, o: Self) -> f64 { self.dx * o.dy - self.dy * o.dx }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BBox { pub min: Point, pub max: Point }
impl BBox {
    pub fn from_points(pts: &[Point]) -> Option<Self> {
        if pts.is_empty() { return None; }
        let mut mnx = f64::INFINITY; let mut mny = f64::INFINITY;
        let mut mxx = f64::NEG_INFINITY; let mut mxy = f64::NEG_INFINITY;
        for p in pts { mnx = mnx.min(p.x); mny = mny.min(p.y); mxx = mxx.max(p.x); mxy = mxy.max(p.y); }
        Some(Self { min: Point::new(mnx, mny), max: Point::new(mxx, mxy) })
    }
    pub fn width(&self) -> f64 { self.max.x - self.min.x }
    pub fn height(&self) -> f64 { self.max.y - self.min.y }
    pub fn centre(&self) -> Point { Point::new((self.min.x + self.max.x)/2.0, (self.min.y + self.max.y)/2.0) }
    pub fn contains(&self, p: Point) -> bool { p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y }
    pub fn inflate(&self, d: f64) -> Self { Self { min: Point::new(self.min.x-d, self.min.y-d), max: Point::new(self.max.x+d, self.max.y+d) } }
}
pub fn normalise_angle_deg(a: f64) -> f64 { let mut v = a % 360.0; if v < 0.0 { v += 360.0; } v }
pub fn deg_to_rad(d: f64) -> f64 { d * PI / 180.0 }
pub fn rad_to_deg(r: f64) -> f64 { r * 180.0 / PI }
pub fn vector_to_angle(v: Vector) -> f64 { normalise_angle_deg(rad_to_deg(v.dy.atan2(v.dx))) }
pub fn angle_to_vector(deg: f64) -> Vector { let r = deg_to_rad(normalise_angle_deg(deg)); Vector::new(r.cos(), r.sin()) }
pub fn angle_between(a_deg: f64, b_deg: f64) -> f64 { let d = (normalise_angle_deg(a_deg)-normalise_angle_deg(b_deg)).abs(); if d > 180.0 { 360.0-d } else { d } }
pub fn rotate(v: Vector, deg: f64) -> Vector { let r = deg_to_rad(deg); let (s,c)=(r.sin(),r.cos()); Vector::new(v.dx*c - v.dy*s, v.dx*s + v.dy*c) }
/// Subdivide an arc. ScanArcAux/ScanArc/DrawArc2 semantics: points on ellipse.
/// Positive sweep = clockwise in screen space. step_deg<=0 defaults to 3.
pub fn scan_arc(centre: Point, rx: f64, ry: f64, start_deg: f64, sweep_deg: f64, step_deg: f64) -> Vec<Point> {
    if rx <= 0.0 || ry <= 0.0 || sweep_deg == 0.0 { return vec![centre]; }
    let step = if step_deg <= 0.0 { 3.0 } else { step_deg };
    let n = ((sweep_deg.abs()/step).ceil() as usize).max(1);
    let mut out = Vec::with_capacity(n+1);
    for i in 0..=n { let a = deg_to_rad(start_deg + sweep_deg*(i as f64)/(n as f64)); out.push(Point::new(centre.x + rx*a.cos(), centre.y + ry*a.sin())); }
    out
}
/// Split polyline into dash "on" runs. DashArc2/DashSeg semantics: dashes
/// measured along polyline, first dash on, trailing partial dash kept.
pub fn dash_polyline(pts: &[Point], dash_on: f64, dash_off: f64) -> Vec<Vec<Point>> {
    if pts.len() < 2 || dash_on <= 0.0 { return vec![pts.to_vec()]; }
    let off = if dash_off < 0.0 { 0.0 } else { dash_off };
    let mut runs: Vec<Vec<Point>> = Vec::new();
    let mut cur: Vec<Point> = vec![pts[0]];
    let mut remaining = dash_on; let mut on = true;
    for w in pts.windows(2) {
        let mut a = w[0]; let b = w[1];
        let mut seg_left = a.distance(b);
        while seg_left > 1e-12 {
            if seg_left <= remaining + 1e-12 {
                if on { cur.push(b); }
                remaining -= seg_left; seg_left = 0.0; a = b;
                if remaining <= 1e-9 {
                    if on && cur.len() >= 2 { runs.push(std::mem::take(&mut cur)); }
                    on = !on; remaining = if on { dash_on } else { off };
                    if on { cur = vec![b]; }
                }
            } else {
                let t = remaining / seg_left;
                let cut = Point::new(a.x + (b.x-a.x)*t, a.y + (b.y-a.y)*t);
                if on { cur.push(cut); runs.push(std::mem::take(&mut cur)); }
                on = !on; remaining = if on { dash_on } else { off };
                if on { cur = vec![cut]; }
                seg_left = cut.distance(b); a = cut;
                if off == 0.0 && !on { on = true; remaining = dash_on; cur = vec![a]; }
            }
        }
    }
    if on && cur.len() >= 2 { runs.push(cur); }
    runs
}
/// Closed uniform cubic B-spline. Original balloon outline: control points
/// (corners + edge midpoints of text rect, inset by padding), smoothed.
/// per_seg = subdivisions per span; 0.3.17 app.rs used 8.
pub fn bspline_closed(ctrl: &[Point], per_seg: usize) -> Vec<Point> {
    if ctrl.len() < 3 { return ctrl.to_vec(); }
    let per = per_seg.max(1); let n = ctrl.len();
    let mut out = Vec::with_capacity(n*per);
    for i in 0..n {
        let p0 = ctrl[(i+n-1)%n]; let p1 = ctrl[i]; let p2 = ctrl[(i+1)%n]; let p3 = ctrl[(i+2)%n];
        for j in 0..per {
            let t = j as f64 / per as f64; let t2=t*t; let t3=t2*t;
            let b0=(1.0-t).powi(3)/6.0; let b1=(3.0*t3-6.0*t2+4.0)/6.0;
            let b2=(-3.0*t3+3.0*t2+3.0*t+1.0)/6.0; let b3=t3/6.0;
            out.push(Point::new(b0*p0.x+b1*p1.x+b2*p2.x+b3*p3.x, b0*p0.y+b1*p1.y+b2*p2.y+b3*p3.y));
        }
    }
    out
}
/// Sinusoidal wavy perturbation along closed polyline (whisper/thought).
/// scale = amplitude px. Phase from arc length, resolution-independent.
pub fn add_waves(pts: &[Point], scale: f64, waves: usize) -> Vec<Point> {
    if pts.is_empty() || scale == 0.0 { return pts.to_vec(); }
    let mut total=0.0; let mut cum=vec![0.0];
    for w in pts.windows(2) { total += w[0].distance(w[1]); cum.push(total); }
    if total==0.0 { return pts.to_vec(); }
    let waves = if waves==0 {12} else {waves} as f64;
    pts.iter().enumerate().map(|(i,p)| {
        let s=cum.get(i).copied().unwrap_or(total);
        let phase=2.0*PI*waves*s/total;
        let prev=pts[(i+pts.len()-1)%pts.len()]; let next=pts[(i+1)%pts.len()];
        let tangent=next.sub(prev).normalised(); let normal=Vector::new(-tangent.dy, tangent.dx);
        p.add(normal.scale(phase.sin()*scale))
    }).collect()
}
/// Spikes for shout balloons: every pitch px, push outward by depth along
/// local normal with triangular profile. Points between spikes stay on base.
pub fn add_spikes(pts: &[Point], pitch: f64, depth: f64) -> Vec<Point> {
    if pts.is_empty() || pitch<=0.0 || depth==0.0 { return pts.to_vec(); }
    let mut total=0.0; let mut cum=vec![0.0];
    for w in pts.windows(2) { total += w[0].distance(w[1]); cum.push(total); }
    if total==0.0 { return pts.to_vec(); }
    let centre=BBox::from_points(pts).map(|b| b.centre()).unwrap_or(Point::origin());
    pts.iter().enumerate().map(|(i,p)| {
        let s=cum.get(i).copied().unwrap_or(0.0);
        let frac=(s/pitch)-(s/pitch).floor();
        let tri=if frac<0.5 { frac*2.0 } else { 2.0-frac*2.0 };
        if tri<0.05 { return *p; }
        p.add(p.sub(centre).normalised().scale(tri*depth))
    }).collect()
}
/// Tail polygon: base_left/base_right -> tip with bend (0=straight).
/// Returns quad + Bezier outer edge for bent tails.
pub fn tail_polygon(base_left: Point, base_right: Point, tip: Point, bend: f64) -> Vec<Point> {
    let mid=Point::new((base_left.x+base_right.x)/2.0,(base_left.y+base_right.y)/2.0);
    let to_tip=tip.sub(mid); let perp=Vector::new(-to_tip.dy,to_tip.dx).normalised();
    let ctrl=tip.add(perp.scale(bend));
    let mut out=vec![base_left, base_right];
    for i in 1..=6 { let t=i as f64/6.0;
        let a=Point::new(mid.x+(ctrl.x-mid.x)*t, mid.y+(ctrl.y-mid.y)*t);
        let b=Point::new(ctrl.x+(tip.x-ctrl.x)*t, ctrl.y+(tip.y-ctrl.y)*t);
        out.push(Point::new(a.x+(b.x-a.x)*t, a.y+(b.y-a.y)*t));
    }
    out
}
/// Think-trail: 3 shrinking circles from thought balloon toward head.
/// Returns (centres, radii). Caller draws each as filled circle.
pub fn think_trail(base_centre: Point, tip: Point, bend: f64, base_radius: f64) -> (Vec<Point>, Vec<f64>) {
    let mut centres=Vec::new(); let mut radii=Vec::new();
    for i in 0..3 { let t=(i as f64+1.0)/4.0;
        let mid=Point::new(base_centre.x+(tip.x-base_centre.x)*t, base_centre.y+(tip.y-base_centre.y)*t);
        let perp=Vector::new(-(tip.y-base_centre.y), tip.x-base_centre.x).normalised();
        centres.push(mid.add(perp.scale(bend*t*(1.0-t)*4.0)));
        radii.push(base_radius*(1.0-t*0.7));
    }
    (centres, radii)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn angle_roundtrip() { for deg in [0.0,45.0,90.0,180.0,270.0,359.0] { let v=angle_to_vector(deg); assert!((vector_to_angle(v)-deg).abs()<1e-9); } }
    #[test] fn bspline_closed_returns_points() {
        let ctrl=vec![Point::new(0.0,0.0),Point::new(10.0,0.0),Point::new(10.0,10.0),Point::new(0.0,10.0)];
        let out=bspline_closed(&ctrl,8); assert_eq!(out.len(),32);
        let bb=BBox::from_points(&out).unwrap(); assert!(bb.min.x>=-1e-9 && bb.max.x<=10.0+1e-9);
    }
    #[test] fn arc_circle() { let pts=scan_arc(Point::new(0.0,0.0),10.0,10.0,0.0,90.0,30.0); assert_eq!(pts.len(),4); assert!((pts[0].x-10.0).abs()<1e-9); }
    #[test] fn tail_has_tip() { let t=tail_polygon(Point::new(0.0,0.0),Point::new(10.0,0.0),Point::new(5.0,20.0),0.0); assert!(t.iter().any(|p| (p.y-20.0).abs()<1e-9)); }
}
