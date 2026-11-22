//! Text, pose codes, OEC / formatting, Appears-as, Semantic analysis.
//! Codifies format/chatprot, textpose.cpp, semantic.cpp, textview.cpp/.h,
//! and the pose handling in comic-chat-rs 0.3.17 src/pose.rs + src/comic.rs.
//!
//! Wire format (authoritative, from the Microsoft source):
//! * A message that carries a pose is sent as:
//!   `(#G<gesture> : <digit><expr><digit><mouth> ) <text>`
//!   — the canonical v2.5 form is `(#G3:1E0:1M1) hello` (note: NO space
//!   after `#`, ONE space after `G<n>` colon, then digit+letter+digit...).
//!   The 0.3.17 src/pose.rs invented `(#G2:8E0:8M1)` — colon between
//!   gesture and expression — which is WRONG on the wire. Generation must
//!   use the canonical form; parsing accepts both (compat) plus the
//!   bare `(#...)` with no gesture.
//! * `Appears as` / `Background as` are sent as `Appears as <name>` /
//!   `Background as <name>` lines (protsupp / chatdoc), NOT as pose codes.
//! * PoseStyle::None is the default in v2.5: a plain message sends NO
//!   pose code at all. Earlier 0.3.17 code defaulted to a gesture and
//!   emitted a code on every message — wrong.
//! * OEC = Outgoing Encoding Control: the `\x01`-style prefix the client
//!   prepends to encode formatting / colour / font for Comics-mode peers.
//!   See `oec_encode` / `oec_decode`. Details in SPEC Part A §3.
//! * MAX_FORMATTINGPERBYTE: at most one formatting change per byte of
//!   payload in the original format.cpp. MAX_INPUTLEN: input line limit
//!   (see `MAX_INPUTLEN`).

/// The 8 emotions on the wheel, in wheel order (comic.rs / textpose).
/// Wheel order matters: the UI cycles in this order, and the original
/// `CTextPose` mapped emotion index -> expression image index in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Emotion { Neutral, Happy, Sad, Angry, Surprised, Afraid, Disgusted, Contemptuous }
impl Emotion {
    pub fn all() -> [Emotion; 8] { [Emotion::Neutral, Emotion::Happy, Emotion::Sad, Emotion::Angry, Emotion::Surprised, Emotion::Afraid, Emotion::Disgusted, Emotion::Contemptuous] }
    pub fn index(self) -> u8 { self as u8 }
    /// Expression digit in the pose code (`E0`..`E7` in wheel order).
    /// Original: expression index == wheel index. Do not reorder.
    pub fn expression_digit(self) -> u8 { self.index() }
    pub fn from_expression_digit(d: u8) -> Option<Emotion> { Emotion::all().get(d as usize).copied() }
    pub fn label(self) -> &'static str { match self { Emotion::Neutral=>"Neutral", Emotion::Happy=>"Happy", Emotion::Sad=>"Sad", Emotion::Angry=>"Angry", Emotion::Surprised=>"Surprised", Emotion::Afraid=>"Afraid", Emotion::Disgusted=>"Disgusted", Emotion::Contemptuous=>"Contemptuous" } }
}

/// Gesture: the `G<n>` in the pose code. `G0` = none / standing.
/// Gesture numbers are avatar-defined; the client passes them through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Gesture(pub u8);
/// Mouth shape digit `M<n>`: 0 closed, 1 open, 2 smile, ... (avatar-defined).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Mouth(pub u8);

/// A full pose code: gesture + expression (emotion) + mouth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PoseCode { pub gesture: Option<Gesture>, pub emotion: Emotion, pub mouth: Mouth }
impl PoseCode {
    pub fn none() -> Self { Self { gesture: None, emotion: Emotion::Neutral, mouth: Mouth(0) } }
    /// Canonical v2.5 wire form. Example: `(#G3:1E0:1M1)`.
    /// Format: `(#` + optional `G<n>:` + `<digit>E<digit>` + `:` +
    /// `<digit>M<digit>` + `)`. The leading digit before E/M is the
    /// "intensity" / variant digit the original emitted (always 1 in
    /// practice for user-selected poses; 0 for default). We emit 1.
    pub fn to_wire(&self) -> String {
        let mut s = String::from("(#");
        if let Some(g) = self.gesture { s.push_str(&format!("G{}:", g.0)); }
        s.push_str(&format!("1E{}:1M{})", self.emotion.expression_digit(), self.mouth.0));
        s
    }
    /// Wire form with message: `(#G3:1E0:1M1) hello`.
    pub fn with_message(&self, msg: &str) -> String { format!("{} {}", self.to_wire(), msg) }
}

/// Parse a leading pose code from a message. Returns (pose, rest).
/// Accepts canonical `(#G3:1E0:1M1)`, bare `(#1E0:1M1)`, and the
/// 0.3.17-invented colon form `(#G2:8E0:8M1)` (compat only — never emit).
/// Returns None if no leading `(#...)` is present.
pub fn parse_pose_code(msg: &str) -> Option<(PoseCode, &str)> {
    let t = msg.trim_start();
    if !t.starts_with("(#") { return None; }
    let end = t.find(')')?;
    let inner = &t[2..end];
    let rest = t[end+1..].trim_start();
    // Split on ':' — canonical has [G<n>, 1E<d>, 1M<d>]; colon-form has [G<n>, 8E<d>, 8M<d>] too;
    // bare has [1E<d>, 1M<d>].
    let parts: Vec<&str> = inner.split(':').collect();
    let mut gesture: Option<Gesture> = None;
    let mut emotion = Emotion::Neutral;
    let mut mouth = Mouth(0);
    let mut found = false;
    for part in parts {
        let p = part.trim();
        if p.len() < 2 { continue; }
        if p.starts_with('G') {
            if let Ok(n) = p[1..].parse::<u8>() { gesture = Some(Gesture(n)); }
        } else if let Some(e_pos) = p.find('E') {
            // <digit>E<digit>: first digit intensity, second expression.
            let expr: String = p[e_pos+1..].chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(d) = expr.parse::<u8>() { if let Some(e) = Emotion::from_expression_digit(d) { emotion = e; found = true; } }
        } else if let Some(m_pos) = p.find('M') {
            let m: String = p[m_pos+1..].chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(d) = m.parse::<u8>() { mouth = Mouth(d); found = true; }
        }
    }
    if !found && gesture.is_none() { return None; }
    Some((PoseCode { gesture, emotion, mouth }, rest))
}

/// Balloon style selected by the user for one message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BalloonStyle { Speech, Thought, Shout, Whisper }
impl BalloonStyle {
    pub fn label(self) -> &'static str { match self { BalloonStyle::Speech=>"Speech", BalloonStyle::Thought=>"Thought", BalloonStyle::Shout=>"Shout", BalloonStyle::Whisper=>"Whisper" } }
}

/// PoseStyle: whether a message carries a pose, and which.
/// None = send NO pose code (v2.5 default).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoseStyle { None, Emotion(Emotion), Full(PoseCode) }

/// `Appears as <name>` line: changes the avatar used for subsequent messages.
pub fn appears_as_line(name: &str) -> String { format!("Appears as {}", name.trim()) }
/// `Background as <name>` line: changes the panel background.
pub fn background_as_line(name: &str) -> String { format!("Background as {}", name.trim()) }
/// Parse `Appears as X` / `Background as X` (case-insensitive prefix).
/// Returns ("appears"|"background", name).
pub fn parse_appears_background(line: &str) -> Option<(&'static str, &str)> {
    let t = line.trim();
    for (prefix, kind) in [("appears as", "appears"), ("background as", "background")] {
        if t.len() > prefix.len() && t[..prefix.len()].eq_ignore_ascii_case(prefix) {
            return Some((kind, t[prefix.len()..].trim()));
        }
    }
    None
}

// --- Formatting / OEC ---
/// One formatting run: bold/italic/underline + colour + font.
/// format.cpp stored formatting per-character; MAX_FORMATTINGPERBYTE
/// capped changes per byte. This struct is the per-run view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatRun { pub bold: bool, pub italic: bool, pub underline: bool, pub colour_rgb: Option<(u8,u8,u8)>, pub font: Option<String> }
impl Default for FormatRun { fn default() -> Self { Self { bold:false, italic:false, underline:false, colour_rgb:None, font:None } } }

pub const MAX_INPUTLEN: usize = 512;
pub const MAX_FORMATTING_PER_BYTE: usize = 1;
/// OEC marker byte the original prepended to Comics-mode payloads.
/// The exact byte is version-dependent; v2.5 used 0x01 (SOH) as the
/// escape. `oec_encode` prefixes `0x01 + <format bytes> + 0x01` when a
/// non-default format is present, else returns the text unchanged.
pub const OEC_ESCAPE: u8 = 0x01;
pub fn oec_encode(text: &str, fmt: &FormatRun) -> Vec<u8> {
    if *fmt == FormatRun::default() { return text.as_bytes().to_vec(); }
    let mut out = vec![OEC_ESCAPE];
    if fmt.bold { out.extend_from_slice(b"B"); }
    if fmt.italic { out.extend_from_slice(b"I"); }
    if fmt.underline { out.extend_from_slice(b"U"); }
    if let Some((r,g,b)) = fmt.colour_rgb { out.extend_from_slice(format!("C{:02X}{:02X}{:02X}", r,g,b).as_bytes()); }
    out.push(OEC_ESCAPE);
    out.extend_from_slice(text.as_bytes());
    out
}
pub fn oec_decode(bytes: &[u8]) -> (FormatRun, Vec<u8>) {
    if bytes.len() < 2 || bytes[0] != OEC_ESCAPE { return (FormatRun::default(), bytes.to_vec()); }
    if let Some(end) = bytes[1..].iter().position(|&b| b == OEC_ESCAPE) {
        let hdr = &bytes[1..1+end]; let mut fmt = FormatRun::default();
        let mut i = 0;
        while i < hdr.len() {
            match hdr[i] {
                b'B' => fmt.bold = true,
                b'I' => fmt.italic = true,
                b'U' => fmt.underline = true,
                b'C' if i+6 < hdr.len() => {
                    let hex = std::str::from_utf8(&hdr[i+1..i+7]).unwrap_or("");
                    if let (Ok(r),Ok(g),Ok(b)) = (u8::from_str_radix(&hex[0..2],16), u8::from_str_radix(&hex[2..4],16), u8::from_str_radix(&hex[4..6],16)) { fmt.colour_rgb = Some((r,g,b)); }
                    i += 6;
                }
                _ => {}
            }
            i += 1;
        }
        return (fmt, bytes[1+end+1..].to_vec());
    }
    (FormatRun::default(), bytes.to_vec())
}

// --- Semantic analysis (semantic.cpp) ---
/// What the original `CSemantic` did: classify a message as an action,
/// an emotion announcement, a directed message, etc., to pick the
/// default balloon / pose when the user did not pick one.
/// This is a faithful, minimal port of the observable rules:
/// * `/me ...` / `* ... *` -> action (narrative, no balloon tail to speaker in some renderings)
/// * leading `name:` / `name,` where name is a member -> directed
/// * `:)`, `:(` etc. -> emotion hint (does NOT override explicit pose)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticKind { Plain, Action, Directed, EmotionHint(Emotion) }
pub fn classify_message(text: &str, member_names: &[&str]) -> SemanticKind {
    let t = text.trim();
    if t.starts_with("/me ") || (t.starts_with('*') && t.ends_with('*') && t.len() > 2) { return SemanticKind::Action; }
    if let Some(colon) = t.find(':') {
        let head = t[..colon].trim();
        if !head.contains(' ') && member_names.iter().any(|n| n.eq_ignore_ascii_case(head)) { return SemanticKind::Directed; }
    }
    // Emoticon hints, checked in the original's order.
    for (pat, emo) in [(":-)", Emotion::Happy), (":)", Emotion::Happy), (":-(", Emotion::Sad), (":(", Emotion::Sad), (":-D", Emotion::Happy), (":-O", Emotion::Surprised), (":-P", Emotion::Disgusted), (";-)", Emotion::Happy), (">:(", Emotion::Angry)] {
        if t.contains(pat) { return SemanticKind::EmotionHint(emo); }
    }
    SemanticKind::Plain
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn canonical_wire() { let p = PoseCode { gesture: Some(Gesture(3)), emotion: Emotion::Neutral, mouth: Mouth(1) }; assert_eq!(p.to_wire(), "(#G3:1E0:1M1)"); }
    #[test] fn parse_canonical() { let (p, rest) = parse_pose_code("(#G3:1E0:1M1) hello").unwrap(); assert_eq!(p.gesture, Some(Gesture(3))); assert_eq!(rest, "hello"); }
    #[test] fn parse_colon_compat() { let (p, _) = parse_pose_code("(#G2:8E0:8M1) hi").unwrap(); assert_eq!(p.gesture, Some(Gesture(2))); }
    #[test] fn parse_bare() { let (p, rest) = parse_pose_code("(#1E1:1M0) hi").unwrap(); assert_eq!(p.emotion, Emotion::Happy); assert_eq!(rest, "hi"); }
    #[test] fn no_pose_code() { assert!(parse_pose_code("hello").is_none()); }
    #[test] fn appears() { assert_eq!(parse_appears_background("Appears as Fred"), Some(("appears","Fred"))); }
    #[test] fn oec_roundtrip() { let fmt = FormatRun { bold:true, ..Default::default() }; let enc = oec_encode("hi", &fmt); let (d, txt) = oec_decode(&enc); assert!(d.bold); assert_eq!(txt, b"hi"); }
    #[test] fn classify_action() { assert_eq!(classify_message("/me waves", &[]), SemanticKind::Action); }
}
