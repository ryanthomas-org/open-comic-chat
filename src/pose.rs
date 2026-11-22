// Pose / character codes — the in-band codes Comic Chat sent with every message.
//
// CORRECTED in 0.3.17 (was wrong in 0.3):
//   Legacy (1.0-2.1, parse-only, visible to non-Comic Chat users as "garbage"):
//     (#G010E010M1)  — G = gesture angle 000-359, E = expression angle 000-359, M = mouth 0-9
//   v2.5 with pose_style="none": sends NO pose code. Gesture / expression are
//     inferred locally from text + emotion wheel. Plain text.
//   Default pose_style="old": sends OldAngle "(#GxxxExxxM1)" LEADING, computed
//     from wheel+text via to_old_angle_string(). See Step 112 in app.rs.
//   The (#G2:8E0:8M1) colon form was INVENTED in this client (0.3) and
//     is supported by NO other client. It is never generated anymore.
//     It is still PARSED for compat with 0.3 builds, but never re-emitted.
//   CAST:Alice,Bob was also invented here and is never sent. Cast is
//     local-only (panel grouping on this client). The wire `raw` never
//     contains CAST.
//
// Character identity (separate line, sent on JOIN + when a new user joins,
// so newcomers learn our character — 0.3 only sent it on our own JOIN):
//   # Appears as Trekkie2018.http://www.rebased.io/characters_for_comic_chat/Trekkie2018.AVB
//   # Appears as TIKI
//   — parsed from PRIVMSG starting with "# Appears as ". Not shown as a balloon.
//
// Balloon kind is NOT in the pose code. It is carried separately:
//   Say     -> PRIVMSG #chan :text
//   Think   -> PRIVMSG #chan :*thinks* text   (visible as "thinks" to text clients)
//   Whisper -> PRIVMSG <nick> :text           (private, not to channel)
//   Action  -> PRIVMSG #chan :\x01ACTION text\x01  (CTCP ACTION, /me)
// See irc.rs for the send side, comic.rs for the display side.

use crate::comic::{Emotion, Gesture};

// --- Step 1 bridge: canonical v2.5 wire form via comic-text ---
// Local wheel (comic.rs) has 9 emotions; comic-text has the 8-emotion
// MS wheel. Mapping is lossy where there is no exact counterpart.
// The local wheel is UNCHANGED; this only translates at the wire boundary.
// Encode: Coy -> Contemptuous, Bored -> Disgusted, Scared -> Afraid,
//   Shout -> Angry, Laugh -> Happy.
// Decode: Surprised -> Scared, Afraid -> Scared, Disgusted -> Bored,
//   Contemptuous -> Coy.
// Gesture number on the wire = local torso index (gesture_index):
//   Neutral -> no G (None), Wave -> G1, PointSelf -> G2, PointOther -> G3,
//   DoublePoint -> G4, Shrug -> G5, Walk3Q -> G6, SideWalk -> G7, Walk3QF -> G8.
// Local PoseCode has no mouth field, so the wire mouth digit is not stored.
pub fn local_emotion_to_wire(e: Emotion) -> comic_text::Emotion {
    match e {
        Emotion::Neutral => comic_text::Emotion::Neutral,
        Emotion::Happy => comic_text::Emotion::Happy,
        Emotion::Coy => comic_text::Emotion::Contemptuous,
        Emotion::Bored => comic_text::Emotion::Disgusted,
        Emotion::Scared => comic_text::Emotion::Afraid,
        Emotion::Sad => comic_text::Emotion::Sad,
        Emotion::Angry => comic_text::Emotion::Angry,
        Emotion::Shout => comic_text::Emotion::Angry,
        Emotion::Laugh => comic_text::Emotion::Happy,
    }
}
pub fn wire_emotion_to_local(e: comic_text::Emotion) -> Emotion {
    match e {
        comic_text::Emotion::Neutral => Emotion::Neutral,
        comic_text::Emotion::Happy => Emotion::Happy,
        comic_text::Emotion::Sad => Emotion::Sad,
        comic_text::Emotion::Angry => Emotion::Angry,
        comic_text::Emotion::Afraid => Emotion::Scared,
        comic_text::Emotion::Surprised => Emotion::Scared,
        comic_text::Emotion::Disgusted => Emotion::Bored,
        comic_text::Emotion::Contemptuous => Emotion::Coy,
    }
}
pub fn local_gesture_to_wire(g: Gesture) -> Option<comic_text::Gesture> {
    match g {
        Gesture::Neutral => None,
        Gesture::Wave => Some(comic_text::Gesture(1)),
        Gesture::PointSelf => Some(comic_text::Gesture(2)),
        Gesture::PointOther => Some(comic_text::Gesture(3)),
        Gesture::DoublePoint => Some(comic_text::Gesture(4)),
        Gesture::Shrug => Some(comic_text::Gesture(5)),
        Gesture::Walk3Q => Some(comic_text::Gesture(6)),
        Gesture::SideWalk => Some(comic_text::Gesture(7)),
        Gesture::Walk3QF => Some(comic_text::Gesture(8)),
    }
}
pub fn wire_gesture_to_local(g: Option<comic_text::Gesture>) -> Gesture {
    match g {
        None => Gesture::Neutral,
        Some(comic_text::Gesture(1)) => Gesture::Wave,
        Some(comic_text::Gesture(2)) => Gesture::PointSelf,
        Some(comic_text::Gesture(3)) => Gesture::PointOther,
        Some(comic_text::Gesture(4)) => Gesture::DoublePoint,
        Some(comic_text::Gesture(5)) => Gesture::Shrug,
        Some(comic_text::Gesture(6)) => Gesture::Walk3Q,
        Some(comic_text::Gesture(7)) => Gesture::SideWalk,
        Some(comic_text::Gesture(8)) => Gesture::Walk3QF,
        Some(_) => Gesture::Neutral,
    }
}
/// Canonical v2.5 wire form for one emotion+gesture, via comic-text.
/// Example: canonical_wire_for(Happy, Wave) == "(#G1:1E1:1M1)".
/// Never emits the invented colon form (#G2:8E0:8M1).
pub fn canonical_wire_for(emotion: Emotion, gesture: Gesture) -> String {
    let p = comic_text::PoseCode {
        gesture: local_gesture_to_wire(gesture),
        emotion: local_emotion_to_wire(emotion),
        mouth: comic_text::Mouth(1),
    };
    p.to_wire()
}
/// Parse a LEADING canonical code via comic-text, converting back to the
/// local PoseCode shape. Returns None when no leading canonical code is
/// present (old-angle codes are handled by the fallback in parse_pose_code).
/// Intensity is 1.0: the wire intensity digit is 1 for user-selected poses.
fn emotion_from_face_index(d: u8) -> Option<Emotion> {
    match d {
        0 => Some(Emotion::Neutral),
        1 => Some(Emotion::Happy),
        2 => Some(Emotion::Coy),
        3 => Some(Emotion::Bored),
        4 => Some(Emotion::Scared),
        5 => Some(Emotion::Sad),
        6 => Some(Emotion::Angry),
        7 => Some(Emotion::Shout),
        8 => Some(Emotion::Laugh),
        _ => None,
    }
}
fn gesture_from_torso_index(d: u8) -> Option<Gesture> {
    match d {
        0 => Some(Gesture::Neutral),
        1 => Some(Gesture::Wave),
        2 => Some(Gesture::PointSelf),
        3 => Some(Gesture::PointOther),
        4 => Some(Gesture::DoublePoint),
        5 => Some(Gesture::Shrug),
        6 => Some(Gesture::Walk3Q),
        7 => Some(Gesture::SideWalk),
        8 => Some(Gesture::Walk3QF),
        _ => None,
    }
}
fn parse_canonical_leading(text: &str) -> Option<(String, PoseCode)> {
    // Parse E/G digits directly as face/torso indices (0..8). Do NOT go
    // through comic_text::Emotion — its table is the old wrong set
    // (Surprised/Afraid/Disgusted/Contemptuous, 0..7) and collapses
    // Shout->Angry, Laugh->Happy, Coy->E7, so real MS Chat E2 (Coy)
    // parsed as Sad and E8 (Laugh) failed entirely.
    let (wire_pose, rest) = comic_text::parse_pose_code(text)?;
    // Re-extract the E digit from the raw wire prefix — comic_text has
    // already validated the shape, we just need the digit it mis-mapped.
    let trimmed = text.trim_start();
    let end = trimmed.find(')')?;
    let inner = &trimmed[2..end];
    let mut e_digit: Option<u8> = None;
    for part in inner.split(':') {
        if let Some(e_pos) = part.find('E') {
            let digits: String = part[e_pos + 1..].chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(d) = digits.parse::<u8>() {
                e_digit = Some(d);
            }
        }
    }
    let emotion = e_digit.and_then(emotion_from_face_index).unwrap_or_else(|| wire_emotion_to_local(wire_pose.emotion));
    let gesture = wire_gesture_to_local(wire_pose.gesture);
    // Gesture digits are already correct via wire_gesture_to_local (G1=Wave
    // etc.), but keep the direct path as the source of truth if present.
    let gesture = {
        let mut g_digit: Option<u8> = None;
        for part in inner.split(':') {
            let pp = part.trim();
            if pp.starts_with('G') {
                if let Ok(d) = pp[1..].parse::<u8>() {
                    g_digit = Some(d);
                }
            }
        }
        g_digit.and_then(gesture_from_torso_index).unwrap_or(gesture)
    };
    let pose = PoseCode::from_emotion_gesture(emotion, gesture, 1.0);
    Some((rest.to_string(), pose))
}




// MSCHAT_POSE_STATE_0317: pose is STATE, not a wire string.
// MS Chat sends no (#G... ) code in 2.5. The renderer picks a face
// record + torso record from the AVB by emotion angle + intensity
// (avatar.h CAvatarComplex::SetEmotions). This struct carries only
// the indices that selection needs, plus the local cast.
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
    /// v2.5 "none" form — always empty. Kept for compat / tests.
    /// Actual send uses to_wire_string(style).
    pub fn to_code_string(&self) -> String { String::new() }
    /// OldAngle legacy code: "(#GxxxExxxM1)" computed from emotion/gesture.
    /// Never includes CAST. Angles invert angle_to_emotion / angle_to_gesture.
    pub fn to_old_angle_string(&self) -> String {
        format!("(#G{:03}E{:03}M1)", gesture_to_angle(self.gesture), emotion_to_angle(self.emotion))
    }
    /// Canonical v2.5 wire form: "(#G<g>:1E<e>:1M1)" using the SAME
    /// face_index / torso_index the local renderer uses for PoseEntry
    /// (Neutral 0, Happy 1, Coy 2, Bored 3, Scared 4, Sad 5, Angry 6,
    /// Shout 7, Laugh 8 / Neutral 0, Wave 1, PointSelf 2, PointOther 3,
    /// DoublePoint 4, Shrug 5, Walk3Q 6, SideWalk 7, Walk3QF 8).
    /// Neutral/Neutral -> "" (v2.5 PoseStyle::None: plain message sends
    /// no pose code). Coy+Neutral -> "(#G0:1E2:1M1)", Neutral+Wave
    /// ("hello") -> "(#G1:1E0:1M1)".
    pub fn to_canonical_string(&self) -> String {
        if self.face_index == 0 && self.torso_index == 0 {
            return String::new();
        }
        format!("(#G{}:1E{}:1M1)", self.torso_index, self.face_index)
    }
    /// Wire string for a pose_style: "old-angle"/"legacy"/"1.0" -> OldAngle
    /// (1.0-2.1 only), "none" -> "" (explicitly no codes), anything else
    /// (including "old" — the previous default — "", "canonical", "v2.5")
    /// -> canonical v2.5. "old" previously meant OldAngle, which MS Chat
    /// 2.5 does not parse (it falls back to Neutral); it now means
    /// "send pose codes" in the form 2.5 understands.
    pub fn to_wire_string(&self, pose_style: &str) -> String {
        match pose_style.trim().to_ascii_lowercase().as_str() {
            "old-angle" | "legacy" | "1.0" | "1.0-2.1" | "2.1" => self.to_old_angle_string(),
            "none" => String::new(),
            _ => self.to_canonical_string(),
        }
    }
    pub fn emotion(&self) -> Emotion { self.emotion }
    pub fn gesture(&self) -> Gesture { self.gesture }
    pub fn strength(&self) -> f32 { self.intensity }
}

/// Emotion -> face-record index: 0 = Neutral, 1..8 = wheel order above.
pub fn emotion_angle_index(e: Emotion) -> i16 {
    match e { Emotion::Neutral => 0, Emotion::Happy => 1, Emotion::Coy => 2, Emotion::Bored => 3, Emotion::Scared => 4, Emotion::Sad => 5, Emotion::Angry => 6, Emotion::Shout => 7, Emotion::Laugh => 8 }
}
/// Gesture -> torso-record index: 0 = Neutral, 1..8 = specials in avatar.h order.
pub fn gesture_index(g: Gesture) -> i16 {
    match g { Gesture::Neutral => 0, Gesture::Wave => 1, Gesture::PointSelf => 2, Gesture::PointOther => 3, Gesture::DoublePoint => 4, Gesture::Shrug => 5, Gesture::Walk3Q => 6, Gesture::SideWalk => 7, Gesture::Walk3QF => 8 }
}
/// Emotion -> OldAngle degrees. Inverse of angle_to_emotion.
/// Neutral 0, Coy 45, Bored 90, Scared 135, Sad 180, Angry 225, Shout 270, Laugh 315, Happy 350 (step 8 -> Happy).
pub fn emotion_to_angle(e: Emotion) -> u16 {
    match e { Emotion::Neutral => 0, Emotion::Happy => 350, Emotion::Coy => 45, Emotion::Bored => 90, Emotion::Scared => 135, Emotion::Sad => 180, Emotion::Angry => 225, Emotion::Shout => 270, Emotion::Laugh => 315 }
}
/// Gesture -> OldAngle degrees. Inverse of angle_to_gesture.
/// Neutral 0, Wave 45, PointSelf 90, PointOther 135, DoublePoint 180, Shrug 225, Walk3Q 270, SideWalk 315, Walk3QF 350 (step 8).
pub fn gesture_to_angle(g: Gesture) -> u16 {
    match g { Gesture::Neutral => 0, Gesture::Wave => 45, Gesture::PointSelf => 90, Gesture::PointOther => 135, Gesture::DoublePoint => 180, Gesture::Shrug => 225, Gesture::Walk3Q => 270, Gesture::SideWalk => 315, Gesture::Walk3QF => 350 }
}
/// Nearest MS Chat emotion for a legacy angle. Legacy-only, never generated.
pub fn angle_to_emotion(angle: u16) -> Emotion {
    let a = angle % 360;
    if a < 23 { return Emotion::Neutral; }
    let step = ((a + 22) / 45) as i16;
    match step { 1 => Emotion::Coy, 2 => Emotion::Bored, 3 => Emotion::Scared, 4 => Emotion::Sad, 5 => Emotion::Angry, 6 => Emotion::Shout, 7 => Emotion::Laugh, _ => Emotion::Happy }
}
/// Nearest MS Chat gesture for a legacy angle. Legacy-only, never generated.
pub fn angle_to_gesture(angle: u16) -> Gesture {
    let a = angle % 360;
    if a < 23 { return Gesture::Neutral; }
    let step = ((a + 22) / 45) as i16;
    match step { 1 => Gesture::Wave, 2 => Gesture::PointSelf, 3 => Gesture::PointOther, 4 => Gesture::DoublePoint, 5 => Gesture::Shrug, 6 => Gesture::Walk3Q, 7 => Gesture::SideWalk, _ => Gesture::Walk3QF }
}

/// Legacy (#GxxxExxxMx) parser: kept so old logs / old clients still
/// parse. Colon form was invented here and is never parsed.
fn parse_pose_inner(inner: &str) -> Option<PoseCode> {
    let (code_part, cast_part) = match inner.find(" CAST:") {
        Some(idx) => (&inner[..idx], Some(&inner[idx + 6..])),
        None => (inner, None),
    };
    let cast: Vec<String> = cast_part.map(|s| s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()).unwrap_or_default();
    if code_part.contains(':') { return None; }
    let after_g = code_part.strip_prefix("#G")?;
    if after_g.len() < 7 { return None; }
    let g_angle: u16 = after_g[..3].parse().ok()?;
    let rest = &after_g[3..];
    let rest = rest.strip_prefix('E')?;
    if rest.len() < 4 { return None; }
    let e_angle: u16 = rest[..3].parse().ok()?;
    let rest = &rest[3..];
    let rest = rest.strip_prefix('M')?;
    let _mouth: u8 = rest.chars().next()?.to_digit(10)? as u8;
    let gesture = angle_to_gesture(g_angle);
    let emotion = angle_to_emotion(e_angle);
    let mut p = PoseCode::from_emotion_gesture(emotion, gesture, 1.0);
    p.cast = cast;
    Some(p)
}









// --- Index <-> Angle <-> Emotion/Gesture tables ---
// Index order matches the emotion wheel order in comic.rs (8 + Neutral).
// Neutral is index 0 in New form; the 8 wheel emotions are 1-8.
// Old form uses wheel angle: 0 = Neutral, then 45° steps around the wheel.






/// v2.5 OEC code: real Microsoft Comic Chat 2.5 sends a leading
/// `(OEC:<n>M<m>)` at the start of every message (screenshot in review
/// finding 4.1). `<n>` mapping is still unverified, so we do NOT map it
/// to emotion/gesture yet, and we NEVER generate it.
/// This only strips it so it never shows as spoken text in a bubble.
/// `raw_with_code` in comic.rs still keeps the original for Plain Text view.
/// Returns (clean, Some((n, m))) when a leading OEC code is present,
/// else (original, None). Only a LEADING code is stripped; a mid-message
/// "(OEC:...)" is left alone so a user quoting one is not mangled.
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

/// Parse a trailing pose code. Returns (clean_text, pose) if found.
/// Old form only: "hello (#G090E090M1)" or "hello (#G090E090M1 CAST:Alice,Bob)".
/// Colon form (#G2:8E0:8M1) is invented, never sent, never parsed. Code must be trailing: anything after ")" means no strip.
pub fn parse_pose_code(text: &str) -> Option<(String, PoseCode)> {
    let trimmed_full = text.trim();
    // Step 1: try canonical LEADING form via comic-text first
    // ((#G1:1E1:1M1) hello, bare (#1E1:1M0), colon compat).
    // Falls through to legacy old-angle handling below.
    if let Some((clean, pose)) = parse_canonical_leading(text) {
        return Some((clean, pose));
    }
    // Leading code: "(#G090E090M1) hey" - web clients send it at the start. Strip it, keep the rest.
    if trimmed_full.starts_with("(#G") {
        if let Some(end_rel) = trimmed_full.find(')') {
            let inner = &trimmed_full[3..end_rel];
            let inner_full = format!("#G{inner}");
            if let Some(pose) = parse_pose_inner(&inner_full) {
                let clean = trimmed_full[end_rel + 1..].trim().to_string();
                return Some((clean, pose));
            }
        }
    }
    let trimmed = text.trim_end();
    // Find last '(' that starts a pose code
    // Silent: this runs per message / per frame. Logging every miss was idle spam.
    let start = match trimmed.rfind("(#G") { Some(s) => s, None => { return None; } };
    let end = trimmed[start..].find(')')? + start;
    // Trailing only: anything after the closing paren means this is not a trailing pose code. Do not strip.
    if !trimmed[end + 1..].trim().is_empty() { return None; }
    let inner = &trimmed[start + 1..end]; // "#G..."
    if !inner.starts_with("#G") { return None; }
    let pose = match parse_pose_inner(inner) { Some(p) => p, None => { return None; } };
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
/// Also: "# Appears as Name. http://..." (space after dot) -> handled.
pub fn parse_appears_as(text: &str) -> Option<(String, Option<String>)> {
    let t = text.trim();
    let rest = match t.strip_prefix("# Appears as ") { Some(r) => r, None => { return None; } };
    let rest = rest.trim();
    if rest.is_empty() { return None; }
    // URL starts at "http://" or "https://"
    if let Some(idx) = rest.find("http://").or_else(|| rest.find("https://")) {
        let name_part = rest[..idx].trim_end_matches('.').trim();
        let url_part = rest[idx..].trim();
        if name_part.is_empty() || url_part.is_empty() { return None; }
        return Some((name_part.to_string(), Some(url_part.to_string())));
    }
    // No URL — whole rest is the name (may contain spaces? take first token + rest as name)
    Some((rest.to_string(), None))
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
    Some((rest.to_string(), None))
}




/// Display text: strip OEC prefix, then drop the single space that
/// followed it (if any). User's own leading spaces are preserved when
/// there was no OEC prefix.
pub fn parse_display_text(s: &str) -> String {
    // Real strip_oec_prefix returns (text, pose) — we only need text.
    let (stripped, _oec) = strip_oec_prefix(s);
    if stripped.len() != s.len() {
        stripped.strip_prefix(' ').unwrap_or(&stripped).to_string()
    } else {
        stripped
    }
}



/// OEC helpers (parse-only, never generate OEC — spec).
/// has_oec_prefix("(OEC:1M2) hi") == true
/// oec_parts("(OEC:1M2) hi") == Some((1, 2))
pub fn has_oec_prefix(s: &str) -> bool {
    strip_oec_prefix(s).1.is_some()
}
pub fn oec_parts(s: &str) -> Option<(u32, u32)> {
    strip_oec_prefix(s).1
}







/// Pose helper: legacy old-style code looks like "(#G090E090M1)" — starts
/// "(#G", contains "E" and "M", ends ")". New v2.5 sends NO code for Say.
/// Used to decide whether to strip a trailing code from an incoming line.
pub fn is_legacy_pose_code(s: &str) -> bool {
    let t = s.trim();
    t.starts_with("(#G") && t.ends_with(')') && t.contains('E') && t.contains('M')
}



/// Strip a trailing legacy pose code "(#G...)" from a line, if present.
/// "hello (#G090E090M1)" -> "hello". No code -> unchanged (trimmed).
/// Uses is_legacy_pose_code (Step 67) for the check.
pub fn strip_legacy_pose_code(s: &str) -> String {
    let t = s.trim_end();
    if let Some(idx) = t.rfind("(#G") {
        let tail = &t[idx..];
        if is_legacy_pose_code(tail) {
            return t[..idx].trim_end().to_string();
        }
    }
    t.to_string()
}







/// CTCP helper: strip leading/trailing 0x01 delimiters.
/// Uses char literal for single codepoint (correct) — see Step 69b fix.
/// "\x01VERSION\x01" -> "VERSION", "hello" -> "hello".
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
/// "hello" -> None
pub fn parse_appears_as_line(s: &str) -> Option<(String, Option<String>)> {
    let t = s.trim();
    let rest = t.strip_prefix("# Appears as ")?;
    let (name, url) = match rest.find(' ') {
        Some(idx) => (&rest[..idx], Some(rest[idx + 1..].trim().to_string())),
        None => (rest, None),
    };
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
    if name.is_empty() { return None; }
    Some((name.to_string(), url.filter(|u| !u.is_empty())))
}



/// Strip "# Appears as " / "# Background as " prefix, if present.
/// "# Appears as Bob http://x" -> "Bob http://x". "hello" -> "hello".
pub fn strip_appears_prefix(s: &str) -> String {
    let t = s.trim();
    t.strip_prefix("# Appears as ").map(|r| r.to_string()).unwrap_or_else(|| t.to_string())
}
pub fn strip_background_prefix(s: &str) -> String {
    let t = s.trim();
    t.strip_prefix("# Background as ").map(|r| r.to_string()).unwrap_or_else(|| t.to_string())
}















































































pub fn emotion_for_pose(s: &str) -> &'static str { let t = s.to_ascii_lowercase(); if t.contains("happy") || t.contains("smile") || t.contains("laugh") { return "happy"; } if t.contains("sad") || t.contains("cry") || t.contains("frown") { return "sad"; } if t.contains("angry") || t.contains("mad") || t.contains("rage") { return "angry"; } if t.contains("surpris") || t.contains("shock") || t.contains("wow") { return "surprised"; } "neutral" }

#[cfg(test)]
mod cleanup_0317_pose_state {
    use super::*;
    use crate::comic::{Emotion, Gesture};
    #[test]
    fn none_sends_nothing() {
        let pose = PoseCode::from_emotion_gesture(Emotion::Happy, Gesture::Wave, 0.8);
        assert_eq!(pose.to_code_string(), "");
        assert_eq!(pose.face_index, 1);
        assert_eq!(pose.torso_index, 1);
        assert_eq!(pose.strength(), 0.8);
    }
    #[test]
    fn legacy_angle_maps_to_ms_chat_tables() {
        // G090 -> PointSelf (step 2), E090 -> Bored (step 3 in EM order: Happy 0, Coy 45, Bored 90).
        let (clean, p) = parse_pose_code("hello (#G090E090M1)").unwrap();
        assert_eq!(clean, "hello");
        assert_eq!(p.gesture(), Gesture::PointSelf);
        assert_eq!(p.emotion(), Emotion::Bored);
        assert!(parse_pose_code("hi (#G2:8E0:8M1)").is_none());
    }
    #[test]
    fn cast_never_on_wire() {
        let pose = PoseCode::from_emotion_gesture(Emotion::Happy, Gesture::Wave, 1.0)
            .with_cast(vec!["Alice".to_string()]);
        assert_eq!(pose.cast, vec!["Alice"]);
        assert_eq!(pose.to_code_string(), "");
    }
    #[test]
    fn old_angle_roundtrip() {
        let pose = PoseCode::from_emotion_gesture(Emotion::Happy, Gesture::Wave, 1.0);
        assert_eq!(pose.to_old_angle_string(), "(#G045E350M1)");
        let (clean, p) = parse_pose_code("(#G045E350M1) hello").unwrap();
        assert_eq!(clean, "hello");
        assert_eq!(p.emotion(), Emotion::Happy);
        assert_eq!(p.gesture(), Gesture::Wave);
        let neutral = PoseCode::from_emotion_gesture(Emotion::Neutral, Gesture::Neutral, 1.0);
        assert_eq!(neutral.to_old_angle_string(), "(#G000E000M1)");
        let bored_self = PoseCode::from_emotion_gesture(Emotion::Bored, Gesture::PointSelf, 1.0);
        assert_eq!(bored_self.to_old_angle_string(), "(#G090E090M1)");
    }
    #[test]
    fn wire_string_dispatch() {
        let pose = PoseCode::from_emotion_gesture(Emotion::Happy, Gesture::Wave, 1.0);
        // "old" (previous default) now means canonical v2.5, not OldAngle —
        // MS Chat 2.5 does not parse OldAngle, it falls back to Neutral.
        assert_eq!(pose.to_wire_string("old"), "(#G1:1E1:1M1)");
        assert_eq!(pose.to_wire_string(""), "(#G1:1E1:1M1)");
        assert_eq!(pose.to_wire_string("canonical"), "(#G1:1E1:1M1)");
        assert_eq!(pose.to_wire_string("legacy"), "(#G045E350M1)");
        assert_eq!(pose.to_wire_string("old-angle"), "(#G045E350M1)");
        assert_eq!(pose.to_wire_string("none"), "");
        // Neutral/Neutral -> no code in canonical (v2.5 PoseStyle::None).
        let neutral = PoseCode::from_emotion_gesture(Emotion::Neutral, Gesture::Neutral, 1.0);
        assert_eq!(neutral.to_wire_string("old"), "");
        assert_eq!(neutral.to_wire_string(""), "");
        assert_eq!(neutral.to_wire_string("canonical"), "");
        assert_eq!(neutral.to_wire_string("none"), "");
    }
}

#[cfg(test)]
mod step1_canonical_bridge_tests {
    use super::{canonical_wire_for, parse_pose_code};
    use crate::comic::{Emotion, Gesture};
    #[test] fn canonical_wire_shape() {
        assert_eq!(canonical_wire_for(Emotion::Happy, Gesture::Wave), "(#G1:1E1:1M1)");
        assert_eq!(canonical_wire_for(Emotion::Neutral, Gesture::Neutral), "(#1E0:1M1)");
        assert_eq!(canonical_wire_for(Emotion::Sad, Gesture::PointSelf), "(#G2:1E2:1M1)");
    }
    #[test] fn parse_leading_canonical() {
        let (clean, p) = parse_pose_code("(#G1:1E1:1M1) hello").unwrap();
        assert_eq!(clean, "hello");
        assert_eq!(p.gesture(), Gesture::Wave);
        assert_eq!(p.emotion(), Emotion::Happy);
        assert_eq!(p.face_index, 1);
        assert_eq!(p.torso_index, 1);
    }
    #[test] fn parse_leading_bare() {
        let (clean, p) = parse_pose_code("(#1E1:1M0) hi").unwrap();
        assert_eq!(clean, "hi");
        assert_eq!(p.emotion(), Emotion::Happy);
        assert_eq!(p.gesture(), Gesture::Neutral);
    }
    #[test] fn roundtrip() {
        let wire = canonical_wire_for(Emotion::Happy, Gesture::Wave);
        let (clean, p) = parse_pose_code(&format!("{wire} hello")).unwrap();
        assert_eq!(clean, "hello");
        assert_eq!(p.emotion(), Emotion::Happy);
        assert_eq!(p.gesture(), Gesture::Wave);
    }
    #[test] fn old_trailing_still_works() {
        let (clean, p) = parse_pose_code("hello (#G090E090M1)").unwrap();
        assert_eq!(clean, "hello");
        assert_eq!(p.gesture(), Gesture::PointSelf);
        assert_eq!(p.emotion(), Emotion::Bored);
    }
    #[test] fn trailing_colon_still_rejected() {
        assert!(parse_pose_code("hi (#G2:8E0:8M1)").is_none());
    }
}
