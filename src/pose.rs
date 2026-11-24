fn clean_appears_as_name(s: &str) -> String { s.trim().trim_end_matches(['.', '?', ' ']).trim_end_matches(['.', '?']).to_string() }
// Pose / character codes — Microsoft Comic Chat 2.5 pose code.
//
// 2.5 sends (#GdddEdddMd) at the START of every message sent from it.
// G = gesture angle 000-359, E = emotion angle 000-359, M = mouth digit.
// Example: (#G000E045M1) is Coy with no gesture, mouth 1.
// Example: (#G090E090M1) is Bored with PointSelf.
// This is the current 2.5 form, not legacy, not superseded.
//
// The colon form (#G3:1E0:1M1) does not come from Microsoft source.
// It comes from the comic-text library. Never generate it, never parse
// it as a pose. If it arrives, leave it as plain text.
//
// CAST:Alice,Bob was also invented here and is never sent. Cast is
// local-only (panel grouping on this client). The wire never contains CAST.
//
// Character identity (separate line, sent on JOIN + when a new user joins,
// so newcomers learn our character):
//   # Appears as Trekkie2018.http://www.rebased.io/characters_for_comic_chat/Trekkie2018.AVB
//   # Appears as TIKI
//   — parsed from PRIVMSG starting with "# Appears as ". Not shown as a balloon.
//
// Balloon kind is NOT in the pose code. It is carried separately:
//   Say     -> PRIVMSG #chan :(#G... )text
//   Think   -> PRIVMSG #chan :(#G... )*thinks* text
//   Whisper -> PRIVMSG <nick> :(#G... )text           (private, not to channel)
//   Action  -> PRIVMSG #chan :\x01ACTION (#G... )text\x01  (CTCP ACTION, /me)
// See irc.rs for the send side, comic.rs for the display side.

use crate::comic::{Emotion, Gesture};
use std::f32::consts::PI;

// --- Original constants, avatar.h ---

pub const NEMOTIONS: usize = 8;
pub const MAXEMOPTS: usize = 10;

pub const EM_HAPPY: f32 = 0.0;
pub const EM_COY: f32 = PI / 4.0;
pub const EM_BORED: f32 = PI / 2.0;
pub const EM_SCARED: f32 = 3.0 * PI / 4.0;
pub const EM_SAD: f32 = PI;
pub const EM_ANGRY: f32 = 5.0 * PI / 4.0;
pub const EM_SHOUT: f32 = 3.0 * PI / 2.0;
pub const EM_LAUGH: f32 = 7.0 * PI / 4.0;
pub const EM_NEUTRAL: f32 = 0.0;

pub const EM_WAVE: f32 = 1001.0;
pub const EM_POINTOTHER: f32 = 1002.0;
pub const EM_POINTSELF: f32 = 1003.0;
pub const EM_DOUBLEPOINT: f32 = 1004.0;
pub const EM_SHRUG: f32 = 1005.0;
pub const EM_3QRWALK: f32 = 1006.0;
pub const EM_SIDEWALK: f32 = 1007.0;
pub const EM_3QFWALK: f32 = 1008.0;

// --- CEmotion, avatar.h ---

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CEmotion {
    pub m_intensity: f32,
    pub m_emotion: f32,
}

impl CEmotion {
    pub fn new(intensity: f32, emotion: f32) -> Self {
        Self { m_intensity: intensity, m_emotion: emotion }
    }
    pub fn set(&mut self, intensity: f32, emotion: f32) {
        self.m_intensity = intensity;
        self.m_emotion = emotion;
    }
    pub fn from_emotion(e: Emotion, intensity: f32) -> Self {
        if e == Emotion::Neutral {
            Self { m_intensity: 0.0, m_emotion: EM_NEUTRAL }
        } else {
            Self { m_intensity: intensity.clamp(0.0, 1.0), m_emotion: emotion_angle_rad(e) }
        }
    }
    pub fn from_gesture(g: Gesture, intensity: f32) -> Self {
        if g == Gesture::Neutral {
            Self { m_intensity: 0.0, m_emotion: EM_NEUTRAL }
        } else {
            let code = match g {
                Gesture::Wave => EM_WAVE,
                Gesture::PointOther => EM_POINTOTHER,
                Gesture::PointSelf => EM_POINTSELF,
                Gesture::DoublePoint => EM_DOUBLEPOINT,
                Gesture::Shrug => EM_SHRUG,
                Gesture::Walk3Q => EM_3QRWALK,
                Gesture::SideWalk => EM_SIDEWALK,
                Gesture::Walk3QF => EM_3QFWALK,
                Gesture::Neutral => EM_NEUTRAL,
            };
            Self { m_intensity: intensity.clamp(0.0, 1.0), m_emotion: code }
        }
    }
    pub fn to_emotion(&self) -> Emotion {
        if self.m_emotion > 7.0 {
            return Emotion::Neutral;
        }
        if self.m_emotion == EM_NEUTRAL && self.m_intensity == 0.0 {
            return Emotion::Neutral;
        }
        let deg = self.m_emotion.to_degrees().rem_euclid(360.0) as u16;
        let mouth = if self.m_intensity == 0.0 { 0 } else { 1 };
        angle_to_emotion_with_mouth(deg, mouth)
    }
    pub fn to_gesture(&self) -> Gesture {
        if self.m_emotion <= 7.0 {
            return Gesture::Neutral;
        }
        match self.m_emotion as u16 {
            1001 => Gesture::Wave,
            1002 => Gesture::PointOther,
            1003 => Gesture::PointSelf,
            1004 => Gesture::DoublePoint,
            1005 => Gesture::Shrug,
            1006 => Gesture::Walk3Q,
            1007 => Gesture::SideWalk,
            1008 => Gesture::Walk3QF,
            _ => Gesture::Neutral,
        }
    }
}

// --- CEmotionOpts, avatar.h ---

#[derive(Debug, Clone)]
pub struct CEmotionOpts {
    pub m_n_opts: u8,
    pub m_emotions: [CEmotion; MAXEMOPTS],
    pub m_priorities: [u8; MAXEMOPTS],
}

impl CEmotionOpts {
    pub fn new() -> Self {
        Self {
            m_n_opts: 0,
            m_emotions: [CEmotion::new(0.0, EM_NEUTRAL); MAXEMOPTS],
            m_priorities: [0; MAXEMOPTS],
        }
    }
    pub fn add(&mut self, emotion: f32, intensity: f32, priority: u8) {
        if (self.m_n_opts as usize) < MAXEMOPTS {
            let idx = self.m_n_opts as usize;
            self.m_emotions[idx] = CEmotion::new(intensity, emotion);
            self.m_priorities[idx] = priority;
            self.m_n_opts += 1;
            return;
        }
        let mut lowest = 0usize;
        for i in 1..MAXEMOPTS {
            if self.m_priorities[i] < self.m_priorities[lowest] {
                lowest = i;
            }
        }
        if priority > self.m_priorities[lowest] {
            self.m_emotions[lowest] = CEmotion::new(intensity, emotion);
            self.m_priorities[lowest] = priority;
        }
    }
    pub fn add_emotion(&mut self, e: Emotion, intensity: f32, priority: u8) {
        let ce = CEmotion::from_emotion(e, intensity);
        self.add(ce.m_emotion, ce.m_intensity, priority);
    }
    pub fn add_gesture(&mut self, g: Gesture, intensity: f32, priority: u8) {
        let ce = CEmotion::from_gesture(g, intensity);
        self.add(ce.m_emotion, ce.m_intensity, priority);
    }
}

impl Default for CEmotionOpts {
    fn default() -> Self { Self::new() }
}

// --- BodyRec / FaceRec / RBodyRec, avatar.h ---

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodyRec {
    pub pose_id: u16,
    pub emotion: f32,
    pub intensity: f32,
    pub x_cx: i16,
    pub y_cx: i16,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceRec {
    pub pose_id: u16,
    pub emotion: f32,
    pub intensity: f32,
    pub x_cx: i16,
    pub y_cx: i16,
    pub delta_x_cx: i16,
    pub delta_y_cx: i16,
    pub face_x: u8,
    pub face_y: u8,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RBodyRec {
    pub pose_id: u16,
    pub emotion: f32,
    pub intensity: f32,
    pub face_x: u8,
    pub face_y: u8,
}

// --- CPose, avatar.h ---

#[derive(Debug, Clone)]
pub struct CPose {
    pub pose_id: u16,
    pub m_dw_offsets: [u32; 3],
    pub m_by_formats: [u8; 3],
    pub m_by_palette_types: [u8; 3],
}

impl CPose {
    pub fn new(pose_id: u16, offsets: [u32; 3], formats: [u8; 3], palette_types: [u8; 3]) -> Self {
        Self { pose_id, m_dw_offsets: offsets, m_by_formats: formats, m_by_palette_types: palette_types }
    }
    pub fn get_drawing(&self) -> Option<u32> {
        if self.m_dw_offsets[0] != 0 { Some(self.m_dw_offsets[0]) } else { None }
    }
    pub fn get_mask(&self) -> Option<u32> {
        if self.m_dw_offsets[1] != 0 { Some(self.m_dw_offsets[1]) } else { None }
    }
    pub fn get_aura(&self) -> Option<u32> {
        if self.m_dw_offsets[2] != 0 { Some(self.m_dw_offsets[2]) } else { None }
    }
}

// --- Helpers ---

pub fn subtract_angles(a: f32, b: f32) -> f32 {
    let mut d = a - b;
    while d > PI { d -= 2.0 * PI; }
    while d < -PI { d += 2.0 * PI; }
    d
}

// --- CAvatarSimple, avatar.h / avatar.cpp ---

#[derive(Debug, Clone)]
pub struct CAvatarSimple {
    pub b_rec: Vec<RBodyRec>,
    pub m_n_bodies: i16,
    pub m_last_body: i16,
    pub avatar_id: u32,
}

impl CAvatarSimple {
    pub fn new(avatar_id: u32, b_rec: Vec<RBodyRec>) -> Self {
        let n = b_rec.len() as i16;
        Self { b_rec, m_n_bodies: n, m_last_body: -1, avatar_id }
    }
    pub fn get_body_index_from_emotion(&self, emotion: &CEmotion) -> i32 {
        if self.b_rec.is_empty() { return -1; }
        if emotion.m_emotion <= 2.0 * PI {
            let mut nearest_angle = 3.0 * PI;
            let mut intensity_of_nearest = 2.0f32;
            let mut b_index: i32 = -1;
            for i in 0..self.b_rec.len() {
                let this_angle = subtract_angles(self.b_rec[i].emotion, emotion.m_emotion).abs();
                if this_angle <= nearest_angle {
                    let delta_i = (emotion.m_intensity - self.b_rec[i].intensity).abs();
                    if this_angle == nearest_angle && delta_i >= intensity_of_nearest {
                        continue;
                    }
                    nearest_angle = this_angle;
                    intensity_of_nearest = delta_i;
                    b_index = i as i32;
                }
            }
            b_index
        } else {
            for i in 0..self.b_rec.len() {
                if emotion.m_emotion == self.b_rec[i].emotion {
                    return i as i32;
                }
            }
            -1
        }
    }
    pub fn get_body_from_emotion(&mut self, emotion: &CEmotion) -> Option<usize> {
        if self.b_rec.is_empty() { return None; }
        let n = self.b_rec.len() as i32;
        let mut nearest_angle = 3.0 * PI;
        let mut intensity_of_nearest = 2.0f32;
        let mut nearest_i: i32 = -1;
        for i in 0..n {
            let index = ((self.m_last_body as i32 + 1 + i).rem_euclid(n)) as usize;
            let rec = self.b_rec[index];
            if rec.emotion > 7.0 { continue; }
            let this_angle = subtract_angles(rec.emotion, emotion.m_emotion).abs();
            let is_first_neutral = rec.emotion == EM_NEUTRAL && rec.intensity == 0.0 && nearest_i == -1;
            if this_angle < PI / NEMOTIONS as f32 || is_first_neutral {
                let delta_i = if is_first_neutral && emotion.m_intensity > 0.0 {
                    1.5
                } else {
                    (emotion.m_intensity - rec.intensity).abs()
                };
                if delta_i < intensity_of_nearest {
                    nearest_angle = this_angle;
                    intensity_of_nearest = delta_i;
                    nearest_i = index as i32;
                }
            }
        }
        let _ = nearest_angle;
        if nearest_i >= 0 {
            self.m_last_body = nearest_i as i16;
            Some(nearest_i as usize)
        } else {
            self.set_body_neutral()
        }
    }
    pub fn get_body_from_emotion_opts(&mut self, opts: &mut CEmotionOpts) -> Option<usize> {
        let mut found: Option<usize> = None;
        loop {
            let mut min_priority: u8 = 0;
            let mut best_index: Option<usize> = None;
            for i in 0..opts.m_n_opts as usize {
                if opts.m_priorities[i] > min_priority {
                    min_priority = opts.m_priorities[i];
                    best_index = Some(i);
                }
            }
            let Some(bi) = best_index else { break; };
            if min_priority == 0 { break; }
            let emo = opts.m_emotions[bi];
            opts.m_priorities[bi] = 0;
            let b_index = self.get_body_index_from_emotion(&emo);
            if b_index >= 0 {
                found = Some(b_index as usize);
                self.m_last_body = b_index as i16;
                break;
            }
        }
        if found.is_none() {
            found = self.set_body_neutral();
        }
        found
    }
    pub fn set_body_neutral(&mut self) -> Option<usize> {
        let mut c = self.m_last_body;
        let n = self.b_rec.len() as i32;
        if n == 0 { return None; }
        for _ in 0..n {
            c = ((c as i32 + 1).rem_euclid(n)) as i16;
            let rec = self.b_rec[c as usize];
            if rec.emotion == EM_NEUTRAL && rec.intensity == 0.0 {
                self.m_last_body = c as i16;
                return Some(c as usize);
            }
        }
        self.m_last_body = 0;
        Some(0)
    }
    pub fn set_neutral(&mut self) -> Option<usize> {
        self.set_body_neutral()
    }
}

// --- CAvatarComplex, avatar.h / avatar.cpp ---

#[derive(Debug, Clone)]
pub struct CAvatarComplex {
    pub f_rec: Vec<FaceRec>,
    pub b_rec: Vec<BodyRec>,
    pub n_faces: i16,
    pub n_torsos: i16,
    pub m_last_face: i16,
    pub m_last_torso: i16,
}

impl CAvatarComplex {
    pub fn new(f_rec: Vec<FaceRec>, b_rec: Vec<BodyRec>) -> Self {
        let nf = f_rec.len() as i16;
        let nt = b_rec.len() as i16;
        Self { f_rec, b_rec, n_faces: nf, n_torsos: nt, m_last_face: -1, m_last_torso: -1 }
    }
    pub fn get_head_and_body_from_emotion(&self, emotion: &CEmotion) -> (i32, i32) {
        let mut f_index: i32 = -1;
        let mut t_index: i32 = -1;
        if emotion.m_emotion <= 2.0 * PI {
            let mut nearest_angle = 3.0 * PI;
            let mut intensity_of_nearest = 2.0f32;
            for i in 0..self.f_rec.len() {
                let this_angle = subtract_angles(self.f_rec[i].emotion, emotion.m_emotion).abs();
                if this_angle <= nearest_angle {
                    let delta_i = (emotion.m_intensity - self.f_rec[i].intensity).abs();
                    if this_angle == nearest_angle && delta_i >= intensity_of_nearest {
                        continue;
                    }
                    nearest_angle = this_angle;
                    intensity_of_nearest = delta_i;
                    f_index = i as i32;
                }
            }
        } else {
            for i in 0..self.b_rec.len() {
                if emotion.m_emotion == self.b_rec[i].emotion {
                    t_index = i as i32;
                    break;
                }
            }
        }
        (f_index, t_index)
    }
    pub fn get_body_from_emotion(&mut self, emotion: &CEmotion) -> (Option<usize>, Option<usize>) {
        let mut nearest_angle = 3.0 * PI;
        let mut intensity_of_nearest = 2.0f32;
        let mut face_idx: Option<usize> = None;
        for i in 0..self.f_rec.len() {
            let this_angle = subtract_angles(self.f_rec[i].emotion, emotion.m_emotion).abs();
            if this_angle <= nearest_angle {
                let delta_i = (emotion.m_intensity - self.f_rec[i].intensity).abs();
                if this_angle == nearest_angle && delta_i >= intensity_of_nearest {
                    continue;
                }
                nearest_angle = this_angle;
                intensity_of_nearest = delta_i;
                face_idx = Some(i);
            }
        }
        if let Some(fi) = face_idx {
            self.m_last_face = fi as i16;
        }
        let mut torso_idx: Option<usize> = None;
        let mut intensity_of_nearest_t = 2.0f32;
        let n = self.b_rec.len() as i32;
        if n > 0 {
            for i in 0..n {
                let index = ((self.m_last_torso as i32 + 1 + i).rem_euclid(n)) as usize;
                let rec = self.b_rec[index];
                if rec.emotion > 7.0 { continue; }
                let this_angle = subtract_angles(rec.emotion, emotion.m_emotion).abs();
                if this_angle < PI / NEMOTIONS as f32 || (rec.emotion == EM_NEUTRAL && rec.intensity == 0.0) {
                    let delta_i = (emotion.m_intensity - rec.intensity).abs();
                    if delta_i < intensity_of_nearest_t {
                        intensity_of_nearest_t = delta_i;
                        torso_idx = Some(index);
                    }
                }
            }
        }
        if let Some(ti) = torso_idx {
            self.m_last_torso = ti as i16;
        } else {
            torso_idx = self.set_torso_neutral();
        }
        if face_idx.is_none() {
            face_idx = self.set_face_neutral();
        }
        (face_idx, torso_idx)
    }
    pub fn set_face_neutral(&mut self) -> Option<usize> {
        let mut c = self.m_last_face;
        let n = self.f_rec.len() as i32;
        if n == 0 { return None; }
        for _ in 0..n {
            c = ((c as i32 + 1).rem_euclid(n)) as i16;
            let rec = self.f_rec[c as usize];
            if rec.emotion == EM_NEUTRAL && rec.intensity == 0.0 {
                self.m_last_face = c as i16;
                return Some(c as usize);
            }
        }
        self.m_last_face = 0;
        Some(0)
    }
    pub fn set_torso_neutral(&mut self) -> Option<usize> {
        let mut c = self.m_last_torso;
        let n = self.b_rec.len() as i32;
        if n == 0 { return None; }
        for _ in 0..n {
            c = ((c as i32 + 1).rem_euclid(n)) as i16;
            let rec = self.b_rec[c as usize];
            if rec.emotion == EM_NEUTRAL && rec.intensity == 0.0 {
                self.m_last_torso = c as i16;
                return Some(c as usize);
            }
        }
        self.m_last_torso = 0;
        Some(0)
    }
}

// --- PoseCode: the wire pose + local selection indices ---

#[derive(Debug, Clone, PartialEq)]
pub struct PoseCode {
    pub face_index: i16,
    pub torso_index: i16,
    pub requested: bool,
    pub emotion: Emotion,
    pub gesture: Gesture,
    pub intensity: f32,
    pub cast: Vec<String>,
}

impl PoseCode {
    pub fn from_emotion_gesture(emotion: Emotion, gesture: Gesture, strength: f32) -> Self {
        let face_index = emotion_angle_index(emotion);
        let torso_index = gesture_index(gesture);
        Self { face_index, torso_index, requested: false, emotion, gesture, intensity: strength.clamp(0.0, 1.0), cast: Vec::new() }
    }
    pub fn with_cast(mut self, cast: Vec<String>) -> Self { self.cast = cast; self }
    pub fn to_code_string(&self) -> String {
        let g = gesture_angle_deg(self.gesture);
        let e = emotion_angle_deg(self.emotion);
        let m = if self.emotion == Emotion::Neutral { 0 } else { 1 };
        format!("(#G{:03}E{:03}M{})", g, e, m)
    }

    // Single CEmotion -> (face_emo, face_int_10). Gesture wins if non-Neutral,
    // else emotion. Neutral is 9, Happy is 1 - they must NOT both be 1.
    // Old code mapped Neutral to (1,0) and Happy to (1,9), both looked up w==1 and both returned first Happy body.
    fn single_face_emo(&self) -> (u8, u8) {
        // Intensity from wheel strength / text heuristic, 0..9. Used to pick closest AVB body for this emotion.
        let int_10 = (self.intensity * 10.0).round() as i32;
        let int_10 = int_10.clamp(0, 9) as u8;
        if self.gesture != Gesture::Neutral {
            let emo = match self.gesture {
                Gesture::Wave => 10,
                Gesture::PointOther => 11,
                Gesture::PointSelf => 12,
                Gesture::DoublePoint => 13,
                Gesture::Shrug => 14,
                _ => 9,
            };
            return (emo, int_10.max(7));
        }
        match self.emotion {
            Emotion::Neutral => (9, 0),
            Emotion::Happy => (1, int_10),
            Emotion::Coy => (2, int_10),
            Emotion::Bored => (3, int_10),
            Emotion::Scared => (4, int_10),
            Emotion::Sad => (5, int_10),
            Emotion::Angry => (6, int_10),
            Emotion::Shout => (7, int_10),
            Emotion::Laugh => (8, int_10),
        }
    }

    pub fn to_code_string_for_avb(&self, avb_name: &str) -> String {
        self.to_code_string_for_avb_cycled(avb_name, 0)
    }
    pub fn to_code_string_for_avb_cycled(&self, avb_name: &str, cycle: usize) -> String {
        let (face_emo, face_int_10) = self.single_face_emo();
        let body = crate::pose_lookup::body_for_pose_cycled(face_emo, face_int_10, avb_name, cycle);
        crate::pose_lookup::wire_for_body(body, face_emo, face_int_10, avb_name)
    }

    pub fn to_wire_string_for_avb(&self, pose_style: &str, avb_name: &str) -> String {
        self.to_wire_string_for_avb_cycled(pose_style, avb_name, 0)
    }
    pub fn to_wire_string_for_avb_cycled(&self, pose_style: &str, avb_name: &str, cycle: usize) -> String {
        if pose_style == "none" { return String::new(); }
        self.to_code_string_for_avb_cycled(avb_name, cycle)
    }
    /// Wire string honouring config `comic.pose_style`.
    /// - "none" -> "" (send nothing, for plain IRC peers)
    /// - anything else ("2.5", "old", "angle", "") -> canonical 2.5 angle code at start.
    /// Colon form is never produced, for any style.
    pub fn to_wire_string(&self, pose_style: &str) -> String {
        // FIX Wire-Log-File v2: log to real debug.log via crate::log
        if pose_style == "none" {
            return String::new();
        }
        self.to_code_string()
    }
    pub fn emotion(&self) -> Emotion { self.emotion }
    pub fn gesture(&self) -> Gesture { self.gesture }
    pub fn strength(&self) -> f32 { self.intensity }
    pub fn to_c_emotions(&self) -> (CEmotion, CEmotion) {
        (CEmotion::from_emotion(self.emotion, self.intensity), CEmotion::from_gesture(self.gesture, self.intensity))
    }
    pub fn from_c_emotions(face: CEmotion, torso: CEmotion, intensity: f32) -> Self {
        let emotion = face.to_emotion();
        let gesture = torso.to_gesture();
        Self::from_emotion_gesture(emotion, gesture, intensity)
    }
}

/// Emotion -> face-record index: 0 = Neutral, 1..8 = wheel order.
pub fn emotion_angle_index(e: Emotion) -> i16 {
    match e { Emotion::Neutral => 0, Emotion::Happy => 1, Emotion::Coy => 2, Emotion::Bored => 3, Emotion::Scared => 4, Emotion::Sad => 5, Emotion::Angry => 6, Emotion::Shout => 7, Emotion::Laugh => 8 }
}
/// Gesture -> torso-record index: 0 = Neutral, 1..8 = specials in avatar.h order.
pub fn gesture_index(g: Gesture) -> i16 {
    match g { Gesture::Neutral => 0, Gesture::Wave => 1, Gesture::PointSelf => 2, Gesture::PointOther => 3, Gesture::DoublePoint => 4, Gesture::Shrug => 5, Gesture::Walk3Q => 6, Gesture::SideWalk => 7, Gesture::Walk3QF => 8 }
}

pub fn emotion_angle_deg(e: Emotion) -> u16 {
    match e { Emotion::Neutral => 0, Emotion::Happy => 0, Emotion::Coy => 45, Emotion::Bored => 90, Emotion::Scared => 135, Emotion::Sad => 180, Emotion::Angry => 225, Emotion::Shout => 270, Emotion::Laugh => 315 }
}
pub fn emotion_angle_rad(e: Emotion) -> f32 {
    match e {
        Emotion::Neutral => EM_NEUTRAL,
        Emotion::Happy => EM_HAPPY,
        Emotion::Coy => EM_COY,
        Emotion::Bored => EM_BORED,
        Emotion::Scared => EM_SCARED,
        Emotion::Sad => EM_SAD,
        Emotion::Angry => EM_ANGRY,
        Emotion::Shout => EM_SHOUT,
        Emotion::Laugh => EM_LAUGH,
    }
}
pub fn gesture_angle_deg(g: Gesture) -> u16 {
    match g {
        Gesture::Neutral => 0,
        Gesture::Wave => 45,
        Gesture::PointSelf => 90,
        Gesture::PointOther => 135,
        Gesture::DoublePoint => 180,
        Gesture::Shrug => 225,
        Gesture::Walk3Q => 270,
        Gesture::SideWalk => 315,
        Gesture::Walk3QF => 359,
    }
}

/// Nearest MS Chat emotion for an angle. Mouth-agnostic compat: E000 -> Neutral.
/// Use angle_to_emotion_with_mouth when the M digit is known.
pub fn angle_to_emotion(angle: u16) -> Emotion {
    angle_to_emotion_with_mouth(angle, 0)
}
pub fn angle_to_emotion_with_mouth(angle: u16, mouth: u8) -> Emotion {
    let a = angle % 360;
    if a < 23 {
        if mouth == 0 { return Emotion::Neutral; }
        return Emotion::Happy;
    }
    let step = ((a + 22) / 45) as i16;
    match step { 1 => Emotion::Coy, 2 => Emotion::Bored, 3 => Emotion::Scared, 4 => Emotion::Sad, 5 => Emotion::Angry, 6 => Emotion::Shout, 7 => Emotion::Laugh, _ => Emotion::Happy }
}
/// Nearest MS Chat gesture for an angle. G359 is Walk3QF (360 wrapped).
pub fn angle_to_gesture(angle: u16) -> Gesture {
    let a = angle % 360;
    if a == 359 { return Gesture::Walk3QF; }
    if a < 23 { return Gesture::Neutral; }
    let step = ((a + 22) / 45) as i16;
    match step { 1 => Gesture::Wave, 2 => Gesture::PointSelf, 3 => Gesture::PointOther, 4 => Gesture::DoublePoint, 5 => Gesture::Shrug, 6 => Gesture::Walk3Q, 7 => Gesture::SideWalk, _ => Gesture::Walk3QF }
}

fn parse_pose_inner(inner: &str) -> Option<PoseCode> {
    let (code_part, cast_part) = match inner.find(" CAST:") {
        Some(idx) => (&inner[..idx], Some(&inner[idx + 6..])),
        None => (inner, None),
    };
    let cast: Vec<String> = cast_part.map(|s| s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()).unwrap_or_default();
    // Do NOT reject ':' - ':' is faceEmo 10 (Wave), ';' is 11 (PointOther) in real MS Chat wire.
    let after_g = code_part.strip_prefix("#G")?;
    if after_g.len() < 7 { return None; }
    let b = after_g.as_bytes();
    let body = (b[0] as u16).saturating_sub(48);
    let _torso_emo = (b[1] as u16).saturating_sub(48);
    let _torso_int = (b[2] as u16).saturating_sub(48);
    let rest = &after_g[3..];
    let rest = rest.strip_prefix('E')?;
    if rest.len() < 4 { return None; }
    let rb = rest.as_bytes();
    let _face_idx = (rb[0] as u16).saturating_sub(48);
    let face_emo = (rb[1] as u16).saturating_sub(48);
    let face_int = (rb[2] as u16).saturating_sub(48);
    let mut idx = 3usize;
    let mut _frozen = false;
    if rb.len() > idx && rb[idx] == b'R' { _frozen = true; idx += 1; }
    // Terminator: M<mode> OR R (already consumed) OR end-of-string. R alone is valid - Coy/Sad end with R, no M.
    if idx < rest.len() {
        let rest2 = &rest[idx..];
        if let Some(after_m) = rest2.strip_prefix('M') {
            let _mouth: u8 = after_m.chars().next()?.to_digit(10)? as u8;
        } else if !rest2.is_empty() {
            return None;
        }
    }
    let (emotion, gesture) = match face_emo {
        0 => (Emotion::Neutral, Gesture::Neutral),
        1 => if face_int == 0 { (Emotion::Neutral, Gesture::Neutral) } else { (Emotion::Happy, Gesture::Neutral) },
        2 => (Emotion::Coy, Gesture::Neutral),
        3 => (Emotion::Bored, Gesture::Neutral),
        4 => (Emotion::Scared, Gesture::Neutral),
        5 => (Emotion::Sad, Gesture::Neutral),
        6 => (Emotion::Angry, Gesture::Neutral),
        7 => (Emotion::Shout, Gesture::Neutral),
        8 => (Emotion::Laugh, Gesture::Neutral),
        9 => (Emotion::Neutral, Gesture::PointSelf),
        10 => (Emotion::Neutral, Gesture::Wave),
        11 => (Emotion::Neutral, Gesture::PointOther),
        _ => (Emotion::Neutral, Gesture::Neutral),
    };
    let _ = body;
    let mut p = PoseCode::from_emotion_gesture(emotion, gesture, face_int as f32 / 10.0);
    p.cast = cast;
    Some(p)
}

// --- OEC ---

/// v2.5 OEC code: real Microsoft Comic Chat 2.5 sends a leading
/// `(OEC:<n>M<m>)` at the start of every message. `<n>` mapping is still unverified,
/// so we do NOT map it to emotion/gesture yet, and we NEVER generate it.
/// This only strips it so it never shows as spoken text in a bubble.
pub fn strip_oec_prefix(text: &str) -> (String, Option<(u32, u32)>) {
    let trimmed = text.trim_start();
    let Some(rest) = trimmed.strip_prefix("(OEC:") else { return (text.to_string(), None); };
    let Some(close) = rest.find(')') else { return (text.to_string(), None); };
    let inner = &rest[..close];
    let Some((n_str, m_str)) = inner.split_once('M') else { return (text.to_string(), None); };
    if n_str.is_empty() || m_str.is_empty() { return (text.to_string(), None); }
    if !n_str.bytes().all(|b| b.is_ascii_digit()) || !m_str.bytes().all(|b| b.is_ascii_digit()) { return (text.to_string(), None); }
    let n: u32 = match n_str.parse() { Ok(v) => v, Err(_) => return (text.to_string(), None) };
    let m: u32 = match m_str.parse() { Ok(v) => v, Err(_) => return (text.to_string(), None) };
    let after = rest[close + 1..].trim_start().to_string();
    (after, Some((n, m)))
}

// --- Parsing ---

/// Parse a pose code. 2.5 form is LEADING: "(#G000E045M1) hello".
/// Trailing form "hello (#G090E090M1)" is also accepted for old logs.
/// Colon form (#G2:8E0:8M1) is never parsed as a pose. Returns (clean, pose).
pub fn parse_pose_code(text: &str) -> Option<(String, PoseCode)> {
    let trimmed_full = text.trim();
    if trimmed_full.starts_with("(#G") {
        if let Some(end_rel) = trimmed_full.find(')') {
            let inner = &trimmed_full[3..end_rel];
            let inner_full = format!("#G{inner}");
            // ':' as VALUE is legal (Wave faceEmo 10). Reject only invented #G<d>:<d>E<d>:<d>M<d>.
            if inner_full.len() == 11 && inner_full.as_bytes()[3] == b':' && inner_full.as_bytes()[7] == b':' && inner_full.as_bytes()[0] == b'#' && inner_full.as_bytes()[1] == b'G' && inner_full.as_bytes()[5] == b'E' && inner_full.as_bytes()[9] == b'M' { return None; }
            if let Some(pose) = parse_pose_inner(&inner_full) {
                let clean = trimmed_full[end_rel + 1..].trim().to_string();
                return Some((clean, pose));
            } else {
                // ':' as VALUE is legal (Wave faceEmo 10). Reject only invented #G<d>:<d>E<d>:<d>M<d>.
                if inner.len() == 11 && inner.as_bytes()[3] == b':' && inner.as_bytes()[7] == b':' && inner.as_bytes()[0] == b'#' && inner.as_bytes()[1] == b'G' && inner.as_bytes()[5] == b'E' && inner.as_bytes()[9] == b'M' { return None; }
            }
        }
    }
    let trimmed = text.trim_end();
    let start = match trimmed.rfind("(#G") { Some(s) => s, None => { return None; } };
    let end = trimmed[start..].find(')')? + start;
    if !trimmed[end + 1..].trim().is_empty() { return None; }
    let inner = &trimmed[start + 1..end];
    if !inner.starts_with("#G") { return None; }
    // ':' as VALUE is legal (Wave faceEmo 10). Reject only invented #G<d>:<d>E<d>:<d>M<d>.
    if inner.len() == 11 && inner.as_bytes()[3] == b':' && inner.as_bytes()[7] == b':' && inner.as_bytes()[0] == b'#' && inner.as_bytes()[1] == b'G' && inner.as_bytes()[5] == b'E' && inner.as_bytes()[9] == b'M' { return None; }
    let pose = parse_pose_inner(inner)?;
    let clean = trimmed[..start].trim_end().to_string();
    Some((clean, pose))
}

// --- Appears as ---

/// Build: "# Appears as Trekkie2018.http://www.rebased.io/characters_for_comic_chat/Trekkie2018.AVB"
pub fn appears_as_line(name: &str, url: Option<&str>) -> String {
    match url {
        Some(u) if !u.is_empty() => format!("# Appears as {name}.{u}"),
        _ => format!("# Appears as {name}"),
    }
}

/// Parse: "# Appears as Trekkie2018.http://.../Trekkie2018.AVB" -> (Trekkie2018, Some(url))
/// Also: "# Appears as TIKI" -> (TIKI, None)
pub fn parse_appears_as(text: &str) -> Option<(String, Option<String>)> {
    let t = text.trim();
    let rest = match t.strip_prefix("# Appears as ") { Some(r) => r, None => { return None; } };
    let rest = rest.trim();
    if rest.is_empty() { return None; }
    if let Some(idx) = rest.find("http://").or_else(|| rest.find("https://")) {
        let name_part = rest[..idx].trim_end_matches('.').trim();
        let url_part = rest[idx..].trim();
        if name_part.is_empty() || url_part.is_empty() { return None; }
        return Some((name_part.to_string(), Some(url_part.to_string())));
    }
    let name = rest.trim_end_matches(['.', '?']).trim();
    if name.is_empty() { return None; }
    Some((name.to_string(), None))
}

/// Background share line, same shape as Appears as but for backgrounds:
/// "# Background as Cafe.http://.../Cafe.BGB"
pub fn background_as_line(name: &str, url: Option<&str>) -> String {
    match url {
        Some(u) if !u.is_empty() => format!("# Background as {name}.{u}"),
        _ => format!("# Background as {name}"),
    }
}
pub fn parse_background_as(text: &str) -> Option<(String, Option<String>)> {
    let t = text.trim();
    let rest = match t.strip_prefix("# Background as ") { Some(r) => r, None => { return None; } };
    let rest = rest.trim();
    if let Some(idx) = rest.find("http://").or_else(|| rest.find("https://")) {
        let name_part = rest[..idx].trim_end_matches('.').trim();
        return Some((name_part.to_string(), Some(rest[idx..].trim().to_string())));
    }
    let name = rest.trim_end_matches(['.', '?']).trim();
    if name.is_empty() { return None; }
    Some((name.to_string(), None))
}

/// Display text: strip OEC prefix, then drop the single space that
/// followed it (if any). User's own leading spaces are preserved when
/// there was no OEC prefix.
pub fn parse_display_text(s: &str) -> String {
    let (stripped, _oec) = strip_oec_prefix(s);
    if stripped.len() != s.len() {
        stripped.strip_prefix(' ').unwrap_or(&stripped).to_string()
    } else {
        stripped
    }
}

/// OEC helpers (parse-only, never generate OEC).
pub fn has_oec_prefix(s: &str) -> bool {
    strip_oec_prefix(s).1.is_some()
}
pub fn oec_parts(s: &str) -> Option<(u32, u32)> {
    strip_oec_prefix(s).1
}

/// CTCP helper: strip leading/trailing 0x01 delimiters.
pub fn strip_ctcp_delimiters(s: &str) -> String {
    s.trim().trim_matches('\u{1}').to_string()
}

/// Wire lines (sent on JOIN + when new user joins):
/// "# Appears as Bob http://x/Bob.AVB"
/// "# Background as Scene http://x/Scene.BGB"
/// If url is None, line is "# Appears as Bob" (no URL).
pub fn appears_as_line_for(name: &str, url: Option<&str>) -> String {
    match url {
        Some(u) => format!("# Appears as {} {}", name, u),
        None => format!("# Appears as {}", name),
    }
}
pub fn background_as_line_for(name: &str, url: Option<&str>) -> String {
    match url {
        Some(u) => format!("# Background as {} {}", name, u),
        None => format!("# Background as {}", name),
    }
}

/// Parse wire lines (inverse of appears_as_line_for / background_as_line_for):
/// "# Appears as Bob http://x/Bob.AVB" -> Some(("Bob", Some("http://x/Bob.AVB")))
/// "# Appears as Bob" -> Some(("Bob", None))
pub fn parse_appears_as_line(s: &str) -> Option<(String, Option<String>)> {
    let t = s.trim();
    let rest = t.strip_prefix("# Appears as ")?;
    let (name, url) = match rest.find(' ') {
        Some(idx) => (&rest[..idx], Some(rest[idx + 1..].trim().to_string())),
        None => (rest, None),
    };
    let name = name.trim_end_matches(['.', '?']).trim();
    if name.is_empty() { return None; }
    Some((name.to_string(), url.filter(|u| !u.is_empty())))
}
pub fn parse_background_as_line(s: &str) -> Option<(String, Option<String>)> {
    let t = s.trim();
    let rest = t.strip_prefix("# Background as ")?;
    let (name, url) = match rest.find(' ') {
        Some(idx) => (&rest[..idx], Some(rest[idx + 1..].trim().to_string())),
        None => (rest, None),
    };
    let name = name.trim_end_matches(['.', '?']).trim();
    if name.is_empty() { return None; }
    Some((name.to_string(), url.filter(|u| !u.is_empty())))
}

/// Strip "# Appears as " / "# Background as " prefix, if present.
pub fn strip_appears_prefix(s: &str) -> String {
    let t = s.trim();
    t.strip_prefix("# Appears as ").map(|r| r.to_string()).unwrap_or_else(|| t.to_string())
}
pub fn strip_background_prefix(s: &str) -> String {
    let t = s.trim();
    t.strip_prefix("# Background as ").map(|r| r.to_string()).unwrap_or_else(|| t.to_string())
}

/// Pose helper: 2.5 pose code looks like "(#G090E090M1)" — starts
/// "(#G", contains "E" and "M", ends ")". Used to decide whether to strip
/// a trailing code from an incoming line.
pub fn is_pose_code(s: &str) -> bool {
    let t = s.trim();
    t.starts_with("(#G") && t.ends_with(')') && t.contains('E') && t.contains('M') && !t.contains(':')
}
pub fn is_legacy_pose_code(s: &str) -> bool { is_pose_code(s) }

/// Strip a trailing 2.5 pose code "(#G...)" from a line, if present.
/// "hello (#G090E090M1)" -> "hello". No code -> unchanged (trimmed).
pub fn strip_pose_code(s: &str) -> String {
    let t = s.trim_end();
    if let Some(idx) = t.rfind("(#G") {
        let tail = &t[idx..];
        if is_pose_code(tail) {
            return t[..idx].trim_end().to_string();
        }
    }
    t.to_string()
}
pub fn strip_legacy_pose_code(s: &str) -> String { strip_pose_code(s) }

pub fn emotion_for_pose(s: &str) -> &'static str {
    let t = s.to_ascii_lowercase();
    if t.contains("happy") || t.contains("smile") || t.contains("laugh") { return "happy"; }
    if t.contains("sad") || t.contains("cry") || t.contains("frown") { return "sad"; }
    if t.contains("angry") || t.contains("mad") || t.contains("rage") { return "angry"; }
    if t.contains("surpris") || t.contains("shock") || t.contains("wow") { return "scared"; }
    "neutral"
}


// --- Step 2: Pose cycle + triggers ---

/// Cycle through neutral poses only, unless another pose is triggered.
#[derive(Debug, Clone)]
pub struct PoseCycler { pub last_body: i16 }

impl PoseCycler {
    pub fn new() -> Self { Self { last_body: -1 } }
    pub fn reset(&mut self) { self.last_body = -1; }
    pub fn next_neutral_simple(&mut self, avatar: &mut CAvatarSimple) -> Option<usize> {
        avatar.m_last_body = self.last_body;
        let r = avatar.set_body_neutral();
        if let Some(idx) = r { self.last_body = idx as i16; }
        r
    }
    pub fn next_neutral_complex(&mut self, avatar: &mut CAvatarComplex) -> (Option<usize>, Option<usize>) {
        avatar.m_last_face = self.last_body;
        avatar.m_last_torso = self.last_body;
        let f = avatar.set_face_neutral();
        let t = avatar.set_torso_neutral();
        if let Some(idx) = t { self.last_body = idx as i16; }
        (f, t)
    }
}

impl Default for PoseCycler { fn default() -> Self { Self::new() } }

/// Triggers (GOALS): sentence starting with I -> PointSelf, starting with you -> PointOther, all caps -> Shout.
 pub fn trigger_for_text(text: &str) -> (Option<Emotion>, Option<Gesture>) {
    let trimmed = text.trim();
    if trimmed.is_empty() { return (None, None); }
    let lower = trimmed.to_lowercase();
    let letters: Vec<char> = trimmed.chars().filter(|c| c.is_alphabetic()).collect();
    let all_caps = !letters.is_empty() && letters.len() > 3 && letters.iter().all(|c| c.is_uppercase());
    // All-caps / !! is Balloon::Shout, NOT avatar Emotion::Shout. Old code returned Shout emotion and cycled avatar to random angry pose - see comic::analyze. Pose stays wheel/neutral here.
    if all_caps || trimmed.ends_with("!!!") || trimmed.ends_with("!!") { return (None, None); }
    if lower == "i" || lower.starts_with("i ") || lower.starts_with("i'") || lower.starts_with("i,") { return (None, Some(Gesture::PointSelf)); }
    if lower == "you" || lower.starts_with("you ") || lower.starts_with("you,") || lower.starts_with("you'") { return (None, Some(Gesture::PointOther)); }
    (None, None)
}

/// Build PoseCode for a message: wheel (if frozen) else wheel as default, then trigger overrides.
 pub fn pose_for_text(text: &str, wheel: (Emotion, f32), wheel_frozen: bool) -> PoseCode {
    let (mut emotion, mut gesture, strength) = if wheel_frozen {
        let g = match wheel.0 { Emotion::Angry => Gesture::Shrug, Emotion::Laugh => Gesture::Wave, Emotion::Happy => Gesture::Wave, _ => Gesture::Neutral };
        (wheel.0, g, wheel.1)
    } else { (wheel.0, Gesture::Neutral, wheel.1) };
    let (e_trig, g_trig) = trigger_for_text(text);
    if let Some(e) = e_trig { emotion = e; }
    if let Some(g) = g_trig { gesture = g; }
    PoseCode::from_emotion_gesture(emotion, gesture, strength)
}

#[cfg(test)]
mod step1_pose_tests {
    use super::*;
    use crate::comic::{Emotion, Gesture};
    #[test]
    fn to_code_string_is_25_angle_at_start() {
        let p = PoseCode::from_emotion_gesture(Emotion::Coy, Gesture::Neutral, 1.0);
        assert_eq!(p.to_code_string(), "(#G000E045M1)");
        let p2 = PoseCode::from_emotion_gesture(Emotion::Bored, Gesture::PointSelf, 1.0);
        assert_eq!(p2.to_code_string(), "(#G090E090M1)");
        let p3 = PoseCode::from_emotion_gesture(Emotion::Coy, Gesture::PointOther, 1.0);
        assert_eq!(p3.to_code_string(), "(#G135E045M1)");
        let p4 = PoseCode::from_emotion_gesture(Emotion::Neutral, Gesture::Neutral, 0.0);
        assert_eq!(p4.to_code_string(), "(#G000E000M0)");
        let p5 = PoseCode::from_emotion_gesture(Emotion::Happy, Gesture::Wave, 1.0);
        assert_eq!(p5.to_code_string(), "(#G045E000M1)");
    }
            #[test] fn parse_leading_25() {
        // Verified 2.5 leading Coy: body 16 '@', faceEmo 2, int 9, frozen R.
        let (clean, p) = parse_pose_code("(#G@10E029R) hello").unwrap();
        assert_eq!(clean, "hello");
        assert_eq!(p.emotion(), Emotion::Coy);
        // Verified leading Wave: faceEmo 10 ':'.
        let (clean2, p2) = parse_pose_code("(#G210E0:8M1) hello").unwrap();
        assert_eq!(clean2, "hello");
        assert_eq!(p2.gesture(), Gesture::Wave);
    }
            #[test] fn parse_trailing_old_logs() {
        // Trailing verified wire, not old angle G090/E090 (invented).
        // Geordi Bored: body 5, faceEmo 3, int 9.
        let (clean, p) = parse_pose_code("hello (#G510E039M1)").unwrap();
        assert_eq!(clean, "hello");
        assert_eq!(p.emotion(), Emotion::Bored);
        // Trekkie Sad: body 17 'A', faceEmo 5, int 9.
        let (clean2, p2) = parse_pose_code("hello (#GA10E059R)").unwrap();
        assert_eq!(clean2, "hello");
        assert_eq!(p2.emotion(), Emotion::Sad);
    }
    #[test]
    fn colon_never_parsed() {
        assert!(parse_pose_code("hi (#G2:8E0:8M1)").is_none());
        assert!(parse_pose_code("(#G1:1E1:1M1) hello").is_none());
        assert!(parse_pose_code("(#1E1:1M0) hi").is_none());
    }
            #[test] fn roundtrip() {
        // Round-trip via verified char wire, not canonical_wire_for colon form.
        // Coy wire G@10E029R -> Coy, Wave wire G210E0:8M1 -> Wave.
        let (clean, p) = parse_pose_code("(#G@10E029R) hello").unwrap();
        assert_eq!(clean, "hello");
        assert_eq!(p.emotion(), Emotion::Coy);
        let (clean2, p2) = parse_pose_code("(#G210E0:8M1) hello").unwrap();
        assert_eq!(clean2, "hello");
        assert_eq!(p2.gesture(), Gesture::Wave);
    }
            #[test] fn neutral_vs_happy_distinguished_by_m() {
        // Verified: same faceEmo 1, distinguished by body+intensity, NOT by M (M is mouth).
        let (_, p_n) = parse_pose_code("(#G010E010M1) hi").unwrap();
        assert_eq!(p_n.emotion(), Emotion::Neutral);
        let (_, p_h) = parse_pose_code("(#G610E019M1) hi").unwrap();
        assert_eq!(p_h.emotion(), Emotion::Happy);
        // M0 vs M1 must NOT change emotion.
        let (_, p_m0) = parse_pose_code("(#G610E019M0) hi").unwrap();
        assert_eq!(p_m0.emotion(), Emotion::Happy);
    }
    #[test]
    fn c_emotion_roundtrip() {
        let ce = CEmotion::from_emotion(Emotion::Coy, 0.8);
        assert_eq!(ce.to_emotion(), Emotion::Coy);
        let cg = CEmotion::from_gesture(Gesture::PointOther, 1.0);
        assert_eq!(cg.to_gesture(), Gesture::PointOther);
        assert_eq!(cg.m_emotion, EM_POINTOTHER);
    }
    #[test]
    fn simple_get_body_from_emotion_picks_closest() {
        let recs = vec![
            RBodyRec { pose_id: 1, emotion: EM_NEUTRAL, intensity: 0.0, face_x: 0, face_y: 0 },
            RBodyRec { pose_id: 2, emotion: EM_COY, intensity: 1.0, face_x: 0, face_y: 0 },
            RBodyRec { pose_id: 3, emotion: EM_HAPPY, intensity: 1.0, face_x: 0, face_y: 0 },
        ];
        let mut av = CAvatarSimple::new(1, recs);
        let emo = CEmotion::from_emotion(Emotion::Coy, 1.0);
        let idx = av.get_body_from_emotion(&emo).unwrap();
        assert_eq!(av.b_rec[idx].pose_id, 2);
    }
    #[test]
    fn cast_never_on_wire() {
        let p = PoseCode::from_emotion_gesture(Emotion::Happy, Gesture::Wave, 1.0).with_cast(vec!["Alice".to_string()]);
        assert_eq!(p.cast, vec!["Alice"]);
        assert_eq!(p.to_code_string(), "(#G045E000M1)");
        assert!(!p.to_code_string().contains("CAST"));
    }
    #[test]
    fn cycler_cycles_neutral_only() {
        let recs = vec![
            RBodyRec { pose_id: 1, emotion: EM_NEUTRAL, intensity: 0.0, face_x: 0, face_y: 0 },
            RBodyRec { pose_id: 2, emotion: EM_COY, intensity: 1.0, face_x: 0, face_y: 0 },
            RBodyRec { pose_id: 3, emotion: EM_NEUTRAL, intensity: 0.0, face_x: 0, face_y: 0 },
        ];
        let mut av = CAvatarSimple::new(1, recs);
        let mut cycler = PoseCycler::new();
        let a = cycler.next_neutral_simple(&mut av).unwrap();
        let b = cycler.next_neutral_simple(&mut av).unwrap();
        let c = cycler.next_neutral_simple(&mut av).unwrap();
        assert_eq!(av.b_rec[a].pose_id, 1);
        assert_eq!(av.b_rec[b].pose_id, 3);
        assert_eq!(av.b_rec[c].pose_id, 1);
        assert_ne!(av.b_rec[a].pose_id, 2);
    }
    #[test]
    fn triggers_i_you_caps() {
        let (e, g) = trigger_for_text("I think this is right");
        assert_eq!(g, Some(Gesture::PointSelf));
        assert_eq!(e, None);
        let (e2, g2) = trigger_for_text("you should see this");
        assert_eq!(g2, Some(Gesture::PointOther));
        assert_eq!(e2, None);
        let (e3, _) = trigger_for_text("HELLO EVERYONE");
        assert_eq!(e3, None); // caps = balloon Shout, avatar stays Neutral
        let (e4, g4) = trigger_for_text("hello there");
        assert_eq!((e4, g4), (None, None));
    }
    #[test]
    fn pose_for_text_uses_trigger() {
        let p = pose_for_text("I did it", (Emotion::Neutral, 0.0), false);
        assert_eq!(p.gesture(), Gesture::PointSelf);
        let p2 = pose_for_text("HELLO!!!", (Emotion::Neutral, 0.0), false);
        assert_eq!(p2.emotion(), Emotion::Neutral); // balloon Shout, not avatar Shout
    }

}

// G-as-POSE-index: parse wire, do not use E alone.
pub use crate::g_pose_index::{parse_wire, expected_file_emotion, verify_gesture};

// FIX Wire-Coy: Coy must NOT map to Neutral wire (1,0). Coy is (2,9/8/7) -> G 16/24/25.
// Neutral plains cycle 0,4,8,19 (file 9). Do not collapse Coy -> Neutral in to_wire_string.
