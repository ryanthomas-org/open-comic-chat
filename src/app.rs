// App UI — original Comic Chat 2.5 layout, clean-room.
// Top: Comic Strip / Plain Text, Connect All, Servers, About, Save Conversation
// Center: comic panels (or plain text log)
// Bottom: Compose — Say / Think / Whisper / Action / Play Sound (sound = .wav name, not played)
// Character / Background are set in config.lua (comic.character / comic.background). ComicArt/*.AVB and *.BGB are authoritative. v0.3.17 decodes the DIBs inside the AVB/BGB (zlib + halo mask) for display; PNG sidecar is only fallback if no DIB decodes.

use crate::avb::{scan_comicart, AvbFile};
use crate::icons;
use crate::comic::{self, ComicMessage, Emotion, MessageKind};
use crate::config::{AppConfig, ServerConfig};
use crate::irc::{IrcCommand, IrcEvent};
use crate::pose::{appears_as_line, background_as_line, PoseCode};
use crate::{Control, HandleMap};
use egui::{Color32, ColorImage, Frame, RichText, ScrollArea, TextureHandle, TextureOptions, Ui};
use rand::seq::SliceRandom;
use std::collections::{HashMap, HashSet};
use tokio::sync::mpsc;

/// Install the display font into the egui context.
///
/// `ui.font` is a FAMILY name — the `family:` value fontconfig reports —
/// e.g. `"Comic Sans MS"`. It is NOT a filename (`comic.ttf`) and NOT a
/// PostScript name (`ComicSansMS`, no space). The program resolves the
/// family to a FILE, then reads that file's bytes into egui. `ui.font`
/// is also used as the egui font-data key / family-list entry, so the
/// key and the family name are the same string.
///
/// How the file is found, in order:
///   1. `ui.font_path`, if set: that exact .ttf, canonicalized
///      (realpath — symlinks resolved). Wins over everything below.
///      A leading `~/` is expanded to the home directory.
///   2. `fc-list`: runs `fc-list "<family>" --format=%{file}:%{style}\n`
///      and reads the file fontconfig reports for that family. The
///      trailing `\n` is load-bearing: without it fc-list concatenates
///      the records into one line (`.../comicbd.ttf.../comic.ttf`),
///      which no line parser can split. This is the standard Linux
///      font lookup, and it is how installs in a
///      subdir named after the family (e.g. `~/.fonts/Comic Sans
///      MS/comic.ttf`) are found without guessing paths. `fc-list` is
///      used deliberately, NOT `fc-match`: `fc-match` substitutes a
///      different font when the family is absent (it returned a URW
///      Chancery clone for "Comic Sans MS" on a machine without it),
///      while `fc-list` returns nothing unless the family really
///      matches. If `fc-list` is missing or returns nothing, fall
///      through to step 3.
///   3. Hardcoded candidate paths: `~/.fonts/Comic Sans MS/comic.ttf`,
///      `~/.local/share/fonts/Comic Sans MS/comic.ttf`, the flat
///      `~/.fonts/comic.ttf` / `~/.local/share/fonts/comic.ttf` forms,
///      and the system msttcorefonts locations.
/// Only the regular face is loaded (`comic.ttf`, not `comicbd.ttf`
/// Bold). If nothing is found, egui's built-in default stays in place
/// and a warning is logged listing everything checked. Called once from
/// main() before the app is created.
///
/// Verify with: `fc-list | grep -i "comic sans"` (inspect) and
/// `fc-list "Comic Sans MS" --format=%{file}:%{style}\n` (what step 2
/// returns — keep the trailing \n, or the records concatenate).
pub fn install_font(ctx: &egui::Context, ui: &crate::config::UiConfig) {
    let wanted = ui.font.trim();
    if wanted.is_empty() || wanted.eq_ignore_ascii_case("default") {
        crate::log::info("font", "ui.font is empty/default — keeping egui default font".to_string());
        return;
    }
    // Canonicalize (realpath) then read. Returns the canonical path +
    // bytes, so the log always shows the real file, not a symlink.
    fn read_font_file(p: &std::path::Path) -> Option<(std::path::PathBuf, Vec<u8>)> {
        let canon = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        let bytes = std::fs::read(&canon).ok()?;
        Some((canon, bytes))
    }
    // Expand a leading ~/ to the home directory; anything else passes
    // through unchanged.
    fn expand_home(p: &str) -> std::path::PathBuf {
        if let Some(rest) = p.strip_prefix("~/") {
            if let Some(home) = dirs::home_dir() { return home.join(rest); }
        }
        std::path::PathBuf::from(p)
    }
    // Standard Linux lookup: ask fontconfig which files serve this
    // family. No shell involved — the family is one argv entry, so
    // spaces in "Comic Sans MS" are safe. `fc-list` (not `fc-match`)
    // because `fc-match` substitutes another font when the family is
    // absent. Prefer the Regular face (comic.ttf over comicbd.ttf),
    // then a Book face (weight-400 under another label), then the
    // first reported file if neither exists.
    fn fc_list_file(family: &str) -> Option<std::path::PathBuf> {
        let out = std::process::Command::new("fc-list")
            .arg(family)
            .arg("--format=%{file}:%{style}\n")
            .output()
            .ok()?;
        if !out.status.success() { return None; }
        let text = String::from_utf8_lossy(&out.stdout);
        let mut first: Option<std::path::PathBuf> = None;
        let mut book: Option<std::path::PathBuf> = None;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() { continue; }
            // Split at the LAST colon: a filename may legally contain
            // a colon, but a style name never does.
            let (file, style) = line.rsplit_once(':').unwrap_or((line, ""));
            let path = std::path::PathBuf::from(file);
            if first.is_none() { first = Some(path.clone()); }
            // Style is a comma list, e.g. "Condensed Bold,Bold".
            let styles: Vec<&str> = style.split(',').map(|s| s.trim()).collect();
            if styles.contains(&"Regular") { return Some(path); }
            // "Book" is fontconfig's weight-400 name — a regular face
            // under another label. Second choice, not first.
            if book.is_none() && styles.contains(&"Book") { book = Some(path.clone()); }
        }
        book.or(first)
    }
    // Step 1: explicit ui.font_path wins.
    let mut tried: Vec<String> = Vec::new();
    if let Some(fp) = &ui.font_path {
        if !fp.trim().is_empty() {
            let p = expand_home(fp.trim());
            tried.push(format!("font_path={}", p.display()));
            if let Some((canon, bytes)) = read_font_file(&p) {
                load_font_bytes_with_chat(ctx, ui, wanted, bytes);
                crate::log::info("font", format!("loaded display font {:?} (family) from {} (via ui.font_path)", wanted, canon.display()));
                return;
            }
        }
    }
    // Step 2: fontconfig.
    if let Some(p) = fc_list_file(wanted) {
        tried.push(format!("fc-list={}", p.display()));
        if let Some((canon, bytes)) = read_font_file(&p) {
            load_font_bytes_with_chat(ctx, ui, wanted, bytes);
            crate::log::info("font", format!("loaded display font {:?} (family) from {} (via fc-list)", wanted, canon.display()));
            return;
        }
    } else {
        tried.push("fc-list=(no match / fc-list missing)".to_string());
    }
    // Step 3: hardcoded candidate paths, including the subdir-named-
    // after-the-family layout fontconfig reports (Comic Sans MS/comic.ttf).
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".fonts/Comic Sans MS/comic.ttf"));
        candidates.push(home.join(".local/share/fonts/Comic Sans MS/comic.ttf"));
        candidates.push(home.join(".fonts/comic.ttf"));
        candidates.push(home.join(".fonts/ComicSansMS.ttf"));
        candidates.push(home.join(".fonts/comic-sans-ms.ttf"));
        candidates.push(home.join(".local/share/fonts/comic.ttf"));
        candidates.push(home.join(".local/share/fonts/ComicSansMS.ttf"));
    }
    for p in [
        "/usr/share/fonts/truetype/msttcorefonts/comic.ttf",
        "/usr/share/fonts/truetype/msttcorefonts/ComicSansMS.ttf",
        "/usr/share/fonts/truetype/comic-sans-ms/comic.ttf",
        "/usr/share/fonts/truetype/mscorefonts/comic.ttf",
        "/usr/share/fonts/truetype/comic.ttf",
        "/usr/share/fonts/TTF/comic.ttf",
        "/usr/share/fonts/comic.ttf",
    ] {
        candidates.push(std::path::PathBuf::from(p));
    }
    for cand in &candidates {
        tried.push(format!("candidate={}", cand.display()));
        if let Some((canon, bytes)) = read_font_file(cand) {
            load_font_bytes_with_chat(ctx, ui, wanted, bytes);
            crate::log::info("font", format!("loaded display font {:?} (family) from {} (via candidate path)", wanted, canon.display()));
            return;
        }
    }
    crate::log::warn("font", format!("display font {:?} (family) not found — tried: {}. Keeping egui default. Fix: install the family so `fc-list {:?} --format=%{{file}}` returns a file, or set ui.font_path in config.lua to the .ttf.", wanted, tried.join("; "), wanted));
}

/// Put already-read font bytes into egui as the proportional family.
/// Chat balloon family: Comic Sans MS Bold, separate from UI Proportional.
/// UI stays Regular, balloons use this family.
pub const CHAT_FONT_FAMILY: &str = "ComicChatBold";
fn load_font_bytes(ctx: &egui::Context, family: &str, bytes: Vec<u8>) {
    // Single-font compat: delegate to the two-font loader with only the UI font.
    load_font_bytes_two(ctx, &[(family.to_owned(), bytes, false)]);
}
fn load_font_bytes_two(ctx: &egui::Context, fonts_in: &[(String, Vec<u8>, bool)]) {
    // Load all fonts in ONE FontDefinitions — calling set_fonts twice
    // would wipe the first font (old bug: chat Bold would erase UI Regular).
    let mut fonts = egui::FontDefinitions::default();
    for (family, bytes, is_chat) in fonts_in {
        fonts.font_data.insert(family.clone(), egui::FontData::from_owned(bytes.clone()));
        if *is_chat {
            fonts.families.insert(egui::FontFamily::Name(CHAT_FONT_FAMILY.into()), vec![family.clone()]);
        } else if let Some(prop) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            prop.insert(0, family.clone());
        }
    }
    ctx.set_fonts(fonts);

}

/// Resolve Chat Bold bytes: chat_font_path if set, else fc-list for the
/// base family ("Comic Sans MS" from "Comic Sans MS Bold") preferring Bold
/// style, else candidate comicbd.ttf paths. Returns (family_label, bytes).
fn resolve_chat_font_bytes(ui: &crate::config::UiConfig) -> Option<(String, Vec<u8>)> {
    let wanted_chat = ui.chat_font.trim();
    if wanted_chat.is_empty() || wanted_chat.eq_ignore_ascii_case("default") { return None; }
    if let Some(fp) = &ui.chat_font_path {
        if !fp.trim().is_empty() {
            let p = if let Some(rest) = fp.trim().strip_prefix("~/") {
                dirs::home_dir().map(|h| h.join(rest)).unwrap_or_else(|| std::path::PathBuf::from(fp.trim()))
            } else { std::path::PathBuf::from(fp.trim()) };
            if let Ok(bytes) = std::fs::read(&p) { return Some((wanted_chat.to_owned(), bytes)); }
        }
    }
    let base = wanted_chat.strip_suffix(" Bold").or_else(|| wanted_chat.strip_suffix("Bold")).unwrap_or(wanted_chat).trim();
    if let Ok(out) = std::process::Command::new("fc-list").arg(base).arg("--format=%{file}:%{style}\n").output() {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            let mut first: Option<std::path::PathBuf> = None;
            let mut bold: Option<std::path::PathBuf> = None;
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() { continue; }
                let (file, style) = line.rsplit_once(':').unwrap_or((line, ""));
                let path = std::path::PathBuf::from(file);
                if first.is_none() { first = Some(path.clone()); }
                let styles: Vec<&str> = style.split(',').map(|s| s.trim()).collect();
                if styles.iter().any(|s| s.eq_ignore_ascii_case("Bold")) && bold.is_none() { bold = Some(path.clone()); }
            }
            if let Some(p) = bold.or(first) {
                if let Ok(bytes) = std::fs::read(&p) { return Some((wanted_chat.to_owned(), bytes)); }
            }
        }
    }
    let mut cands: Vec<std::path::PathBuf> = Vec::new();
    if let Some(home) = dirs::home_dir() {
        cands.push(home.join(".fonts/Comic Sans MS/comicbd.ttf"));
        cands.push(home.join(".local/share/fonts/Comic Sans MS/comicbd.ttf"));
        cands.push(home.join(".fonts/comicbd.ttf"));
        cands.push(home.join(".local/share/fonts/comicbd.ttf"));
    }
    for p in ["/usr/share/fonts/truetype/msttcorefonts/comicbd.ttf", "/usr/share/fonts/truetype/msttcorefonts/ComicSansMS-Bold.ttf", "/usr/share/fonts/truetype/comic-sans-ms/comicbd.ttf"] {
        cands.push(std::path::PathBuf::from(p));
    }
    for cand in cands {
        if let Ok(bytes) = std::fs::read(&cand) { return Some((wanted_chat.to_owned(), bytes)); }
    }
    None
}
fn load_font_bytes_with_chat(ctx: &egui::Context, ui: &crate::config::UiConfig, wanted: &str, bytes: Vec<u8>) {
    let mut items: Vec<(String, Vec<u8>, bool)> = vec![(wanted.to_owned(), bytes, false)];
    if let Some((chat_label, chat_bytes)) = resolve_chat_font_bytes(ui) {
        let chat_key = if chat_label == wanted { format!("{} Bold", chat_label) } else { chat_label };
        items.push((chat_key, chat_bytes, true));
    }
    load_font_bytes_two(ctx, &items);
}


#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ViewMode { Comic, PlainText }

#[derive(Default, PartialEq)]
pub struct ChannelState {
    pub messages: Vec<ComicMessage>,
    pub text_log: Vec<(String, String)>,
    pub members: Vec<String>,
    /// Channel operators (hosts) — @ in NAMES, MODE +o. Hammer icon.
    pub ops: Vec<String>,
    /// Away members — nick -> away message. Coffee cup icon.
    pub away: HashMap<String, String>,
    pub char_map: HashMap<String, (String, Option<String>)>, // nick -> (AVB name, URL)
    /// First NAMES after a join has not yet fired: when it does, emit
    /// the room-entry title card (Starring pane) once. Armed on join
    /// (same dedupe as "Now chatting in"), cleared when the card is
    /// emitted. See emit_title_card.
    pub title_pending: bool,
    pub bg_map: HashMap<String, (String, Option<String>)>,
}
#[allow(dead_code)]
pub struct ServerState {
    pub config: ServerConfig,
    pub connected: bool,
    pub status: String,
    pub channels: HashMap<String, ChannelState>,
}

#[allow(dead_code)]
pub struct ComicChatApp {
    pub cfg: AppConfig,
    pub event_rx: mpsc::UnboundedReceiver<IrcEvent>,
    pub handles: HandleMap,
    pub control_tx: mpsc::UnboundedSender<Control>,
    pub event_tx: mpsc::UnboundedSender<IrcEvent>,
    pub servers: HashMap<String, ServerState>,
    pub active_server: String,
    pub active_channel: String,
    pub input: String,
    pub input_kind: MessageKind,
    pub whisper_target: Option<String>,
    pub panel_cast: Vec<String>, // clicked members to include in next panel (Ctrl-click = multi)
    /// Our actual nick per server, as the server sees it (learned from
    /// 001 / JOIN echo — the server may truncate the configured nick).
    /// See my_nick_for(): this is our ONE identity per server.
    pub my_nick_by_server: HashMap<String, String>,
    /// Focus the compose input after sending (Enter / Send), so typing
    /// continues immediately with the cursor in the field.
    pub focus_compose: bool,
    /// Cast selected for the next <Chr> pose-only message. Set when
    /// sending <Chr>, consumed by the local echo in drain_events.
    pub pending_chr_cast: Vec<String>,
    /// Servers window open — read-only list (Connect / Disconnect).
    /// Servers are configured in config.lua, never in the GUI.
    pub show_servers: bool,
    /// About window open (Help > About).
    pub show_about: bool,
    /// Cached About logo texture from `ui.about_logo`. None = not set
    /// or failed to load (placeholder shown). Loaded once when About
    /// opens, not every frame.
    pub about_logo_texture: Option<TextureHandle>,
    pub view_mode: ViewMode,
    pub wheel_emotion: Emotion,
    pub wheel_strength: f32,
    pub wheel_frozen: bool,
    pub neutral_cycle: usize,
    pub avb_chars: Vec<AvbFile>,
    pub avb_bgs: Vec<AvbFile>,
    pub char_textures: HashMap<String, TextureHandle>,
    pub bg_textures: HashMap<String, TextureHandle>,
    pub save_status: String,
    pub copy_status: String,
    pub sound_name: String,
    /// Negative cache: character names whose PNG sidecar failed to
    /// load, so char_texture does not retry every frame.
    pub failed_char_textures: HashSet<String>,
    /// Same, for backgrounds.
    pub failed_bg_textures: HashSet<String>,
    /// Last sound-play result, shown under Play Sound (path + error).
    pub sound_status: String,
    /// Whisper box: private messages from people OUTSIDE the room
    /// (people in the room get dashed whisper bubbles in the comic).
    /// (nick, text) — the text as shown, pose code stripped.
    pub whisper_lines: Vec<(String, String)>,
    /// Reply typed in the Whisper box.
    pub whisper_reply: String,
    /// Whisper box window open.
    pub show_whisper_box: bool,
    /// Who a Whisper-box reply goes to (the last other party).
    pub whisper_peer: Option<String>,
    /// Unread private messages from outside the room (badge on the
    /// Whisper box button; cleared when the box is opened).
    pub whisper_unread: usize,
    /// Our away message, if we are away (coffee cup next to our name).
    pub away_message: Option<String>,
    /// Away message typed in the toolbar field. Default "Gone fishing."
    pub away_input: String,
}

impl ComicChatApp {
    pub fn new(cfg: AppConfig, event_tx: mpsc::UnboundedSender<IrcEvent>, event_rx: mpsc::UnboundedReceiver<IrcEvent>, handles: HandleMap, control_tx: mpsc::UnboundedSender<Control>) -> Self {
        let mut servers = HashMap::new();
        for s in &cfg.servers {
            let mut st = ServerState { config: s.clone(), connected: false, status: "Idle".into(), channels: HashMap::new() };
            for ch in &s.channels { st.channels.insert(ch.clone(), ChannelState::default()); }
            servers.insert(s.name.clone(), st);
        }
        let active_server = cfg.servers.first().map(|s| s.name.clone()).unwrap_or_default();
        let active_channel = cfg.servers.first().and_then(|s| s.channels.first().cloned()).unwrap_or_else(|| "#libera".into());
        crate::log::info("app", format!("ComicChatApp::new: starting GUI — me.nick={} active_server={} active_channel={} view=Comic character={} background={} servers={}",
            cfg.me.nick, active_server, active_channel, cfg.comic.character, cfg.comic.background, cfg.servers.len()));
        let (chars, bgs) = scan_comicart();
        crate::log::debug("app", format!("ComicChatApp::new: scan_comicart returned {} char(s) + {} bg(s) — textures will be loaded lazily on first panel draw", chars.len(), bgs.len()));
        let mut app = Self {
            cfg, event_rx, event_tx, handles, control_tx, servers, active_server, active_channel,
            input: String::new(), input_kind: MessageKind::Say, whisper_target: None, panel_cast: Vec::new(),
            my_nick_by_server: HashMap::new(), focus_compose: false, pending_chr_cast: Vec::new(),
            show_servers: false, show_about: false, about_logo_texture: None,
            view_mode: ViewMode::Comic, wheel_emotion: Emotion::Neutral, wheel_strength: 0.4, wheel_frozen: false, neutral_cycle: 0,
            avb_chars: chars, avb_bgs: bgs, char_textures: HashMap::new(), bg_textures: HashMap::new(),
            save_status: String::new(), copy_status: String::new(), sound_name: String::new(),
            away_message: None, away_input: "Gone fishing.".to_string(),
            failed_char_textures: HashSet::new(), failed_bg_textures: HashSet::new(),
            sound_status: String::new(), whisper_lines: Vec::new(), whisper_reply: String::new(),
            show_whisper_box: false, whisper_peer: None, whisper_unread: 0,
        };
        // Seed own char_map — keyed per server by the nick the server
        // will see (server-block nick, else me-block nick), NOT by
        // me.nick alone, or we appear twice (title card as me.nick,
        // messages as the server nick).
        let ch_name = app.cfg.comic.character.clone();
        let ch_url = app.cfg.comic.character_url.clone();
        let server_nicks: Vec<(String, String)> = app.cfg.servers.iter().map(|s| (s.name.clone(), s.effective_nick(&app.cfg.me.nick))).collect();
        for (name, me) in server_nicks {
            if let Some(s) = app.servers.get_mut(&name) {
                for (_, ch) in s.channels.iter_mut() { ch.char_map.insert(me.clone(), (ch_name.clone(), ch_url.clone())); }
            }
        }
        app
    }

    fn rescan(&mut self) {
        crate::log::info("app", "rescan: user clicked Rescan ComicArt — re-scanning ComicArt dirs");
        let (c, b) = scan_comicart();
        self.avb_chars = c; self.avb_bgs = b;
        // New files may now exist — clear negative caches + loaded
        // textures so the next draw retries them instead of showing
        // the stale placeholder forever.
        self.failed_char_textures.clear();
        self.failed_bg_textures.clear();
        self.char_textures.clear();
        self.bg_textures.clear();
        self.copy_status = format!("Rescanned ComicArt: {} characters, {} backgrounds", self.avb_chars.len(), self.avb_bgs.len());
    }

    /// Our nick on `server` as the server sees it: the server-assigned
    /// nick if learned (001 / JOIN echo — the server may truncate the
    /// configured nick, e.g. oarion-tests -> oarion-te), else the server
    /// block's nick, else the me block's nick. This is the ONE identity
    /// for "me" — the me-block nick is never used as an identity on its
    /// own, so we never appear in a room twice.
    fn my_nick_for(&self, server: &str) -> String {
        if let Some(n) = self.my_nick_by_server.get(server) { return n.clone(); }
        if let Some(srv) = self.cfg.servers.iter().find(|s| s.name == server) {
            return srv.effective_nick(&self.cfg.me.nick);
        }
        self.cfg.me.nick.clone()
    }
    /// Our nick on the active server.
    fn my_nick(&self) -> String { let s = self.active_server.clone(); self.my_nick_for(&s) }

    
    // NEUTRAL_POSES_0317: neutral gestures for current avatar.
    // Dynamic: distinct gestures with Emotion::Neutral, minus keyword gestures.
    // Falls back to [Neutral, DoublePoint, Shrug] if AVB can't be read.
    fn neutral_gestures_for_current(&self) -> Vec<comic::Gesture> {
        // NEUTRAL_POSES_0317: neutral cycle, keyword gestures excluded.
        // Hardcoded: dib::PoseEntry is { gesture: u8, expression, image_index }
        // with no emotion field, so AVB-table lookup needs a separate
        // u8 -> Gesture / expression -> Emotion mapping. Trekkie cycles
        // these 3. Wave / PointSelf / PointOther only via trigger_for_text().
        let _ = self;
        vec![comic::Gesture::Neutral, comic::Gesture::DoublePoint, comic::Gesture::Shrug]
    }

    fn current_pose_code(&self) -> PoseCode {
        // FIX Wire-Log-File v2: log to real debug.log via crate::log
        // FIX Wire-Coy: send path must use same pose as preview, not hardcoded Neutral.
        // If a selected / preview pose exists, it wins. Neutral cycle only for plain sends.
        // The actual selection is resolved below via preview_pose / selected_pose / pose_cycler if present.

        // POSE_WHEEL_TEXT_0317: one source of truth. analyze() keeps
        // wheel emotion + text gesture, so preview label, preview image
        // and sent pose all agree. Frozen is handled inside analyze().
        let (emotion, _balloon, mut gesture, strength) = comic::analyze(&self.input, Some((self.wheel_emotion, self.wheel_strength)), self.wheel_frozen);
        // Plain Neutral stays Neutral / Neutral here.
        // Distinct neutral images come from ComicMessage.neutral_variant
        // (pose_image_index_seeded), not from cycling Wave / PointSelf /
        // PointOther as gestures. Those gestures are only via triggers.
        // Step 4: pose.rs is single source for I/you gesture triggers.
        // analyze() already handles lol/haha/:/etc for emotion; this
        // overrides gesture only so I -> PointSelf, you -> PointOther
        // even when wheel is frozen.
        if let (_, Some(g_trig)) = crate::pose::trigger_for_text(&self.input) { gesture = g_trig; }
        let mut pose = PoseCode::from_emotion_gesture(emotion, gesture, strength);
        // Cast: panel_cast minus self, minus duplicates
        let me = self.my_nick();
        let mut cast: Vec<String> = self.panel_cast.iter().filter(|n| *n != &me).cloned().collect();
        cast.sort(); cast.dedup();
        pose = pose.with_cast(cast);
        pose
    }

    fn title_cast_for(ch: &ChannelState, me: &str) -> Vec<String> {
        ch.members.iter().filter(|m| m.as_str() != me).take(comic::TITLE_CAST_MAX_OTHERS).cloned().collect()
    }

    fn emit_title_card(&mut self, server: &str, channel: &str) {
        let mut pool: Vec<String> = comic::BUILTIN_TITLES.iter().map(|s| s.to_string()).collect();
        pool.extend(self.cfg.comic.titles.iter().filter(|t| !t.trim().is_empty()).cloned());
        let Some(title) = pool.choose(&mut rand::thread_rng()).cloned() else { return };
        let me = self.my_nick_for(server);
        let Some(s) = self.servers.get_mut(server) else { return };
        let Some(ch) = s.channels.get_mut(channel) else { return };
        // Consume the pending flag: the card fires exactly once per
        // join (the Joined handler re-arms it on rejoin).
                // Render batch: double NAMES must not emit a second title card mid-history.
        // Part removes the channel (Step 62), so rejoin starts fresh with no Title —
        // a new card there is still allowed. Same-channel double NAMES is not.
        if ch.messages.iter().any(|m| m.kind == MessageKind::Title) { ch.title_pending = false; return; }
ch.title_pending = false;
        // Starring cast: self + the first three others in NAMES order
        // (static — the card is the first frame only, see title_cast_for).
        let others: Vec<String> = Self::title_cast_for(ch, &me);
        let starring: Vec<String> = std::iter::once(me.clone()).chain(others.iter().cloned()).collect();
        // character = our AVB (the title message's speaker is us); the
        // other cast members' AVBs resolve at draw time from char_map.
        let msg = comic::make_message_full(&me, &title, true, MessageKind::Title, None, false, &self.cfg.comic.character, None, others);
        let now_line = format!("Now chatting in {channel}");
        match ch.messages.iter().rposition(|m| m.nick == "System" && m.text == now_line) {
            Some(p) => ch.messages.insert(p, msg),
            None => ch.messages.push(msg),
        }
        let log_line = format!("*** {} - starring {}", title, starring.join(", "));
        match ch.text_log.iter().rposition(|(n, t)| n == "System" && t == &now_line) {
            Some(p) => ch.text_log.insert(p, ("System".into(), log_line)),
            None => ch.text_log.push(("System".into(), log_line)),
        }
        ch.title_pending = false;
        crate::log::info("app", format!("emit_title_card: {channel} title={title:?} starring={starring:?}"));
    }

    fn drain_events(&mut self) {
        while let Ok(ev) = self.event_rx.try_recv() {
            match ev {
                IrcEvent::Connected { server } => { crate::log::info("app", format!("drain_events: Connected event for {server} — marking connected, UI will show Connected")); if let Some(s) = self.servers.get_mut(&server) { s.connected = true; s.status = "Connected".into(); } }
                IrcEvent::Disconnected { server, reason } => { crate::log::warn("app", format!("drain_events: Disconnected event for {server} reason={reason:?} — marking disconnected. If unexpected, see irc logs above for TCP/TLS/DNS failure.")); if let Some(s) = self.servers.get_mut(&server) { s.connected = false; s.status = reason; } }
                IrcEvent::Status { server, text } => { if let Some(s) = self.servers.get_mut(&server) { s.status = text; } }
                IrcEvent::Joined { server, channel } => {
                    if let Some(s) = self.servers.get_mut(&server) {
                        let ch = s.channels.entry(channel.clone()).or_default();
                        // "Now chatting in <channel>" — shown once per
                        // join, in both views: a System line in the text
                        // log, a System panel in comics mode, and the
                        // server status. Joined fires twice (001
                        // pending-join + server JOIN echo), so dedupe on
                        // the last text-log line.
                        let line = format!("Now chatting in {channel}");
                        let already = ch.text_log.last().map(|(_, t)| t == &line).unwrap_or(false);
                        // Arm the title card once per join, on the same
                        // dedupe as "Now chatting in": the JOIN echo must
                        // not re-arm it after NAMES already consumed it.
                        if !already { ch.title_pending = true; }
                        if !already {
                            // Text log + status only. Never a comic panel - no fake System comics.
                            ch.text_log.push(("System".into(), line.clone()));
                        }
                        s.status = line;
                    }
                    if server == self.active_server { self.active_channel = channel.clone(); }
                    // Auto-announce our character on join (original did this via Appears as)
                    let line = appears_as_line(&self.cfg.comic.character, self.cfg.comic.character_url.as_deref());
                    let srv = server.clone(); let ch = channel.clone();
                    self.send_appears_as(&srv, &ch, &line);
                    // Announce happens via send_appears_as which needs handles — done below via a pending flag:
                    // Instead, directly send here (we have handles):
                    // (send_appears_as already sent, so no duplicate)
                }
                IrcEvent::Members { server, channel, members, ops } => {
                    if let Some(s) = self.servers.get_mut(&server) {
                        let ch = s.channels.entry(channel.clone()).or_default();
                        ch.members = members;
                        ch.ops = ops;
                    }
                    // Title card: first NAMES after a join, once per join.
                    // NAMES is the first moment the member list is known,
                    // so the Starring cast can include others — not just us.
                    let pending = self.servers.get(&server).and_then(|s| s.channels.get(&channel)).map(|c| c.title_pending).unwrap_or(false);
                    if pending {
                        self.emit_title_card(&server, &channel);
                        // APPEARSAS_QUERY_0317: existing members never re-send
                        // their AppearsAs just because we joined (we missed
                        // theirs, sent before we arrived). Privately send
                        // ours to each existing member so they reply with
                        // theirs (MS Chat replies to a private AppearsAs).
                        // Channel announcement alone does not trigger this.
                        let my_nick = self.my_nick_for(&server);
                        let members: Vec<String> = self.servers.get(&server).and_then(|s| s.channels.get(&channel)).map(|c| c.members.clone()).unwrap_or_default();
                        let (my_char, my_url) = (self.cfg.comic.character.clone(), self.cfg.comic.character_url.clone());
                        let line = crate::pose::appears_as_line(&my_char, my_url.as_deref());
                        for m in members {
                            if m.eq_ignore_ascii_case(&my_nick) { continue; }
                            if let Ok(map) = self.handles.lock() {
                                if let Some(tx) = map.get(&server) {
                                    let _ = tx.send(IrcCommand::Whisper { target: m, text: line.clone(), pose_code: String::new() });
                                }
                            }
                        }
                    }
                }
                IrcEvent::MemberJoined { server, channel, nick } => {
                    if let Some(s) = self.servers.get_mut(&server) {
                        if let Some(ch) = s.channels.get_mut(&channel) {
                            if !ch.members.contains(&nick) { ch.members.push(nick.clone()); }
                            ch.text_log.push((nick.clone(), format!("* {nick} has joined {channel}")));
                        }
                    }
                    // 0.3.17 fix: a user who joins AFTER us never saw our
                    // Appears-as (we only sent it on our own JOIN). Re-send
                    // it now so the newcomer learns our character. Do not
                    // send it for our own JOIN (Joined already sent it).
                    if nick != self.my_nick_for(&server) {
                        let line = appears_as_line(&self.cfg.comic.character, self.cfg.comic.character_url.as_deref());
                        let srv = server.clone(); let ch = channel.clone();
                        self.send_appears_as(&srv, &ch, &line);
                    }
                }
                
            IrcEvent::Parted { server, channel } => {
                // We left (own PART echo). Drop the channel from UI.
                // If it was active, switch to another channel on that server if any.
                if let Some(s) = self.servers.get_mut(&server) {
                    s.channels.remove(&channel);
                    if self.active_server == server && self.active_channel == channel {
                        if let Some(next) = s.channels.keys().next().cloned() {
                            self.active_channel = next;
                        }
                    }
                }
                crate::log::info("app", format!("Parted {channel} on {server}"));
            }
            IrcEvent::MemberLeft { server, channel, nick } => {
                    if let Some(s) = self.servers.get_mut(&server) {
                        if channel.is_empty() { for (_, ch) in s.channels.iter_mut() { ch.members.retain(|m| m != &nick); ch.ops.retain(|m| m != &nick); ch.away.remove(&nick); } }
                        else if let Some(ch) = s.channels.get_mut(&channel) { ch.members.retain(|m| m != &nick); ch.ops.retain(|m| m != &nick); ch.away.remove(&nick); }
                    }
                }
                IrcEvent::Away { server, channel, nick, message } => {
                    // Coffee cup: Some(msg) = away, None = back.
                    // Empty channel (private CTCP AWAY / 301) applies to
                    // every channel that nick shares with us. Never
                    // creates a channel entry for a channel we are not in.
                    if let Some(s) = self.servers.get_mut(&server) {
                        let mut targets: Vec<String> = Vec::new();
                        if channel.is_empty() {
                            for (name, ch) in s.channels.iter() { if ch.members.contains(&nick) { targets.push(name.clone()); } }
                        } else if s.channels.contains_key(&channel) {
                            targets.push(channel.clone());
                        }
                        for name in targets {
                            if let Some(ch) = s.channels.get_mut(&name) {
                                match &message {
                                    Some(m) => {
                                        ch.away.insert(nick.clone(), m.clone());
                                        ch.text_log.push((nick.clone(), format!("* {nick} is away: {m}")));
                                        // Info panel: the away user alone, no balloon, no bg — same layout as a version panel.
                                        let (char_name, char_url) = ch.char_map.get(&nick).cloned().unwrap_or_else(|| (nick.clone(), None));
                                        let info = comic::make_message_full(&nick, &format!("{nick} is away: {m}"), false, MessageKind::Info, None, false, &char_name, char_url, Vec::new());
                                        ch.messages.push(info);
                                        if ch.messages.len() > 300 { ch.messages.drain(0..50); }
                                    }
                                    None => { ch.away.remove(&nick); ch.text_log.push((nick.clone(), format!("* {nick} is back"))); }
                                }
                            }
                        }
                    }
                }
                IrcEvent::VersionReply { server, nick, version } => {
                    // VERSION reply arrived as a private NOTICE — no channel on the wire.
                    // Show it in the channel we share with that nick, preferring the active one.
                    let target_channel = {
                        let fallback = self.active_channel.clone();
                        if let Some(s) = self.servers.get(&server) {
                            if let Some(ch) = s.channels.get(&fallback) {
                                if ch.members.contains(&nick) { fallback.clone() } else {
                                    s.channels.iter().find(|(_, ch)| ch.members.contains(&nick)).map(|(name, _)| name.clone()).unwrap_or(fallback)
                                }
                            } else {
                                s.channels.iter().find(|(_, ch)| ch.members.contains(&nick)).map(|(name, _)| name.clone()).unwrap_or(fallback)
                            }
                        } else { fallback }
                    };
                    let (char_name, char_url) = {
                        let mut found = (nick.clone(), None);
                        if let Some(s) = self.servers.get(&server) {
                            if let Some(ch) = s.channels.get(&target_channel) {
                                if let Some((n, u)) = ch.char_map.get(&nick) { found = (n.clone(), u.clone()); }
                            }
                        }
                        found
                    };
                    let text = if version.trim().is_empty() { format!("{nick}'s version: (no reply)") } else { format!("{nick}'s version: {version}") };
                    let info = comic::make_message_full(&nick, &text, false, MessageKind::Info, None, false, &char_name, char_url, Vec::new());
                    if let Some(s) = self.servers.get_mut(&server) {
                        let ch = s.channels.entry(target_channel.clone()).or_default();
                        ch.messages.push(info);
                        ch.text_log.push((nick.clone(), format!("* {nick} version: {version}")));
                        if ch.messages.len() > 300 { ch.messages.drain(0..50); }
                    }
                }
                IrcEvent::OpChanged { server, channel, nick, is_op } => {
                    if let Some(s) = self.servers.get_mut(&server) {
                        if let Some(ch) = s.channels.get_mut(&channel) {
                            if is_op { if !ch.ops.contains(&nick) { ch.ops.push(nick); } }
                            else { ch.ops.retain(|m| m != &nick); }
                        }
                    }
                }
                IrcEvent::MemberRenamed { server, old_nick, new_nick } => {
                    // NICK: move members / ops / away / character /
                    // background entries to the new nick (mIRC Away.mrc
                    // does the same for its away list).
                    if let Some(s) = self.servers.get_mut(&server) {
                        for (_, ch) in s.channels.iter_mut() {
                            if let Some(i) = ch.members.iter().position(|m| m == &old_nick) { ch.members[i] = new_nick.clone(); }
                            if let Some(i) = ch.ops.iter().position(|m| m == &old_nick) { ch.ops[i] = new_nick.clone(); }
                            if let Some(msg) = ch.away.remove(&old_nick) { ch.away.insert(new_nick.clone(), msg); }
                            if let Some(v) = ch.char_map.remove(&old_nick) { ch.char_map.insert(new_nick.clone(), v); }
                            if let Some(v) = ch.bg_map.remove(&old_nick) { ch.bg_map.insert(new_nick.clone(), v); }
                        }
                    }
                }
                IrcEvent::MyNick { server, nick } => {
                    // The server assigned us a nick (truncation / change).
                    // This is now our ONE identity on that server. Move any
                    // state keyed by the old nick to the new nick, and drop
                    // the old nick from members so we never appear twice.
                    let old = self.my_nick_for(&server);
                    if old != nick {
                        crate::log::info("app", format!("drain_events: MyNick on {server}: {old:?} -> {nick:?}"));
                        self.my_nick_by_server.insert(server.clone(), nick.clone());
                        if let Some(s) = self.servers.get_mut(&server) {
                            for (_, ch) in s.channels.iter_mut() {
                                if ch.members.contains(&old) {
                                    if ch.members.contains(&nick) { ch.members.retain(|m| m != &old); }
                                    else if let Some(i) = ch.members.iter().position(|m| m == &old) { ch.members[i] = nick.clone(); }
                                }
                                ch.ops.retain(|m| m != &old);
                                if let Some(v) = ch.char_map.remove(&old) { ch.char_map.insert(nick.clone(), v); }
                                if let Some(v) = ch.bg_map.remove(&old) { ch.bg_map.insert(nick.clone(), v); }
                                if let Some(v) = ch.away.remove(&old) { ch.away.insert(nick.clone(), v); }
                            }
                        }
                    }
                }
                IrcEvent::AppearsAs { server, channel, nick, character, url } => {
                    crate::log::debug("app", format!("drain_events: AppearsAs {nick} -> {character:?} url={url:?} in {channel} on {server} — updating char_map, auto-download may fetch AVB if missing"));
                    // Hoisted: my_nick_for borrows *self, so it must run
                    // before the mutable borrow of self.servers below.
                    let my = self.my_nick_for(&server);
                    if let Some(s) = self.servers.get_mut(&server) {
                        // Private AppearsAs (PRIVMSG <our nick> :# Appears as ...) arrives with
                        // channel == our nick, not #garage. Mirror it into every joined channel
                        // on this server so existing users get a character in #garage, not a silhouette.
                        if channel.eq_ignore_ascii_case(&my) {
                            let targets: Vec<String> = s.channels.keys().cloned().collect();
                            for tc in targets {
                                if let Some(och) = s.channels.get_mut(&tc) {
                                    och.char_map.insert(nick.clone(), (character.clone(), url.clone()));
                                }
                            }
                        }
                        let ch = s.channels.entry(channel.clone()).or_default();
                        ch.char_map.insert(nick.clone(), (character.clone(), url.clone()));
                        // Reply privately with our own AppearsAs so the joiner
                        // learns our character. MS Chat does this; without it
                        // a newcomer sees us as silhouette. Private replies
                        // (channel == my) do NOT trigger a reply — no loop.
                        if nick != my && !channel.eq_ignore_ascii_case(&my) {
                            let (my_char, my_url) = (self.cfg.comic.character.clone(), self.cfg.comic.character_url.clone());
                            // APPEARSAS_0317: use dot format (Name.URL), same as
                            // join announcement. Space format was ignored by
                            // MS Chat, so the joiner never learned our character.
                            let line = crate::pose::appears_as_line(&my_char, my_url.as_deref());
                            let server_c = server.clone();
                            let nick_c = nick.clone();
                            if let Ok(map) = self.handles.lock() {
                                if let Some(tx) = map.get(&server_c) {
                                    let _ = tx.send(IrcCommand::Whisper { target: nick_c, text: line, pose_code: String::new() });
                                }
                            }
                        }
                        let url_txt = url.as_deref().unwrap_or("");
                        ch.text_log.push((nick.clone(), format!("* {nick} appears as {character} {url_txt}")));
                        if nick != my && self.cfg.comic.auto_download {
                            if let Some(u) = &url {
                                // No HTTP client in this build — give the user the exact curl to fetch it.
                                {
                                // Step 112: actually fetch it once. If the file already
                                // exists locally, do nothing. Otherwise run curl
                                // synchronously (one-time per new character) and
                                // rescan so the peer's sprite appears next frame.
                                let dest = std::path::PathBuf::from("ComicArt").join(format!("{character}.AVB"));
                                let need = !dest.exists() && !self.avb_chars.iter().any(|f| f.name.eq_ignore_ascii_case(&character));
                                if need {
                                    let _ = std::fs::create_dir_all("ComicArt");
                                    let out = std::process::Command::new("curl").arg("-L").arg("-s").arg("-o").arg(&dest).arg(u).output();
                                    match out {
                                        Ok(o) if o.status.success() && dest.exists() => {
                                            self.failed_char_textures.clear();
                                            self.failed_bg_textures.clear();
                                            self.char_textures.clear();
                                            self.bg_textures.clear();
                                            let (c, b) = scan_comicart();
                                            self.avb_chars = c; self.avb_bgs = b;
                                            ch.text_log.push(("System".into(), format!("Downloaded {character}.AVB from {u}")));
                                        }
                                        _ => {
                                            ch.text_log.push(("System".into(), format!("Auto-download: {character} not in ComicArt — run: curl -o ComicArt/{character}.AVB \"{u}\"")));
                                        }
                                    }
                                }
                            }
                            }
                        }
                    }
                }
                IrcEvent::BackgroundAs { server, channel, nick, background, url } => {
                    crate::log::debug("app", format!("drain_events: BackgroundAs {nick} -> {background:?} url={url:?} in {channel} on {server} — updating bg_map"));
                    if let Some(s) = self.servers.get_mut(&server) {
                        let ch = s.channels.entry(channel.clone()).or_default();
                        ch.bg_map.insert(nick.clone(), (background.clone(), url.clone()));
                        ch.text_log.push((nick.clone(), format!("* {nick} background as {background}")));
                    }
                }
                IrcEvent::Message { server, channel, nick, text, is_me, kind } => {
                    // Private message: the "channel" is a nick, not a
                    // channel (no # & + ! prefix). Whispers from people
                    // in the room become dashed bubbles in the room's
                    // comic; whispers from people outside it go to the
                    // Whisper box (the original's behaviour).
                    if !matches!(channel.chars().next(), Some('#' | '&' | '+' | '!')) {
                        self.route_private(&server, &channel, &nick, &text, is_me, kind);
                        continue;
                    }
                    // Character for this nick (from Appears as, or nick fallback)
                    let (char_name, char_url) = {
                        let mut found = (nick.clone(), None);
                        if let Some(s) = self.servers.get(&server) {
                            if let Some(ch) = s.channels.get(&channel) {
                                if let Some((n, u)) = ch.char_map.get(&nick) { found = (n.clone(), u.clone()); }
                            }
                        }
                        found
                    };
                    let wheel = if is_me { Some((self.wheel_emotion, self.wheel_strength)) } else { None };
                    let frozen = is_me && self.wheel_frozen;
                    // For incoming, pose code in text wins; for outgoing, wheel is fallback but pose code in wire text also wins
                    let msg = comic::make_message_full(&nick, &text, is_me, kind, wheel, frozen, &char_name, char_url, Vec::new());
                    // <Chr> from another user (or our own echo) is NOT a
                    // bubble — it is a pose update in the active (last)
                    // panel. If the sender is already in the last panel,
                    // just their pose changes; if others are there and the
                    // sender is not, the sender is added to it.
                    if msg.text.trim() == "<Chr>" {
                        let cast = if is_me { std::mem::take(&mut self.pending_chr_cast) } else { msg.cast.clone() };
                        self.apply_chr_pose(&server, &channel, &nick, &msg, cast);
                        continue;
                    }
                    if let Some(s) = self.servers.get_mut(&server) {
                        let ch = s.channels.entry(channel.clone()).or_default();
                        ch.messages.push(msg);
                        ch.text_log.push((nick, text.clone()));
                        if ch.messages.len() > 300 { ch.messages.drain(0..50); }
                    }
                    // A received (sound) message plays from our Sound
                    // Search Path. Our own echo is skipped: send_sound
                    // / send_current already played it locally.
                    if !is_me { self.maybe_play_sound(&text); }
                }
                IrcEvent::Action { server, channel, nick, text, is_me } => {
                    let (char_name, char_url) = {
                        let mut found = (nick.clone(), None);
                        if let Some(s) = self.servers.get(&server) {
                            if let Some(ch) = s.channels.get(&channel) {
                                if let Some((n, u)) = ch.char_map.get(&nick) { found = (n.clone(), u.clone()); }
                            }
                        }
                        found
                    };
                    let msg = comic::make_message_full(&nick, &text, is_me, MessageKind::Action, None, false, &char_name, char_url, Vec::new());
                    if let Some(s) = self.servers.get_mut(&server) {
                        let ch = s.channels.entry(channel.clone()).or_default();
                        ch.messages.push(msg);
                        ch.text_log.push((nick.clone(), format!("* {nick} {text}")));
                    }
                }
            }
        }
    }

    fn send_appears_as(&mut self, server: &str, channel: &str, line: &str) {
        let cmd = IrcCommand::AppearsAs { channel: channel.to_string(), line: line.to_string() };
        if let Ok(map) = self.handles.lock() {
            if let Some(tx) = map.get(server) { let _ = tx.send(cmd); }
        }
    }
    fn send_background_as(&mut self, server: &str, channel: &str, line: &str) {
        let cmd = IrcCommand::BackgroundAs { channel: channel.to_string(), line: line.to_string() };
        if let Ok(map) = self.handles.lock() {
            if let Some(tx) = map.get(server) { let _ = tx.send(cmd); }
        }
    }

    fn send_version_query(&mut self, server: &str, target: &str) {
        let cmd = IrcCommand::VersionQuery { target: target.to_string() };
        if let Ok(map) = self.handles.lock() {
            if let Some(tx) = map.get(server) { let _ = tx.send(cmd); }
        }
        self.copy_status = format!("Asked {target} for their version…");
    }

    fn send_current(&mut self) {
        // FIX Wire-Coy: log at send time. See debug.log for WIRE_POSE.
        crate::log::info("WIRE_POSE", format!("send fn=send_current"));

        let __variant = self.neutral_cycle as u64;
        // NEUTRAL_CYCLE_SEND_0317: capture before any pose is used. Cycle moves once,
        // after send, so preview == wire == local.
        let __cycle_should_advance = { let __p = self.current_pose_code(); __p.emotion() == Emotion::Neutral && !self.wheel_frozen };
        // IRC is line-based: a pasted \r / \n must never split one PRIVMSG
        // into two wire lines (injection). Replace with space.
        let text = crate::irc::sanitize_outgoing_line(self.input.trim());
        // Empty Enter = <Chr>: pose-only, NOT a bubble. Microsoft Chat:
        // it updates our pose in the last panel, or adds us to it if
        // others are there and we are not. Members selected in the
        // Members list (panel_cast) are included via pending_chr_cast,
        // consumed by the local echo in drain_events.
        if text.is_empty() {
            crate::log::info("app", format!("send_current: empty input — sending <Chr> pose-only, cast={:?}", self.panel_cast));
            // CHR_CAST_0317: build the pose BEFORE clearing panel_cast.
            // Clearing first made pose.cast empty, so the wire <Chr> had
            // no Tnick and peers never added the included user. Local echo
            // worked via pending_chr_cast, which masked the wire bug.
            let pose = self.current_pose_code();
                crate::log::info("WIRE_POSE", format!("send_current pose={:?}", pose));
            self.pending_chr_cast = self.panel_cast.clone();
            self.panel_cast.clear();
            let pose_str = pose.to_wire_string(&self.cfg.comic.pose_style);
                crate::log::info("WIRE_POSE", format!("send_current pose_str={:?}", pose_str));
            let channel = self.active_channel.clone();
            let server = self.active_server.clone();
            let sent = {
                if let Ok(map) = self.handles.lock() {
                    if let Some(tx) = map.get(&server) {
                        tx.send(IrcCommand::SendMessage { channel: channel.clone(), text: "<Chr>".to_string(), pose_code: pose_str }).is_ok()
                    } else { false }
                } else { false }
            };
            if !sent {
                // Offline: no echo will come, so apply the pose update locally.
                let me = self.my_nick();
                let (char_name, char_url) = (self.cfg.comic.character.clone(), self.cfg.comic.character_url.clone());
                let probe = comic::make_message_full(&me, "<Chr>", true, MessageKind::Say, Some((self.wheel_emotion, self.wheel_strength)), self.wheel_frozen, &char_name, char_url, Vec::new());
                let cast = std::mem::take(&mut self.pending_chr_cast);
                self.apply_chr_pose(&server, &channel, &me, &probe, cast);
            }
            if !self.wheel_frozen {
                self.wheel_emotion = Emotion::Neutral;
                self.wheel_strength = 0.4;
            }
            self.input.clear();
        if __cycle_should_advance { self.neutral_cycle = self.neutral_cycle.wrapping_add(1); } // NEUTRAL_CYCLE_SEND_0317: advance once on send

            self.focus_compose = true;
            return;
        }
        crate::log::info("app", format!("send_current: kind={:?} server={} channel={} whisper_target={:?} cast={:?} preview={:?}",
            self.input_kind.label(), self.active_server, self.active_channel, self.whisper_target, self.panel_cast, crate::log::preview_redacted(&text)));
        // Slash commands handled locally, never sent as chat:
        // /away [message] — away with that message. Bare /away toggles:
        //   away -> back (FAQ: "type: /away" to return), not away ->
        //   away with the default "Gone fishing."
        // /back — back.
        if text.eq_ignore_ascii_case("/back") || text.get(..6).map_or(false, |p| p.eq_ignore_ascii_case("/back ")) {
            self.set_away(None);
            self.input.clear();
        if __cycle_should_advance { self.neutral_cycle = self.neutral_cycle.wrapping_add(1); } // NEUTRAL_CYCLE_SEND_0317: advance once on send

            self.focus_compose = true;
            return;
        }
        if text.eq_ignore_ascii_case("/away") || text.get(..6).map_or(false, |p| p.eq_ignore_ascii_case("/away ")) {
            let arg = text.get(..5).filter(|p| p.eq_ignore_ascii_case("/away")).map(|_| &text[5..]).unwrap().trim();
            if arg.is_empty() {
                if self.away_message.is_some() { self.set_away(None); }
                else { self.set_away(Some("Gone fishing.".to_string())); }
            } else {
                self.set_away(Some(arg.to_string()));
            }
            self.input.clear();
        if __cycle_should_advance { self.neutral_cycle = self.neutral_cycle.wrapping_add(1); } // NEUTRAL_CYCLE_SEND_0317: advance once on send

            self.focus_compose = true;
            return;
        }
        // /version [nick] — ask a nick for their client version (CTCP VERSION).
        // Bare /version uses the whisper target, else the selected member.
        if text.eq_ignore_ascii_case("/version") || text.get(..9).map_or(false, |p| p.eq_ignore_ascii_case("/version ")) {
            let arg = text.get(8..).unwrap_or("").trim();
            let target = if !arg.is_empty() { arg.split_whitespace().next().unwrap_or("").to_string() } else if let Some(t) = self.whisper_target.clone() { t } else if let Some(t) = self.panel_cast.first().cloned() { t } else { String::new() };
            if target.is_empty() {
                self.copy_status = "Usage: /version <nick> — or select a member first.".to_string();
            } else {
                let server = self.active_server.clone();
                self.send_version_query(&server, &target);
            }
            self.input.clear();
        if __cycle_should_advance { self.neutral_cycle = self.neutral_cycle.wrapping_add(1); } // NEUTRAL_CYCLE_SEND_0317: advance once on send

            self.focus_compose = true;
            return;
        }
        // Step 1 (0.3.17, review 3.9): Unknown slash commands are NEVER sent as chat.
        // Before this fix, any "/xxx" not handled above fell through to SendMessage
        // and was posted to the channel as public chat — including
        // "/msg NickServ IDENTIFY <password>".
        // Known slash inputs that may pass through: "/me <text>" and "/join <channel>"
        // when the input kind is Say (handled downstream in irc.rs SendMessage).
        // "/away" and "/back" already returned above. Everything else starting
        // with '/' is blocked here. Only the command name is logged/shown,
        // never the args, so a mistyped password is not written to the log or
        // the text log. "//" escape (send a literal leading '/') is NOT yet
        // implemented; "//..." is blocked as unknown for now.
        if text.starts_with('/') {
            let cmd_name = text.split_whitespace().next().unwrap_or("/").to_string();
            let has_arg = text.splitn(2, ' ').nth(1).map(|a| !a.trim().is_empty()).unwrap_or(false);
            let is_known = (cmd_name.eq_ignore_ascii_case("/me") || cmd_name.eq_ignore_ascii_case("/join")) && has_arg && self.input_kind == MessageKind::Say;
            if !is_known {
                crate::log::warn("app", format!("send_current: blocked unknown slash command {cmd_name:?} — not sent as chat"));
                let line = format!("Unknown command: {cmd_name} — not sent.");
                self.copy_status = line.clone();
                if let Some(s) = self.servers.get_mut(&self.active_server) {
                    if let Some(ch) = s.channels.get_mut(&self.active_channel) {
                        ch.text_log.push(("System".into(), line));
                    }
                }
                self.input.clear();
        if __cycle_should_advance { self.neutral_cycle = self.neutral_cycle.wrapping_add(1); } // NEUTRAL_CYCLE_SEND_0317: advance once on send

                self.focus_compose = true;
                return;
            }
        }
        let kind = self.input_kind;
        // A (sound) message we send plays locally. Whisper is the
        // exception: its server echo comes back through route_private,
        // which plays it — playing here too would play it twice.
        // /part [channel] — bare /part leaves the current channel.
        // Was ignored (Step 34 stub). Explicit arg wins, bare uses active_channel.
        if text == "/part" || text.starts_with("/part ") {
            let arg = text.strip_prefix("/part").unwrap().trim();
            let target = if arg.is_empty() {
                self.active_channel.clone()
            } else {
                let first = arg.split_whitespace().next().unwrap_or("");
                if first.starts_with('#') || first.starts_with('&') || first.starts_with('+') || first.starts_with('!') {
                    first.to_string()
                } else if first.is_empty() {
                    self.active_channel.clone()
                } else {
                    format!("#{first}")
                }
            };
            if !target.is_empty() {
                let server = self.active_server.clone();
                if let Ok(map) = self.handles.lock() {
                    if let Some(tx) = map.get(&server) { let _ = tx.send(IrcCommand::Part { channel: target.clone() }); }
                }
                crate::log::info("app", format!("send_current: /part -> PART {target} on {server}"));
            }
            self.input.clear();
        if __cycle_should_advance { self.neutral_cycle = self.neutral_cycle.wrapping_add(1); } // NEUTRAL_CYCLE_SEND_0317: advance once on send

            self.focus_compose = true;
            return;
        }
        if kind != MessageKind::Whisper { self.maybe_play_sound(&text); }
        let channel = self.active_channel.clone();
        let server = self.active_server.clone();
        let pose = self.current_pose_code();
            crate::log::info("WIRE_POSE", format!("send_current pose={:?}", pose));
        let pose_str = pose.to_wire_string(&self.cfg.comic.pose_style);
            crate::log::info("WIRE_POSE", format!("send_current pose_str={:?}", pose_str));
        // Cast is on the wire as Tnick in the pose code (see pose.rs
        // to_wire_string) and stored on the local message via the
        // pose's `cast` field.
        let cmd = match kind {
            MessageKind::Say => IrcCommand::SendMessage { channel: channel.clone(), text: text.clone(), pose_code: pose_str.clone() },
            MessageKind::Think => IrcCommand::Think { channel: channel.clone(), text: text.clone(), pose_code: pose_str.clone() },
            MessageKind::Whisper => {
                if let Some(t) = self.whisper_target.clone() { IrcCommand::Whisper { target: t, text: text.clone(), pose_code: pose_str.clone() } }
                else { IrcCommand::SendMessage { channel: channel.clone(), text: text.clone(), pose_code: pose_str.clone() } }
            }
            MessageKind::Action => IrcCommand::Action { channel: channel.clone(), text: text.clone(), pose_code: pose_str.clone() },
            // Unreachable: title cards are local-only, never composed
            // (input_kind is only set from the four kind buttons). This
            // arm exists so the match stays total.
            MessageKind::Title | MessageKind::Info => IrcCommand::SendMessage { channel: channel.clone(), text: text.clone(), pose_code: pose_str.clone() },
        };
        let sent = {
            if let Ok(map) = self.handles.lock() {
                if let Some(tx) = map.get(&server) { tx.send(cmd).is_ok() } else { false }
            } else { false }
        };
        if sent {
            crate::log::debug("app", format!("send_current: queued to IRC handle for {} — wire will be PRIVMSG with pose {}", self.active_server, pose_str));
        }
        if !sent {
            crate::log::warn("app", format!("send_current: no IRC handle for {} (not connected) — showing locally as offline demo so panel still renders. Fix: Connect All / check server status, see irc logs for connect failure.", self.active_server));
            // Offline demo: show locally with the same pose code so the panel looks identical
            let wire = if pose_str.is_empty() { text.clone() } else { format!("{pose_str} {text}") };
                crate::log::info("WIRE_POSE", format!("send_current wire={:?}", wire));
            let (char_name, char_url) = (self.cfg.comic.character.clone(), self.cfg.comic.character_url.clone());
            let me = self.my_nick();
            let msg = comic::make_message_full(&me, &wire, true, kind, Some((self.wheel_emotion, self.wheel_strength)), self.wheel_frozen, &char_name, char_url, self.panel_cast.clone());
            if let Some(s) = self.servers.get_mut(&server) {
                let ch = s.channels.entry(channel).or_default();
                ch.messages.push(msg);
            }
        }
        // Panel cast is for the NEXT panel only — clear after sending (original behaviour)
        self.panel_cast.clear();
        if !self.wheel_frozen {
            self.wheel_emotion = Emotion::Neutral;
            self.wheel_strength = 0.4;
        }
        self.input.clear();
        if __cycle_should_advance { self.neutral_cycle = self.neutral_cycle.wrapping_add(1); } // NEUTRAL_CYCLE_SEND_0317: advance once on send

        self.focus_compose = true;
    }

    /// Apply a <Chr> pose-only message: update the sender's pose in the
    /// last (active) panel, or add the sender (and `cast`) to it.
    /// Never creates a bubble. `probe` is the parsed <Chr> message
    /// (emotion / gesture / wire CAST); `cast` is the effective cast
    /// (for our own echo, the members selected in the Members list).
    fn apply_chr_pose(&mut self, server: &str, channel: &str, nick: &str, probe: &comic::ComicMessage, cast: Vec<String>) {
        let Some(s) = self.servers.get_mut(server) else { return; };
        let Some(ch) = s.channels.get_mut(channel) else { return; };
        crate::log::info("app", format!("apply_chr_pose: {nick} in {channel} on {server} cast={cast:?} emotion={:?} gesture={:?}", probe.emotion, probe.gesture));
        // Last panel, as drawn (same grouping as draw_comics).
        let panels = comic::group_into_panels(&ch.messages, self.cfg.comic.messages_per_panel);
        let last = panels.last();
        let in_last_speaker = last.map(|p| p.messages.iter().any(|m| m.nick == nick) || p.actions.iter().any(|m| m.nick == nick)).unwrap_or(false);
        let in_last_cast = last.map(|p| p.messages.iter().any(|m| m.cast.iter().any(|c| c == nick))).unwrap_or(false);
        if in_last_speaker {
            // Already in the last panel: just the pose changes. The
            // sender's most recent message IS the one in the last
            // panel (one balloon per character per panel, chronological).
            if let Some(m) = ch.messages.iter_mut().rev().find(|m| m.nick == nick) {
                m.emotion = probe.emotion;
                m.gesture = probe.gesture;
                m.strength = probe.strength;
                m.pose = probe.pose.clone();
            }
            return;
        }
        if in_last_cast {
            // Already in the last panel as a silent cast member; there
            // is no message to attach a pose to, so nothing to update.
            return;
        }
        // Not in the last panel: add the sender (and cast) to it, by
        // appending to the cast of the most recent Say/Think/Whisper
        // message (which is in the last panel). If there is no such
        // message (empty room / only title card), start a new panel
        // with a pose-only message (empty text = no balloon).
        if let Some(m) = ch.messages.iter_mut().rev().find(|m| m.nick != "System" && m.kind != MessageKind::Title && m.kind != MessageKind::Action && m.kind != MessageKind::Info) {
            if !m.cast.iter().any(|c| c == nick) { m.cast.push(nick.to_string()); }
            for c in &cast {
                if c != nick && !m.cast.iter().any(|x| x == c) { m.cast.push(c.clone()); }
            }
            return;
        }
        let (char_name, char_url) = ch.char_map.get(nick).cloned().unwrap_or_else(|| (nick.to_string(), None));
        let mut full_cast: Vec<String> = cast.clone();
        full_cast.retain(|c| c != nick);
        let msg = comic::make_message_full(nick, "", probe.is_me, MessageKind::Say, None, false, &char_name, char_url, full_cast);
        ch.messages.push(msg);
    }

    /// Set / clear our away state. Updates our own coffee-cup entry in
    /// every channel on the active server, logs it, and sends
    /// IrcCommand::SetAway (standard AWAY + CTCP AWAY to all channels).
    fn set_away(&mut self, message: Option<String>) {
        let server = self.active_server.clone();
        let me = self.my_nick();
        self.away_message = message.clone();
        if let Some(s) = self.servers.get_mut(&server) {
            for (_, ch) in s.channels.iter_mut() {
                match &message {
                    Some(m) => { ch.away.insert(me.clone(), m.clone()); }
                    None => { ch.away.remove(&me); }
                }
            }
        }
        let line = match &message {
            Some(m) => format!("You are away: {m}"),
            None => "You are back.".to_string(),
        };
        crate::log::info("app", format!("set_away: server={server} message={message:?}"));
        self.copy_status = line.clone();
        if let Some(s) = self.servers.get_mut(&server) {
            if let Some(ch) = s.channels.get_mut(&self.active_channel) {
                ch.text_log.push(("System".into(), line));
            }
        }
        if let Ok(map) = self.handles.lock() {
            if let Some(tx) = map.get(&server) { let _ = tx.send(IrcCommand::SetAway { message }); }
        }
    }

    fn send_sound(&mut self) {
        let name = self.sound_name.trim().to_string();
        if name.is_empty() { return; }
        // Original: Play Sound sends "(sound.wav)" as a message; receivers play the .wav
        // from their own Sound Search Path if they have the same filename.
        // We play it locally too (the server echo is skipped in drain_events).
        self.play_sound(&name);
        let text = format!("({name})");
        let channel = self.active_channel.clone();
        let server = self.active_server.clone();
        let pose = self.current_pose_code();
        let cmd = IrcCommand::SendMessage { channel: channel.clone(), text, pose_code: pose.to_wire_string(&self.cfg.comic.pose_style) };
        if let Ok(map) = self.handles.lock() {
            if let Some(tx) = map.get(&server) { let _ = tx.send(cmd); }
        }
        self.sound_name.clear();
    }

    /// Route a private message (the "channel" is a nick, not a
    /// channel). The other party is the target for our own echo,
    /// the sender for an incoming DM. If they are in the room we
    /// are in, the message becomes a dashed Whisper bubble in that
    /// room's comic; otherwise it goes to the text Whisper box
    /// (the original's behaviour for whispers from outside the
    /// room). A `(sound)` message plays either way.
    fn route_private(&mut self, server: &str, channel: &str, nick: &str, text: &str, is_me: bool, kind: MessageKind) {
        let other = if is_me { channel.to_string() } else { nick.to_string() };
        // Our own whisper echo carries a "(whisper to X) " prefix
        // (irc.rs) — strip it for display. Both branches also
        // strip the pose code, so the Whisper box shows clean text.
        let mut display = text.to_string();
        if is_me && kind == MessageKind::Whisper {
            let prefix = format!("(whisper to {channel}) ");
            if let Some(rest) = display.strip_prefix(&prefix) { display = rest.to_string(); }
        }
        let clean = crate::pose::parse_pose_code(&display).map(|(c, _)| c).unwrap_or_else(|| display.clone());
        // <Chr> whispered to us is a pose update, not a bubble. Apply
        // it to the active room's last panel (Microsoft Chat) and do
        // not show it in the Whisper box.
        if clean.trim() == "<Chr>" {
            let (char_name, char_url) = {
                let mut found = (nick.to_string(), None);
                if let Some(s) = self.servers.get(server) {
                    if let Some(ch) = s.channels.get(&self.active_channel) {
                        if let Some((n, u)) = ch.char_map.get(nick) { found = (n.clone(), u.clone()); }
                    }
                }
                found
            };
            let wheel = if is_me { Some((self.wheel_emotion, self.wheel_strength)) } else { None };
            let frozen = is_me && self.wheel_frozen;
            let probe = comic::make_message_full(nick, &display, is_me, MessageKind::Whisper, wheel, frozen, &char_name, char_url, Vec::new());
            let chan = self.active_channel.clone();
            self.apply_chr_pose(server, &chan, nick, &probe, Vec::new());
            return;
        }
        // A (sound) message plays from our Sound Search Path.
        self.maybe_play_sound(&display);
        let in_room = self.servers.get(server).and_then(|s| s.channels.get(&self.active_channel)).map(|ch| ch.members.contains(&other)).unwrap_or(false);
        if in_room {
            let (char_name, char_url) = {
                let mut found = (nick.to_string(), None);
                if let Some(s) = self.servers.get(server) {
                    if let Some(ch) = s.channels.get(&self.active_channel) {
                        if let Some((n, u)) = ch.char_map.get(nick) { found = (n.clone(), u.clone()); }
                    }
                }
                found
            };
            let wheel = if is_me { Some((self.wheel_emotion, self.wheel_strength)) } else { None };
            let frozen = is_me && self.wheel_frozen;
            let msg = comic::make_message_full(nick, &display, is_me, MessageKind::Whisper, wheel, frozen, &char_name, char_url, Vec::new());
            if let Some(s) = self.servers.get_mut(server) {
                let ch = s.channels.entry(self.active_channel.clone()).or_default();
                ch.messages.push(msg);
                ch.text_log.push((nick.to_string(), format!("*whisper* {clean}")));
                if ch.messages.len() > 300 { ch.messages.drain(0..50); }
            }
        } else {
            self.whisper_lines.push((nick.to_string(), clean));
            self.whisper_peer = Some(other);
            self.show_whisper_box = true;
            if !is_me { self.whisper_unread += 1; }
        }
    }

    /// If `text` is a sound message — `(ding.wav)` / `(ding)` — play
    /// the file from our Sound Search Path. Anything else (including
    /// a parenthesised phrase with a space, like "(hello world)")
    /// is a normal message, not a sound. Pose code is stripped
    /// first, so `(ding.wav) (#G...)` still counts.
    fn maybe_play_sound(&mut self, text: &str) {
        let (text_no_oec, _) = crate::pose::strip_oec_prefix(text);
        let clean = crate::pose::parse_pose_code(&text_no_oec).map(|(c, _)| c).unwrap_or_else(|| text_no_oec.clone());
        let t = clean.trim();
        if t.len() < 3 || !t.starts_with('(') || !t.ends_with(')') { return; }
        let inner = &t[1..t.len() - 1];
        if inner.is_empty() || inner.contains(char::is_whitespace) { return; }
        self.play_sound(inner);
    }

    /// Play a sound file from the Sound Search Path
    /// (`comic.sound_path`, `~` = home). `name` is the bare name
    /// from inside the parens, with or without `.wav`. Tries the
    /// system players in order (PipeWire, PulseAudio, ALSA, ffplay)
    /// and reports the resolved path or the miss in the status line.
    fn play_sound(&mut self, name: &str) {
        let base = crate::about::expand_home(&self.cfg.comic.sound_path);
        let candidates: Vec<String> = if name.contains('.') { vec![name.to_string()] } else { vec![format!("{name}.wav")] };
        let Some(path) = candidates.iter().map(|c| base.join(c)).find(|p| p.is_file()) else {
            self.sound_status = format!("Sound not found: {name} in {}", self.cfg.comic.sound_path);
            crate::log::warn("app", format!("play_sound: {name} not found under {}", self.cfg.comic.sound_path));
            return;
        };
        let players: [(&str, &[&str]); 4] = [
            ("pw-play", &[]),
            ("paplay", &[]),
            ("aplay", &[]),
            ("ffplay", &["-nodisp", "-autoexit", "-loglevel", "quiet"]),
        ];
        for (player, args) in players {
            match std::process::Command::new(player).args(args).arg(&path).spawn() {
                Ok(_) => {
                    self.sound_status = format!("Playing {name} from {}", path.display());
                    crate::log::info("app", format!("play_sound: {name} via {player} from {}", path.display()));
                    return;
                }
                Err(_) => continue,
            }
        }
        self.sound_status = "No audio player found (install PipeWire, PulseAudio, ALSA utils, or ffmpeg)".into();
        crate::log::warn("app", "play_sound: no audio player found (pw-play/paplay/aplay/ffplay)");
    }

    /// Reply typed in the Whisper box. Goes to the last other party
    /// as a PRIVMSG (a whisper). If we are offline, there is no echo,
    /// so the line is shown locally instead.
    fn send_whisper_box_reply(&mut self) {
        let text = self.whisper_reply.trim().to_string();
        if text.is_empty() { return; }
        let Some(peer) = self.whisper_peer.clone() else { return; };
        let server = self.active_server.clone();
        let cmd = IrcCommand::Whisper { target: peer.clone(), text: text.clone(), pose_code: String::new() };
        let sent = if let Ok(map) = self.handles.lock() {
            if let Some(tx) = map.get(&server) { tx.send(cmd).is_ok() } else { false }
        } else { false };
        if !sent {
            let me = self.cfg.me.nick.clone();
            self.whisper_lines.push((me, text));
        }
        self.whisper_reply.clear();
    }

    fn connect_server(&mut self, cfg: ServerConfig) {
        crate::log::info("app", format!("connect_server: user requested Connect for {} host={} port={} tls={} channels={:?}", cfg.name, cfg.host, cfg.effective_port(), cfg.tls, cfg.channels));
        if let Some(s) = self.servers.get(&cfg.name) { if s.connected || s.status == "Connecting\u{2026}" { crate::log::info("app", format!("connect_server: {} already connected/connecting - skipping duplicate", cfg.name)); return; } }
        if let Some(s) = self.servers.get_mut(&cfg.name) { s.status = "Connecting…".into(); }
        let _ = self.control_tx.send(Control::Connect(cfg));
    }
    fn disconnect_server(&mut self, name: &str) {
        crate::log::info("app", format!("disconnect_server: user requested Disconnect for {name}"));
        let _ = self.control_tx.send(Control::Disconnect(name.to_string()));
        if let Some(s) = self.servers.get_mut(name) { s.connected = false; s.status = "Disconnected".into(); }
    }
    fn panels_per_row(&self, avail_w: f32) -> usize {
        if self.cfg.ui.panels_per_row > 0 { return self.cfg.ui.panels_per_row; }
        if avail_w < 520.0 { 1 } else if avail_w < 900.0 { 2 } else { 3 }
    }
    fn save_conversation(&mut self) {
        let logs = self.servers.get(&self.active_server).and_then(|s| s.channels.get(&self.active_channel)).map(|c| c.text_log.clone()).unwrap_or_default();
        let mut out = format!("Comic Chat RS — {} {} — saved\n\n", self.active_server, self.active_channel);
        for (nick, text) in &logs { out.push_str(&format!("<{nick}> {text}\n")); }
        let path = std::env::temp_dir().join(format!("comic-chat-{}-{}.txt", self.active_server, self.active_channel.trim_start_matches('#')));
        match std::fs::write(&path, out) {
            Ok(_) => { crate::log::info("app", format!("save_conversation: saved {} line(s) from {} {} to {}", logs.len(), self.active_server, self.active_channel, path.display())); self.save_status = format!("Saved to {}", path.display()); }
            Err(e) => { crate::log::error("app", format!("save_conversation: failed to write {}: {e} — check temp dir writable, disk space", path.display())); self.save_status = format!("Save failed: {e}"); }
        }
    }

    // --- Textures ---
    fn char_texture(&mut self, ctx: &egui::Context, name: &str) -> Option<TextureHandle> {
        if let Some(t) = self.char_textures.get(name) { return Some(t.clone()); }
        // Negative cache: a name that failed before returns None silently.
        // Without this, every frame re-decoded + re-logged (idle spam).
        // Cleared on Rescan so newly-added files can load.
        if self.failed_char_textures.contains(name) { return None; }
        crate::log::debug("app", format!("char_texture: loading texture for character {name:?} — trying decoded AVB (DIB+mask) first, PNG sidecar second, placeholder last"));
        // v0.3.17: decoded AVB is primary (DIB + halo mask -> RGBA). PNG sidecar is only fallback.
        if let Some(avb) = self.avb_chars.iter().find(|f| f.name.eq_ignore_ascii_case(name)) {
            if let Some(dec) = crate::dib::decode_avb_file(&avb.path) {
                if let Some(img) = dec.images.first() {
                    let pixels: Vec<Color32> = img.rgba.chunks_exact(4).map(|p| Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3])).collect();
                    let cimg = ColorImage { size: [img.width as usize, img.height as usize], pixels };
                    crate::log::info("app", format!("char_texture: {name:?} decoded from AVB DIB — {}x{} RGBA, using DIB texture (not PNG)", img.width, img.height));
                    if img.had_mask { let _ = crate::dib::cache_png(name, img); }
                    let tex = ctx.load_texture(format!("char-{name}-dib"), cimg, TextureOptions::default());
                    self.char_textures.insert(name.to_string(), tex.clone());
                    self.failed_char_textures.remove(name);
                    return Some(tex);
                }
            }
        }
        crate::log::debug("app", format!("char_texture: {name:?} — DIB path gave no image (no AVB, decode failed, or 0 images) — trying PNG sidecar fallback"));
        // Fallback: PNG sidecar
        let preview = self.avb_chars.iter().find(|f| f.name.eq_ignore_ascii_case(name)).and_then(|f| f.preview_png.clone());
        let preview = preview.or_else(|| {
            // Fallback: ComicArt/<name>.png
            let p = std::path::PathBuf::from("ComicArt").join(format!("{name}.png"));
            if p.exists() { Some(p) } else { None }
        });
        let preview = preview.or_else(|| {
            // Fallback: Cache/png/<name>.png (Step 3 cache, never ComicArt)
            let safe: String = name.chars().map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
            let p = dirs::home_dir()?.join(format!(".config/comic-chat/Cache/png/{safe}.png"));
            if p.exists() { Some(p) } else { None }
        });
        let path = match preview { Some(p) => p, None => {
            // Log once at debug (not warn per frame) + negative-cache it.
            crate::log::debug("app", format!("char_texture: {name:?} — no DIB image and no PNG sidecar found in ComicArt/ — returning None, panel will draw placeholder. Fix: put {name}.AVB (real, not stub) or {name}.png in ComicArt/ and Rescan."));
            self.failed_char_textures.insert(name.to_string());
            return None;
        } };
        crate::log::info("app", format!("char_texture: {name:?} — using PNG sidecar fallback {}", path.display()));
        let img = match image::open(&path) { Ok(i) => i.to_rgba8(), Err(e) => { crate::log::debug("app", format!("char_texture: {name:?} — PNG sidecar {} failed to open: {e}", path.display())); self.failed_char_textures.insert(name.to_string()); return None; } };
        let (w, h) = img.dimensions();
        let pixels: Vec<Color32> = img.pixels().map(|p| Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3])).collect();
        let cimg = ColorImage { size: [w as usize, h as usize], pixels };
        let tex = ctx.load_texture(format!("char-{name}"), cimg, TextureOptions::default());
        self.char_textures.insert(name.to_string(), tex.clone());
        self.failed_char_textures.remove(name);
        Some(tex)
    }
    // Original char.cpp: pose = f(emotion, gesture), pinned per utterance.
    // AVB images: [0] = icon (smallest), [1..] = body poses. Picking the
    // largest DIB gave the wrong pose (e.g. always Cheer). This hashes
    // (emotion, gesture) deterministically so the same utterance always
    // gets the same pose, different emotions get different poses when the
    // AVB has >1 pose, and a single-pose AVB always returns that pose.
    fn pose_image_index(count: usize, emotion: comic::Emotion, gesture: comic::Gesture) -> usize { Self::pose_image_index_seeded(count, emotion, gesture, 0, &[]) }
    fn face_index(e: comic::Emotion) -> usize { match e { comic::Emotion::Neutral => 0, comic::Emotion::Happy => 1, comic::Emotion::Coy => 2, comic::Emotion::Bored => 3, comic::Emotion::Scared => 4, comic::Emotion::Sad => 5, comic::Emotion::Angry => 6, comic::Emotion::Shout => 7, comic::Emotion::Laugh => 8 } }
    fn gesture_index(g: comic::Gesture) -> usize { match g { comic::Gesture::Neutral => 0, comic::Gesture::Wave => 1, comic::Gesture::PointSelf => 2, comic::Gesture::PointOther => 3, comic::Gesture::DoublePoint => 4, comic::Gesture::Shrug => 5, comic::Gesture::Walk3Q => 6, comic::Gesture::SideWalk => 7, comic::Gesture::Walk3QF => 8 } }
    fn pose_image_index_seeded(count: usize, emotion: comic::Emotion, gesture: comic::Gesture, msg_seed: u64, poses: &[crate::dib::PoseEntry]) -> usize {

        // NEUTRAL_VARIANT_0317: Neutral/Neutral cycles all Neutral-expression poses
        // minus keyword gestures, by seed. This is what makes plain messages
        // show different neutrals even though gesture stays Neutral.
        if emotion == comic::Emotion::Neutral && gesture == comic::Gesture::Neutral {
            let neutral_code = comic::Emotion::Neutral as u8;
            let kw = [comic::Gesture::Wave as u8, comic::Gesture::PointSelf as u8, comic::Gesture::PointOther as u8];
            let mut neutrals: Vec<usize> = Vec::new();
            for po in poses {
                let expr_u8 = po.expression as u8;
                let gest_u8 = po.gesture as u8;
                if expr_u8 == neutral_code && !kw.contains(&gest_u8) {
                    neutrals.push(po.image_index as usize);
                }
            }
            if neutrals.len() > 1 {
                return neutrals[msg_seed as usize % neutrals.len()];
            }
            // Fallthrough to exact-match logic if only 1 neutral pose exists.
        }
        if count <= 1 { return 0; }
        // POSE_TABLE_0317: when the AVB has a POSE table, it is the only
        // source of truth. There is no "PointOther variant of Coy" — one
        // Coy face, one PointOther torso; the table says which DIB is which.
        // Fallback order (avatar.cpp): exact, gesture-only, expression-only, first.
        if !poses.is_empty() {
            let g = Self::gesture_index(gesture) as u8;
            let e = Self::face_index(emotion) as u8;
            if let Some(p) = poses.iter().find(|p| p.gesture == g && p.expression == e) {
                if p.image_index < count { return p.image_index; }
            }
            if let Some(p) = poses.iter().find(|p| p.gesture == g) {
                if p.image_index < count { return p.image_index; }
            }
            if let Some(p) = poses.iter().find(|p| p.expression == e) {
                if p.image_index < count { return p.image_index; }
            }
            if let Some(p) = poses.first() {
                if p.image_index < count { return p.image_index; }
            }
        }
        // No POSE table: face-order fallback, SAME as previous patch so
        // behaviour does not shift by one: icon at 0, Neutral 1, Happy 2,
        // Coy 3, Bored 4, Scared 5, Sad 6, Angry 7, Shout 8, Laugh 9.
        // (face_index 0..8 + 1). This is still a guess — log in dib.rs
        // says when it is used. POSE table, when present, overrides it.
        let _ = msg_seed;
        let f = Self::face_index(emotion); // 0 Neutral .. 8 Laugh
        1 + (f % (count - 1))
    }
    fn char_pose_texture(&mut self, ctx: &egui::Context, name: &str, emotion: comic::Emotion, gesture: comic::Gesture) -> Option<TextureHandle> { self.char_pose_texture_seeded(ctx, name, emotion, gesture, 0) }
    fn char_pose_texture_seeded(&mut self, ctx: &egui::Context, name: &str, emotion: comic::Emotion, gesture: comic::Gesture, msg_seed: u64) -> Option<TextureHandle> {
        // G-as-POSE-index: G selects gesture entry, E verifies with Neutral (1,0)->9 exception.
        // Do not select image by E alone; E (1,0) maps to 4 Trekkie / 3 Geordi entries.
        // See crate::g_pose_index::parse_wire / verify_gesture.

        let key = format!("{name}:pose:{emotion:?}:{gesture:?}:{msg_seed}");
        if let Some(t) = self.char_textures.get(&key) { return Some(t.clone()); }
        if self.failed_char_textures.contains(&key) { return None; }
        if let Some(avb) = self.avb_chars.iter().find(|f| f.name.eq_ignore_ascii_case(name)) {
            if let Some(dec) = crate::dib::decode_avb_file(&avb.path) {
                if !dec.images.is_empty() {
                    crate::log::debug("app", format!("char_pose_texture: {name:?} emotion={emotion:?} gesture={gesture:?} images={} raw_dibs={} poses={:?}", dec.images.len(), dec.raw_dibs.len(), dec.poses));
                    let idx = Self::pose_image_index_seeded(dec.images.len(), emotion, gesture, msg_seed, &dec.poses);
                    crate::log::debug("app", format!("char_pose_texture: {name:?} picked idx={idx} for emotion={emotion:?} gesture={gesture:?}"));
                    let img = &dec.images[idx.min(dec.images.len() - 1)];
                    let pixels: Vec<Color32> = img.rgba.chunks_exact(4).map(|p| Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3])).collect();
                    let cimg = ColorImage { size: [img.width as usize, img.height as usize], pixels };
                    let tex = ctx.load_texture(format!("char-{name}-pose-{idx}-{msg_seed}"), cimg, TextureOptions::default());
                    self.char_textures.insert(key, tex.clone());
                    return Some(tex);
                }
            }
        }
        self.failed_char_textures.insert(key);
        None
    }
    fn char_body_texture(&mut self, ctx: &egui::Context, name: &str) -> Option<TextureHandle> {
        // Back-compat wrapper: Neutral pose. Callers that know the
        // message's emotion/gesture should call char_pose_texture directly.
        self.char_pose_texture(ctx, name, comic::Emotion::Neutral, comic::Gesture::Neutral)
    }
    #[allow(dead_code)]
    fn char_body_texture_legacy(&mut self, ctx: &egui::Context, name: &str) -> Option<TextureHandle> {
        let key = format!("{name}:body");
        if let Some(t) = self.char_textures.get(&key) { return Some(t.clone()); }
        if self.failed_char_textures.contains(&key) { return None; }
        if let Some(avb) = self.avb_chars.iter().find(|f| f.name.eq_ignore_ascii_case(name)) {
            if let Some(dec) = crate::dib::decode_avb_file(&avb.path) {
                // First body image = deterministic default pose. Largest was arbitrary: AVB holds many poses, largest is not Neutral, so TEST/HELLO (both Neutral) showed different poses. Full gesture-table selection is a later dib.rs step; this makes pose stable.
                let _max_area = dec.images.iter().map(|i| i.width * i.height).max().unwrap_or(0);
                if let Some(img) = dec.images.iter().filter(|i| !i.is_mask && i.width >= 40 && i.height >= 40).find(|i| { let w = i.width as usize; let h = i.height as usize; if w == 0 || h == 0 { return false; } let corner = |x: usize, y: usize| i.rgba[(y * w + x) * 4 + 3]; let opaque_corners = [corner(0,0), corner(w-1,0), corner(0,h-1), corner(w-1,h-1)].iter().filter(|&&a| a > 200).count(); opaque_corners < 3 }).or_else(|| dec.images.iter().find(|i| !i.is_mask)) {
                    let pixels: Vec<Color32> = img.rgba.chunks_exact(4).map(|p| Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3])).collect();
                    let cimg = ColorImage { size: [img.width as usize, img.height as usize], pixels };
                    let tex = ctx.load_texture(format!("char-{name}-body"), cimg, TextureOptions::default());
                    self.char_textures.insert(key, tex.clone());
                    return Some(tex);
                }
            }
        }
        self.failed_char_textures.insert(key);
        None
    }
    fn bg_texture(&mut self, ctx: &egui::Context, name: &str) -> Option<TextureHandle> {
        if let Some(t) = self.bg_textures.get(name) { return Some(t.clone()); }
        if self.failed_bg_textures.contains(name) { return None; }
        crate::log::debug("app", format!("bg_texture: loading texture for background {name:?} — trying decoded BGB first, PNG second"));
        // v0.3.17: decoded BGB is primary (single DIB background). PNG sidecar is only fallback.
        if let Some(bgb) = self.avb_bgs.iter().find(|f| f.name.eq_ignore_ascii_case(name)) {
            if let Some(dec) = crate::dib::decode_avb_file(&bgb.path) {
                let img = dec.bg_image.as_ref().or(dec.images.first());
                if let Some(img) = img {
                    let pixels: Vec<Color32> = img.rgba.chunks_exact(4).map(|p| Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3])).collect();
                    let cimg = ColorImage { size: [img.width as usize, img.height as usize], pixels };
                    crate::log::info("app", format!("bg_texture: {name:?} decoded from BGB DIB — {}x{} RGBA", img.width, img.height));
                    let _ = crate::dib::cache_png(name, img);
                    let tex = ctx.load_texture(format!("bg-{name}-dib"), cimg, TextureOptions::default());
                    self.bg_textures.insert(name.to_string(), tex.clone());
                    self.failed_bg_textures.remove(name);
                    return Some(tex);
                }
            }
        }
        crate::log::debug("app", format!("bg_texture: {name:?} — DIB path gave no image — trying PNG sidecar fallback"));
        let preview = self.avb_bgs.iter().find(|f| f.name.eq_ignore_ascii_case(name)).and_then(|f| f.preview_png.clone());
        let path = match preview { Some(p) => p, None => { crate::log::debug("app", format!("bg_texture: {name:?} — no DIB image and no PNG sidecar — returning None, panel will draw placeholder. Fix: put {name}.BGB or {name}.png in ComicArt/ and Rescan.")); self.failed_bg_textures.insert(name.to_string()); return None; } };
        crate::log::info("app", format!("bg_texture: {name:?} — using PNG sidecar fallback {}", path.display()));
        let img = match image::open(&path) { Ok(i) => i.to_rgba8(), Err(e) => { crate::log::debug("app", format!("bg_texture: {name:?} — PNG sidecar {} failed to open: {e}", path.display())); self.failed_bg_textures.insert(name.to_string()); return None; } };
        let (w, h) = img.dimensions();
        let pixels: Vec<Color32> = img.pixels().map(|p| Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3])).collect();
        let cimg = ColorImage { size: [w as usize, h as usize], pixels };
        let tex = ctx.load_texture(format!("bg-{name}"), cimg, TextureOptions::default());
        self.bg_textures.insert(name.to_string(), tex.clone());
        self.failed_bg_textures.remove(name);
        Some(tex)
    }
}
fn draw_character_box(ui: &mut Ui, nick: &str, char_name: &str, emotion: Emotion, tex: Option<TextureHandle>, size: f32, slot_w: f32, flip: bool, seed: u64, torso_up: bool) -> egui::Response {
    // Returns the sprite painter's Response (Sense::click) so callers can
    // do `if avatar_resp.clicked() { toggle cast }`. The vertical labels
    // below are not part of the clickable rect; clicking the sprite is
    // the toggle, which matches the panel code.
    let mut click_resp: Option<egui::Response> = None;
    ui.vertical(|ui| {
        if let Some(t) = tex {
            // Preserve aspect: AVB art is not square. size() is [usize; 2]
            // in this egui (not Vec2), so use ts[0] / ts[1].
            let ts = t.size();
            let aspect = if ts[1] > 0 { ts[0] as f32 / ts[1] as f32 } else { 1.0 };
            let aspect = aspect.clamp(0.4, 2.5);
            let (resp, painter) = ui.allocate_painter(egui::Vec2::new(slot_w, size), egui::Sense::click());
            let avail = resp.rect;
            let (dw, dh) = if aspect > 1.0 { (size, size / aspect) } else { (size * aspect, size) };
            // Torso-up: full sprite at size/0.65, top-aligned, clipped to slot.
            // Full-body: centred, height = size, aspect preserved.
            let (dw_torso, dh_torso) = if torso_up {
                let s = size / 0.65;
                if aspect > 1.0 { (s, s / aspect) } else { (s * aspect, s) }
            } else { (dw, dh) };
            let draw_rect = if torso_up {
                egui::Rect::from_min_size(egui::Pos2::new(avail.center().x - dw_torso / 2.0, avail.top()), egui::Vec2::new(dw_torso, dh_torso))
            } else {
                egui::Rect::from_center_size(avail.center(), egui::Vec2::new(dw, dh))
            };
            let mut uv = if flip { egui::Rect::from_min_max(egui::Pos2::new(1.0, 0.0), egui::Pos2::new(0.0, 1.0)) } else { egui::Rect::from_min_max(egui::Pos2::new(0.0, 0.0), egui::Pos2::new(1.0, 1.0)) };
            // Torso-up: NO UV stretch. Draw full sprite at height
            // size/0.65, top-aligned, clipped by the painter to the slot.
            // Visible = top 65% at correct aspect, legs cut, torso to edge.
            // UV stays full (flip only); draw_rect below handles the crop.
            let _ = torso_up;
            painter.image(t.id(), draw_rect, uv, Color32::WHITE);
            click_resp = Some(resp.on_hover_text(nick.to_owned()));
        } else {
            // Clamp to column: k = size/72 blows up to ~5.5 when size ~400 (short tail),
            // body 44*k = 242px > slot_w, covers the other char. Fit body width to slot_w.
            let k = (size / 72.0).min((slot_w / 44.0).max(0.5));
            let (resp, painter) = ui.allocate_painter(egui::Vec2::new(slot_w, size), egui::Sense::click());
            let rect = resp.rect;
            let col = comic::avatar_color(nick, seed);
            let cx = rect.center().x;
            let head_c = egui::Pos2::new(cx, rect.top() + 16.0 * k);
            painter.circle_filled(head_c, 17.0 * k, Color32::WHITE);
            painter.rect_filled(egui::Rect::from_min_max(egui::Pos2::new(cx - 22.0 * k, rect.top() + 30.0 * k), egui::Pos2::new(cx + 22.0 * k, rect.bottom())), 8.0 * k, Color32::WHITE);
            painter.circle_filled(head_c, 13.0 * k, col);
            painter.rect_filled(egui::Rect::from_min_max(egui::Pos2::new(cx - 18.0 * k, rect.top() + 32.0 * k), egui::Pos2::new(cx + 18.0 * k, rect.bottom())), 6.0 * k, col);
            let dot_col = match emotion {
                Emotion::Happy | Emotion::Laugh => egui::Color32::from_rgb(255, 220, 0),
                Emotion::Angry | Emotion::Shout => egui::Color32::RED,
                Emotion::Sad => egui::Color32::from_rgb(80, 120, 200),
                _ => egui::Color32::WHITE,
            };
            painter.circle_filled(egui::Pos2::new(cx, rect.top() + 44.0 * k), 4.0 * k, dot_col);
            click_resp = Some(resp.on_hover_text(nick.to_owned()));
        }
        // No nick / pose labels under the avatar in-panel.
        // They pushed the sprite 30px above the frame edge.
    });
    // Fallback: vertical always allocates a painter above, so this should
    // never fire. If it does, return a zero-size clickable response.
    match click_resp {
        Some(r) => r,
        None => ui.allocate_response(egui::Vec2::ZERO, egui::Sense::click()),
    }
}





/// Spec Section 1 balloon geometry. All constants are INFERRED from the
/// relayed spec (SIGGRAPH 96 prose + Pirillo notes), NOT verified against
/// balloon.cpp. Do not ship them as verified original constants.
const SPEC_REF_PANE_W: f32 = 640.0;

struct BalloonMetrics {
    scale: f32,
    font_px: f32,
    stroke_w: f32,
    halo_w: f32,
    dash_len: f32,
    gap_len: f32,
    tail_base_w: f32,
    overlap_px: f32,
}

fn balloon_metrics(panel_w: f32) -> BalloonMetrics {
    // Spec 1.8. Font is fixed, not scaled by camera zoom; it scales only
    // with pane width, clamped for readability (clamp = porter deviation).
    let scale = panel_w / SPEC_REF_PANE_W;
    BalloonMetrics {
        scale,
        font_px: (18.0 * scale).round().clamp(14.0, 24.0),
        stroke_w: (2.5 * scale).round().max(2.0),
        halo_w: (6.0 * scale).round().max(2.0),
        dash_len: (6.0 * scale).round().max(2.0),
        gap_len: (4.0 * scale).round().max(2.0),
        tail_base_w: (18.0 * scale).round().max(10.0), // Slender MS Chat taper, not a stubby wedge // MS Chat: ~20px base at typical scale, not a needle
        overlap_px: (12.0 * scale).round().max(4.0), // deep overlap so fill hides tail join
    }
}

/// Spec 1.4 padding at REF: (left, right, top, bottom). Caller scales.
fn balloon_padding(kind: MessageKind, shout: bool) -> (f32, f32, f32, f32) {
    match kind {
        MessageKind::Action => (10.0, 10.0, 8.0, 8.0),
        _ if shout => (26.0, 26.0, 22.0, 24.0),
        _ => (20.0, 20.0, 16.0, 18.0),
    }
}

/// Spec 1.4 minimum balloon size at REF: (width, height).
fn balloon_min_size(kind: MessageKind, shout: bool) -> (f32, f32) {
    match kind {
        MessageKind::Action => (32.0, 24.0),
        _ if shout => (64.0, 44.0),
        _ => (48.0, 32.0),
    }
}

/// Closed uniform cubic B-spline sampled to a polyline (spec 1.1.0 step 7).
/// Control points come from per-line text edges; the curve hugs them and
/// rounds the corners, which is the flowing look. Tension 5.0 from the
/// paper is not a parameter of the uniform basis, so it is carried by
/// control-point placement; marked INFERRED.
fn bspline_closed(ctrl: &[egui::Pos2], per_seg: usize) -> Vec<egui::Pos2> {
    let n = ctrl.len();
    if n < 4 { return ctrl.to_vec(); }
    let mut out = Vec::new();
    for i in 0..n {
        let p0 = ctrl[(i + n - 1) % n];
        let p1 = ctrl[i];
        let p2 = ctrl[(i + 1) % n];
        let p3 = ctrl[(i + 2) % n];
        for s in 0..per_seg {
            let t = s as f32 / per_seg as f32;
            let t2 = t * t;
            let t3 = t2 * t;
            let b0 = (1.0 - t).powi(3) / 6.0;
            let b1 = (3.0 * t3 - 6.0 * t2 + 4.0) / 6.0;
            let b2 = (-3.0 * t3 + 3.0 * t2 + 3.0 * t + 1.0) / 6.0;
            let b3 = t3 / 6.0;
            out.push(egui::Pos2::new(
                b0 * p0.x + b1 * p1.x + b2 * p2.x + b3 * p3.x,
                b0 * p0.y + b1 * p1.y + b2 * p2.y + b3 * p3.y,
            ));
        }
    }
    out
}

/// Spec 1.1.1 Rule C: on long straight runs, add wave points alternating
/// toward/away from the text. Applied to the control polygon; the B-spline
/// smooths them into gentle waves. Deterministic (segment parity), no rand.
fn add_waves(ctrl: &[egui::Pos2], scale: f32) -> Vec<egui::Pos2> {
    // Cloud edge: subdivide every edge into ~22px steps and push each
    // midpoint alternately in / out by ~5px. The old version only waved
    // edges longer than 48px, so short / single-line bubbles stayed
    // smooth ovals — in the reference even a 3-letter bubble ("CYA")
    // has a wavy cloud edge.
    let n = ctrl.len();
    if n < 4 { return ctrl.to_vec(); }
    let mut out = Vec::new();
    let mut wave_i = 0usize;
    for i in 0..n {
        let a = ctrl[i];
        let b = ctrl[(i + 1) % n];
        out.push(a);
        let len = a.distance(b);
        let step = 22.0 * scale;
        let n_seg = (len / step).floor().max(1.0) as usize;
        let mut perp = egui::Vec2::new(-(b.y - a.y), b.x - a.x);
        if perp.length() < 0.001 { perp = egui::Vec2::new(0.0, -1.0); }
        perp = perp.normalized();
        for s in 1..=n_seg {
            let t = s as f32 / (n_seg as f32 + 1.0);
            let mid = egui::Pos2::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
            let sign = if wave_i % 2 == 0 { 1.0 } else { -1.0 };
            wave_i += 1;
            out.push(egui::Pos2::new(
                mid.x + perp.x * sign * 5.0 * scale,
                mid.y + perp.y * sign * 5.0 * scale,
            ));
        }
    }
    out
}
/// Spec 1.1.4: SHOUT spikes. One apex per long control edge, displaced
/// outward from the polygon centroid. The B-spline shrinks isolated apexes,
/// so depth is doubled (INFERRED compensation). Spikes are part of the one
/// filled path, not separate triangles.
fn add_spikes(ctrl: &[egui::Pos2], pitch: f32, depth: f32) -> Vec<egui::Pos2> {
    let n = ctrl.len();
    if n < 4 { return ctrl.to_vec(); }
    let (mut cx, mut cy) = (0.0f32, 0.0f32);
    for p in ctrl { cx += p.x; cy += p.y; }
    cx /= n as f32; cy /= n as f32;
    let mut out = Vec::new();
    for i in 0..n {
        let a = ctrl[i];
        let b = ctrl[(i + 1) % n];
        out.push(a);
        if a.distance(b) >= pitch * 0.75 {
            let mid = egui::Pos2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
            let mut odir = egui::Vec2::new(mid.x - cx, mid.y - cy);
            if odir.length() < 0.001 { odir = egui::Vec2::new(0.0, -1.0); }
            odir = odir.normalized();
            out.push(egui::Pos2::new(
                mid.x + odir.x * depth * 2.0,
                mid.y + odir.y * depth * 2.0,
            ));
        }
    }
    out
}

/// Spec 1.5.1: tapered tail with quadratic Bezier sides. Control points are
/// pushed outward from the base-centre-to-tip midline, so the tail curves
/// toward the speaker side. Returns the closed tail polygon.
fn tail_polygon(base_left: egui::Pos2, base_right: egui::Pos2, tip: egui::Pos2, bend: f32) -> Vec<egui::Pos2> {
    let base_c = egui::Pos2::new((base_left.x + base_right.x) / 2.0, (base_left.y + base_right.y) / 2.0);
    let centre_mid = egui::Pos2::new((base_c.x + tip.x) / 2.0, (base_c.y + tip.y) / 2.0);
    // Asymmetrical tail: left side curves out 1.6x, right side 0.4x — one side
    // concave, one convex, like the original. Symmetrical tails look stuck-on.
    let ctrl_for = |base: egui::Pos2, b: f32| -> egui::Pos2 {
        let mid = egui::Pos2::new((base.x + tip.x) / 2.0, (base.y + tip.y) / 2.0);
        let mut out = egui::Vec2::new(mid.x - centre_mid.x, mid.y - centre_mid.y);
        if out.length() < 0.001 { out = egui::Vec2::new(0.0, 0.0); } else { out = out.normalized(); }
        egui::Pos2::new(mid.x + out.x * b, mid.y + out.y * b)
    };
    let c_left = ctrl_for(base_left, bend * 1.6);
    let c_right = ctrl_for(base_right, bend * 0.4);
    let mut pts = Vec::new();
    let steps = 12usize;
    // Left side: base_left -> tip.
    for s in 0..=steps {
        let t = s as f32 / steps as f32;
        let u = 1.0 - t;
        pts.push(egui::Pos2::new(
            u * u * base_left.x + 2.0 * u * t * c_left.x + t * t * tip.x,
            u * u * base_left.y + 2.0 * u * t * c_left.y + t * t * tip.y,
        ));
    }
    // Right side: tip -> base_right (reverse order closes the polygon).
    for s in (0..=steps).rev() {
        let t = s as f32 / steps as f32;
        let u = 1.0 - t;
        pts.push(egui::Pos2::new(
            u * u * base_right.x + 2.0 * u * t * c_right.x + t * t * tip.x,
            u * u * base_right.y + 2.0 * u * t * c_right.y + t * t * tip.y,
        ));
    }
    pts
}

/// Spec 1.5.2: THINK trail - 3 ovals along the tail centre curve.
fn think_trail_centre(base_c: egui::Pos2, tip: egui::Pos2, bend: f32) -> (egui::Pos2, egui::Pos2) {
    // Centre control: midpoint pushed outward by the same bend direction
    // the tail sides use. For a symmetric tail this is ~midpoint, so the
    // trail is a straight line; for an offset speaker it arcs.
    let mid = egui::Pos2::new((base_c.x + tip.x) / 2.0, (base_c.y + tip.y) / 2.0);
    let mut perp = egui::Vec2::new(-(tip.y - base_c.y), tip.x - base_c.x);
    if perp.length() < 0.001 { perp = egui::Vec2::new(0.0, 0.0); } else { perp = perp.normalized(); }
    let ctrl = egui::Pos2::new(mid.x + perp.x * bend * 0.5, mid.y + perp.y * bend * 0.5);
    (mid, ctrl)
}

/// Draw one balloon + its tail in ONE painter (spec Section 1).
/// The tail base overlaps the balloon body and is drawn BEFORE the balloon
/// fill, so the fill hides the seam (spec 1.5). Text is ALL-CAPS, CENTRED
/// except ACTION (spec 1.6). Fills are all white, outlines all black
/// (spec 1.3). `col_w` is the column width; `panel_w` is the pane width
/// for the 0.85 max-balloon-width rule (spec 1.6). The balloon is centred
/// in its column, so the tail lands on the speaker below.

fn draw_balloon(ui: &mut Ui, msg: &ComicMessage, col_w: f32, panel_w: f32, extra_tail: f32) {
    let caps = msg.text.to_uppercase();
    if caps.trim().is_empty() {
        ui.allocate_space(egui::Vec2::new(col_w, 1.0));
        return;
    }
    let m = balloon_metrics(panel_w);
    let shout = matches!(msg.balloon, comic::Balloon::Shout);
    let action = msg.kind == MessageKind::Action;
    let (pad_l_r, pad_r_r, pad_t_r, pad_b_r) = balloon_padding(msg.kind, shout);
    let pad_l = (pad_l_r * m.scale * 0.65).round();
    let pad_r = (pad_r_r * m.scale * 0.65).round();
    let pad_t = (pad_t_r * m.scale * 0.62).round();
    let pad_b = (pad_b_r * m.scale * 0.62).round();
    let (min_w_r, min_h_r) = balloon_min_size(msg.kind, shout);
    let min_w = min_w_r * m.scale;
    let min_h = min_h_r * m.scale;
    let italic = msg.kind == MessageKind::Whisper;
    // Comic balloons use Chat Bold, UI uses Proportional Regular.
    let font_id = egui::FontId::new(m.font_px, egui::FontFamily::Name(CHAT_FONT_FAMILY.into()));
    // Wavies are scaled with the pane, and the painter must leave room for
    // them: the outline bumps WAVE_H outside the balloon box on every side.
    // Without this the top bumps poke above the painter clip rect and the
    // top stroke is cut off.
    let mut wave_h = (4.6667 * m.scale as f64) as f32;
    let mut wave_interval = (20.0 * m.scale as f64) as f64;
    let mut wave_margin = wave_h + m.stroke_w * 0.5;
    let mut max_w = (col_w - pad_l - pad_r - 8.0 - 2.0 * wave_margin).max(60.0);
    max_w = max_w.min((0.85 * panel_w - 16.0).max(60.0));
    let galley = ui.fonts(|f| {
        let mut job = egui::text::LayoutJob::default();
        job.wrap = egui::text::TextWrapping { max_width: max_w, max_rows: usize::MAX, break_anywhere: false, overflow_character: None };
        job.halign = if action { egui::Align::Min } else { egui::Align::Center };
        job.append(&caps, 0.0, egui::text::TextFormat { font_id: font_id.clone(), color: Color32::BLACK, italics: italic, ..Default::default() });
        f.layout_job(job)
    });
    let text_w = galley.size().x;
    let text_h = galley.size().y;
    if galley.rows.len() == 1 {
        wave_h *= 1.8;
        wave_interval *= 0.7;
        wave_margin = wave_h + m.stroke_w * 0.5;
    } else {
        wave_h *= 1.4;
        wave_interval *= 0.85;
        wave_margin = wave_h + m.stroke_w * 0.5;
    }
    let balloon_w = (text_w + pad_l + pad_r).max(min_w).min(col_w);
    let balloon_h = (text_h + pad_t + pad_b).max(min_h);
    let base_tail = if action { 0.0 } else { (30.0 * m.scale).round().max(16.0) };
    let tail_h = base_tail + extra_tail.max(0.0);
    let (resp, painter) = ui.allocate_painter(egui::Vec2::new(col_w, balloon_h + tail_h), egui::Sense::hover());
    let rect = resp.rect;
    let balloon_x = (rect.left() + (col_w - balloon_w) / 2.0).max(rect.left() + wave_margin + 2.0);
    let balloon_y = rect.top() + wave_margin;
    let text_x0 = balloon_x + (balloon_w - text_w) / 2.0;
    let text_y0 = balloon_y + (balloon_h - text_h) / 2.0;
    let cx = balloon_x + balloon_w / 2.0;
    let cy = balloon_y + balloon_h / 2.0;
    let rx = balloon_w / 2.0;
    let ry = balloon_h / 2.0;
    // Body: closed ellipse (rect for Action), WITHOUT tail. balloon.cpp:
    // body is a closed spline, tail is a separate arrow over the gap
    // (BreakSpline). Merging tail into body (v3/v4) made a self-intersecting
    // bow-tie that tessellates only as the tail sliver. Keep separate.
    // DEBUG-BALLOON-4b: temporary, remove after diagnosis.
    crate::log::debug("balloon", format!("BALLOON action={} shout={} text=({:.0},{:.0}) balloon_w={:.0} balloon_x={:.0} text_x0={:.0} text_y0={:.0} rect_l={:.0} col_w={:.0} panel_w={:.0}", action, shout, text_w, text_h, balloon_w, balloon_x, text_x0, text_y0, rect.left(), col_w, panel_w));
    let body_pts: Vec<egui::Pos2> = if action {
        vec![
            egui::Pos2::new(balloon_x, balloon_y),
            egui::Pos2::new(balloon_x + balloon_w, balloon_y),
            egui::Pos2::new(balloon_x + balloon_w, balloon_y + balloon_h),
            egui::Pos2::new(balloon_x, balloon_y + balloon_h),
        ]
    } else {
        // Step 4b: per-line outline from comic-balloon (control points +
        // spline + style). Crate works in galley-local coords; translate
        // into painter coords. per_seg = 8, as in the crate's outline().
        // Tail and Think trail below are untouched - tail stays separate.
        let line_widths: Vec<f64> = galley.rows.iter().map(|r| (r.rect.max.x - r.rect.min.x) as f64).collect();
        let row_tops: Vec<f64> = galley.rows.iter().map(|r| r.rect.min.y as f64).collect();
        let row_bottoms: Vec<f64> = galley.rows.iter().map(|r| r.rect.max.y as f64).collect();
        let style = if shout {
            comic_balloon::Style::Shout
        } else if msg.kind == MessageKind::Think {
            comic_balloon::Style::Thought
        } else if msg.kind == MessageKind::Whisper {
            comic_balloon::Style::Whisper
        } else {
            comic_balloon::Style::Speech
        };
        comic_balloon::outline_per_line(
            &line_widths, &row_tops, &row_bottoms,
            text_w as f64, text_h as f64,
            pad_l as f64, pad_r as f64, pad_t as f64, pad_b as f64,
            balloon_w as f64, balloon_h as f64,
            style, 8,
        ).iter().map(|p| egui::Pos2::new(p.x as f32 + text_x0, p.y as f32 + text_y0)).collect()
    };
    let face_x = rect.center().x;
    let tail_half = m.tail_base_w / 2.0;
    let tip = egui::Pos2::new(face_x, rect.bottom() - 2.0);
    let m_crate = comic_balloon::Metrics { tail_half_width: tail_half as f64, ..Default::default() };
    let pts_f64 = body_pts.iter().map(|p| comic_balloon::Point::new(p.x as f64, p.y as f64)).collect::<Vec<_>>();
    let head_pt = comic_balloon::Point::new(tip.x as f64, tip.y as f64);
    let tail_base_opt = comic_balloon::tail_base(&pts_f64, head_pt, &m_crate);
    let (base_left, base_right, gap_idx) = if !action && msg.kind != MessageKind::Think {
        if let Some((l, r)) = tail_base_opt {
            let mut il = 0usize;
            let mut ir = 0usize;
            let mut dl = f64::INFINITY;
            let mut dr = f64::INFINITY;
            for (i, p) in body_pts.iter().enumerate() {
                let d = (p.x as f64 - l.x).powi(2) + (p.y as f64 - l.y).powi(2);
                if d < dl { dl = d; il = i; }
                let d2 = (p.x as f64 - r.x).powi(2) + (p.y as f64 - r.y).powi(2);
                if d2 < dr { dr = d2; ir = i; }
            }
            (egui::Pos2::new(l.x as f32, l.y as f32), egui::Pos2::new(r.x as f32, r.y as f32), Some((il, ir)))
        } else {
            let base_cx = face_x.clamp(balloon_x + tail_half, balloon_x + balloon_w - tail_half);
            let base_y = balloon_y + balloon_h;
            (egui::Pos2::new(base_cx - tail_half, base_y), egui::Pos2::new(base_cx + tail_half, base_y), None)
        }
    } else {
        let base_cx = face_x.clamp(balloon_x + tail_half, balloon_x + balloon_w - tail_half);
        let base_y = balloon_y + balloon_h;
        (egui::Pos2::new(base_cx - tail_half, base_y), egui::Pos2::new(base_cx + tail_half, base_y), None)
    };
    let base_c = egui::Pos2::new((base_left.x + base_right.x) / 2.0, (base_left.y + base_right.y) / 2.0);
    let bend = 6.0 * m.scale; // fixed: 0.18*dist made long tails a fat leaf
    let black = egui::Stroke::new(m.stroke_w, Color32::BLACK);
    // Body fill + outline first. Fill stays closed (solid white).
    // Outline is OPEN with a BreakSpline gap at the tail base when we
    // have a tail — the tail sides meet the gap edges, no stroke crosses
    // the tail, and no side segment draws inside the body.
    painter.add(egui::Shape::Path(egui::epaint::PathShape { points: body_pts.clone(), closed: true, fill: Color32::WHITE, stroke: egui::Stroke::NONE }));
    let body_outline: Vec<egui::Pos2> = if let Some((il, ir)) = gap_idx {
        let n = body_pts.len();
        let mut v = Vec::new();
        let mut i = ir;
        loop {
            if let Some(pt) = body_pts.get(i) { v.push(*pt); }
            if i == il { break; }
            i = (i + 1) % n;
            if v.len() > n + 1 { break; }
        }
        v
    } else {
        body_pts.clone()
    };
    let body_closed = gap_idx.is_none();
    if msg.kind == MessageKind::Whisper {
        if body_closed {
            painter.add(egui::Shape::closed_line(body_outline.clone(), egui::Stroke::new(m.halo_w, Color32::WHITE)));
            let mut closed = body_outline.clone(); closed.push(body_outline[0]);
            painter.add(egui::Shape::dashed_line(&closed, black, m.dash_len, m.gap_len));
        } else {
            painter.add(egui::Shape::Path(egui::epaint::PathShape { points: body_outline.clone(), closed: false, fill: Color32::TRANSPARENT, stroke: egui::Stroke::new(m.halo_w, Color32::WHITE) }));
            painter.add(egui::Shape::dashed_line(&body_outline, black, m.dash_len, m.gap_len));
        }
    } else if body_closed {
        painter.add(egui::Shape::closed_line(body_outline.clone(), black));
    } else {
        painter.add(egui::Shape::Path(egui::epaint::PathShape { points: body_outline, closed: false, fill: Color32::TRANSPARENT, stroke: black }));
    }
    // Tail: separate filled polygon on the outline (BreakSpline gap above),
    // then only its two side polylines. The base edge itself is never
    // stroked (it is the gap edge).
    if !action && msg.kind != MessageKind::Think {
        let tail_pts = tail_polygon(base_left, base_right, tip, bend);
        painter.add(egui::Shape::Path(egui::epaint::PathShape { points: tail_pts.clone(), closed: true, fill: Color32::WHITE, stroke: egui::Stroke::NONE }));
        let tip_idx = tail_pts.iter().enumerate().min_by(|(_, a), (_, b)| a.distance(tip).partial_cmp(&b.distance(tip)).unwrap_or(std::cmp::Ordering::Equal)).map(|(i, _)| i).unwrap_or(0);
        let left_side: Vec<egui::Pos2> = tail_pts[0..=tip_idx].to_vec();
        let right_side: Vec<egui::Pos2> = tail_pts[tip_idx..].to_vec();
        if msg.kind == MessageKind::Whisper {
            painter.add(egui::Shape::dashed_line(&left_side, black, m.dash_len, m.gap_len));
            painter.add(egui::Shape::dashed_line(&right_side, black, m.dash_len, m.gap_len));
        } else {
            painter.add(egui::Shape::Path(egui::epaint::PathShape { points: left_side, closed: false, fill: Color32::TRANSPARENT, stroke: black }));
            painter.add(egui::Shape::Path(egui::epaint::PathShape { points: right_side, closed: false, fill: Color32::TRANSPARENT, stroke: black }));
        }
    }
    if msg.kind == MessageKind::Think {
        let (_mid, cctrl) = think_trail_centre(base_c, tip, bend);
        for (frac, dia_r) in [(0.25f32, 14.0f32), (0.55, 9.0), (0.82, 5.0)] {
            let t = frac;
            let u = 1.0 - t;
            let c = egui::Pos2::new(
                u * u * base_c.x + 2.0 * u * t * cctrl.x + t * t * tip.x,
                u * u * base_c.y + 2.0 * u * t * cctrl.y + t * t * tip.y,
            );
            let r = dia_r * m.scale / 2.0;
            painter.circle_filled(c, r, Color32::WHITE);
            painter.circle_stroke(c, r, black);
        }
    }
    let tx = if action { balloon_x + pad_l } else { text_x0 + text_w / 2.0 };
    painter.galley(egui::Pos2::new(tx, text_y0), galley, Color32::BLACK);
}






impl eframe::App for ComicChatApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_events();
        ctx.set_pixels_per_point(self.cfg.ui.scale);
        if self.cfg.ui.theme == "dark" { ctx.set_visuals(egui::Visuals::dark()); } else { ctx.set_visuals(egui::Visuals::light()); }
        ctx.request_repaint_after(std::time::Duration::from_millis(300));

        // Top menu
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("💬 Comic Chat");
                ui.separator();
                let comic_sel = self.view_mode == ViewMode::Comic;
                if ui.selectable_label(comic_sel, icons::icon_label("comic", "Comic Strip")).clicked() { self.view_mode = ViewMode::Comic; }
                if ui.selectable_label(!comic_sel, icons::icon_label("text", "Plain Text")).clicked() { self.view_mode = ViewMode::PlainText; }
                ui.separator();
                if icons::icon_button(ui, &self.cfg.icons, "connect", "Connect All").clicked() {
                    let cfgs: Vec<ServerConfig> = self.cfg.servers.clone();
                    for s in cfgs { self.connect_server(s); }
                }
                // Servers is read-only: Connect / Disconnect only. To add, edit, or remove a server, edit config.lua (servers = { ... }) and restart.
                if icons::icon_button(ui, &self.cfg.icons, "members", "Servers…").clicked() { self.show_servers = true; }
                // Help > About: the original Comic Chat About screen,
                // re-created with this project's name, version, and MIT
                // license (see about.rs). The logo is loaded once here,
                // from ui.about_logo, so a failed load does not retry
                // every frame — the placeholder is shown instead.
                if icons::icon_button(ui, &self.cfg.icons, "info", "About…").clicked() {
                    self.show_about = true;
                    self.about_logo_texture = None;
                    if let Some(path) = self.cfg.ui.about_logo.clone() {
                        self.about_logo_texture = crate::about::load_about_logo(ctx, &path);
                    }
                }
                if icons::icon_button(ui, &self.cfg.icons, "save", "Save Conversation").clicked() { self.save_conversation(); }
                if icons::icon_button(ui, &self.cfg.icons, "refresh", "Rescan ComicArt").clicked() { self.rescan(); }
                // Whisper box: private messages from people outside the
                // room (people in the room get dashed whisper bubbles
                // in the comic). Badge = unread count.
                if icons::icon_button(ui, &self.cfg.icons, "whisper", "Whisper box").clicked() { self.show_whisper_box = true; self.whisper_unread = 0; }
                if self.whisper_unread > 0 { ui.label(RichText::new(format!("({})", self.whisper_unread)).small().strong()); }
                // Away — coffee cup on the toolbar (FAQ: click the cup,
                // enter a message; click again or /away to come back).
                // The message field defaults to "Gone fishing."
                let away_label = if self.away_message.is_some() { "Back" } else { "Away" };
                if icons::icon_button(ui, &self.cfg.icons, "coffee", away_label).clicked() {
                    if self.away_message.is_some() { self.set_away(None); }
                    else {
                        let msg = self.away_input.trim().to_string();
                        let msg = if msg.is_empty() { "Gone fishing.".to_string() } else { msg };
                        self.set_away(Some(msg));
                    }
                }
                ui.add(egui::TextEdit::singleline(&mut self.away_input).desired_width(140.0))
                    .on_hover_text("Away message — default: Gone fishing. Click Away / type /away <message>; /away or Back to return.");
                ui.label(RichText::new(self.save_status.clone()).small());
                ui.label(RichText::new(self.copy_status.clone()).small());
            });
            ui.horizontal(|ui| {
                ui.label(format!("{} {}", self.active_server, self.active_channel));
                // Channel tabs
                let channels: Vec<String> = self.servers.get(&self.active_server).map(|s| s.channels.keys().cloned().collect()).unwrap_or_default();
                for ch in channels {
                    let sel = ch == self.active_channel;
                    if ui.selectable_label(sel, &ch).clicked() { self.active_channel = ch; }
                }
            });
        });

        // Right sidebar: Members + Selfview + Emotion wheel
        egui::SidePanel::right("sidebar").default_width(260.0).show(ctx, |ui| {
            ui.heading("Members");
            let members: Vec<String> = self.servers.get(&self.active_server).and_then(|s| s.channels.get(&self.active_channel)).map(|c| c.members.clone()).unwrap_or_default();
            // Clone char_map for display
            let char_map: HashMap<String, (String, Option<String>)> = self.servers.get(&self.active_server).and_then(|s| s.channels.get(&self.active_channel)).map(|c| c.char_map.clone()).unwrap_or_default();
            let ops: Vec<String> = self.servers.get(&self.active_server).and_then(|s| s.channels.get(&self.active_channel)).map(|c| c.ops.clone()).unwrap_or_default();
            let away_map: HashMap<String, String> = self.servers.get(&self.active_server).and_then(|s| s.channels.get(&self.active_channel)).map(|c| c.away.clone()).unwrap_or_default();
            let in_comics = matches!(self.view_mode, ViewMode::Comic);
            ScrollArea::vertical().scroll_bar_visibility(egui::containers::scroll_area::ScrollBarVisibility::AlwaysVisible).stick_to_bottom(true).max_height(220.0).show(ui, |ui| {
                for m in &members {
                    let in_cast = self.panel_cast.contains(m);
                    let (char_name, _) = char_map.get(m).cloned().unwrap_or_else(|| (m.clone(), None));
                    let is_op = ops.contains(m);
                    let away_msg = away_map.get(m).cloned();
                    ui.horizontal(|ui| {
                        // Comics mode: the member's AVB avatar is the icon
                        // next to their name (the AVB's first decoded
                        // frame — the same texture as the comic panels).
                        // Text mode keeps names plain.
                        if in_comics {
                            if let Some(t) = self.char_texture(ctx, &char_name) {
                                ui.image((t.id(), egui::Vec2::new(24.0, 24.0)));
                            } else {
                                // Placeholder keeps row layout stable when AVB
                                // texture not yet loaded / evicted — avatar must not vanish.
                                ui.label(egui::RichText::new("◯").size(24.0));
                            }
                        }
                        // Hammer = host / channel operator. Coffee cup =
                        // away, with the away message on hover.
                        if is_op { ui.label(icons::icon_label("hammer", "")).on_hover_text("Channel operator (host)"); }
                        if let Some(msg) = &away_msg { ui.label(icons::icon_label("coffee", "")).on_hover_text(format!("Away: {msg}")); }
                        let label = format!("{} — {}", m, char_name);
                        // Original: clicking a member ADDRESSES them (whisper).
                        // Ctrl-click toggles <Chr> cast. Previously every click
                        // toggled cast, so you could never just address someone.
                        // No checkbox here — plain-click checkbox used to whisper,
                        // Ctrl-click toggled cast, so it looked like a cast box but
                        // whispered. Include button below handles cast; Whisper
                        // button + compose kind handle whisper.
                        // Explicit Include button for <Chr> cast (no modifier needed).
                        if ui.small_button("Include").clicked() {
                            if in_cast { self.panel_cast.retain(|x| x != m); }
                            else { self.panel_cast.push(m.clone()); }
                        }
                        ui.label(&label);
                        if ui.small_button(icons::icon_label("whisper", "Whisper")).clicked() {
                            self.whisper_target = Some(m.clone());
                            self.input_kind = MessageKind::Whisper;
                        }
                        if ui.small_button("Version").clicked() {
                            let server = self.active_server.clone();
                            let target = m.clone();
                            self.send_version_query(&server, &target);
                        }
                    });
                }
                if members.is_empty() { ui.label("No members yet — connect and join a channel."); }
            });
            ui.separator();
            ui.separator();
            ui.heading("Selfview");
            let my_char = self.cfg.comic.character.clone();
            // Preview ABOVE the wheel: the EXACT pose about to be sent.
            // current_pose_code() is the single source: wheel if frozen,
            // else analyze(input). Texture + box both use it, so preview == sent.
            // Do not use wheel_emotion / gesture_for_emotion here — they ignore
            // input triggers (I / you / caps / !!!) and would show a different pose.
            let preview_pose = self.current_pose_code();
            let my_tex = self.char_pose_texture(ctx, &my_char, preview_pose.emotion(), preview_pose.gesture());
            ui.horizontal(|ui| {
                draw_character_box(ui, &self.cfg.me.nick.clone(), &my_char, preview_pose.emotion(), my_tex, 96.0, 96.0, false, self.cfg.comic.avatar_seed, false);
                ui.vertical(|ui| {
                    ui.label(format!("Character: {my_char}"));
                    if let Some(u) = &self.cfg.comic.character_url { ui.label(format!("URL: {u}")).on_hover_text(u.clone()); }
                    ui.label(format!("Background: {}", self.cfg.comic.background));
                    ui.label(format!("Pose: {} / {:?}", self.current_pose_code().emotion().label(), self.current_pose_code().gesture()));
                });
            });
            // Character / Background are set in config.lua (comic.character / comic.background). No picker in the GUI — edit Lua and restart.
            // The buttons below are usage, not config: share the current background in the channel, or copy its Background-as line.
            ui.horizontal(|ui| {
                if icons::icon_button(ui, &self.cfg.icons, "background", "Share background in channel").clicked() {
                    let line = background_as_line(&self.cfg.comic.background.clone(), self.cfg.comic.background_url.as_deref());
                    let srv = self.active_server.clone(); let ch = self.active_channel.clone();
                    self.send_background_as(&srv, &ch, &line);
                    self.copy_status = "Shared background".into();
                }
                if icons::icon_button(ui, &self.cfg.icons, "copy", "Copy Background-as line").clicked() {
                    let line = background_as_line(&self.cfg.comic.background.clone(), self.cfg.comic.background_url.as_deref());
                    ctx.copy_text(line); self.copy_status = "Copied Background-as line".into();
                }
            });
            ui.label(RichText::new("Character / Background are set in config.lua — edit Lua, restart.").small().weak());
            ui.separator();
            // The emotion-wheel icon is a real button: it toggles Freeze
            // on the wheel below and reports the new state in the status
            // line, so the icon is GUI feedback, not a label.
            if icons::icon_button(ui, &self.cfg.icons, "emotion-wheel", "Emotion wheel").clicked() {
                self.wheel_frozen = !self.wheel_frozen;
                self.copy_status = if self.wheel_frozen { "Emotion wheel frozen".to_string() } else { "Emotion wheel live".to_string() };
            }
            self.draw_emotion_wheel(ui);
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.wheel_frozen, "Freeze");
                ui.label(format!("{} {:.0}%", self.wheel_emotion.label(), self.wheel_strength * 100.0));
            });
            ui.separator();
            ui.heading("Play Sound");
            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut self.sound_name);
                if icons::icon_button(ui, &self.cfg.icons, "sound", "Play Sound").clicked() { self.send_sound(); }
            });
            ui.label(format!("Sounds: {}", self.cfg.comic.sound_path));
            if !self.sound_status.is_empty() { ui.label(RichText::new(self.sound_status.clone()).small()); }
        });

        // Center: comic or plain text
        egui::CentralPanel::default().show(ctx, |ui| {
            match self.view_mode {
                ViewMode::Comic => { self.draw_comic(ui, ctx); }
                ViewMode::PlainText => { self.draw_plain_text(ui); }
            }
        });

        // 0.3.17: status_bar at very bottom, compose_bar directly above it.
        // egui allocates bottom panels from the bottom edge in creation order,
        // so status_bar is created first (bottom-most), compose_bar second.
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let (conn, st) = self.servers.get(&self.active_server).map(|sv| (sv.connected, sv.status.clone())).unwrap_or((false, "Idle".to_string()));
                ui.label(format!("{} {} | {} | {}", self.active_server, self.active_channel, if conn { "Connected" } else { "Not connected" }, st));
                if !self.save_status.is_empty() { ui.label(self.save_status.clone()); }
                if !self.copy_status.is_empty() { ui.label(self.copy_status.clone()); }
            });
        });
        egui::TopBottomPanel::bottom("compose_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                for (kind, label, icon) in [(MessageKind::Say, "Say", "say"), (MessageKind::Think, "Think", "think"), (MessageKind::Whisper, "Whisper", "whisper"), (MessageKind::Action, "Action", "action")] {
                    if ui.selectable_label(self.input_kind == kind, icons::icon_label(icon, label)).clicked() { self.input_kind = kind; if kind != MessageKind::Whisper { self.whisper_target = None; } }
                }
                if self.input_kind == MessageKind::Whisper {
                    ui.label("To:");
                    let members: Vec<String> = self.servers.get(&self.active_server).and_then(|s| s.channels.get(&self.active_channel)).map(|c| c.members.clone()).unwrap_or_default();
                    egui::ComboBox::from_label("").selected_text(self.whisper_target.clone().unwrap_or_else(|| "— pick a member —".into())).show_ui(ui, |ui| {
                        for m in &members { ui.selectable_value(&mut self.whisper_target, Some(m.clone()), m); }
                    });
                }
            });
            ui.horizontal(|ui| {
                let resp = ui.add(egui::TextEdit::singleline(&mut self.input).desired_width(f32::INFINITY));
                // focus_compose was set by send_current but never consumed —
                // that is why focus was lost after Send. Now it is honoured.
                if self.focus_compose { resp.request_focus(); self.focus_compose = false; }
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) { self.send_current(); }
                if icons::icon_button(ui, &self.cfg.icons, "send", "Send").clicked() { self.send_current(); }
            });
                        ui.label("Pose code is appended to every message (original 1.0-2.1 did this visibly). Comic clients strip it and pose the character; text clients see it. CAST: in the code lists the panel cast you clicked.");
            ui.horizontal(|ui| {
                let line = appears_as_line(&self.cfg.comic.character.clone(), self.cfg.comic.character_url.as_deref());
                ui.label(egui::RichText::new(format!("Your character line: {line}")).monospace().small());
                if icons::icon_button(ui, &self.cfg.icons, "announce", "Announce character in channel").clicked() {
                    let srv = self.active_server.clone(); let ch = self.active_channel.clone();
                    self.send_appears_as(&srv, &ch, &line);
                    self.copy_status = "Announced character".into();
                }
                if icons::icon_button(ui, &self.cfg.icons, "copy", "Copy Appears-as line").clicked() { ctx.copy_text(line); self.copy_status = "Copied Appears-as line".into(); }
            });
        });

        // Servers — read-only. Connect / Disconnect only.
        // Servers are configured in config.lua (servers = { ... }).
        // To add, edit, or remove a server, edit config.lua and restart.
        if self.show_servers {
            let mut open = true;
            egui::Window::new("Servers").open(&mut open).show(ctx, |ui| {
                ui.label("Servers are configured in config.lua. Edit Lua and restart to change them.");
                for s in self.cfg.servers.clone() {
                    ui.horizontal(|ui| {
                        ui.label(format!("{} — {}:{} tls={} channels={:?}", s.name, s.host, s.effective_port(), s.tls, s.channels));
                        if icons::icon_button(ui, &self.cfg.icons, "connect", "Connect").clicked() { self.connect_server(s.clone()); }
                        if icons::icon_button(ui, &self.cfg.icons, "disconnect", "Disconnect").clicked() { self.disconnect_server(&s.name.clone()); }
                    });
                }
                if self.cfg.servers.is_empty() { ui.label("No servers in config.lua (servers = { ... })."); }
            });
            if !open { self.show_servers = false; }
        }

        // About (Help > About) — see about.rs for the layout.
        if self.show_about {
            let mut open = true;
            let mut close = false;
            egui::Window::new("About Open Comic Chat").open(&mut open).show(ctx, |ui| {
                if crate::about::show_about_body(ui, &self.cfg, self.about_logo_texture.as_ref()) {
                    close = true;
                }
            });
            if !open || close { self.show_about = false; }
        }

        // Whisper box: private messages from people OUTSIDE the room
        // (people in the room get dashed whisper bubbles in the
        // comic). Text, not comic panels — the original's Whisper
        // box is a plain text window. Opens itself on a new outside
        // whisper; the toolbar button reopens it.
        if self.show_whisper_box {
            let mut open = true;
            egui::Window::new("Whisper box").open(&mut open).show(ctx, |ui| {
                ui.label("Private messages from people outside the room. Whispers from people in the room appear as dashed bubbles in the comic.");
                ScrollArea::vertical().scroll_bar_visibility(egui::containers::scroll_area::ScrollBarVisibility::AlwaysVisible).stick_to_bottom(true).max_height(220.0).show(ui, |ui| {
                    for (n, t) in &self.whisper_lines {
                        ui.label(format!("<{n}> {t}"));
                    }
                    if self.whisper_lines.is_empty() { ui.label("No whispers yet."); }
                });
                ui.horizontal(|ui| {
                    let peer = self.whisper_peer.clone().unwrap_or_else(|| "—".into());
                    ui.label(format!("Reply to {peer}:"));
                    let resp = ui.text_edit_singleline(&mut self.whisper_reply);
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) { self.send_whisper_box_reply(); }
                    if ui.button("Send").clicked() { self.send_whisper_box_reply(); }
                });
            });
            if !open { self.show_whisper_box = false; self.whisper_unread = 0; }
        }
    }
}

impl ComicChatApp {
    fn draw_emotion_wheel(&mut self, ui: &mut Ui) {
        // Colour-chooser disc (PDF Fig 1): 8 emotion wedges around a
        // grey Neutral centre, dot = current. Click/drag sets emotion + strength.
        use egui::{Pos2, Sense, Vec2};
        let size = Vec2::new(180.0, 180.0);
        let (resp, painter) = ui.allocate_painter(size, Sense::click_and_drag());
        let center = resp.rect.center();
        let radius = 70.0;
        // One wedge per emotion, in Emotion::all() order, clockwise
        // from the top. Translucent so the rim markers stay readable.
        for (i, e) in Emotion::all().iter().enumerate() {
            let mid = (i as f32 / 8.0) * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
            let half = std::f32::consts::TAU / 16.0;
            let (a0, a1) = (mid - half, mid + half);
            let mut fan = vec![center];
            for k in 0..=8 {
                let a = a0 + (a1 - a0) * (k as f32 / 8.0);
                fan.push(Pos2::new(center.x + radius * a.cos(), center.y + radius * a.sin()));
            }
            let c = e.color();
            painter.add(egui::Shape::convex_polygon(fan, Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 110), egui::Stroke::NONE));
        }
        painter.circle_stroke(center, radius, egui::Stroke::new(1.0_f32, Color32::GRAY));
        painter.circle_filled(center, 20.0, Color32::from_rgb(200, 200, 200));
        painter.circle_filled(center, 4.0, Color32::BLACK);
        for (i, e) in Emotion::all().iter().enumerate() {
            let ang = (i as f32 / 8.0) * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
            let pos = Pos2::new(center.x + radius * ang.cos(), center.y + radius * ang.sin());
            let is_sel = *e == self.wheel_emotion;
            painter.circle_filled(pos, if is_sel { 14.0 } else { 10.0 }, if is_sel { Color32::from_rgb(255, 180, 0) } else { Color32::LIGHT_GRAY });
            painter.text(pos, egui::Align2::CENTER_CENTER, e.emoji(), egui::FontId::default(), Color32::BLACK);
            // Small label below
            painter.text(Pos2::new(pos.x, pos.y + 16.0), egui::Align2::CENTER_CENTER, e.label(), egui::FontId::monospace(8.0), Color32::DARK_GRAY);
        }
        // Dot for current emotion + strength (radius = strength)
        // Neutral dot at centre. Old unwrap_or(0) put Neutral dot on Happy.
        let dot = if self.wheel_emotion == Emotion::Neutral {
            center
        } else {
            let idx = Emotion::all().iter().position(|e| *e == self.wheel_emotion).unwrap_or(0);
            let ang = (idx as f32 / 8.0) * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
            Pos2::new(center.x + radius * self.wheel_strength * ang.cos(), center.y + radius * self.wheel_strength * ang.sin())
        };
        painter.circle_filled(dot, 6.0, Color32::BLACK);
        if let Some(p) = resp.interact_pointer_pos() {
            if resp.clicked() || resp.dragged() {
                let d = p - center;
                let dist = d.length() / radius;
                // Centre (dist < 0.3) = Neutral. Previously impossible:
                // centre drag still picked an angle, so Neutral was unreachable.
                if dist < 0.3 {
                    self.wheel_emotion = Emotion::Neutral;
                    self.wheel_strength = 0.4;
                } else {
                self.wheel_strength = dist.clamp(0.0, 1.0);
                let mut ang2 = d.angle();
                if ang2 < 0.0 { ang2 += std::f32::consts::TAU; }
                // Convert angle back to nearest emotion (offset by -90°)
                let shifted = (ang2 + std::f32::consts::FRAC_PI_2) % std::f32::consts::TAU;
                let idx2 = ((shifted / std::f32::consts::TAU) * 8.0).round() as usize % 8;
                self.wheel_emotion = Emotion::all()[idx2];
                }
            }
        }
        ui.horizontal(|ui| {
            if ui.small_button("Neutral").clicked() { self.wheel_emotion = Emotion::Neutral; self.wheel_strength = 0.4; }
            for e in Emotion::all() {
                if ui.small_button(e.emoji()).clicked() { self.wheel_emotion = e; }
            }
        });
        ui.add(egui::Slider::new(&mut self.wheel_strength, 0.0..=1.0).text("Strength"));
    }

    fn draw_comic(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let messages: Vec<ComicMessage> = self.servers.get(&self.active_server).and_then(|s| s.channels.get(&self.active_channel)).map(|c| c.messages.clone()).unwrap_or_default();
        let bg_name = self.cfg.comic.background.clone();
        let panels = comic::group_into_panels(&messages, self.cfg.comic.messages_per_panel);
        let per_row = self.panels_per_row(ui.available_width());
        let total_w = ui.available_width();
        let _ = total_w;
        let _spacing = 8.0;
        let normal_panel_w = ((total_w - (per_row as f32 - 1.0) * _spacing - 16.0) / per_row as f32).max(240.0); // Step 112: account for row spacing + Frame margins so the row never overflows and clips the 2nd/3rd panel // panel.cpp: ONE fixed 3000x3000 space, scaled uniformly — not window_width/per_row
        // Original: establishing shot is wide but SAME HEIGHT as normal.
        // total_w made the opening panel huge and broke row alignment.
        let wide_panel_w = 320.0; // panel.cpp: no wide pane — establishing is same size, zoom 0.8 only
        // Background texture (if BGB has PNG sidecar)
        let bg_tex = self.bg_texture(ctx, &bg_name);
        ScrollArea::vertical().scroll_bar_visibility(egui::containers::scroll_area::ScrollBarVisibility::AlwaysVisible).stick_to_bottom(true).show(ui, |ui| {
            if panels.is_empty() { ui.label("No messages yet — say hello! Your character will appear in a panel. Click a member on the right to include them in the next panel."); return; }
            // Only establishing shots are wide. Title cards are a normal single pane, packed per_row like everything else.
            let mut row: Vec<&comic::Panel> = Vec::new();
            for panel in &panels {
                let wide = panel.establishing;
                if wide {
                    if !row.is_empty() {
                        ui.horizontal(|ui| {
                            for p2 in row.drain(..) { self.draw_panel(ui, ctx, p2, &bg_name, bg_tex.clone(), normal_panel_w); }
                        });
                    }
                    ui.horizontal(|ui| { self.draw_panel(ui, ctx, panel, &bg_name, bg_tex.clone(), wide_panel_w); });
                } else {
                    row.push(panel);
                    if row.len() >= per_row {
                        ui.horizontal(|ui| {
                            for p2 in row.drain(..) { self.draw_panel(ui, ctx, p2, &bg_name, bg_tex.clone(), normal_panel_w); }
                        });
                    }
                }
            }
            if !row.is_empty() {
                ui.horizontal(|ui| {
                    for p2 in row.drain(..) { self.draw_panel(ui, ctx, p2, &bg_name, bg_tex.clone(), normal_panel_w); }
                });
            }
        });
    }

    fn draw_panel(&mut self, ui: &mut Ui, ctx: &egui::Context, panel: &comic::Panel, bg_name: &str, bg_tex: Option<TextureHandle>, panel_w_in: f32) {
        // 0.3.17: MS Comic Chat layout. Panel = thick black border, white
        // fill. Background full-bleed behind everything. ACTIONS render as
        // a white RECTANGULAR narration bar at the very top (PDF §8 +
        // real screenshots: "OARION71 RUNS AWAY"), never as a balloon.
        // Balloons in a row above characters. Characters in a row below,
        // sprite or silhouette + nick, with a white halo (PDF §4.4).
        // No pose code shown in balloons (v2.5 sends none). No per-message
        // "+ Include" button — click the character to toggle cast.
        // panel_w comes from draw_comic (computed once from ScrollArea width). Never recompute from ui.available_width() here: inside a horizontal row that is the REMAINING width, so the 2nd panel in a row gets too wide and spills past the sidebar.
        let panel_w = (panel_w_in - 16.0).max(120.0);
        Frame::default()
            .fill(Color32::WHITE)
            .stroke(egui::Stroke::new(3.0_f32, Color32::BLACK))
            .rounding(0.0_f32)
            .inner_margin(egui::Margin::same(0.0))
            .show(ui, |ui| {
                ui.set_width(panel_w);
                // Title card (Starring pane): the first panel on entering
                // a room. No background, no balloons, no Include buttons —
                // big title, STARRING, then the cast's AVB icons + nicks,
                // the same icons the member list uses (char_texture from
                // each nick's Appears-as AVB; silhouette fallback when an
                // AVB has not arrived yet). Matches the original's first
                // pane (screenshots: "JUST US CHUMPS STARRING ProperDave,
                // nobody"). The title message is alone in its panel (see
                // group_into_panels), so panel.messages[0] is the card.
                if let Some(title_msg) = panel.messages.iter().find(|m| m.kind == MessageKind::Title) {
                    let mut _seen: std::collections::HashSet<String> = std::collections::HashSet::new();
                    let starring: Vec<String> = std::iter::once(title_msg.nick.clone()).chain(title_msg.cast.iter().cloned()).filter(|n| _seen.insert(n.to_lowercase())).collect(); // Step 112: dedupe case-insensitive so self is not listed twice
                    // Resolve each starring nick's character: us -> our
                    // AVB; others -> their Appears-as char_map entry;
                    // fallback -> the nick (char_texture draws a silhouette).
                    let chars: Vec<(String, String)> = starring.iter().map(|n| {
                        let c = if *n == self.my_nick() { self.cfg.comic.character.clone() } // Step 112: compare against server nick, not config nick
                            else { self.servers.get(&self.active_server).and_then(|s| s.channels.get(&self.active_channel)).and_then(|c| c.char_map.get(n)).map(|(c, _)| c.clone()).unwrap_or_else(|| n.clone()) };
                        (n.clone(), c)
                    }).collect();
                                        // Title padding lives inside the frame now (outer margin is 0).
                    ui.add_space(10.0);
ui.vertical_centered(|ui| {
                        ui.set_width(panel_w);
                        // Title card is a normal pane: same square height.
                        // panel_w+24 made the opening panel taller than every other.
                        ui.set_height(panel_w);
                        ui.label(RichText::new(title_msg.text.to_uppercase()).size(26.0).strong());
                        ui.label(RichText::new("STARRING").strong());
                        ui.horizontal_top(|ui| {
                            for (nick, char_name) in &chars {
                                let tex = self.char_texture(ctx, char_name);
                                ui.vertical(|ui| {
                                    if let Some(t) = tex {
                                        // PDF §4.4 halo: white frame around the AVB sprite.
                                        Frame::default().fill(Color32::WHITE).inner_margin(egui::Margin::same(3.0)).show(ui, |ui| {
                                            ui.image((t.id(), egui::Vec2::new(72.0, 72.0)));
                                        });
                                    } else {
                                        // Same silhouette as draw_character_box's
                                        // fallback, minus the ▲ marker and the
                                        // emotion label (title panels have no
                                        // balloons, so nothing to link to).
                                        let (resp, painter) = ui.allocate_painter(egui::Vec2::new(72.0, 72.0), egui::Sense::hover());
                                        let rect = resp.rect;
                                        let col = comic::avatar_color(nick, self.cfg.comic.avatar_seed);
                                        let cx = rect.center().x;
                                        painter.circle_filled(egui::Pos2::new(cx, rect.top() + 16.0), 17.0, Color32::WHITE);
                                        painter.rect_filled(egui::Rect::from_min_max(egui::Pos2::new(cx - 22.0, rect.top() + 30.0), egui::Pos2::new(cx + 22.0, rect.bottom())), 8.0, Color32::WHITE);
                                        painter.circle_filled(egui::Pos2::new(cx, rect.top() + 16.0), 13.0, col);
                                        painter.rect_filled(egui::Rect::from_min_max(egui::Pos2::new(cx - 18.0, rect.top() + 32.0), egui::Pos2::new(cx + 18.0, rect.bottom())), 6.0, col);
                                    }
                                    ui.label(RichText::new(nick).strong().small());
                                });
                            }
                        });
                    });
                    return;
                }
                // Info panel (version / away): the original's one-character
                // panel — white, NO background, NO balloon. Text at the
                // top (wrapped, upper-cased like bubble text), the
                // subject's character standing alone below, no nick /
                // emotion label under them. Same layout as the version
                // panel in the reference screenshot:
                // "OARION7'S VERSION: MICROSOFT CHAT 3.8 (8.67.6401)
                // (COMICS MODE)" + the wolf alone.
                if let Some(info_msg) = panel.messages.iter().find(|m| m.kind == MessageKind::Info) {
                    let char_name = if info_msg.nick == self.cfg.me.nick { self.cfg.comic.character.clone() }
                        else { self.servers.get(&self.active_server).and_then(|s| s.channels.get(&self.active_channel)).and_then(|c| c.char_map.get(&info_msg.nick)).map(|(c, _)| c.clone()).unwrap_or_else(|| info_msg.nick.clone()) };
                    let nick = info_msg.nick.clone();
                    let text = info_msg.text.to_uppercase();
                    let tex = self.char_body_texture(ctx, &char_name);
                    ui.vertical_centered(|ui| {
                        ui.set_width(panel_w);
                        ui.label(RichText::new(text).size(14.0));
                        ui.add_space(6.0);
                        if let Some(t) = tex {
                            let size = 72.0 * panel.zoom;
                            let (resp, painter) = ui.allocate_painter(egui::Vec2::new(size, size), egui::Sense::hover());
                            painter.image(t.id(), resp.rect, egui::Rect::from_min_max(egui::Pos2::new(0.0, 0.0), egui::Pos2::new(1.0, 1.0)), Color32::WHITE);
                        } else {
                            let size = 72.0 * panel.zoom;
                            let k = size / 72.0;
                            let (resp, painter) = ui.allocate_painter(egui::Vec2::new(size, size), egui::Sense::hover());
                            let rect = resp.rect;
                            let col = comic::avatar_color(&nick, self.cfg.comic.avatar_seed);
                            let cx = rect.center().x;
                            painter.circle_filled(egui::Pos2::new(cx, rect.top() + 16.0 * k), 17.0 * k, Color32::WHITE);
                            painter.rect_filled(egui::Rect::from_min_max(egui::Pos2::new(cx - 22.0 * k, rect.top() + 30.0 * k), egui::Pos2::new(cx + 22.0 * k, rect.bottom())), 8.0 * k, Color32::WHITE);
                            painter.circle_filled(egui::Pos2::new(cx, rect.top() + 16.0 * k), 13.0 * k, col);
                            painter.rect_filled(egui::Rect::from_min_max(egui::Pos2::new(cx - 18.0 * k, rect.top() + 32.0 * k), egui::Pos2::new(cx + 18.0 * k, rect.bottom())), 6.0 * k, col);
                        }
                    });
                    return;
                }
                // Vertical wrapper: Frame inherits the parent horizontal layout, so without this the panel's children lay side-by-side.
                // Fixed square height: panels must not grow with balloon text.
                ui.vertical(|ui| {
                ui.set_height(panel_w);
                // Background: full-bleed image, or flat colour fallback.
                // We draw the bg as an image sized to panel width, then
                // overlay caption + balloons + characters on top by using
                // a vertical layout with the image first (egui stacks).
                // Square pane, bg fills it: bg height = panel width. Then negative space pulls the cursor back to the top so balloons + characters overlay the bg (original Comic Chat), not stack below it (which made panes tall with a bg strip on top).
                // Pane grows with content: tall balloon (7 lines) + tail + character
                // must fit, otherwise the character is cut off at the bottom.
                // Estimate balloon height from the longest message in this panel.
                let mut balloon_h_est: f32 = 0.0;
                for mm in &panel.messages {
                    let col_w_est = ((panel_w - 16.0) / panel.messages.len().max(1) as f32).max(80.0);
                    let max_w_est = (col_w_est - 32.0).max(60.0).min((0.85 * panel_w - 16.0).max(60.0));
                    let chars_per_line = (max_w_est / 11.0).max(8.0);
                    let lines_est = ((mm.text.chars().count() as f32 / chars_per_line).ceil() as usize).max(1) as f32;
                    let h_est = lines_est * 20.0 + 16.0 + 22.0;
                    if h_est > balloon_h_est { balloon_h_est = h_est; }
                }
                let char_h_est = 72.0 * panel.zoom + 36.0;
                let content_h = 8.0 + balloon_h_est + char_h_est + 8.0 + (panel.actions.len() as f32 * 28.0);
                let bg_h = panel_w.max(content_h);
                if let Some(tex) = &bg_tex {
                    ui.image((tex.id(), egui::Vec2::new(panel_w, bg_h)));
                    ui.add_space(-bg_h);
                } else {
                    Frame::default().fill(comic::background_color(bg_name)).show(ui, |ui| {
                        ui.set_width(panel_w);
                        ui.set_height(bg_h);
                        ui.label(egui::RichText::new(format!("Background: {bg_name}")).small().italics());
                    });
                    ui.add_space(-bg_h);
                }
                // ACTIONS — PDF §8 + real screenshots: IRC /me is a NARRATION
                // box, NOT a balloon. White rectangle, black border, black
                // bold ALL-CAPS text, full width, at the very top of the
                // panel. Screenshot: "OARION71 RUNS AWAY". The actor still
                // stands below in the character row (see display_msgs).
                for act in &panel.actions {
                    Frame::default().fill(Color32::WHITE).stroke(egui::Stroke::new(2.0_f32, Color32::BLACK)).rounding(0.0_f32).inner_margin(egui::Margin::same(6.0_f32)).show(ui, |ui| {
                        ui.set_width(panel_w);
                        ui.label(egui::RichText::new(format!("{} {}", act.nick.to_uppercase(), act.text.to_uppercase())).strong());
                    });
                }
                // Narration caption (join/leave etc.) — yellow box, kept
                // separate from action bars. No longer used for /me.
                if let Some(cap) = &panel.caption {
                    Frame::default().fill(Color32::from_rgb(255, 255, 190)).stroke(egui::Stroke::new(1.0_f32, Color32::BLACK)).rounding(2.0_f32).inner_margin(egui::Margin::same(6.0_f32)).show(ui, |ui| {
                        ui.label(egui::RichText::new(cap.clone()).italics());
                    });
                }
                // Breathing room so the balloon does not touch the panel's top border.
                ui.add_space(8.0);
                // Balloons + tails in ONE painter per column (spec 1.5):
                // the tail base overlaps the balloon body and is drawn
                // BEFORE the balloon fill, so the fill hides the seam.
                // Original panel.cpp GetInterveningBBox: a balloon is pushed
                // down only when it horizontally overlaps one already placed
                // (asymmetric level test — only a balloon entirely to the left
                // permits level tops). Narrow balloons from speakers stood
                // apart sit shoulder-to-shoulder instead of stacking.
                let mut balloon_y: Vec<f32> = vec![0.0; panel.messages.len()];
                {
                    let n = panel.messages.len().max(1) as f32;
                    let col_w = ((panel_w - 16.0) / n).max(80.0);
                    // Estimate each balloon's x-range from its TEXT width
                    // (GetCloudEstimate: balloon x near speaker's m_arrowX =
                    // column centre, width = text). Column-width overlap never
                    // fires because columns are adjacent, not overlapping.
                    for i in 0..panel.messages.len() {
                        let est_w = ((panel.messages[i].text.len() as f32 * 7.0) + 28.0).min(col_w);
                        let cx = (i as f32 + 0.5) * col_w;
                        let x0 = cx - est_w / 2.0;
                        let x1 = cx + est_w / 2.0;
                        let mut y = 0.0f32;
                        for j in 0..i {
                            let jest_w = ((panel.messages[j].text.len() as f32 * 7.0) + 28.0).min(col_w);
                            let jcx = (j as f32 + 0.5) * col_w;
                            let jx0 = jcx - jest_w / 2.0;
                            let jx1 = jcx + jest_w / 2.0;
                            let overlaps = x0 < jx1 && x1 > jx0;
                            // Asymmetric: only push down on overlap.
                            if overlaps { y = y.max(balloon_y[j] + 22.0); }
                        }
                        balloon_y[i] = y;
                    }
                }
                // extra_tail = short MS Chat gap, so tail tip lands on head right under balloon.
                // Defined once so the spacer below can subtract the same
                // height the balloon row actually takes (balloon_h_est +
                // extra_tail), not just balloon_h_est.
                let extra_tail = if panel.messages.len() <= 1 { 30.0 } else { 70.0 }; // Fig 8-1: tail varies with shot — close-up shorter, wide longer, slender taper
                ui.horizontal_top(|ui| {
                    for (bi, msg) in panel.messages.iter().enumerate() {
                        let bw = ((panel_w - 16.0) / panel.messages.len().max(1) as f32).max(80.0);
                        ui.vertical(|ui| {
                            if balloon_y[bi] > 0.0 { ui.add_space(balloon_y[bi]); }
                            draw_balloon(ui, msg, bw, panel_w, extra_tail); // extra_tail = ground-line gap, so tail tip lands on head
                        });
                    }
                    if panel.messages.is_empty() {
                        ui.label(egui::RichText::new("(empty panel)").small().italics());
                    }
                });
                // Characters row: sprite or silhouette + nick below.
                // Click a character to toggle them in the next panel's cast.
                // Characters: balloon speakers + action actors (dedup by nick),
                // so an action-only panel still shows its actor standing below.
                // panel.cpp GetBalloonRect: balloons in UPPER region, characters
                // stand on ground line at panel bottom. Without this the
                // characters float mid-panel (right screenshot).
                let _balloon_h_est = balloon_h_est;
                // LayoutAvatars (panel.cpp): maxBodyHeight = unitHeight/1.9,
                // base width = maxBodyHeight * aspect (avg 0.7, normHeight=100
                // for all in GetDimInfo), if sumWidth > unitWidth: reduction,
                // else zoomFactor = unitWidth/sumWidth capped by headFactor
                // 1.666 (headHeight = height/2), if <1.1 then 1.0.
                let _max_body_h = bg_h / 1.9;
                let _n_chars = panel.messages.len().max(1) as f32;
                let _sum_w = _max_body_h * 0.7 * _n_chars;
                let _char_h_est = if _sum_w > panel_w {
                    _max_body_h * (panel_w / _sum_w)
                } else {
                    let mut _zf = panel_w / _sum_w;
                    _zf = _zf.min(1.666);
                    if _zf < 1.1 { _zf = 1.0; }
                    _max_body_h * _zf
                };
                let _spacer = (bg_h - 8.0 - _balloon_h_est - extra_tail - _char_h_est).max(0.0);
                ui.add_space(_spacer);
                // Torso-up slot: fill exactly what is left to the frame edge.
                // Fixed 150*zoom leaves a gap or overflows by the spacing
                // the estimate does not count. Full-body keeps 150*zoom.
                let char_slot_h = ui.available_height().max(50.0);
                let mut display_msgs: Vec<&ComicMessage> = Vec::new();
                for m in &panel.messages { display_msgs.push(m); }
                for a in &panel.actions { if !display_msgs.iter().any(|m| m.nick == a.nick) { display_msgs.push(a); } }
                // Character columns share the balloon columns' width,
                // so each tail lands on its speaker. Characters scale
                // with the panel zoom (PDF §6.2); balloons do not.
                // Torso-up rule: one distinct character, no actions (an action
                // needs legs to read), no listeners. Otherwise full body so
                // every character fits.
                let mut all_cast: Vec<&String> = Vec::new();
                for m in &panel.messages { for c in &m.cast { if !all_cast.contains(&c) { all_cast.push(c); } } }
                let has_listener = all_cast.iter().any(|c| !display_msgs.iter().any(|m| &m.nick == *c));
                let torso_up = display_msgs.len() == 1 && panel.actions.is_empty() && !has_listener;
                let col_w = ((panel_w - 16.0) / panel.messages.len().max(1) as f32).max(80.0);
                ui.horizontal_top(|ui| {
                    for (idx, msg) in display_msgs.iter().enumerate() {
                        // Pose-aware: emotion+gesture from the message, not largest DIB.
                        // Same (emotion, gesture) => same AVB pose, preview == panel.
                        // Do not hash nick+text: that made "you are" show Coy in preview
                        // and Happy in-panel for the same Neutral / PointOther.
                        let tex = self.char_pose_texture_seeded(ctx, &msg.character, msg.emotion, msg.gesture, msg.neutral_variant);
                        let nick = msg.nick.clone();
                        let in_cast = self.panel_cast.contains(&nick);
                        // Facing (PDF §4.3): AVB art faces RIGHT, so the
                        // first character faces right (toward the rest)
                        // and every later character faces left (toward
                        // them). Only flipped when needed — a lone
                        // character, or a title card, is never flipped.
                        let flip = idx > 0;
                        ui.vertical_centered(|ui| {
                            ui.set_width(col_w);
                            let avatar_resp = draw_character_box(ui, &msg.nick, &msg.character, msg.emotion, tex, if torso_up { char_slot_h } else { _char_h_est }, col_w, flip, self.cfg.comic.avatar_seed, torso_up);
                            // Clicking avatar in frame = Include
                            if avatar_resp.clicked() {
                                if in_cast { self.panel_cast.retain(|x| x != &nick); }
                                else { self.panel_cast.push(nick.clone()); }
                            }
                            // No Include / Remove button here — the links
                            // at the bottom of message panes made no sense.
                            // To include someone, select them in the Members
                            // list, then Enter on an empty message (<Chr>).
                        });
                    }
                    // Cast listeners who have not spoken in this panel.
                    for msg in &panel.messages {
                        for cast_nick in &msg.cast {
                            if panel.messages.iter().any(|m| &m.nick == cast_nick) { continue; }
                            let char_name = self.servers.get(&self.active_server).and_then(|s| s.channels.get(&self.active_channel)).and_then(|c| c.char_map.get(cast_nick)).map(|(n, _)| n.clone()).unwrap_or_else(|| cast_nick.clone());
                            let tex = self.char_pose_texture(ctx, &char_name, comic::Emotion::Neutral, comic::Gesture::Neutral);
                            // Listeners stand behind the speakers and
                            // face LEFT toward them (PDF §4.3).
                            ui.vertical_centered(|ui| {
                                ui.set_width(col_w);
                                let avatar_resp = draw_character_box(ui, cast_nick, &char_name, Emotion::Neutral, tex, _char_h_est, col_w, true, self.cfg.comic.avatar_seed, false);
                                if avatar_resp.clicked() {
                                    if self.panel_cast.contains(cast_nick) { self.panel_cast.retain(|x| x != cast_nick); }
                                    else { self.panel_cast.push(cast_nick.clone()); }
                                }
                            });
                        }
                    }
                });
                }); // close vertical wrapper
            });
    }

    fn draw_plain_text(&mut self, ui: &mut Ui) {
        let logs: Vec<(String, String)> = self.servers.get(&self.active_server).and_then(|s| s.channels.get(&self.active_channel)).map(|c| c.text_log.clone()).unwrap_or_default();
        ScrollArea::vertical().scroll_bar_visibility(egui::containers::scroll_area::ScrollBarVisibility::AlwaysVisible).stick_to_bottom(true).show(ui, |ui| {
            for (nick, text) in &logs {
                // If show_pose_codes is false, strip the pose code for display
                let display = if self.cfg.comic.show_pose_codes_in_text { text.clone() } else {
                    crate::pose::parse_pose_code(text).map(|(c, _)| c).unwrap_or_else(|| text.clone())
                };
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("<{nick}>")).strong().small());
                    ui.label(&display);
                });
            }
            if logs.is_empty() { ui.label("No messages yet."); }
        });
    }
}
