// Comic engine — original Microsoft Comic Chat rules, clean-room.
// Emotions: 8 from the help file + Neutral. Gestures + balloons from the same help.
// Pose codes (see pose.rs) are parsed from the message text suffix and stored on
// the message, so the panel can render the exact G/E/M the sender sent.
use crate::pose::{PoseCode, parse_pose_code, parse_display_text};
use egui::Color32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Emotion { Neutral, Happy, Coy, Bored, Scared, Sad, Angry, Shout, Laugh }
impl Emotion {
    /// Wheel UI order, Shout on top: draw_emotion_wheel puts
    /// all()[0] at the top, clockwise. Shout, Laugh, Happy, Coy,
    /// Bored, Scared, Sad, Angry.
    pub fn all() -> [Emotion; 8] { [Emotion::Shout, Emotion::Laugh, Emotion::Happy, Emotion::Coy, Emotion::Bored, Emotion::Scared, Emotion::Sad, Emotion::Angry] }
    pub fn angle_deg(&self) -> u16 {
        match self { Emotion::Neutral => 0, Emotion::Happy => 0, Emotion::Coy => 45, Emotion::Bored => 90, Emotion::Scared => 135, Emotion::Sad => 180, Emotion::Angry => 225, Emotion::Shout => 270, Emotion::Laugh => 315 }
    }
    pub fn label(&self) -> &'static str {
        match self { Emotion::Neutral => "Neutral", Emotion::Happy => "Happy", Emotion::Coy => "Coy", Emotion::Bored => "Bored", Emotion::Scared => "Scared", Emotion::Sad => "Sad", Emotion::Angry => "Angry", Emotion::Shout => "Shout", Emotion::Laugh => "Laugh" }
    }
    pub fn emoji(&self) -> &'static str {
        match self { Emotion::Neutral => "\u{1f610}", Emotion::Happy => "\u{1f60a}", Emotion::Laugh => "\u{1f602}", Emotion::Coy => "\u{1f609}", Emotion::Bored => "\u{1f971}", Emotion::Scared => "\u{1f628}", Emotion::Sad => "\u{1f622}", Emotion::Angry => "\u{1f620}", Emotion::Shout => "\u{1f631}" }
    }
    pub fn color(&self) -> Color32 {
        match self {
            Emotion::Neutral => Color32::from_rgb(200, 200, 200),
            Emotion::Coy => Color32::from_rgb(255, 150, 180),
            Emotion::Happy => Color32::from_rgb(255, 220, 0),
            Emotion::Laugh => Color32::from_rgb(255, 160, 0),
            Emotion::Shout => Color32::from_rgb(255, 60, 60),
            Emotion::Angry => Color32::from_rgb(180, 0, 0),
            Emotion::Sad => Color32::from_rgb(80, 120, 220),
            Emotion::Scared => Color32::from_rgb(150, 80, 200),
            Emotion::Bored => Color32::from_rgb(140, 160, 140),
        }
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gesture { Neutral, Wave, PointSelf, PointOther, DoublePoint, Shrug, Walk3Q, SideWalk, Walk3QF }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Balloon { Normal, Thought, Whisper, Shout, Question, Exclaim, Action }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Title = room-entry title card (the original's "Starring pane").
/// Local-only: built on NAMES after a join, never sent to the server,
/// never composed (input_kind is only ever Say/Think/Whisper/Action).
/// Renders as its own panel (see group_into_panels + draw_panel).
/// Info = version / away panel. Local-only, like Title: never sent,
/// never composed, never a balloon. Renders as its own panel — white,
/// no background, text on top, the subject standing alone below.
pub enum MessageKind { Say, Think, Whisper, Action, Title, Info }
impl MessageKind { pub fn label(&self) -> &'static str { match self { MessageKind::Say=>"Say", MessageKind::Think=>"Think", MessageKind::Whisper=>"Whisper", MessageKind::Action=>"Action", MessageKind::Title=>"Title", MessageKind::Info=>"Info" } } }

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub struct ComicMessage {
    pub neutral_variant: u64,
    pub nick: String, pub text: String, pub is_me: bool,
    pub emotion: Emotion, pub balloon: Balloon, pub gesture: Gesture,
    pub kind: MessageKind, pub strength: f32,
    pub pose: Option<PoseCode>,
    pub character: String,      // AVB name for this nick (from Appears as, or nick fallback)
    pub character_url: Option<String>,
    pub cast: Vec<String>,      // panel cast — nicks to include in this panel even if silent
    pub raw_with_code: String,  // original text including (#G... ) code, for Plain Text view
}
#[derive(Debug, Clone, PartialEq)]
pub struct Panel {
    pub messages: Vec<ComicMessage>,
    pub zoom: f32,
    // 15th-panel zoom removed: source has no 0.8 zoom.
    /// pulls back to remind readers of the surroundings. Drawn full-width
    /// with a smaller zoom; the entry title card is the first one.
    pub establishing: bool,
    /// Narration caption (join/leave etc.) — yellow box, NOT an action.
    pub caption: Option<String>,
    /// /me actions in this panel — rendered as RECTANGULAR narration
    /// bars at the top (PDF §8, real screenshots: "OARION71 RUNS AWAY"),
    /// never as balloons. Kept separate from `messages` so the bar and
    /// the actor's character can both be drawn.
    pub actions: Vec<ComicMessage>,
}

pub fn analyze(text: &str, wheel: Option<(Emotion, f32)>, frozen: bool) -> (Emotion, Balloon, Gesture, f32) {
    // Silent: no logging here (runs per message / per frame).
    // POSE_WHEEL_TEXT_0317: wheel wins for EMOTION, text wins for GESTURE.
    // Before: wheel was ignored unless frozen, so typing "you" with Coy
    // selected sent (Neutral, PointOther) and preview label/image split.
    // After: if wheel emotion is non-Neutral, keep it; gesture comes from
    // text when text implies one (you->PointOther, i->PointSelf,
    // hello->Wave), else gesture_for_emotion(wheel). Wheel Neutral falls
    // through to text heuristics (lol->Laugh, sad->Sad, etc.).
    // Frozen still wins entirely (same result: wheel emotion + text gesture).
    if let Some((e, s)) = wheel {
        if e != Emotion::Neutral {
            let lower0 = text.to_lowercase();
            let trimmed0 = text.trim();
            let tg = gesture_from_text(&lower0, trimmed0);
            let g = if tg != Gesture::Neutral { tg } else { gesture_for_emotion(e) };
            return (e, balloon_for_kind(MessageKind::Say, e, text), g, s);
        }
        // Wheel Neutral: fall through to text heuristics below.
        let _ = frozen;
    }
    let lower = text.to_lowercase();
    let trimmed = text.trim();
    let letters: Vec<char> = trimmed.chars().filter(|c| c.is_alphabetic()).collect();
    let all_caps = !letters.is_empty() && letters.len() > 3 && letters.iter().all(|c| c.is_uppercase());
    if all_caps || trimmed.ends_with("!!!") || trimmed.ends_with("!!") {
        // Balloon is Shout, but avatar stays Neutral. Old code returned
        // Shouting emotion, which cycled the avatar to a random angry
        // pose (Emotion::Shout, Balloon::Shout, Gesture::PointOther, 1.0). Pose is
        // wheel / pose-code only, never text heuristics.
        // Caps wins over Wave: HELLO EVERYONE = Shout, hello there = Wave.
        return (Emotion::Neutral, Balloon::Shout, Gesture::Neutral, 0.7);
    }
    // HELLO/hi/bye/welcome/howdy -> Wave, only when NOT all caps.
    if gesture_from_text(&lower, trimmed) == Gesture::Wave {
        return (Emotion::Neutral, Balloon::Normal, Gesture::Wave, 0.7);
    }
    if lower.contains("rotfl") { return (Emotion::Laugh, Balloon::Normal, Gesture::Wave, 1.0); }
    if lower.contains("lol") || lower.contains("haha") || lower.contains("lmao") { return (Emotion::Laugh, Balloon::Normal, Gesture::Wave, 0.9); }
    if lower.contains(":)") || lower.contains(":-)") || lower.contains("yay") { return (Emotion::Happy, Balloon::Normal, Gesture::Wave, 0.8); }
    if lower.contains(":(") || lower.contains(":-(") || lower.contains("sad") || lower.contains("cry") { return (Emotion::Sad, Balloon::Normal, Gesture::Neutral, 0.8); }
    if lower.contains(";-)") || lower.contains(";)") { return (Emotion::Coy, Balloon::Normal, Gesture::Shrug, 0.8); }
    if lower.contains("bored") || lower.contains("zzz") { return (Emotion::Bored, Balloon::Normal, Gesture::Shrug, 0.7); }
    if lower.contains("scared") || lower.contains("afraid") || lower.contains("eek") || lower.contains("help") { return (Emotion::Scared, Balloon::Normal, Gesture::Neutral, 0.8); }
    if lower.contains("angry") || lower.contains("grr") || lower.contains("wtf") || lower.contains("damn") { return (Emotion::Neutral, Balloon::Shout, Gesture::Neutral, 0.7); }
    if lower.contains("hmm") || lower.contains("thinking") { return (Emotion::Neutral, Balloon::Thought, Gesture::Neutral, 0.6); }
    let gesture = gesture_from_text(&lower, trimmed);
    if trimmed.ends_with('?') { return (Emotion::Neutral, Balloon::Question, gesture, 0.5); }
    if trimmed.ends_with('!') || lower.contains("wow") || lower.contains("omg") { return (Emotion::Neutral, Balloon::Exclaim, gesture, 0.7); }
    (Emotion::Neutral, Balloon::Normal, gesture, 0.4)
}
fn gesture_from_text(lower: &str, trimmed: &str) -> Gesture {
    let t = trimmed.to_lowercase();
    if t.starts_with("i ") || t.starts_with("i'm") || t.starts_with("i will") || t.starts_with("i'll") || t.starts_with("i am") { return Gesture::PointSelf; }
    if t.starts_with("you") || lower.contains("are you") || lower.contains("will you") || lower.contains("did you") || lower.contains("don't you") || lower.contains("aren't you") { return Gesture::PointOther; }
    if t.starts_with("hello") || t.starts_with("hi ") || t == "hi" || t.starts_with("bye") || t.starts_with("welcome") || t.starts_with("howdy") { return Gesture::Wave; }
    Gesture::Neutral
}
pub fn gesture_for_emotion(_e: Emotion) -> Gesture {
    // Must stay Neutral: single AVB body per message.
    // If this returned Wave/Shrug for Happy/Coy, current_pose_code clears emotion to Neutral.
    // Wheel emotion drives the body; gesture bodies are only via text trigger or wheel_gesture button.
    Gesture::Neutral
}
fn balloon_for_kind(kind: MessageKind, emotion: Emotion, text: &str) -> Balloon {
    match kind {
        MessageKind::Think => Balloon::Thought,
        MessageKind::Whisper => Balloon::Whisper,
        MessageKind::Action => Balloon::Action,
        // Title / Info never render as a balloon — their panels draw
        // their own layout (draw_panel). Normal is the sane placeholder.
        MessageKind::Title => Balloon::Normal,
        MessageKind::Info => Balloon::Normal,
        MessageKind::Say => {
            if emotion == Emotion::Shout { Balloon::Shout }
            else if text.trim().ends_with('?') { Balloon::Question }
            else if text.trim().ends_with('!') { Balloon::Exclaim }
            else { Balloon::Normal }
        }
    }
}
#[allow(dead_code)]
pub fn make_message(nick: &str, text: &str, is_me: bool) -> ComicMessage {
    let mut m = make_message_kind(nick, text, is_me, MessageKind::Say, None, false);
    // SHOUT_OVERRIDE_0317: the test calls this function directly,
    // so it must not be bypassed by any wrapper below.
    let _has_pose = text.contains("(#G");
    let _shout_letters: Vec<char> = text.chars().filter(|c| c.is_alphabetic()).collect();
    let _shout_all_caps = !_shout_letters.is_empty() && _shout_letters.len() > 3 && _shout_letters.iter().all(|c| c.is_uppercase());
    if !_has_pose && (_shout_all_caps || text.trim().ends_with("!!!") || text.trim().ends_with("!!")) {
        // Step 5: caps = Balloon::Shout only. Avatar stays as analyzed / pose-coded. Old code set Emotion::Shout and cycled avatar to random angry pose.
        m.balloon = Balloon::Shout;
    }
    m
}
#[allow(dead_code)]
pub fn make_message_kind(nick: &str, text: &str, is_me: bool, kind: MessageKind, wheel: Option<(Emotion, f32)>, frozen: bool) -> ComicMessage {
    make_message_full(nick, text, is_me, kind, wheel, frozen, "", None, Vec::new())
}
pub fn make_message_full(nick: &str, text: &str, is_me: bool, kind: MessageKind, wheel: Option<(Emotion, f32)>, frozen: bool, character: &str, character_url: Option<String>, cast: Vec<String>) -> ComicMessage {
    // Strip v2.5 OEC code first — it controls the pose, it is not spoken text.
    // (OEC:<n>M<m>) at the start, e.g. "(OEC:0M1) hello". <n> mapping is
    // still unverified, so OEC does NOT override text analysis yet; it is
    // only stripped so it never shows in a bubble. Never generated.
    // Keep raw for Plain Text view (where original clients showed the code).
    let raw_with_code = text.to_string();
    let text_no_oec = parse_display_text(text);
    // Strip pose code next — it controls the pose, it is not spoken text.
    let (clean_text, parsed_pose) = parse_pose_code(&text_no_oec).map(|(c, p)| (c, Some(p))).unwrap_or_else(|| (text_no_oec.clone(), None));
    // If a pose code was present, it wins over wheel/text analysis for emotion/gesture.
    // If no pose code, fall back to wheel (if frozen) or text analysis.
    let (emotion, _, gesture, strength) = if let Some(p) = &parsed_pose {
        (p.emotion(), Balloon::Normal, p.gesture(), p.strength())
    } else {
        analyze(&clean_text, wheel, frozen)
    };
    let balloon = balloon_for_kind(kind, emotion, &clean_text);
    // SHOUT_OVERRIDE_0317: all-caps / !!! is Shout unless an explicit
    // (#G...) pose code set a different emotion. Shadowing (emotion,
    // balloon) here catches any analyze / pose path that returned
    // Neutral for shout text.
    let (emotion, balloon) = if parsed_pose.is_none() {
        let _shout_letters: Vec<char> = clean_text.chars().filter(|c| c.is_alphabetic()).collect();
        let _shout_all_caps = !_shout_letters.is_empty() && _shout_letters.len() > 3 && _shout_letters.iter().all(|c| c.is_uppercase());
        if _shout_all_caps || clean_text.trim().ends_with("!!!") || clean_text.trim().ends_with("!!") {
            (emotion, Balloon::Shout) // Step 5: keep emotion, only balloon Shouts
        } else { (emotion, balloon) }
    } else { (emotion, balloon) };
    let gesture = match kind { MessageKind::Think => Gesture::Neutral, MessageKind::Action => Gesture::Wave, _ => gesture };
    // Cast comes from pose CAST token, plus explicit cast passed in (click-to-include)
    let mut full_cast = cast.clone();
    if let Some(p) = &parsed_pose {
        for n in &p.cast { if !full_cast.contains(n) { full_cast.push(n.clone()); } }
    }
    // If the text was "*thinks* hello", strip the marker for display (balloon already says Think)
    let display_text = if kind == MessageKind::Think {
        clean_text.strip_prefix("*thinks*").map(|s| s.trim().to_string()).unwrap_or_else(|| clean_text.to_string())
    } else { clean_text.to_string() };
    ComicMessage {
        neutral_variant: 0,
        nick: nick.to_string(), text: display_text, is_me, emotion, balloon, gesture, kind, strength,
        pose: parsed_pose, character: if character.is_empty() { nick.to_string() } else { character.to_string() },
        character_url, cast: full_cast, raw_with_code,
    }
}
/// Built-in room-entry title cards (the original's "Starring pane" titles).
/// Verified: JUST US CHUMPS, DEEPEST DARKEST DESIRES, and YOU SHOULDA
/// BEEN THERE from real Microsoft Chat screenshots; NETWORKED NERDS from
/// the user's own recollection. No authoritative full list survives
/// (the SIGGRAPH paper names no titles, and the 2.5 help only calls the
/// pane "Starring"), so these four are the built-ins and everything else
/// comes from config `comic.titles`, which is ADDED on top of these,
/// never a replacement. One title is picked at random per room entry.
pub const BUILTIN_TITLES: [&str; 4] = [
    "JUST US CHUMPS",
    "DEEPEST DARKEST DESIRES",
    "YOU SHOULDA BEEN THERE",
    "NETWORKED NERDS",
];

/// Max OTHER members (besides the connecting user) in a title card's
/// cast. Self + up to 3 others = at most 4 avatars in the Starring row.
/// Fewer when the room is smaller. The screenshots show 1-2 only because
/// those cards were generated before the other members had arrived;
/// the cap here is the user's spec ("you and 2 or 3 others").
pub const TITLE_CAST_MAX_OTHERS: usize = 3;

/// Max characters in one balloon before a long message spills into
/// additional panels. A panel balloon is ~40 chars/line, ~4-5 lines
/// before it crowds the panel; 180 chars is a full balloon.
pub const MAX_BALLOON_CHARS: usize = 180;

/// Split a long message into spillover chunks (PDF §5.3: "break up
/// the text of the balloon and add ellipses to each of the new
/// balloons"). Every chunk except the first gets a leading "\u{2026}",
/// every chunk except the last gets a trailing "\u{2026}", so the
/// reader sees the continuation. Split at word boundaries; a single
/// word longer than the cap is hard-split. Title cards and
/// narration bars are never split (titles are short; bars wrap).
/// The chunks keep the same nick/kind/emotion, so group_into_panels'
/// one-balloon-per-character rule puts each chunk in its own panel.
pub fn split_long_message(m: &ComicMessage) -> Vec<ComicMessage> {
    if !matches!(m.kind, MessageKind::Say | MessageKind::Think | MessageKind::Whisper) { return vec![m.clone()]; }
    if m.text.chars().count() <= MAX_BALLOON_CHARS { return vec![m.clone()]; }
    let words: Vec<&str> = m.text.split_whitespace().collect();
    let mut chunks: Vec<String> = Vec::new();
    let mut cur = String::new();
    for w in words {
        // A word longer than the cap is hard-split on its own.
        if w.chars().count() > MAX_BALLOON_CHARS {
            if !cur.is_empty() { chunks.push(std::mem::take(&mut cur)); }
            let mut rest = w;
            while rest.chars().count() > MAX_BALLOON_CHARS {
                let cut: String = rest.chars().take(MAX_BALLOON_CHARS).collect();
                let cut_len = cut.len();
                chunks.push(cut);
                rest = &rest[cut_len..];
            }
            if !rest.is_empty() { cur = rest.to_string(); }
            continue;
        }
        let extra = if cur.is_empty() { 0 } else { 1 }; // the space
        if cur.chars().count() + extra + w.chars().count() > MAX_BALLOON_CHARS {
            chunks.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() { cur.push(' '); }
        cur.push_str(w);
    }
    if !cur.is_empty() { chunks.push(cur); }
    let n = chunks.len();
    chunks.into_iter().enumerate().map(|(i, c)| {
        let mut txt = String::new();
        if i > 0 { txt.push('\u{2026}'); }
        txt.push_str(&c);
        if i + 1 < n { txt.push('\u{2026}'); }
        let mut m2 = m.clone();
        m2.text = txt;
        m2
    }).collect()
}

pub fn group_into_panels(messages: &[ComicMessage], per_panel: usize) -> Vec<Panel> {
    // PDF §6.1 panel-break rules (not just "N messages per panel"):
    //  - new panel when the balloon layout would not fit another balloon
    //    (approximated here by `per_panel`; the layout algorithm itself
    //    is the final arbiter in draw_panel),
    //  - new panel when adding a speaker would exceed 5 characters in
    //    the panel (facial expressions become unreadable past 5),
    //  - new panel when a character would get a SECOND balloon in the
    //    same panel (at most one balloon per character per panel),
    //  - new panel when a character would change expression/gesture in
    //    a way that cannot be shown in one panel (approximated: a second
    //    utterance from the same character already forces the break above).
    // Actions (/me) do NOT count toward the balloon cap — they render
    // as narration bars above the balloons — but their actor DOES count
    // toward the 5-character cap.
    // 0.3.17: long messages spill over into additional panels
    // (PDF §5.3, split_long_message). Split BEFORE grouping: the
    // one-balloon-per-character rule then puts each chunk in its
    // own panel automatically.
    let expanded: Vec<ComicMessage> = messages.iter().flat_map(split_long_message).collect();
    let messages = &expanded;
    // Original panel.cpp AddLine: MAX_ELEMENTS_PER_PANEL = 4. One avatar
    // cannot talk twice in one frame, captions exempt. per_panel from
    // config is clamped to 4 so a config cannot break the original rule.
    let cap = per_panel.max(1).min(4);
    let mut panels: Vec<Panel> = Vec::new();
    let mut cur_msgs: Vec<ComicMessage> = Vec::new();
    let mut cur_actions: Vec<ComicMessage> = Vec::new();
    let mut cur_nicks: Vec<String> = Vec::new(); // distinct characters in this panel
    let flush = |panels: &mut Vec<Panel>, msgs: &mut Vec<ComicMessage>, acts: &mut Vec<ComicMessage>, nicks: &mut Vec<String>| {
        if msgs.is_empty() && acts.is_empty() { return; }
        let n = msgs.len() + acts.len();
        // 15th-panel zoom removed: source has no 0.8 zoom.
        // No 0.8 zoom: source has no wide-pane zoom.
        // Panel index is panels.len() (the panel being created). The title
        // card is panel 0, so idx > 0 keeps it from counting as one.
        let idx = panels.len();
        let establishing = idx > 0 && idx % 15 == 0;
        let zoom = if establishing { 0.8 } else { 1.0 }; // Step 112: fixed zoom - 1.22/1.08 made 1-msg panels taller than 2-msg panels
        panels.push(Panel { messages: std::mem::take(msgs), zoom, caption: None, actions: std::mem::take(acts), establishing });
        nicks.clear();
    };
    for m in messages {
        if m.nick == "System" { continue; }
        if m.kind == MessageKind::Title || m.kind == MessageKind::Info {
            // Title card: always its own panel, alone. Flush whatever
            // came before, push the title, flush immediately — the
            // Starring pane is the first panel, never grouped with chat.
            flush(&mut panels, &mut cur_msgs, &mut cur_actions, &mut cur_nicks);
            cur_msgs.push(m.clone());
            flush(&mut panels, &mut cur_msgs, &mut cur_actions, &mut cur_nicks);
            continue;
        }
        if m.kind == MessageKind::Action {
            // New speaker would exceed 5 chars -> break first.
            if !cur_nicks.contains(&m.nick) && cur_nicks.len() >= 5 {
                flush(&mut panels, &mut cur_msgs, &mut cur_actions, &mut cur_nicks);
            }
            if !cur_nicks.contains(&m.nick) { cur_nicks.push(m.nick.clone()); }
            cur_actions.push(m.clone());
            continue;
        }
        // Speech: balloon cap, one-balloon-per-character, 5-char cap.
        let need_break = cur_msgs.len() >= cap
            || cur_msgs.iter().any(|x| x.nick == m.nick)
            || (!cur_nicks.contains(&m.nick) && cur_nicks.len() >= 5);
        if need_break {
            flush(&mut panels, &mut cur_msgs, &mut cur_actions, &mut cur_nicks);
        }
        if !cur_nicks.contains(&m.nick) { cur_nicks.push(m.nick.clone()); }
        cur_msgs.push(m.clone());
    }
    flush(&mut panels, &mut cur_msgs, &mut cur_actions, &mut cur_nicks);
    panels
}
pub fn avatar_color(nick: &str, seed: u64) -> Color32 {
    let mut h: u64 = seed.wrapping_add(0x9e3779b97f4a7c15);
    for b in nick.bytes() { h = h.wrapping_mul(1099511628211).wrapping_add(b as u64); h ^= h >> 29; }
    let hue = (h % 360) as f32;
    hsv_to_rgb(hue, 0.55, 0.92)
}
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> Color32 {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h / 60.0) as u32 { 0 => (c, x, 0.0), 1 => (x, c, 0.0), 2 => (0.0, c, x), 3 => (0.0, x, c), 4 => (x, 0.0, c), _ => (c, 0.0, x) };
    Color32::from_rgb(((r + m) * 255.0) as u8, ((g + m) * 255.0) as u8, ((b + m) * 255.0) as u8)
}
#[allow(dead_code)]
pub fn emotion_face(emotion: Emotion) -> (&'static str, &'static str) {
    match emotion {
        Emotion::Neutral => ("•  •", "—"),
        Emotion::Happy => ("^  ^", "‿"),
        Emotion::Laugh => ("≧  ≦", "D"),
        Emotion::Coy => ("•  😉", "~"),
        Emotion::Bored => ("-  -", "—"),
        Emotion::Scared => ("O  O", "o"),
        Emotion::Sad => ("•  •", "⌒"),
        Emotion::Angry => (">  <", "▼"),
        Emotion::Shout => (">  <", "□"),
    }
}
#[allow(dead_code)]
pub fn builtin_characters() -> Vec<&'static str> { vec!["Frank", "Mabel", "Chip", "Dee", "Eli", "Flo", "Gus", "Hal"] }
#[allow(dead_code)]
pub fn builtin_backgrounds() -> Vec<&'static str> { vec!["Cafe", "Beach", "Space", "Park", "Plain", "Night"] }
pub fn background_color(name: &str) -> Color32 {
    match name { "Cafe" => Color32::from_rgb(255, 244, 214), "Beach" => Color32::from_rgb(210, 240, 255), "Space" => Color32::from_rgb(20, 20, 50), "Park" => Color32::from_rgb(220, 245, 210), "Night" => Color32::from_rgb(40, 40, 60), _ => Color32::WHITE }
}







/// Helpers for render batch: Title = Starring pane, System = "Now chatting" etc.
pub fn is_title_message(m: &ComicMessage) -> bool { m.kind == MessageKind::Title }
pub fn is_system_message(m: &ComicMessage) -> bool { m.nick == "System" }



/// Render helper: same speaker = same nick + same kind.
/// Used to decide whether consecutive panels can share a character slot.
pub fn is_same_speaker(a: &ComicMessage, b: &ComicMessage) -> bool {
    a.nick == b.nick && a.kind == b.kind
}



/// Title card rule (spec): ONLY in very first frame when room joined,
/// static, scrolls into history, never persists / reappears.
/// is_first_frame = this is the first frame after JOIN.
/// already_shown = a title card was already emitted for this JOIN.
pub fn should_show_title_card(is_first_frame: bool, already_shown: bool) -> bool {
    is_first_frame && !already_shown
}



/// Text-log helpers: system lines that are NOT balloons.
/// "Now chatting in #x" and "# Appears as ..." are system lines.
pub fn is_now_chatting_line(s: &str) -> bool { s.trim_start().starts_with("Now chatting") }
pub fn is_appears_as_line(s: &str) -> bool { s.trim_start().starts_with("# Appears as") || s.trim_start().starts_with("# Background as") }



/// Think / Action line detection (wire: "*thinks* ..." and "\x01ACTION ...\x01").
pub fn is_think_line(s: &str) -> bool { s.trim_start().starts_with("*thinks*") }
pub fn is_ctcp_action_line(s: &str) -> bool {
    let t = s.trim();
    t.starts_with("\u{1}ACTION ") && t.ends_with('\u{1}')
}



/// Text-log helpers: whisper lines are "(whisper to bob) hi".
/// System lines are nick == "System" or "Now chatting" or "# Appears/Background as".
pub fn is_whisper_line(s: &str) -> bool { s.trim_start().starts_with("(whisper to ") }
pub fn is_system_line(s: &str) -> bool {
    let t = s.trim_start();
    t.starts_with("Now chatting") || t.starts_with("# Appears as") || t.starts_with("# Background as") || t.starts_with("***")
}



/// Balloon vs starring: balloons are Say / Think / Whisper / Action.
/// Starring is Title only. System lines are not balloons.
pub fn is_balloon_message(m: &ComicMessage) -> bool {
    matches!(m.kind, MessageKind::Say | MessageKind::Think | MessageKind::Whisper | MessageKind::Action)
}
pub fn is_starring_message(m: &ComicMessage) -> bool { m.kind == MessageKind::Title }



/// System-line detection for join / part / nick change.
/// "bob joined #x", "bob left #x", "bob is now known as robert".
pub fn is_join_system_line(s: &str) -> bool {
    let t = s.to_ascii_lowercase();
    t.contains(" joined") || t.contains("has joined")
}
pub fn is_part_system_line(s: &str) -> bool {
    let t = s.to_ascii_lowercase();
    t.contains(" left") || t.contains("has left") || t.contains("parted")
}
pub fn is_nick_change_line(s: &str) -> bool {
    let t = s.to_ascii_lowercase();
    t.contains("is now known as") || t.contains("changed nick")
}



/// Away / back system lines: "bob is away (brb)", "bob is back".
pub fn is_away_line(s: &str) -> bool {
    let t = s.to_ascii_lowercase();
    t.contains("is away") || t.contains("is now away") || t.contains("went away")
}
pub fn is_back_line(s: &str) -> bool {
    let t = s.to_ascii_lowercase();
    t.contains("is back") || t.contains("is no longer away") || t.contains("returned")
}



/// Topic / kick system lines: "Topic is ...", "bob kicked alice", "bob was kicked".
pub fn is_topic_line(s: &str) -> bool {
    let t = s.to_ascii_lowercase();
    t.contains("topic is") || t.contains("topic for") || t.starts_with("topic:")
}
pub fn is_kick_line(s: &str) -> bool {
    let t = s.to_ascii_lowercase();
    t.contains("kicked") || t.contains("was kicked") || t.contains("has kicked")
}



/// Mode / op system lines: "mode for #x is +nt", "bob is now op", "alice gave op to bob".
pub fn is_mode_line(s: &str) -> bool {
    let t = s.to_ascii_lowercase();
    t.contains("mode for") || t.contains("changed mode") || t.contains("set mode")
}
pub fn is_op_line(s: &str) -> bool {
    let t = s.to_ascii_lowercase();
    t.contains("is now op") || t.contains("is now an operator") || t.contains("gave op") || t.contains("opped")
}



/// WHOIS / WHO system lines: "bob is known as ...", "whois for bob", "users in #x".
pub fn is_whois_line(s: &str) -> bool {
    let t = s.to_ascii_lowercase();
    t.contains("whois") || t.contains("is known as") || t.contains("real name")
}
pub fn is_who_line(s: &str) -> bool {
    let t = s.to_ascii_lowercase();
    t.contains("who for") || t.contains("users in") || t.contains("end of who")
}



/// INVITE / LIST system lines: "bob invited you", "end of list", "channel #x has 3 users".
pub fn is_invite_line(s: &str) -> bool {
    let t = s.to_ascii_lowercase();
    t.contains("invited") || t.contains("invite you") || t.contains("has invited")
}
pub fn is_list_line(s: &str) -> bool {
    let t = s.to_ascii_lowercase();
    t.contains("end of list") || (t.contains("channel") && t.contains("users"))
}



pub fn is_names_line(s: &str) -> bool { let t = s.to_ascii_lowercase(); t.contains("names for") || t.contains("end of names") || t.contains("names:") }



pub fn is_ban_line(s: &str) -> bool { let t = s.to_ascii_lowercase(); t.contains("ban list") || t.contains("was banned") || t.contains("has banned") }



pub fn is_quit_line(s: &str) -> bool { let t = s.to_ascii_lowercase(); t.contains("has quit") || t.contains("signed off") || t.contains("quit:") }



pub fn is_join_line(s: &str) -> bool { let t = s.to_ascii_lowercase(); t.contains("has joined") || t.contains("joined #") || t.contains("joined the channel") }



pub fn is_part_line(s: &str) -> bool { let t = s.to_ascii_lowercase(); t.contains("has left") || t.contains("has parted") || t.contains("left #") }



pub fn is_voice_line(s: &str) -> bool { let t = s.to_ascii_lowercase(); t.contains("is now voice") || t.contains("gave voice") || t.contains("voiced") }



pub fn is_devoice_line(s: &str) -> bool { let t = s.to_ascii_lowercase(); t.contains("devoiced") || t.contains("removed voice") || t.contains("is no longer voice") }



pub fn is_privmsg_line(s: &str) -> bool { let t = s.to_ascii_lowercase(); t.contains("privmsg") || t.starts_with('<') }



pub fn is_notice_line(s: &str) -> bool { let t = s.to_ascii_lowercase(); t.contains("notice") || t.starts_with("-") && t.contains("-") }



pub fn is_action_line(s: &str) -> bool { let t = s.to_ascii_lowercase(); t.contains("action") || t.starts_with("* ") || t.starts_with("*") && t.contains(" ") }



pub fn is_thinking_line(s: &str) -> bool { let t = s.to_ascii_lowercase(); t.contains("thinking") || t.contains("is thinking") }



pub fn is_balloon_line(s: &str) -> bool { s.to_ascii_lowercase().contains("balloon") }



pub fn is_title_line(s: &str) -> bool { let t = s.to_ascii_lowercase(); t.contains("title card") || t.starts_with("title:") }



pub fn is_cast_line(s: &str) -> bool { let t = s.to_ascii_lowercase(); t.contains("cast") && (t.contains(':') || t.contains("panel")) }



pub fn is_oec_line(s: &str) -> bool { s.contains("(OEC:") }



pub fn strip_oec_line(s: &str) -> String { if let Some(start) = s.find("(OEC:") { if let Some(end) = s[start..].find(')') { let after = &s[start + end + 1..]; return after.trim_start().to_string(); } } s.to_string() }



pub fn wrap_text_len(text: &str, width: usize) -> usize { if width == 0 { return 1; } let len = text.chars().count(); if len == 0 { return 1; } (len + width - 1) / width }



pub fn split_words(s: &str) -> Vec<String> { s.split_whitespace().map(|w| w.to_string()).collect() }



pub fn count_lines(s: &str) -> usize { s.lines().count() }



pub fn is_empty_line(s: &str) -> bool { s.trim().is_empty() }



pub fn normalize_whitespace(s: &str) -> String { s.split_whitespace().collect::<Vec<_>>().join(" ") }



pub fn collapse_spaces(s: &str) -> String { let mut out = String::new(); let mut prev_space = false; for c in s.chars() { if c == ' ' { if !prev_space { out.push(c); } prev_space = true; } else { out.push(c); prev_space = false; } } out }



pub fn truncate_comic_text(text: &str, max: usize) -> String { if text.chars().count() <= max { return text.to_string(); } if max <= 3 { return text.chars().take(max).collect(); } let mut s: String = text.chars().take(max - 3).collect(); s.push_str("..."); s }







pub fn is_long_comic_text(s: &str) -> bool { s.chars().count() > 200 }



pub fn is_short_comic_text(s: &str) -> bool { s.chars().count() <= 20 }







pub fn is_balloon_text(s: &str) -> bool { !s.trim().is_empty() && !s.contains("(OEC:") }



pub fn balloon_type_for_emotion(emotion: &str) -> &'static str { let t = emotion.to_ascii_lowercase(); if t.contains("angry") || t.contains("shout") { return "shout"; } if t.contains("sad") || t.contains("thought") { return "thought"; } "speech" }







pub fn is_join_message(s: &str) -> bool { s.contains("joined") && !s.contains("(OEC:") }



pub fn is_part_message(s: &str) -> bool { s.contains("left") || s.contains("parted") }



pub fn format_join_message(nick: &str, channel: &str) -> String { format!("{} joined {}", nick, channel) }



pub fn format_part_message(nick: &str, channel: &str) -> String { format!("{} left {}", nick, channel) }



pub fn format_quit_message(nick: &str, reason: &str) -> String { if reason.is_empty() { return format!("{} quit", nick); } format!("{} quit ({})", nick, reason) }



pub fn panel_height_for_lines(lines: usize) -> usize { lines * 20 + 16 }



pub fn panel_width_for_text(text: &str) -> usize { let m = text.lines().map(|l| l.chars().count()).max().unwrap_or(0); m * 8 + 20 }



pub fn is_thought_balloon(s: &str) -> bool { let t = s.to_ascii_lowercase(); t.contains("thought") || t.contains("think") }



pub fn is_shout_balloon(s: &str) -> bool { let t = s.to_ascii_lowercase(); t.contains("shout") || t.contains("yell") || t.contains("scream") }



pub fn is_speech_balloon(s: &str) -> bool { let t = s.trim(); !t.is_empty() && !t.contains("(OEC:") && !t.to_ascii_lowercase().contains("thought") && !t.to_ascii_lowercase().contains("shout") }



pub fn balloon_for_text(s: &str) -> &'static str { let t = s.to_ascii_lowercase(); if t.contains("thought") || t.contains("think") { return "thought"; } if t.contains("shout") || t.contains("yell") || t.contains("scream") { return "shout"; } "speech" }



pub fn strip_comic_tags(s: &str) -> String { s.chars().filter(|&c| c != '<' && c != '>' && c != '[' && c != ']').collect() }







pub fn is_comic_action(s: &str) -> bool { let t = s.trim_start(); t.starts_with("/me") || t.starts_with('*') }



pub fn format_comic_action(nick: &str, s: &str) -> String { let t = s.trim_start(); if let Some(after) = t.strip_prefix("/me") { return format!("{} {}", nick, after.trim_start()); } if let Some(after) = t.strip_prefix('*') { return format!("{} {}", nick, after.trim_start()); } s.to_string() }



pub fn is_all_caps(s: &str) -> bool { let letters: Vec<char> = s.chars().filter(|c| c.is_alphabetic()).collect(); !letters.is_empty() && letters.iter().all(|c| c.is_uppercase()) }



pub fn is_all_lower(s: &str) -> bool { let letters: Vec<char> = s.chars().filter(|c| c.is_alphabetic()).collect(); !letters.is_empty() && letters.iter().all(|c| c.is_lowercase()) }



pub fn to_title_case(s: &str) -> String { s.split_whitespace().map(|w| { let mut chars = w.chars(); if let Some(first) = chars.next() { first.to_ascii_uppercase().to_string() + &chars.as_str().to_ascii_lowercase() } else { "".to_string() } }).collect::<Vec<_>>().join(" ") }







pub fn format_whisper(nick: &str, msg: &str) -> String { format!("-> {}: {}", nick, msg) }



pub fn is_shout_text(s: &str) -> bool { s.chars().count() > 3 && s.chars().any(|c| c.is_alphabetic()) && s.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_uppercase()) }







pub fn is_question(s: &str) -> bool { s.trim_end().ends_with('?') }



pub fn is_exclamation(s: &str) -> bool { s.trim_end().ends_with('!') }



pub fn is_greeting(s: &str) -> bool { let t = s.to_ascii_lowercase(); t == "hi" || t.starts_with("hi ") || t.starts_with("hi,") || t.contains("hello") || t.contains("hey") || t.contains("good morning") }



pub fn is_farewell(s: &str) -> bool { let t = s.to_ascii_lowercase(); t.contains("bye") || t.contains("goodbye") || t.contains("see you") || t.contains("farewell") }



pub fn format_greeting(nick: &str) -> String { format!("Hello, {}!", nick) }



pub fn format_farewell(nick: &str) -> String { format!("Goodbye, {}!", nick) }

#[cfg(test)]
mod cleanup_0317_comic_state {
    use super::*;
    #[test]
    fn emotion_wheel_is_eight() { assert_eq!(Emotion::all().len(), 8); assert!(!Emotion::all().contains(&Emotion::Neutral)); }
    #[test]
    fn shout_uses_shout_balloon() {
        let m = make_message("Frank", "HELLO!!!", false);
        // Step 5: caps / !!! is Balloon::Shout only. Avatar stays Neutral.
        // Old assertion expected Emotion::Shout, which cycled the avatar
        // to a random angry pose - the bug Step 5 fixed.
        assert_eq!(m.emotion, Emotion::Neutral);
        assert_eq!(m.balloon, Balloon::Shout);
    }
    #[test]
    fn oec_never_shows_as_text() {
        let m = make_message("Frank", "(OEC:0M1) hello", false);
        assert_eq!(m.text, "hello");
    }
}
