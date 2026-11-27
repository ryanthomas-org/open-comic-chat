//! IRC / IRCX wire protocol for Microsoft Chat.
//! Codifies ircproto.h, ircsock.cpp, protsupp.cpp, chatprot, and the
//! protocol handling in comic-chat-rs 0.3.17.
//!
//! Wire format (authoritative):
//! * Messages are `:<prefix> <command> <params> :<trailing>` + CRLF.
//!   Lines are split on CRLF; a bare LF is NOT a terminator in the
//!   original (it was treated as part of the line). Max line length:
//!   512 bytes INCLUDING CRLF (RFC 1459). Longer lines must be split
//!   by the sender, never truncated silently by the receiver.
//! * Commands are case-insensitive on the wire; the parser uppercases
//!   them. Parameters are case-sensitive except nicks/channels (see below).
//! * CTCP: `\x01ACTION ...\x01` inside PRIVMSG = /me action. Other CTCP
//!   (VERSION, PING, etc.) are `\x01CMD [args]\x01`. The original
//!   replied to VERSION with `Microsoft Comic Chat <version>` — see
//!   `ctcp_version_reply`.
//! * IRCX (Microsoft extensions): `JOIN` with `*` / mode extensions,
//!   `IRCX` command, `LISTX`, `PROPERTY`, and the Comics-mode negotiation
//!   (`MODE <nick> +x` / `-x` in some servers; see SPEC Part A §7 — the
//!   exact negotiation is server-dependent and MUST be probed, not assumed).
//! * Nick/channel comparison: IRC case folding (RFC 1459): `{}|^` map to
//!   `[]\\~` respectively, and comparison is case-insensitive after that
//!   mapping. `irc_eq` implements it. Plain `eq_ignore_ascii_case` is WRONG
//!   for nicks containing `{}` etc.
//! * Flood control: the original throttled outgoing PRIVMSG to at most
//!   allow burst of 3, then 1 per second. Do NOT send faster — servers
//!   kick for flooding.
//! * Slash commands (client-side, never sent raw): /join, /part, /nick,
//!   /msg, /me, /query, /whois, /topic, /kick, /ban, /mode, /invite,
//!   /away, /quit, /appears (Appears as), /background (Background as).
//!   See `parse_slash`.

/// One parsed IRC line.
#[derive(Debug, Clone, PartialEq)]
pub struct IrcMessage { pub prefix: Option<String>, pub command: String, pub params: Vec<String>, pub trailing: Option<String> }

/// Parse one IRC line (without CRLF). Returns None on empty / malformed.
/// The parser does NOT strip a trailing `\r`; callers must strip CRLF first.
pub fn parse_line(line: &str) -> Option<IrcMessage> {
    let mut rest = line;
    if rest.is_empty() { return None; }
    let mut prefix = None;
    if let Some(stripped) = rest.strip_prefix(':') {
        let end = stripped.find(' ')?;
        prefix = Some(stripped[..end].to_string());
        rest = stripped[end+1..].trim_start();
    }
    if rest.is_empty() { return None; }
    let mut parts = rest.splitn(2, ' ');
    let command = parts.next()?.to_uppercase();
    let mut params = Vec::new();
    let mut trailing = None;
    if let Some(after) = parts.next() {
        let mut cur = after;
        loop {
            cur = cur.trim_start();
            if cur.is_empty() { break; }
            if let Some(t) = cur.strip_prefix(':') { trailing = Some(t.to_string()); break; }
            if let Some(sp) = cur.find(' ') { params.push(cur[..sp].to_string()); cur = &cur[sp+1..]; } else { params.push(cur.to_string()); break; }
        }
    }
    Some(IrcMessage { prefix, command, params, trailing })
}

/// Format a PRIVMSG. `target` = channel or nick. Text must already be
/// pose/OEC-encoded if Comics mode. Caller must split at 512 bytes.
pub fn privmsg(target: &str, text: &str) -> String { format!("PRIVMSG {} :{}", target, text) }
/// Format a CTCP ACTION (/me).
pub fn action_msg(target: &str, text: &str) -> String { format!("PRIVMSG {} :\x01ACTION {}\x01", target, text) }
/// CTCP VERSION reply the original sent.
pub fn ctcp_version_reply(version: &str) -> String { format!("Microsoft Comic Chat {}", version) }
/// Parse CTCP from a PRIVMSG trailing param. Returns (cmd, args) if CTCP.
pub fn parse_ctcp(trailing: &str) -> Option<(String, String)> {
    let t = trailing.strip_prefix('\x01')?.strip_suffix('\x01')?;
    let mut it = t.splitn(2, ' ');
    Some((it.next()?.to_string(), it.next().unwrap_or("").to_string()))
}

/// IRC case folding (RFC 1459): lowercase + {}|^ -> []\\~.
pub fn irc_lower(s: &str) -> String {
    s.chars().map(|c| match c { '{' => '[', '}' => ']', '|' => '\\', '^' => '~', _ => c.to_ascii_lowercase() }).collect()
}
pub fn irc_eq(a: &str, b: &str) -> bool { irc_lower(a) == irc_lower(b) }

/// Nick validity: 1..=30 chars, starts with letter or one of `[]\\`^{|}_-`,
/// rest letters/digits/`-`/`_`/`[]\\`^{|}`. Servers vary; this is the
/// strict original rule — do not accept spaces or `:`/`#`/`!`/`@`.
pub fn is_valid_nick(nick: &str) -> bool {
    if nick.is_empty() || nick.len() > 30 { return false; }
    let mut chars = nick.chars();
    let first = chars.next().unwrap();
    if !(first.is_ascii_alphabetic() || "[]\\`^{|}_-".contains(first)) { return false; }
    chars.all(|c| c.is_ascii_alphanumeric() || "-_[]\\`^{|}".contains(c))
}
/// Channel validity: starts with `#` or `&`, 2..=200 chars, no space/`,`/`:`, no NUL.
pub fn is_valid_channel(ch: &str) -> bool {
    if ch.len() < 2 || ch.len() > 200 { return false; }
    if !ch.starts_with('#') && !ch.starts_with('&') { return false; }
    !ch.chars().any(|c| c == ' ' || c == ',' || c == ':' || c == '\0')
}

/// Split a long PRIVMSG body into 512-byte-safe chunks.
/// Each chunk is `PRIVMSG <target> :<text>` <= 512 bytes incl. CRLF.
/// Splits on word boundaries where possible; never splits a UTF-8 char.
/// Pose code (leading `(#...)`) is kept on the FIRST chunk only.
pub fn split_privmsg(target: &str, text: &str) -> Vec<String> {
    let overhead = format!("PRIVMSG {} :", target).len() + 2; // +CRLF
    let max_body = 512usize.saturating_sub(overhead);
    if text.len() <= max_body { return vec![privmsg(target, text)]; }
    let mut out = Vec::new();
    let mut rest = text;
    let mut first = true;
    while !rest.is_empty() {
        let mut take = max_body.min(rest.len());
        // Back off to a word boundary if not the last chunk.
        if take < rest.len() {
            if let Some(sp) = rest[..take].rfind(' ') { if sp > 0 { take = sp; } }
            // Back off to a UTF-8 boundary.
            while take > 0 && !rest.is_char_boundary(take) { take -= 1; }
        }
        let (chunk, next) = rest.split_at(take);
        let _ = first; first = false;
        out.push(privmsg(target, chunk.trim_end()));
        rest = next.trim_start();
    }
    out
}

/// Client-side slash command.
#[derive(Debug, Clone, PartialEq)]
pub enum SlashCommand {
    Join(String), Part(Option<String>), Nick(String), Msg(String, String),
    Me(String), Query(String), Whois(String), Topic(Option<String>),
    Kick(String, Option<String>), Ban(String), Mode(String, String),
    Invite(String, String), Away(Option<String>), Quit(Option<String>),
    Appears(String), Background(String), Unknown(String, String),
}
/// Parse `/cmd args`. Returns None if the line does not start with `/`.
/// `//` = literal leading slash (send as text).
pub fn parse_slash(line: &str) -> Option<SlashCommand> {
    if !line.starts_with('/') { return None; }
    if let Some(lit) = line.strip_prefix("//") { return Some(SlashCommand::Unknown("say".into(), lit.to_string())); }
    let body = &line[1..];
    let mut it = body.splitn(2, ' ');
    let cmd = it.next().unwrap_or("").to_ascii_lowercase();
    let args = it.next().unwrap_or("").trim().to_string();
    let two = |a: &str| { let mut s = a.splitn(2, ' '); (s.next().unwrap_or("").to_string(), s.next().unwrap_or("").trim().to_string()) };
    Some(match cmd.as_str() {
        "join" | "j" => SlashCommand::Join(args),
        "part" | "leave" => SlashCommand::Part(if args.is_empty() { None } else { Some(args) }),
        "nick" => SlashCommand::Nick(args),
        "msg" | "tell" => { let (t, m) = two(&args); SlashCommand::Msg(t, m) }
        "me" => SlashCommand::Me(args),
        "query" => SlashCommand::Query(args),
        "whois" => SlashCommand::Whois(args),
        "topic" => SlashCommand::Topic(if args.is_empty() { None } else { Some(args) }),
        "kick" => { let (t, m) = two(&args); SlashCommand::Kick(t, if m.is_empty() { None } else { Some(m) }) }
        "ban" => SlashCommand::Ban(args),
        "mode" => { let (t, m) = two(&args); SlashCommand::Mode(t, m) }
        "invite" => { let (t, m) = two(&args); SlashCommand::Invite(t, m) }
        "away" => SlashCommand::Away(if args.is_empty() { None } else { Some(args) }),
        "quit" => SlashCommand::Quit(if args.is_empty() { None } else { Some(args) }),
        "appears" => SlashCommand::Appears(args),
        "background" => SlashCommand::Background(args),
        _ => SlashCommand::Unknown(cmd, args),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn parse_privmsg() { let m = parse_line(":nick!u@h PRIVMSG #chan :hello world").unwrap(); assert_eq!(m.command, "PRIVMSG"); assert_eq!(m.params, vec!["#chan"]); assert_eq!(m.trailing, Some("hello world".into())); }
    #[test] fn parse_no_prefix() { let m = parse_line("PING :server").unwrap(); assert_eq!(m.command, "PING"); assert_eq!(m.trailing, Some("server".into())); }
    #[test] fn irc_fold() { assert!(irc_eq("A{B", "a[b")); assert!(irc_eq("A|B", "a\\b")); assert!(!irc_eq("abc", "abd")); }
    #[test] fn ctcp_action() { let (c, a) = parse_ctcp("\x01ACTION waves\x01").unwrap(); assert_eq!(c, "ACTION"); assert_eq!(a, "waves"); }
    #[test] fn slash_me() { assert_eq!(parse_slash("/me waves"), Some(SlashCommand::Me("waves".into()))); }
    #[test] fn slash_unknown() { assert!(matches!(parse_slash("/foo bar"), Some(SlashCommand::Unknown(_, _)))); }
    #[test] fn split_long() { let long = "word ".repeat(200); let chunks = split_privmsg("#chan", &long); assert!(chunks.len() > 1); for c in &chunks { assert!(c.len() <= 512); } }
    #[test] fn nick_valid() { assert!(is_valid_nick("Alice_1")); assert!(!is_valid_nick("1Alice")); assert!(!is_valid_nick("a b")); }
}
