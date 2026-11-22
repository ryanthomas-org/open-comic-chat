// Optional debug log — off unless enabled in config.lua.
// 
// Config forms (all optional; if `debug_log` is absent, nothing is logged):
//   debug_log = true                                    -- enabled, level=debug, default file
//   debug_log = "trace"                                 -- enabled, level=trace
//   debug_log = { enabled = true, level = "trace", file = "/tmp/comic.log" }
//   debug_log = { enabled = false }                     -- explicitly off
//
// Default file when `file` is not set:
//   ~/.config/comic-chat/debug.log  (or ./comic-chat-debug.log if no home)
// Levels: error < warn < info < debug < trace. A message is written only if
// its level <= configured level. Trace is the most verbose.
// 
// What gets logged (every level of the app):
//   config — config path, whether file existed, parse result, server count, debug_log settings
//   irc    — DNS, TCP connect, TLS handshake, NICK/USER, JOIN/PART, PING/PONG,
//            PRIVMSG in/out (preview only, not full text), Appears/Background lines,
//            CTCP ACTION, errors, disconnect reason, reconnect
//   avb    — ComicArt scan, AVB/BGB found, header/version, Name/URL, image count,
//            preview PNG found/missing
//   dib    — zlib streams found, inflate success/failure + bytes, DIB header parse,
//            bit depth / palette / dimensions, mask pairing, face/mouth anchors
//   pose   — pose code parse success/failure + why, Appears/Background parse, generated code
//   comic  — text analysis -> emotion/balloon/gesture/strength, panel build, cast
//   app/gui— texture load (DIB vs PNG vs fallback), send message, save, connect/disconnect
//
// Privacy: SASL password is NEVER logged. Chat text is logged only as a short
// preview (first 120 chars, newlines escaped) so the log stays readable and
// does not become a full chat transcript. Full text stays in the in-app log.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level { Error = 1, Warn = 2, Info = 3, Debug = 4, Trace = 5 }

impl Level {
    pub fn parse(s: &str) -> Level {
        match s.to_ascii_lowercase().trim() {
            "error" => Level::Error,
            "warn" | "warning" => Level::Warn,
            "info" => Level::Info,
            "trace" => Level::Trace,
            "debug" => Level::Debug,
            // Unknown level strings fall back to debug so the user still gets output
            // rather than silence; the fallback is logged at init.
            _ => Level::Debug,
        }
    }
    pub fn label(self) -> &'static str {
        match self { Level::Error => "ERROR", Level::Warn => "WARN", Level::Info => "INFO", Level::Debug => "DEBUG", Level::Trace => "TRACE" }
    }
}

struct Logger {
    enabled: bool,
    level: Level,
    file: Option<File>,
    path: PathBuf,
    // True once init() has run. Separate from `enabled` — the old code
    // checked `guard.enabled` to detect "already initialised", but the
    // freshly-created Logger already had enabled=true when logging was
    // on, so the first init() returned early WITHOUT opening the file.
    // Result: file stayed None forever and every log went to stderr.
    started: bool,
}

static LOGGER: OnceLock<Mutex<Logger>> = OnceLock::new();

fn default_log_path() -> PathBuf {
    if let Some(home) = dirs::home_dir() {
        home.join(".config/comic-chat/debug.log")
    } else {
        PathBuf::from("comic-chat-debug.log")
    }
}

/// Resolve a user-supplied file string: expand leading ~/ , keep absolute paths
/// as-is, treat relative paths as relative to the current working directory.
fn resolve_path(raw: Option<String>) -> PathBuf {
    match raw {
        None => default_log_path(),
        Some(s) => {
            let s = s.trim().to_string();
            if s.is_empty() { return default_log_path(); }
            if let Some(rest) = s.strip_prefix("~/") {
                if let Some(home) = dirs::home_dir() { return home.join(rest); }
            }
            PathBuf::from(s)
        }
    }
}

/// Initialise the global logger. Safe to call twice — the second call is ignored
/// so `main` and tests cannot double-open the file. If `enabled` is false, no
/// file is created and every log call is a no-op.
pub fn init(enabled: bool, file: Option<String>, level_str: &str) {
    let path = resolve_path(file);
    let level = Level::parse(level_str);
    // Fresh logger starts with started=false / enabled=false. `started`
    // is the only "already initialised" signal — never `enabled`.
    let logger = Logger {
        enabled: false,
        level,
        file: None,
        path: path.clone(),
        started: false,
    };
    // If already initialised, keep the first logger (first init wins).
    let mutex = LOGGER.get_or_init(|| Mutex::new(logger));
    let mut guard = match mutex.lock() { Ok(g) => g, Err(p) => p.into_inner() };
    if guard.started {
        // Already initialised — do not reopen.
        return;
    }
    guard.started = true;
    guard.level = level;
    guard.path = path.clone();
    if !enabled {
        guard.enabled = false;
        return;
    }
    // Enabled: try to open (append). Create parent dirs first.
    if let Some(parent) = path.parent() { let _ = std::fs::create_dir_all(parent); }
    match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(mut f) => {
            let ts = timestamp();
            let _ = writeln!(f, "=== Comic Chat RS debug log started {ts} level={} ===", level.label());
            let _ = writeln!(f, "=== Config: debug_log enabled, file={} ===", path.display());
            let _ = f.flush();
            guard.enabled = true;
            guard.level = level;
            guard.file = Some(f);
            guard.path = path;
        }
        Err(e) => {
            // If the file cannot be opened, fall back to stderr once so the user
            // knows logging is broken instead of silently losing diagnostics.
            eprintln!("[comic-chat] debug log: cannot open {}: {e} — logging to stderr", path.display());
            guard.enabled = true;
            guard.level = level;
            guard.file = None;
            guard.path = path;
        }
    }
    let unknown_level = !matches!(level_str.to_ascii_lowercase().trim(), "error" | "warn" | "warning" | "info" | "debug" | "trace");
    // Drop the lock BEFORE calling log() — log() locks the same mutex, so
    // calling it while holding `guard` would deadlock.
    drop(guard);
    if unknown_level {
        // Unknown level string: note the fallback so the user is not confused.
        log(Level::Warn, "log", &format!("unknown debug_log level {level_str:?}, falling back to DEBUG"));
    }
}

#[allow(dead_code)]
pub fn is_enabled() -> bool {
    match LOGGER.get() { Some(m) => match m.lock() { Ok(g) => g.enabled, Err(p) => p.into_inner().enabled }, None => false }
}

#[allow(dead_code)]
pub fn current_level() -> Level {
    match LOGGER.get() { Some(m) => match m.lock() { Ok(g) => g.level, Err(p) => p.into_inner().level }, None => Level::Error }
}

fn timestamp() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = now.as_secs();
    let millis = now.subsec_millis();
    // Human-readable-ish: seconds since epoch + millis. No chrono dep.
    // Format: 1763572800.123
    format!("{secs}.{millis:03}")
}

/// Core log function. `target` is the subsystem: config, irc, avb, dib, pose, comic, app, log, main.
pub fn log(level: Level, target: &str, msg: &str) {
    let Some(m) = LOGGER.get() else { return; };
    let mut guard = match m.lock() { Ok(g) => g, Err(p) => p.into_inner() };
    if !guard.enabled { return; }
    if level > guard.level { return; }
    let line = format!("[{}] {:5} [{}] {}\n", timestamp(), level.label(), target, msg);
    if let Some(f) = guard.file.as_mut() {
        let _ = f.write_all(line.as_bytes());
        let _ = f.flush();
    } else {
        // Fallback: file could not be opened, log to stderr.
        eprint!("{line}");
    }
}

pub fn error(target: &str, msg: impl Into<String>) { let s = msg.into(); log(Level::Error, target, &s); }
pub fn warn(target: &str, msg: impl Into<String>) { let s = msg.into(); log(Level::Warn, target, &s); }
pub fn info(target: &str, msg: impl Into<String>) { let s = msg.into(); log(Level::Info, target, &s); }
pub fn debug(target: &str, msg: impl Into<String>) { let s = msg.into(); log(Level::Debug, target, &s); }
pub fn trace(target: &str, msg: impl Into<String>) { let s = msg.into(); log(Level::Trace, target, &s); }

/// Short preview for chat / wire text: first 120 chars, newlines and control
/// chars escaped, so the log stays one line per event and does not dump full transcripts.
pub fn preview(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars().take(120) {
        match ch {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 32 => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    if text.chars().count() > 120 { out.push_str("..."); }
    out
}

/// Returns true if `text` looks like it carries a secret (password /
/// identify). Used to keep secrets out of the debug log and out of
/// local "Unknown command" messages, which show only the command name.
pub fn is_secret_text(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let trimmed = lower.trim_start();
    if trimmed.starts_with("pass ")
        || trimmed.starts_with("oper ")
        || trimmed.starts_with("authenticate ")
    {
        return true;
    }
    if lower.contains("nickserv") && lower.contains("identify") {
        return true;
    }
    if lower.contains("privmsg nickserv")
        || lower.contains("/msg nickserv")
        || lower.contains("/ns ")
    {
        return true;
    }
    false
}

/// preview() but redacted when the text looks like a secret.
pub fn preview_redacted(text: &str) -> String {
    if is_secret_text(text) {
        "[redacted]".to_string()
    } else {
        preview(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn level_parse() {
        assert_eq!(Level::parse("trace"), Level::Trace);
        assert_eq!(Level::parse("TRACE"), Level::Trace);
        assert_eq!(Level::parse("warn"), Level::Warn);
        assert_eq!(Level::parse("bogus"), Level::Debug);
    }
    #[test]
    fn preview_truncates_and_escapes() {
        let p = preview("hello\nworld");
        assert_eq!(p, "hello\\nworld");
        let long = "x".repeat(200);
        let p2 = preview(&long);
        assert!(p2.ends_with("..."));
        assert_eq!(p2.chars().count(), 123);
    }
}


#[cfg(test)]
mod log_level_edge_tests {
    use super::{current_level, is_enabled};

    #[test]
    fn level_helpers_no_panic() {
        let _ = is_enabled();
        let _ = current_level();
        // Calling twice must not panic and should be consistent
        let a = is_enabled();
        let b = is_enabled();
        assert_eq!(a, b);
    }
}

pub fn parse_log_level(s: &str) -> &'static str { match s.trim().to_ascii_uppercase().as_str() { "ERROR" => "ERROR", "WARN" | "WARNING" => "WARN", "DEBUG" => "DEBUG", "TRACE" => "TRACE", _ => "INFO" } }



pub fn is_error_log(s: &str) -> bool { s.contains("ERROR") || s.contains("error:") }



pub fn is_warn_log(s: &str) -> bool { s.contains("WARN") }



pub fn is_info_log(s: &str) -> bool { s.contains("INFO") }



pub fn is_debug_log(s: &str) -> bool { s.contains("DEBUG") }



pub fn log_level_order(level: &str) -> u8 { match level.trim().to_ascii_uppercase().as_str() { "ERROR" => 0, "WARN" | "WARNING" => 1, "INFO" => 2, "DEBUG" => 3, "TRACE" => 4, _ => 2 } }



pub fn log_level_at_least(level: &str, min: &str) -> bool { log_level_order(level) <= log_level_order(min) }



pub fn format_log_line(level: &str, msg: &str) -> String { format!("[{0}] {1}", level.trim().to_ascii_uppercase(), msg) }



pub fn is_trace_log(s: &str) -> bool { s.contains("TRACE") }



pub fn is_fatal_log(s: &str) -> bool { s.contains("FATAL") || s.contains("PANIC") }



pub fn log_line_level(s: &str) -> &'static str { if s.contains("ERROR") { return "ERROR"; } if s.contains("WARN") { return "WARN"; } if s.contains("DEBUG") { return "DEBUG"; } if s.contains("TRACE") { return "TRACE"; } "INFO" }



pub fn log_line_message(s: &str) -> String { if let Some(pos) = s.find("] ") { return s[pos + 2..].to_string(); } s.to_string() }



pub fn strip_log_level_prefix(s: &str) -> String { if let Some(pos) = s.find("] ") { return s[pos + 2..].to_string(); } s.to_string() }



pub fn is_warning_log(s: &str) -> bool { s.contains("WARN") || s.contains("WARNING") }



pub fn format_log_with_level(level: &str, msg: &str) -> String { format!("[{0}] {1}", level.trim().to_ascii_uppercase(), msg) }



pub fn is_log_line(s: &str) -> bool { s.starts_with('[') && s.contains("] ") }



pub fn log_level_from_line(s: &str) -> &'static str { if s.contains("[ERROR]") { return "ERROR"; } if s.contains("[WARN]") { return "WARN"; } if s.contains("[DEBUG]") { return "DEBUG"; } if s.contains("[TRACE]") { return "TRACE"; } "INFO" }



pub fn log_has_error(s: &str) -> bool { s.contains("ERROR") }



pub fn log_has_warning(s: &str) -> bool { s.contains("WARN") }



pub fn format_log_error(msg: &str) -> String { format!("[ERROR] {}", msg) }



pub fn log_is_empty(s: &str) -> bool { s.trim().is_empty() }



pub fn log_is_long(s: &str) -> bool { s.chars().count() > 500 }



pub fn log_truncate(s: &str, max: usize) -> String { if s.chars().count() <= max { return s.to_string(); } if max <= 3 { return s.chars().take(max).collect(); } let mut out: String = s.chars().take(max - 3).collect(); out.push_str("..."); out }



pub fn log_wrap_len(s: &str, width: usize) -> usize { if width == 0 { return 1; } let len = s.chars().count(); if len == 0 { return 1; } (len + width - 1) / width }



pub fn log_word_count(s: &str) -> usize { s.split_whitespace().count() }



pub fn log_has_info(s: &str) -> bool { s.contains("INFO") }



pub fn log_has_debug(s: &str) -> bool { s.contains("DEBUG") }



pub fn log_has_trace(s: &str) -> bool { s.contains("TRACE") }



pub fn log_format_info(msg: &str) -> String { format!("[INFO] {}", msg) }



pub fn log_format_warn(msg: &str) -> String { format!("[WARN] {}", msg) }



pub fn log_format_debug(msg: &str) -> String { format!("[DEBUG] {}", msg) }



pub fn log_format_trace(msg: &str) -> String { format!("[TRACE] {}", msg) }



pub fn log_is_error(s: &str) -> bool { s.contains("ERROR") }



pub fn log_is_warn(s: &str) -> bool { s.contains("WARN") }



pub fn log_is_info(s: &str) -> bool { s.contains("INFO") }



pub fn log_trim(s: &str) -> String { s.trim().to_string() }



pub fn log_upper(s: &str) -> String { s.to_ascii_uppercase() }



pub fn log_lower(s: &str) -> String { s.to_ascii_lowercase() }



pub fn log_reverse(s: &str) -> String { s.chars().rev().collect() }



pub fn log_contains(s: &str, sub: &str) -> bool { s.contains(sub) }



pub fn log_is_debug(s: &str) -> bool { s.contains("DEBUG") }



pub fn log_is_trace(s: &str) -> bool { s.contains("TRACE") }



pub fn log_len(s: &str) -> usize { s.chars().count() }



pub fn log_is_success(s: &str) -> bool { s.contains("SUCCESS") || s.contains("OK") }



pub fn log_is_failure(s: &str) -> bool { s.contains("FAIL") }



pub fn log_is_warning(s: &str) -> bool { s.contains("WARNING") || s.contains("WARN") }



pub fn log_is_critical(s: &str) -> bool { s.contains("CRITICAL") || s.contains("FATAL") }



pub fn log_is_notice(s: &str) -> bool { s.contains("NOTICE") }



pub fn log_starts_with(s: &str, prefix: &str) -> bool { s.starts_with(prefix) }



pub fn log_ends_with(s: &str, suffix: &str) -> bool { s.ends_with(suffix) }



pub fn log_split(s: &str) -> Vec<String> { s.split_whitespace().map(|w| w.to_string()).collect() }



pub fn log_first_word(s: &str) -> String { s.split_whitespace().next().unwrap_or("").to_string() }



pub fn log_last_word(s: &str) -> String { s.split_whitespace().last().unwrap_or("").to_string() }



pub fn log_trim_start(s: &str) -> String { s.trim_start().to_string() }



pub fn log_trim_end(s: &str) -> String { s.trim_end().to_string() }



pub fn log_pad(s: &str, width: usize) -> String { if s.chars().count() >= width { return s.to_string(); } let mut out = s.to_string(); while out.chars().count() < width { out.push(' '); } out }



pub fn log_pad_start(s: &str, width: usize) -> String { let len = s.chars().count(); if len >= width { return s.to_string(); } let mut out = String::new(); for _ in 0..(width - len) { out.push(' '); } out.push_str(s); out }



pub fn log_repeat(s: &str, n: usize) -> String { s.repeat(n) }



pub fn log_is_blank(s: &str) -> bool { s.trim().is_empty() }



pub fn log_char_count(s: &str) -> usize { s.chars().count() }



pub fn log_contains_ignore_case(s: &str, needle: &str) -> bool { s.to_ascii_lowercase().contains(&needle.to_ascii_lowercase()) }



pub fn log_starts_with_ignore_case(s: &str, prefix: &str) -> bool { s.to_ascii_lowercase().starts_with(&prefix.to_ascii_lowercase()) }



pub fn log_ends_with_ignore_case(s: &str, suffix: &str) -> bool { s.to_ascii_lowercase().ends_with(&suffix.to_ascii_lowercase()) }



pub fn log_equals_ignore_case(a: &str, b: &str) -> bool { a.eq_ignore_ascii_case(b) }



pub fn log_to_upper(s: &str) -> String { s.to_ascii_uppercase() }



pub fn log_to_lower(s: &str) -> String { s.to_ascii_lowercase() }



pub fn log_to_title(s: &str) -> String { s.split_whitespace().map(|w| { let mut cs = w.chars(); match cs.next() { Some(f) => f.to_ascii_uppercase().to_string() + &cs.as_str().to_ascii_lowercase(), None => String::new() } }).collect::<Vec<String>>().join(" ") }



pub fn log_capitalize(s: &str) -> String { let mut cs = s.chars(); match cs.next() { Some(f) => f.to_ascii_uppercase().to_string() + cs.as_str(), None => String::new() } }



pub fn log_is_error_line(s: &str) -> bool { s.contains("ERROR") || s.contains("error") }



pub fn log_is_warn_line(s: &str) -> bool { s.contains("WARN") || s.contains("warn") }



pub fn log_is_info_line(s: &str) -> bool { s.contains("INFO") || s.contains("info") }



pub fn log_is_debug_line(s: &str) -> bool { s.contains("DEBUG") || s.contains("debug") }



pub fn log_is_trace_line(s: &str) -> bool { s.contains("TRACE") || s.contains("trace") }



pub fn log_split_lines(s: &str) -> Vec<String> { s.lines().map(|l| l.to_string()).collect() }



pub fn log_split_words(s: &str) -> Vec<String> { s.split_whitespace().map(|w| w.to_string()).collect() }



pub fn log_join_lines(parts: &[&str]) -> String { parts.join("\n") }



pub fn log_join_words(parts: &[&str]) -> String { parts.join(" ") }



pub fn log_first_line(s: &str) -> String { s.lines().next().unwrap_or("").to_string() }



pub fn log_last_line(s: &str) -> String { s.lines().last().unwrap_or("").to_string() }



pub fn log_line_count(s: &str) -> usize { s.lines().count() }



pub fn log_contains_error(s: &str) -> bool { s.contains("ERROR") || s.contains("error") }



pub fn log_contains_warn(s: &str) -> bool { s.contains("WARN") || s.contains("warn") }



pub fn log_contains_info(s: &str) -> bool { s.contains("INFO") || s.contains("info") }



pub fn log_contains_debug(s: &str) -> bool { s.contains("DEBUG") || s.contains("debug") }



pub fn log_contains_trace(s: &str) -> bool { s.contains("TRACE") || s.contains("trace") }



pub fn log_is_space_only(s: &str) -> bool { !s.is_empty() && s.chars().all(|c| c.is_whitespace()) }



pub fn log_is_upper(s: &str) -> bool { !s.is_empty() && s.chars().any(|c| c.is_ascii_alphabetic()) && s.chars().filter(|c| c.is_ascii_alphabetic()).all(|c| c.is_ascii_uppercase()) }



pub fn log_is_lower(s: &str) -> bool { !s.is_empty() && s.chars().any(|c| c.is_ascii_alphabetic()) && s.chars().filter(|c| c.is_ascii_alphabetic()).all(|c| c.is_ascii_lowercase()) }



pub fn log_is_numeric(s: &str) -> bool { !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()) }



pub fn log_count_lines(s: &str) -> usize { s.lines().count() }



pub fn log_count_words(s: &str) -> usize { s.split_whitespace().count() }



pub fn log_count_chars(s: &str) -> usize { s.chars().count() }



pub fn log_has_word(s: &str, word: &str) -> bool { s.split_whitespace().any(|w| w == word) }



pub fn log_pad_left(s: &str, width: usize) -> String { let len = s.chars().count(); if len >= width { return s.to_string(); } " ".repeat(width - len) + s }



pub fn log_pad_right(s: &str, width: usize) -> String { let len = s.chars().count(); if len >= width { return s.to_string(); } s.to_string() + &" ".repeat(width - len) }



pub fn log_ellipsis(s: &str, max: usize) -> String { if s.chars().count() <= max { return s.to_string(); } if max < 3 { return s.chars().take(max).collect(); } let mut out: String = s.chars().take(max - 3).collect(); out.push_str("..."); out }



pub fn log_indent(s: &str, prefix: &str) -> String { s.lines().map(|l| format!("{}{}", prefix, l)).collect::<Vec<String>>().join("\n") }



pub fn log_count_vowels(s: &str) -> usize { s.chars().filter(|c| matches!(c.to_ascii_lowercase(), 'a' | 'e' | 'i' | 'o' | 'u')).count() }



pub fn log_count_consonants(s: &str) -> usize { s.chars().filter(|c| c.is_ascii_alphabetic() && !matches!(c.to_ascii_lowercase(), 'a' | 'e' | 'i' | 'o' | 'u')).count() }



pub fn log_remove_vowels(s: &str) -> String { s.chars().filter(|c| !matches!(c.to_ascii_lowercase(), 'a' | 'e' | 'i' | 'o' | 'u')).collect() }



pub fn log_is_palindrome(s: &str) -> bool { let clean: String = s.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect(); !clean.is_empty() && clean == clean.chars().rev().collect::<String>() }



pub fn log_has_vowel(s: &str) -> bool { s.chars().any(|c| matches!(c.to_ascii_lowercase(), 'a' | 'e' | 'i' | 'o' | 'u')) }



pub fn log_count_digits(s: &str) -> usize { s.chars().filter(|c| c.is_ascii_digit()).count() }



pub fn log_count_punct(s: &str) -> usize { s.chars().filter(|c| c.is_ascii_punctuation()).count() }



pub fn log_remove_digits(s: &str) -> String { s.chars().filter(|c| !c.is_ascii_digit()).collect() }



pub fn log_has_digit(s: &str) -> bool { s.chars().any(|c| c.is_ascii_digit()) }



pub fn log_has_punct(s: &str) -> bool { s.chars().any(|c| c.is_ascii_punctuation()) }



pub fn log_has_upper(s: &str) -> bool { s.chars().any(|c| c.is_ascii_uppercase()) }



pub fn log_has_lower(s: &str) -> bool { s.chars().any(|c| c.is_ascii_lowercase()) }



pub fn log_capitalize_first(s: &str) -> String { let mut chars = s.chars(); match chars.next() { None => String::new(), Some(f) => format!("{}{}", f.to_ascii_uppercase(), chars.collect::<String>()) } }



pub fn log_remove_spaces(s: &str) -> String { s.chars().filter(|c| !c.is_whitespace()).collect() }



pub fn log_collapse_spaces(s: &str) -> String { s.split_whitespace().collect::<Vec<&str>>().join(" ") }



pub fn log_replace(s: &str, from: &str, to: &str) -> String { s.replace(from, to) }



pub fn log_len_chars(s: &str) -> usize { s.chars().count() }



pub fn log_sort_chars(s: &str) -> String { let mut v: Vec<char> = s.chars().collect(); v.sort(); v.into_iter().collect() }



pub fn log_unique_chars(s: &str) -> String { let mut out: Vec<char> = Vec::new(); for c in s.chars() { if !out.contains(&c) { out.push(c); } } out.into_iter().collect() }



pub fn log_swap_case(s: &str) -> String { s.chars().map(|c| if c.is_ascii_uppercase() { c.to_ascii_lowercase() } else if c.is_ascii_lowercase() { c.to_ascii_uppercase() } else { c }).collect() }



pub fn log_title_case(s: &str) -> String { s.split_whitespace().map(|w| { let mut ch = w.chars(); match ch.next() { None => String::new(), Some(f) => format!("{}{}", f.to_ascii_uppercase(), ch.collect::<String>()) } }).collect::<Vec<_>>().join(" ") }


