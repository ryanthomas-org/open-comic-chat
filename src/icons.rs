// UI icons — SVGs in icons/*.svg, drawn as a 90s toolbar row.
// Style guide: the original Microsoft Comic Chat toolbar (MIT source,
// github.com/microsoft/comic-chat, released July 16 2026) used one
// fixed-size BMP bitmap strip for its toolbar — flat, chunky, 16-colour,
// every button a literal picture of what it did. These SVGs keep that
// look (flat fills, thick outlines, literal pictures) but as vectors,
// so they scale cleanly on HiDPI instead of going blurry like a
// stretched BMP. SVG is kept deliberately: it is the format that
// scales, and the 90s look is intentional, not a placeholder.
//
// Format note: egui 0.27 cannot rasterise SVG natively, so the .svg
// files are the canonical icons (used by docs, the icon gallery, and
// any future SVG-capable renderer). In the egui UI each button shows
// [colourful emoji glyph] + text label, and its tooltip names the
// SVG file that glyph stands for, so the mapping is never hidden.
//
// AUDIT RULE — every icon must serve a purpose. The point of adding
// icons is GUI feedback: each key below is attached to a visible
// button / selectable that does something observable (opens a window,
// toggles a state, sends a line, selects a mode) and reports it via
// tooltip + status line. If you add a key, wire a button for it in
// app.rs or delete the key. Audit (key -> UI attachment -> feedback):
//   connect -> Connect All / Servers Connect -> status line "Connected"
//   disconnect -> Servers Disconnect -> status line "Disconnected"
//   say/think/whisper/action -> Compose kind selectables -> kind changes,
//     Send button label + balloon shape change
//   whisper -> member Whisper button -> whisper target + compose kind
//   character -> Selfview "Character..." button + Options heading -> opens Options
//   background -> Selfview "Background..." button + Options heading +
//     Share background button -> opens Options / sends Background-as line
//   comic -> Comic Strip view selectable -> view switches to comic
//   text -> Plain Text view selectable -> view switches to text log
//   save -> Save Conversation / Save & Connect / Save settings -> save_status
//   copy -> Copy pose code / Copy code / Copy Appears-as / Copy Background-as
//     -> copy_status "Copied ..." + clipboard
//   sound -> Play Sound button -> sends (sound.wav) + status
//   send -> Compose Send -> message sent + input cleared
//   settings -> Options... button -> opens Options window
//   refresh -> Rescan ComicArt -> rescans + status
//   members -> Servers... button -> opens Servers window
//   announce -> Announce character button -> sends Appears-as + status
//   emotion-wheel -> Emotion wheel button -> toggles Freeze + status
//   coffee -> Away button (top bar) -> toggles away / back + status;
//     member list: coffee cup next to an away nick, hover = away message
//   hammer -> member list: hammer next to a channel operator (host, @ in
//     NAMES / MODE +o), hover = "Channel operator (host)"
//
// Override: config.lua `icons = { say = "icons/my-say.svg", ... }`
// replaces the SVG path for that key. An override does not have to
// exist on disk to be recorded; a missing file falls back to the
// built-in glyph in the UI and is logged by the caller.
// BRACKETS: hyphenated keys MUST use bracket syntax in Lua —
// `icons = { ["emotion-wheel"] = "icons/my-wheel.svg" }`.
// Bare `emotion-wheel = "..."` is parsed as `emotion - wheel` and
// breaks the config. GUI Save writes bracket form automatically.
use std::collections::HashMap;

/// Every icon key the UI uses. Add a key here + an icons/<key>.svg file
/// + a glyph in `icon_glyph` to add a new UI icon.
#[allow(dead_code)]
pub const ICON_NAMES: &[&str] = &[
    "connect", "disconnect", "say", "think", "whisper", "action",
    "character", "background", "comic", "text", "save", "copy",
    "sound", "send", "settings", "refresh", "members", "announce",
    "emotion-wheel", "coffee", "hammer",
];

/// Default SVG path for a key, relative to the project / config dir.
pub fn default_icon_path(name: &str) -> String { format!("icons/{name}.svg") }

/// Resolve an icon key through the user's config overrides.
/// `overrides` is `cfg.icons` from config.lua. Unknown keys fall back
/// to the default path; an explicit override always wins.
pub fn resolve_icon_path(overrides: &HashMap<String, String>, name: &str) -> String {
    overrides.get(name).cloned().unwrap_or_else(|| default_icon_path(name))
}

/// Colourful emoji glyph that stands for the SVG in the egui UI.
/// Kept 1:1 with the SVG so the button reads the same in both places.
pub fn icon_glyph(name: &str) -> &'static str {
    match name {
        "connect" => "\u{1f50c}",
        "disconnect" => "\u{274c}",
        "say" => "\u{1f4ac}",
        "think" => "\u{1f4ad}",
        "whisper" => "\u{1f92b}",
        "action" => "\u{1f4e3}",
        "character" => "\u{1f642}",
        "background" => "\u{1f304}",
        "comic" => "\u{1f5bc}",
        "text" => "\u{1f4c4}",
        "save" => "\u{1f4be}",
        "copy" => "\u{1f4cb}",
        "sound" => "\u{1f514}",
        "send" => "\u{2708}",
        "settings" => "\u{2699}",
        "refresh" => "\u{1f504}",
        "members" => "\u{1f465}",
        "announce" => "\u{1f4e2}",
        "emotion-wheel" => "\u{1f3ad}",
        "coffee" => "\u{2615}",
        "hammer" => "\u{1f528}",
        _ => "\u{25a1}",
    }
}

/// "💬 Say" — glyph + label, for selectable_labels and small_buttons.
pub fn icon_label(name: &str, label: &str) -> String { format!("{} {}", icon_glyph(name), label) }

/// Icon button for the egui UI. Shows glyph + label, tooltips the SVG
/// path (so an override is visible) and the config key to override it.
pub fn icon_button(
    ui: &mut egui::Ui,
    overrides: &HashMap<String, String>,
    name: &str,
    label: &str,
) -> egui::Response {
    let path = resolve_icon_path(overrides, name);
    ui.button(icon_label(name, label))
        .on_hover_text(format!("Icon: {path}  (override: icons.{name} = \"...\" in config.lua)"))
}


#[cfg(test)]
mod icon_names_edge_tests {
    use super::ICON_NAMES;

    #[test]
    fn icon_names_non_empty() {
        assert!(!ICON_NAMES.is_empty(), "ICON_NAMES should not be empty");
        // All names should be non-empty strings
        for name in ICON_NAMES {
            assert!(!name.is_empty(), "ICON_NAMES entry should not be empty");
        }
    }
}
