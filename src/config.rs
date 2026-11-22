//! Config: Lua is authoritative and immutable except by the user.
//!
//! The GUI never writes config.lua. There is no `save()` in this module.
//! All configuration is done exclusively in Lua (see config.lua.example):
//! edit the file in a text editor and restart. The program reads it once
//! at startup. `load_or_create` writes the file only when it does not
//! exist yet (first run), from `DEFAULT_CONFIG_LUA`. A file that fails
//! to parse is left untouched and defaults are used for that run.

use mlua::{Lua, Table, Value};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::collections::HashMap;

/// The first-run config.lua text — identical to config.lua.example in the
/// repo. Written once by `load_or_create` when the file does not exist.
/// After that, only the user edits it; the program only reads it.
/// It includes the About-logo reproduction instructions (ui.about_logo).
pub const DEFAULT_CONFIG_LUA: &str = r##"-- comic-chat-rs 0.3.17 — example config
-- Version line: 0.3.x — this release is 0.3.17. Do NOT bump to 0.4 / 1.0.
-- Patch increments only (0.3.17, 0.3.17, ...). The version in Cargo.toml
-- and `VERSION` in src/main.rs are the single source of truth; CTCP
-- VERSION replies `Open Comic Chat (Version 0.3.17)` from that constant.
--
-- LUA IS AUTHORITATIVE AND IMMUTABLE EXCEPT BY YOU.
-- The GUI never writes this file. There is no Options window, no Servers
-- editor, no Save button for settings. All usage is in the GUI (chat,
-- connect, whisper, away, Save Conversation); all configuration is done
-- exclusively here, in Lua, in a text editor. To change anything — nick,
-- character, background, servers, theme, font, About logo, CTCP VERSION —
-- edit this file and restart the program. Restart is required: the
-- program reads config.lua once at startup and never re-reads it.
--
-- If this file does not exist at startup, the program creates it from
-- this example (the same text as config.lua.example in the repo).
-- If it fails to parse, the program keeps the file untouched, starts
-- with defaults, and logs the error — fix the Lua and restart.
--
-- AVB/BGB live in ComicArt/ (flat, like C:\Program Files\Microsoft Chat\COMICART)
-- PNG with same stem = preview for rendering. AVB/BGB = authoritative identity + URL.
-- Style guide / history: the original Microsoft Comic Chat source is now
-- MIT licensed at github.com/microsoft/comic-chat. This Rust client is
-- clean-room, but its toolbar, balloons, panel rules, and Comic Sans
-- default follow that source. We do NOT copy its BMP bitmaps or MFC
-- code — our icons are SVGs (see icons note below).
return {
  me = { nick = "comicfan", username = "comicfan", realname = "Comic Chat RS" },
  -- ui.font: the display font, as a FAMILY name — the `family:` value
  -- fontconfig reports. NOT a filename, NOT a PostScript name:
  --   family:         "Comic Sans MS"  <- put THIS in ui.font
  --   file:           "/home/ryan/.fonts/Comic Sans MS/comic.ttf"  <- put THIS in ui.font_path, if you set it
  --   postscriptname: "ComicSansMS"  <- NEVER put this in ui.font; it is not a family name
  -- Default "Comic Sans MS". The original client drew everything in
  -- Comic Sans (it shipped the font). This build does NOT bundle the
  -- font (it is a Microsoft font, not MIT), so it loads a copy already
  -- on this machine.
  -- How the program finds the .ttf, in order:
  --   1. ui.font_path, if set: that exact file, canonicalized (realpath).
  --      It wins over everything below. It goes INSIDE the ui = { ... }
  --      table, e.g. ui = { ..., font = "Comic Sans MS",
  --      font_path = "/home/ryan/.fonts/Comic Sans MS/comic.ttf" }.
  --      A leading ~/ is expanded to the home directory.
  --   2. fc-list: runs `fc-list "Comic Sans MS" --format=%{file}:%{style}\n`
  --      and reads the file fontconfig reports for that family (Regular
  --      face preferred, so comic.ttf wins over comicbd.ttf Bold).
  --      fc-list is used, NOT fc-match: fc-match substitutes a different
  --      font when the family is absent, while fc-list returns nothing
  --      unless the family really matches.
  --   3. Hardcoded candidates: ~/.fonts/Comic Sans MS/comic.ttf,
  --      ~/.local/share/fonts/Comic Sans MS/comic.ttf, flat
  --      ~/.fonts/comic.ttf / ~/.local/share/fonts/comic.ttf, and the
  --      system msttcorefonts locations under /usr/share/fonts/truetype/.
  -- Only the regular face is loaded (comic.ttf, not comicbd.ttf Bold).
  -- If nothing is found, egui's built-in default is used and a warning
  -- is logged listing everything checked.
  -- Set font = "default" to skip the search and always use egui's default.
  -- Check your install:  fc-list | grep -i "comic sans"
  -- See what the program will load:  fc-list "Comic Sans MS" --format=%{file}
  --
  -- About logo (Help > About):
  -- By default the About window shows a drawn placeholder (program name
  -- in a comic bubble). To show the original About bitmap instead,
  -- reproduce it yourself from the MIT-licensed Microsoft repo:
  --   1. Get the repo:  git clone https://github.com/microsoft/comic-chat
  --      It preserves snapshots from a 1996 pre-release through the
  --      2.5 beta, plus the SDK and docs. The About dialog lives in the
  --      2.5 snapshot. Search that snapshot for the About dialog
  --      resource / bitmap — that bitmap is the logo the original
  --      About screen showed on the left.
  --   2. Export / save that bitmap as a PNG or JPG, e.g.
  --      comic-chat-about.png. Any PNG/JPG works — the About window
  --      scales it to fit (max 180 px) and keeps the aspect ratio.
  --      If you already have the bitmap as a .bmp, convert it:
  --        convert about.bmp ~/Pictures/comic-chat-about.png
  --      (ImageMagick) or open it in any image editor and Save As PNG.
  --   3. Put the file somewhere stable, e.g.
  --      ~/Pictures/comic-chat-about.png
  --   4. Uncomment the about_logo line below and set it to that path.
  --      A leading ~/ is expanded to your home directory, same as
  --      ui.font_path. Absolute and relative paths also work.
  --   5. Restart. Help > About now shows your image on the left,
  --      with "Open Comic Chat / Version 0.3.17 / MIT License" on the right.
  -- Do NOT ship the bitmap as your own — it is a Microsoft trademark,
  -- MIT-licensed as part of the Microsoft repo. This project does not
  -- bundle it, and the placeholder is the default for that reason.
  -- If about_logo is unset, or the file cannot be read or decoded,
  -- the placeholder is shown instead. No error dialog — the About
  -- window still opens.
  ui = {
    scale = 1.0, panels_per_row = 0, theme = "light", show_text_log = true, font = "Comic Sans MS",
    -- interface_font: UI (Regular). chat_font: comic balloons (Bold).
    -- Defaults apply when these are absent — no edit needed to get
    -- Comic Sans MS UI + Comic Sans MS Bold comic.
    interface_font = "Comic Sans MS", chat_font = "Comic Sans MS Bold",
    -- font_path = "/home/ryan/.fonts/Comic Sans MS/comic.ttf",
    -- about_logo = "~/Pictures/comic-chat-about.png",
  },
  comic = {
    messages_per_panel = 3, avatar_seed = 7,
    character = "Frank",
    -- character_url = "http://example.com/characters/Frank.AVB",
    background = "Cafe",
    -- background_url = "http://example.com/backgrounds/Cafe.BGB",
    pose_style = "old", -- "old" = OldAngle (#GxxxExxxM1) computed from wheel+text (default), "none" = v2.5 sends no pose code
    auto_download = true,
    show_pose_codes_in_text = true,
    -- titles: custom room-entry title cards (the "Starring pane" — the
    -- first panel on entering a room). These are ADDED on top of the
    -- built-ins, not a replacement. Built-ins: JUST US CHUMPS, DEEPEST
    -- DARKEST DESIRES, YOU SHOULDA BEEN THERE, NETWORKED NERDS. One is
    -- picked at random on each room entry. Empty = built-ins only.
    titles = {},
    sound_path = "~/.config/comic-chat/Sounds",
    -- CTCP VERSION override: when unset, a direct CTCP VERSION query is
    -- answered with `Open Comic Chat (Version 0.3.17)` — the number is
    -- concatenated from VERSION in src/main.rs, so it can never drift
    -- from the release. When set, it replaces the WHOLE string, e.g. to
    -- identify as another client or to hide the version:
    -- ctcp_version = "Open Comic Chat (Version 2.5)",
  },
  -- debug_log: optional verbose debug log. OFF if absent (no file created).
  -- Uncomment to enable: writes to ~/.config/comic-chat/debug.log by default.
  -- debug_log = { enabled = true, level = "debug" }, -- level: error|warn|info|debug|trace, file = "/tmp/comic.log" optional
  -- icons: override any UI icon SVG. Omit the whole table (or a key) to
  -- use the built-in icons/<key>.svg. Every key maps to a real button
  -- that does something observable and reports it (tooltip + status
  -- line) — no icon is decoration. Full audit is in src/icons.rs.
  -- Keys: connect, disconnect, say, think, whisper, action,
  -- character, background, comic, text, save, copy, sound, send,
  -- settings, refresh, members, announce, emotion-wheel, coffee, hammer.
  -- Note: the GUI no longer has an Options window, so the "settings"
  -- icon is unused by any button now, but the key is still accepted
  -- (future use). The "character" / "background" icons are used by the
  -- Share-background / Copy-background-as buttons in the side panel.
  -- BRACKETS — READ THIS BEFORE OVERRIDING emotion-wheel:
  -- Lua table keys that are not plain identifiers MUST use bracket
  -- syntax. "emotion-wheel" contains a hyphen, so it MUST be written as
  --   ["emotion-wheel"] = "icons/my-wheel.svg"
  -- Bare `emotion-wheel = "icons/my-wheel.svg"` is parsed as
  -- `emotion - wheel` (a subtraction) and the whole config FAILS to
  -- load. All other keys are plain identifiers, so bare `say = ...`
  -- is fine for them.
  -- Examples (uncomment one to test):
  -- icons = { say = "icons/my-say.svg", action = "/home/anon/icons/my-action.svg" },
  -- icons = { ["emotion-wheel"] = "icons/my-wheel.svg" },
  icons = {
    -- say = "icons/my-say.svg",
    -- action = "icons/my-action.svg",
    -- ["emotion-wheel"] = "icons/my-wheel.svg",
  },
  servers = {
    { name = "Libera", host = "irc.libera.chat", port = 6697, tls = true, channels = { "#libera" }, autoconnect = true },
    -- { name = "Example", host = "irc.example.com", port = 6697, tls = true, nick = "comicfan", channels = { "#comic" }, autoconnect = false },
    -- SASL (NickServ): uncomment and fill in to identify on connect:
    -- { name = "Libera", host = "irc.libera.chat", port = 6697, tls = true, channels = { "#libera" }, sasl = { user = "comicfan", pass = "hunter2" }, autoconnect = true },
  },
}
"##;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Sasl {
    pub user: String,
    pub pass: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServerConfig {
    pub name: String,
    pub host: String,
    pub port: Option<u16>,
    pub tls: bool,
    pub nick: Option<String>,
    pub channels: Vec<String>,
    pub sasl: Option<Sasl>,
    #[serde(default = "default_true")]
    pub autoconnect: bool,
}

fn default_true() -> bool { true }

impl ServerConfig {
    pub fn effective_port(&self) -> u16 {
        if let Some(p) = self.port { return p; }
        if self.tls { 6697 } else { 6667 }
    }
    pub fn effective_nick(&self, fallback: &str) -> String {
        self.nick.clone().unwrap_or_else(|| fallback.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MeConfig {
    pub nick: String,
    pub username: String,
    pub realname: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UiConfig {
    pub scale: f32,
    pub panels_per_row: usize, // 0 = auto
    pub theme: String,
    pub show_text_log: bool,
    /// Display font, as a FAMILY name — the `family:` value fontconfig
    /// reports, e.g. "Comic Sans MS". NOT a filename (`comic.ttf`) and NOT
    /// a PostScript name (`ComicSansMS`, no space). The original client
    /// drew everything in Comic Sans (it shipped the font). Default
    /// "Comic Sans MS": install_font() resolves the family to a file
    /// (ui.font_path, then `fc-list`, then candidate paths) and falls
    /// back to egui's default with a log hint if none is found.
    #[serde(default = "default_font")]
    pub font: String,
    /// Interface font family, e.g. "Comic Sans MS". Default "Comic Sans MS".
    /// If `interface_font` is set it wins; else legacy `font` is used.
    /// config.lua: `interface_font = "Comic Sans MS"` inside ui = {}.
    #[serde(default = "default_interface_font")]
    pub interface_font: String,
    /// Chat (comic balloon) font family, e.g. "Comic Sans MS Bold".
    /// Default "Comic Sans MS Bold". config.lua: `chat_font = "Comic Sans MS Bold"`.
    #[serde(default = "default_chat_font")]
    pub chat_font: String,
    /// Optional .ttf path for interface_font. None = resolve via fc-list / candidates.
    #[serde(default)]
    pub interface_font_path: Option<String>,
    /// Optional .ttf path for chat_font (e.g. comicbd.ttf). None = resolve Bold face.
    #[serde(default)]
    pub chat_font_path: Option<String>,
    /// Optional FILE path to a .ttf for `font`, e.g.
    /// "/home/ryan/.fonts/Comic Sans MS/comic.ttf". This is a file, not
    /// a family name. If set, it wins over the `fc-list` / candidate
    /// search: the file is canonicalized (realpath) and read directly.
    /// A leading `~/` is expanded to the home directory.
    /// None = resolve the family via `fc-list` / candidate paths.
    #[serde(default)]
    pub font_path: Option<String>,
    /// About screen logo: path to an image (PNG/JPG) shown in Help > About.
    /// None = the drawn placeholder (program name in a comic bubble).
    /// A leading `~/` is expanded. The original About bitmap is a
    /// Microsoft trademark — see config.lua for where to find it in the
    /// MIT-licensed repo; do not redistribute it as your own.
    /// config.lua: `about_logo = "~/Pictures/comic-chat-about.png"` inside ui = {}.
    #[serde(default)]
    pub about_logo: Option<String>,
}
fn default_font() -> String { "Comic Sans MS".to_string() }
fn default_interface_font() -> String { "Comic Sans MS".to_string() }
fn default_chat_font() -> String { "Comic Sans MS Bold".to_string() }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ComicConfig {
    pub messages_per_panel: usize,
    pub avatar_seed: u64,
    pub character: String,
    pub background: String,
    #[serde(default)]
    pub character_url: Option<String>,
    #[serde(default)]
    pub background_url: Option<String>,
    #[serde(default = "default_pose_style")]
    pub pose_style: String, // "old" = OldAngle (#GxxxExxxM1) computed from wheel+text (default), "none" = v2.5 plain text, no code. "new" (invented colon form) is migrated to "none" on load.
    #[serde(default = "default_true")]
    pub auto_download: bool, // Comics View: auto-download AVB/BGB you do not have
    #[serde(default = "default_true")]
    pub show_pose_codes_in_text: bool, // Plain Text view shows (#G...) codes (original did)
    #[serde(default)]
    pub strip_pose_codes_in_comic: bool, // not used — comic always strips
    /// Custom room-entry title cards, ADDED on top of the built-ins
    /// (comic::BUILTIN_TITLES), never a replacement. One is picked at
    /// random per room entry. Empty = built-ins only.
    /// config.lua: `titles = { "THE USUAL SUSPECTS" }` inside comic = {}.
    #[serde(default)]
    pub titles: Vec<String>,
    /// Sound Search Path: where .wav files live for Play Sound and
    /// incoming (sound) messages. `~` = home. Default
    /// `~/.config/comic-chat/Sounds`. config.lua:
    /// `sound_path = "~/.config/comic-chat/Sounds"` inside comic = {}.
    #[serde(default = "default_sound_path")]
    pub sound_path: String,
    /// CTCP VERSION override: the exact string sent in reply to a direct
    /// CTCP VERSION query. None = the default built from the version
    /// number, `Open Comic Chat (Version 0.3.17)` (see version_reply_with
    /// in main.rs — the numerical version is concatenated there, so the
    /// reply can never drift from the release number). Set it to
    /// identify as another client, or to hide the version.
    /// config.lua: `ctcp_version = "Open Comic Chat (Version 2.5)"` inside comic = {}.
    #[serde(default)]
    pub ctcp_version: Option<String>,
}
fn default_pose_style() -> String { "old".to_string() }
fn default_sound_path() -> String { "~/.config/comic-chat/Sounds".to_string() }

/// Optional debug log. Disabled when `debug_log` is absent from config.lua.
/// Forms: `debug_log = true`, `debug_log = "trace"`, or
/// `debug_log = { enabled = true, level = "trace", file = "/tmp/comic.log" }`.
#[derive(Debug, Clone, PartialEq)]
pub struct DebugLogConfig {
    pub enabled: bool,
    pub file: Option<String>,
    pub level: String, // "error" | "warn" | "info" | "debug" | "trace"
}
impl Default for DebugLogConfig {
    fn default() -> Self { Self { enabled: false, file: None, level: "debug".into() } }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AppConfig {
    pub me: MeConfig,
    pub ui: UiConfig,
    pub servers: Vec<ServerConfig>,
    pub comic: ComicConfig,
    pub debug_log: DebugLogConfig,
    /// Icon overrides: config.lua `icons = { say = "icons/my-say.svg", ... }`.
    /// Empty = use the built-in icons/*.svg defaults (see src/icons.rs).
    pub icons: HashMap<String, String>,
    pub path: PathBuf,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            me: MeConfig { nick: "comicfan".into(), username: "comicfan".into(), realname: "Comic Chat RS".into() },
            ui: UiConfig { scale: 1.0, panels_per_row: 0, theme: "light".into(), show_text_log: true, font: default_font(), font_path: None, about_logo: None, interface_font: default_interface_font(), chat_font: default_chat_font(), interface_font_path: None, chat_font_path: None },
            servers: vec![ServerConfig {
                name: "Libera".into(), host: "irc.libera.chat".into(), port: Some(6697), tls: true,
                nick: None, channels: vec!["#libera".into()], sasl: None, autoconnect: true,
            }],
            comic: ComicConfig { messages_per_panel: 3, avatar_seed: 7, character: "Frank".into(), background: "Cafe".into(), character_url: None, background_url: None, pose_style: "old".into(), auto_download: true, show_pose_codes_in_text: true, strip_pose_codes_in_comic: true, titles: Vec::new(), sound_path: default_sound_path(), ctcp_version: None },
            debug_log: DebugLogConfig::default(),
            icons: HashMap::new(),
            path: default_config_path(),
        }
    }
}

pub fn default_config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("comic-chat")
        .join("config.lua")
}

fn lua_str(t: &Table, key: &str, fallback: &str) -> String {
    t.get::<String>(key).unwrap_or_else(|_| fallback.to_string())
}
fn lua_bool(t: &Table, key: &str, fallback: bool) -> bool {
    t.get::<bool>(key).unwrap_or(fallback)
}
fn lua_f32(t: &Table, key: &str, fallback: f32) -> f32 {
    t.get::<f64>(key).map(|v| v as f32).unwrap_or(fallback)
}
fn lua_usize(t: &Table, key: &str, fallback: usize) -> usize {
    t.get::<i64>(key).map(|v| v.max(0) as usize).unwrap_or(fallback)
}

pub fn load_or_create(path: &PathBuf) -> AppConfig {
    if !path.exists() {
        if let Some(parent) = path.parent() { let _ = fs::create_dir_all(parent); }
        // Lua is authoritative: the GUI never writes this file. First run
        // creates it from DEFAULT_CONFIG_LUA (the same text as
        // config.lua.example in the repo). Edit that file, restart.
        // This write is the ONLY write config.rs ever does.
        let _ = fs::write(path, DEFAULT_CONFIG_LUA);
        let mut cfg = AppConfig::default();
        cfg.path = path.clone();
        // Logger is not yet initialised here (main inits it after load), so
        // this log line is buffered only if the logger was already started
        // (e.g. in tests). In normal startup it is a no-op until main inits.
        crate::log::info("config", format!("config file did not exist at {}, created default (debug_log disabled by default, no log file yet)", path.display()));
        return cfg;
    }
    match load(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("config load failed ({e}), using defaults, keeping file untouched");
            crate::log::error("config", format!("config load failed for {}: {e} — using defaults, keeping file untouched. Fix: check Lua syntax, missing me/ui/servers tables, or a bad debug_log value.", path.display()));
            let mut c = AppConfig::default();
            c.path = path.clone();
            c
        }
    }
}

/// Parse `debug_log` from the config root. Absent = disabled.
/// Accepts: bool, string (level), or table { enabled, level, file/path }.
fn parse_debug_log(root: &Table) -> DebugLogConfig {
    let val: Result<Value, _> = root.get("debug_log");
    let val = match val { Ok(v) => v, Err(_) => return DebugLogConfig::default() };
    match val {
        Value::Nil => DebugLogConfig::default(),
        Value::Boolean(b) => DebugLogConfig { enabled: b, file: None, level: "debug".into() },
        Value::String(s) => {
            let lvl = match s.to_str() { Ok(v) => v.to_string(), Err(_) => "debug".to_string() };
            DebugLogConfig { enabled: true, file: None, level: lvl }
        }
        Value::Table(tbl) => {
            // If table is present but `enabled` is missing, treat as enabled —
            // the user explicitly wrote a debug_log table, so they intend logging.
            let enabled: bool = tbl.get("enabled").unwrap_or(true);
            let level: String = tbl.get::<String>("level").unwrap_or_else(|_| "debug".to_string());
            // Accept `file`, `path`, or `filename` — same meaning.
            let file: Option<String> = tbl.get::<String>("file")
                .or_else(|_| tbl.get::<String>("path"))
                .or_else(|_| tbl.get::<String>("filename")).ok();
            DebugLogConfig { enabled, file, level }
        }
        _ => DebugLogConfig::default(),
    }
}

pub fn load(path: &PathBuf) -> Result<AppConfig, Box<dyn std::error::Error>> {
    let src = fs::read_to_string(path)?;
    let lua = Lua::new();
    let val: Value = lua.load(&src).eval()?;
    let root = match val {
        Value::Table(t) => t,
        _ => return Err("config.lua must return a table".into()),
    };

    // Debug log: absent = disabled (no file created, no logging). This parse
    // must run before any other logging, so it never logs itself.
    let debug_log = parse_debug_log(&root);

    // Icons: optional `icons = { say = "path/to/say.svg", ... }` table.
    // Any key may be overridden; unknown keys are kept (future icons).
    // Missing / empty table = all defaults from icons/*.svg.
    let mut icons: HashMap<String, String> = HashMap::new();
    if let Ok(icons_t) = root.get::<Table>("icons") {
        for pair in icons_t.pairs::<String, Value>() {
            if let Ok((k, Value::String(v))) = pair {
                if let Ok(s) = v.to_str() { icons.insert(k, s.to_string()); }
            }
        }
    }

    let me_t: Table = root.get("me")?;
    let ui_t: Table = root.get("ui")?;
    let comic_t: Table = root.get("comic").unwrap_or_else(|_| lua.create_table().unwrap());

    let servers_t: Table = root.get("servers")?;
    let mut servers = Vec::new();
    for pair in servers_t.pairs::<Value, Table>() {
        let (_, s) = pair?;
        let sasl = match s.get::<Table>("sasl") {
            Ok(st) => Some(Sasl {
                user: lua_str(&st, "user", ""),
                pass: lua_str(&st, "pass", ""),
            }),
            Err(_) => None,
        };
        let mut channels = Vec::new();
        if let Ok(ch) = s.get::<Table>("channels") {
            for p in ch.pairs::<Value, String>() { if let Ok((_, c)) = p { channels.push(c); } }
        }
        servers.push(ServerConfig {
            name: lua_str(&s, "name", "Unnamed"),
            host: lua_str(&s, "host", "irc.libera.chat"),
            port: s.get::<i64>("port").ok().map(|v| v as u16),
            tls: lua_bool(&s, "tls", true),
            nick: s.get::<String>("nick").ok(),
            channels,
            sasl,
            autoconnect: lua_bool(&s, "autoconnect", true),
        });
    }

    crate::log::info("config", format!("loaded config from {}: {} server(s), character={} background={} pose_style={} debug_log enabled={} level={} file={:?}",
        path.display(), servers.len(), lua_str(&comic_t, "character", "Frank"), lua_str(&comic_t, "background", "Cafe"), lua_str(&comic_t, "pose_style", "none"), debug_log.enabled, debug_log.level, debug_log.file));

    // comic.titles: custom title cards, same shape as channels (a Lua
    // array of strings). Added on top of the built-ins, never a
    // replacement. Missing / non-table = no custom titles.
    let mut titles = Vec::new();
    if let Ok(t) = comic_t.get::<Table>("titles") {
        for p in t.pairs::<Value, String>() { if let Ok((_, s)) = p { titles.push(s); } }
    }

    Ok(AppConfig {
        debug_log,
        icons,
        me: MeConfig {
            nick: lua_str(&me_t, "nick", "comicfan"),
            username: lua_str(&me_t, "username", "comicfan"),
            realname: lua_str(&me_t, "realname", "Comic Chat RS"),
        },
        ui: UiConfig {
            scale: lua_f32(&ui_t, "scale", 1.0),
            panels_per_row: lua_usize(&ui_t, "panels_per_row", 0),
            theme: lua_str(&ui_t, "theme", "light"),
            show_text_log: lua_bool(&ui_t, "show_text_log", true),
            font: lua_str(&ui_t, "interface_font", &lua_str(&ui_t, "font", "Comic Sans MS")),
            font_path: ui_t.get::<String>("interface_font_path").ok().filter(|s| !s.trim().is_empty()).or_else(|| ui_t.get::<String>("font_path").ok().filter(|s| !s.trim().is_empty())),
            interface_font: lua_str(&ui_t, "interface_font", &lua_str(&ui_t, "font", "Comic Sans MS")),
            chat_font: lua_str(&ui_t, "chat_font", "Comic Sans MS Bold"),
            interface_font_path: ui_t.get::<String>("interface_font_path").ok().filter(|s| !s.trim().is_empty()).or_else(|| ui_t.get::<String>("font_path").ok().filter(|s| !s.trim().is_empty())),
            chat_font_path: ui_t.get::<String>("chat_font_path").ok().filter(|s| !s.trim().is_empty()),
            about_logo: ui_t.get::<String>("about_logo").ok().filter(|s| !s.trim().is_empty()),
        },
        comic: ComicConfig {
            messages_per_panel: lua_usize(&comic_t, "messages_per_panel", 3).max(1),
            avatar_seed: comic_t.get::<i64>("avatar_seed").unwrap_or(7) as u64,
            character: lua_str(&comic_t, "character", "Frank"),
            background: lua_str(&comic_t, "background", "Cafe"),
            character_url: comic_t.get::<String>("character_url").ok(),
            background_url: comic_t.get::<String>("background_url").ok(),
            pose_style: {
                let raw = lua_str(&comic_t, "pose_style", "old");
                // Migrate the invented "new" colon form to "none" (v2.5
                // sends no pose code). "old" is kept as legacy opt-in.
                match raw.trim().to_ascii_lowercase().as_str() {
                    "old" | "legacy" => "old".to_string(),
                    _ => "none".to_string(),
                }
            },
            auto_download: lua_bool(&comic_t, "auto_download", true),
            show_pose_codes_in_text: lua_bool(&comic_t, "show_pose_codes_in_text", true),
            strip_pose_codes_in_comic: true,
            titles,
            sound_path: lua_str(&comic_t, "sound_path", "~/.config/comic-chat/Sounds"),
            ctcp_version: comic_t.get::<String>("ctcp_version").ok().filter(|s| !s.trim().is_empty()),
        },
        servers,
        path: path.clone(),
    })
}


pub fn is_lua_config(path: &str) -> bool { path.trim().to_ascii_lowercase().ends_with(".lua") }



pub fn config_file_name() -> &'static str { "config.lua" }



pub fn comicart_dir_name() -> &'static str { "comicart" }



pub fn cache_dir_name() -> &'static str { "cache" }



pub fn is_valid_port(port: u32) -> bool { port >= 1 && port <= 65535 }



pub fn is_valid_nick(nick: &str) -> bool { let t = nick.trim(); !t.is_empty() && t.chars().count() <= 30 && !t.contains(' ') && !t.contains(':') && !t.contains('#') }



pub fn is_valid_channel(chan: &str) -> bool { let t = chan.trim(); t.starts_with('#') && t.chars().count() >= 2 && !t.contains(' ') }



pub fn default_irc_port() -> u32 { 6667 }



pub fn default_nick_prefix() -> &'static str { "Guest" }



pub fn is_valid_server(s: &str) -> bool { let t = s.trim(); !t.is_empty() && !t.contains(' ') && (t.contains('.') || t == "localhost") }



pub fn is_valid_realname(s: &str) -> bool { let t = s.trim(); !t.is_empty() && t.chars().count() <= 50 }



pub fn is_valid_password(s: &str) -> bool { s.chars().count() <= 100 }



pub fn default_irc_server() -> &'static str { "irc.libera.chat" }



pub fn default_channel_name() -> &'static str { "#comicchat" }



pub fn is_valid_font_size(size: u32) -> bool { size >= 8 && size <= 32 }



pub fn is_valid_theme(theme: &str) -> bool { matches!(theme.trim().to_ascii_lowercase().as_str(), "light" | "dark" | "system") }



pub fn is_valid_language(s: &str) -> bool { s.len() == 2 && s.chars().all(|c| c.is_ascii_alphabetic()) }



pub fn default_font_size() -> u32 { 12 }



pub fn default_theme() -> &'static str { "system" }



pub fn is_valid_username(s: &str) -> bool { let t = s.trim(); !t.is_empty() && t.chars().count() <= 30 && !t.contains(' ') && !t.contains(':') && !t.contains('#') }



pub fn is_valid_email(s: &str) -> bool { let t = s.trim(); if t.contains(' ') || !t.contains('@') { return false; } if let Some(at) = t.find('@') { return t[at..].contains('.'); } false }



pub fn is_valid_url(s: &str) -> bool { let t = s.trim(); (t.starts_with("http://") || t.starts_with("https://")) && t.contains('.') }



pub fn default_username() -> &'static str { "Guest" }



pub fn default_email() -> &'static str { "" }



pub fn is_valid_host(s: &str) -> bool { let t = s.trim(); !t.is_empty() && !t.contains(' ') && (t.contains('.') || t == "localhost") }



pub fn default_port() -> u32 { 6667 }



pub fn default_host() -> &'static str { "irc.libera.chat" }



pub fn is_valid_timeout(v: u32) -> bool { v >= 1 && v <= 300 }



pub fn is_valid_retries(v: u32) -> bool { v <= 10 }



pub fn default_timeout() -> u32 { 30 }



pub fn default_retries() -> u32 { 3 }



pub fn is_valid_cache_size(v: u32) -> bool { v <= 10000 }



pub fn is_valid_max_users(v: u32) -> bool { v >= 1 && v <= 1000 }



pub fn is_valid_max_channels(v: u32) -> bool { v >= 1 && v <= 100 }



pub fn default_max_users() -> u32 { 100 }



pub fn default_max_channels() -> u32 { 10 }



pub fn is_valid_autojoin_channels(s: &str) -> bool { let t = s.trim(); if t.is_empty() { return true; } t.split(',').all(|c| c.trim().starts_with('#')) }



pub fn is_valid_nick_len(v: u32) -> bool { v >= 1 && v <= 30 }



pub fn is_valid_channel_len(v: u32) -> bool { v >= 1 && v <= 50 }



pub fn default_nick_len() -> u32 { 30 }



pub fn default_channel_len() -> u32 { 50 }



pub fn is_valid_server_port(v: u32) -> bool { v >= 1 && v <= 65535 }



pub fn is_valid_log_level(s: &str) -> bool { matches!(s, "DEBUG" | "INFO" | "WARN" | "ERROR" | "TRACE") }



pub fn default_log_level() -> &'static str { "INFO" }



pub fn default_language() -> &'static str { "en" }



pub fn is_valid_encoding(s: &str) -> bool { matches!(s, "UTF-8" | "utf-8" | "ascii" | "ASCII") }



pub fn default_realname() -> &'static str { "Comic Chat User" }



pub fn is_valid_password_len(v: usize) -> bool { v <= 100 }



pub fn is_valid_port_u16(v: u32) -> bool { v >= 1 && v <= 65535 }



pub fn is_valid_secure_mode(s: &str) -> bool { matches!(s, "tls" | "ssl" | "plain" | "TLS" | "SSL") }



pub fn is_valid_realname_len(s: &str) -> bool { let n = s.trim().chars().count(); n >= 1 && n <= 50 }



pub fn is_valid_retry(v: u32) -> bool { v <= 10 }



pub fn default_retry() -> u32 { 3 }



pub fn is_valid_max_retries(v: u32) -> bool { v <= 20 }



pub fn is_valid_max_connections(v: u32) -> bool { v >= 1 && v <= 100 }



pub fn default_max_retries() -> u32 { 5 }



pub fn default_max_connections() -> u32 { 10 }



pub fn is_valid_window_width(v: u32) -> bool { v >= 200 && v <= 4000 }



pub fn is_valid_window_height(v: u32) -> bool { v >= 200 && v <= 3000 }



pub fn default_window_size() -> (u32, u32) { (800, 600) }



pub fn default_cache_size() -> u32 { 100 }



pub fn is_valid_max_history(v: u32) -> bool { v >= 1 && v <= 10000 }



pub fn is_valid_max_panels(v: u32) -> bool { v >= 1 && v <= 100 }



pub fn default_max_panels() -> u32 { 10 }



pub fn is_valid_balloon_padding(v: u32) -> bool { v <= 100 }



pub fn default_balloon_padding() -> u32 { 8 }



pub fn is_valid_panel_gap(v: u32) -> bool { v <= 100 }



pub fn is_valid_max_messages(v: u32) -> bool { v >= 1 && v <= 10000 }



pub fn is_valid_timeout_secs(v: u32) -> bool { v >= 1 && v <= 300 }



pub fn default_timeout_secs() -> u32 { 30 }



pub fn is_valid_retry_delay(v: u32) -> bool { v <= 60 }



pub fn default_max_history() -> u32 { 1000 }



pub fn is_valid_history_size(v: u32) -> bool { v >= 1 && v <= 100000 }



pub fn default_history_size() -> u32 { 1000 }



pub fn is_valid_max_log_size(v: u32) -> bool { v >= 1 && v <= 1000000 }



pub fn is_valid_max_threads(v: u32) -> bool { v >= 1 && v <= 64 }



pub fn default_max_threads() -> u32 { 4 }



pub fn is_valid_max_file_size(v: u32) -> bool { v >= 1 && v <= 100000000 }



pub fn is_valid_max_cache_size(v: u32) -> bool { v >= 1 && v <= 1000000 }



pub fn default_max_cache_size() -> u32 { 10000 }



pub fn is_valid_cache_ttl(v: u32) -> bool { v >= 1 && v <= 86400 }



pub fn default_cache_ttl() -> u32 { 3600 }



pub fn is_valid_max_concurrent(v: u32) -> bool { v >= 1 && v <= 1000 }



pub fn default_retry_delay() -> u32 { 1000 }



pub fn is_valid_max_timeout(v: u32) -> bool { v >= 1 && v <= 3600 }



pub fn default_max_log_size() -> u32 { 1000000 }



pub fn is_valid_max_message_length(v: u32) -> bool { v >= 1 && v <= 10000 }



pub fn is_valid_max_nick_length(v: u32) -> bool { v >= 1 && v <= 50 }



pub fn default_max_nick_length() -> u32 { 30 }



pub fn is_valid_max_channel_length(v: u32) -> bool { v >= 1 && v <= 100 }



pub fn default_max_channel_length() -> u32 { 50 }



pub fn is_valid_max_topic_length(v: u32) -> bool { v >= 1 && v <= 500 }



pub fn is_valid_max_avatar_size(v: u32) -> bool { v >= 1 && v <= 10000000 }



pub fn default_max_avatar_size() -> u32 { 1000000 }



pub fn is_valid_max_bio_length(v: u32) -> bool { v >= 1 && v <= 1000 }



pub fn default_max_bio_length() -> u32 { 200 }



pub fn is_valid_max_password_length(v: u32) -> bool { v >= 1 && v <= 128 }


