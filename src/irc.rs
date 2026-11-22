// IRC + Comic Chat wire protocol.
// See pose.rs for the full spec. Summary (v2.5 behaviour, 0.3.17):
// - CTCP VERSION: direct PRIVMSG \x01VERSION\x01 -> NOTICE \x01VERSION Open Comic Chat (Version 0.3.17)\x01 (see crate::version_reply).
// - Say:    PRIVMSG #ch :(#GxxxExxxM1) text  (pose_style="old", default) — plain text when pose_style="none"
// - Think:  PRIVMSG #ch :*thinks* text
// - Whisper: PRIVMSG <nick> :text
// - Action: PRIVMSG #ch :\x01ACTION text\x01
// - Character: PRIVMSG #ch :# Appears as Name.http://.../Name.AVB  (sent on JOIN + whenever a new user joins, so they learn our character)
// - Background: PRIVMSG #ch :# Background as Name.http://.../Name.BGB (share, optional)
// pose_style="old" (default): Say prepends OldAngle (#GxxxExxxM1) computed from wheel+text. pose_style="none": plain text, no code.
// The invented (#G2:8E0:8M1) colon form is never sent. CAST:... is never sent.
// Incoming Appears/Background lines are NOT balloons — they update the per-nick
// character/background map and are shown as a system line in the text log.

use crate::config::ServerConfig;
use crate::pose::{parse_appears_as, parse_background_as};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{lookup_host, TcpStream};
use tokio::sync::mpsc;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_rustls::TlsConnector;
use rustls::{ClientConfig, RootCertStore};

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum IrcEvent {
    Connected { server: String },
    Disconnected { server: String, reason: String },
    Status { server: String, text: String },
    Message { server: String, channel: String, nick: String, text: String, is_me: bool, kind: crate::comic::MessageKind },
    AppearsAs { server: String, channel: String, nick: String, character: String, url: Option<String> },
    BackgroundAs { server: String, channel: String, nick: String, background: String, url: Option<String> },
    Joined { server: String, channel: String },
    Members { server: String, channel: String, members: Vec<String>, ops: Vec<String> },
    /// Someone set / cleared away. `message` = Some(msg) for away,
    /// None for back. `channel` empty means the notice arrived privately
    /// (CTCP AWAY to us on join, or numeric 301) — the app applies it to
    /// every channel that nick shares with us.
    Away { server: String, channel: String, nick: String, message: Option<String> },
    /// CTCP VERSION reply, arrived as a private NOTICE. No channel on
    /// the wire — the app shows it in a channel shared with that nick.
    VersionReply { server: String, nick: String, version: String },
    /// Someone changed nick. The app moves their members / ops / away /
    /// character entries from old_nick to new_nick.
    MemberRenamed { server: String, old_nick: String, new_nick: String },
    /// The server assigned US a nick (truncation / change) — 001 welcome
    /// or our own JOIN echo with a different nick. The app adopts it as
    /// our ONE identity on that server so we never appear twice.
    MyNick { server: String, nick: String },
    /// Channel operator change (MODE +o / -o). `is_op` = true for +o.
    OpChanged { server: String, channel: String, nick: String, is_op: bool },
    MemberJoined { server: String, channel: String, nick: String },
    MemberLeft { server: String, channel: String, nick: String },
    Parted { server: String, channel: String },
    Action { server: String, channel: String, nick: String, text: String, is_me: bool },
}

#[derive(Debug, Clone, PartialEq)]
pub enum IrcCommand {
    SendMessage { channel: String, text: String, pose_code: String },
    Think { channel: String, text: String, pose_code: String },
    Whisper { target: String, text: String, pose_code: String },
    Action { channel: String, text: String, pose_code: String },
    AppearsAs { channel: String, line: String },
    BackgroundAs { channel: String, line: String },
    Join { channel: String },
    Part { channel: String },
    Quit { reason: String },
    /// Away / back. Bundles Microsoft's nonstandard CTCP AWAY (coffee
    /// cup) with standard IRC AWAY: the server gets `AWAY :msg` (or bare
    /// `AWAY` to clear), and every joined channel gets
    /// `PRIVMSG <chan> :\x01AWAY <msg>\x01` (or bare `\x01AWAY\x01`).
    /// `Some(msg)` = away with that message; `None` = back.
    SetAway { message: Option<String> },
    /// Ask a nick for their client version: PRIVMSG <nick> :\x01VERSION\x01.
    /// The reply comes back as NOTICE \x01VERSION ...\x01 -> VersionReply.
    VersionQuery { target: String },
}

#[allow(dead_code)]
pub struct ConnectionHandle { pub tx: mpsc::UnboundedSender<IrcCommand> }

pub trait AsyncReadWrite: AsyncRead + AsyncWrite {}
impl<T: AsyncRead + AsyncWrite + ?Sized> AsyncReadWrite for T {}
type BoxStream = Box<dyn AsyncReadWrite + Unpin + Send>;

async fn connect_tcp(host: &str, port: u16) -> std::io::Result<TcpStream> {
    crate::log::debug("irc", format!("DNS lookup: {host}:{port} — resolving host to addresses"));
    let addrs: Vec<std::net::SocketAddr> = match lookup_host((host, port)).await {
        Ok(it) => it.collect(),
        Err(e) => { crate::log::error("irc", format!("DNS lookup failed for {host}:{port}: {e} — check host spelling, DNS, and network. No TCP attempt was made.")); return Err(e); }
    };
    crate::log::debug("irc", format!("DNS lookup succeeded for {host}:{port}: {} address(es) {:?}", addrs.len(), addrs));
    if addrs.is_empty() { crate::log::error("irc", format!("DNS: no addresses for {host} — lookup returned empty, cannot connect")); return Err(std::io::Error::new(std::io::ErrorKind::NotFound, format!("DNS: no addresses for {host}"))); }
    let mut last_err = None;
    for addr in addrs {
        crate::log::debug("irc", format!("TCP connect attempt to {addr} for {host}:{port}"));
        match TcpStream::connect(addr).await {
            Ok(s) => { let _ = s.set_nodelay(true); crate::log::info("irc", format!("TCP connected to {addr} for {host}:{port}")); return Ok(s); }
            Err(e) => { crate::log::warn("irc", format!("TCP connect to {addr} failed: {e} — trying next address if any")); last_err = Some(e); }
        }
    }
    let e = last_err.unwrap_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "connect failed"));
    crate::log::error("irc", format!("TCP connect failed for {host}:{port}: {e} — all addresses tried, check firewall / port / server is listening"));
    Err(e)
}
fn tls_config() -> Arc<ClientConfig> {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    Arc::new(ClientConfig::builder().with_root_certificates(roots).with_no_client_auth())
}

pub async fn run_server(cfg: ServerConfig, me_nick: String, mut cmd_rx: mpsc::UnboundedReceiver<IrcCommand>, event_tx: mpsc::UnboundedSender<IrcEvent>, ctcp_version: Option<String>) {
    let server_name = cfg.name.clone();
    crate::log::info("irc", format!("run_server starting: server={} host={} port={} tls={} nick_fallback={} channels={:?} sasl_user={:?} autoconnect={}",
        server_name, cfg.host, cfg.effective_port(), cfg.tls, me_nick, cfg.channels, cfg.sasl.as_ref().map(|s| s.user.clone()), cfg.autoconnect));
    let send_ev = |ev: IrcEvent| { let _ = event_tx.send(ev); };
    let tcp = match connect_tcp(&cfg.host, cfg.effective_port()).await {
        Ok(t) => t,
        Err(e) => { crate::log::error("irc", format!("run_server {}: TCP connect failed: {e} — disconnecting, no IRC handshake attempted", server_name)); send_ev(IrcEvent::Disconnected { server: server_name, reason: format!("connect: {e}") }); return; }
    };
    let stream: BoxStream = if cfg.tls {
        crate::log::debug("irc", format!("TLS handshake starting for {} host={} — using webpki roots, SNI={}", server_name, cfg.host, cfg.host));
        let connector = TlsConnector::from(tls_config());
        let dns_name = match cfg.host.clone().try_into() { Ok(n) => n, Err(_) => { crate::log::error("irc", format!("TLS: invalid hostname {:?} for {} — hostname must be a valid DNS name for SNI, check config", cfg.host, server_name)); send_ev(IrcEvent::Disconnected { server: server_name, reason: "invalid TLS hostname".into() }); return; } };
        match connector.connect(dns_name, tcp).await { Ok(tls) => { crate::log::info("irc", format!("TLS handshake succeeded for {}", server_name)); Box::new(tls) }, Err(e) => { crate::log::error("irc", format!("TLS handshake failed for {}: {e} — check server TLS, port (6697 for TLS, 6667 for plain), and system roots", server_name)); send_ev(IrcEvent::Disconnected { server: server_name, reason: format!("TLS: {e}") }); return; } }
    } else { crate::log::debug("irc", format!("Plain TCP (no TLS) for {} — wire text including SASL if any will be unencrypted", server_name)); Box::new(tcp) };
    let (reader, mut writer) = tokio::io::split(stream);
    let mut lines = BufReader::new(reader).lines();
    fn truncate_nick(s: &str) -> String { s.chars().take(9).collect() }
    let mut nick = truncate_nick(&cfg.effective_nick(&me_nick));
    let mut my_nick = nick.clone();
    let user = truncate_nick(&me_nick);
    let real = "Comic Chat RS";
    let send_line = async |w: &mut tokio::io::WriteHalf<BoxStream>, s: &str| { let owned = sanitize_outgoing_line(s); let s = owned.as_str(); crate::log::info("irc", format!("OUT {}", crate::log::preview_redacted(s))); let _ = w.write_all(format!("{s}\r\n").as_bytes()).await; let _ = w.flush().await; };
    if cfg.sasl.is_some() { crate::log::debug("irc", format!("SASL configured for {} user={:?} — sending CAP LS 302 first, password is never logged", server_name, cfg.sasl.as_ref().map(|s| s.user.clone()))); send_line(&mut writer, "CAP LS 302").await; }
    crate::log::debug("irc", format!("IRC handshake: NICK {nick} / USER {user} for {} — waiting for 001 welcome", server_name));
    send_line(&mut writer, &format!("NICK {nick}")).await;
    send_line(&mut writer, &format!("USER {user} 0 * :{real}")).await;
    let mut sasl_started = false;
    let mut welcomed = false;
    let mut pending_joins: Vec<String> = cfg.channels.clone();
    // Channels we are currently in (from our own JOIN echoes) — CTCP
    // AWAY goes to all of these. Standard AWAY goes to the server.
    let mut joined_channels: Vec<String> = Vec::new();
    // Channels we sent JOIN for but have not yet seen our own JOIN echo
    // for. Used to recognise our own echo when the server truncated /
    // changed our nick (the echo nick then differs from my_nick).
    let mut join_sent: Vec<String> = Vec::new();
    // Our own away message, if we are away. Used to tell joiners
    // (private CTCP AWAY) and to re-announce on our own JOIN.
    let mut self_away: Option<String> = None;
    // 0.3.17: Connected is sent only on 001, not on TCP/TLS connect.

    // Helper: prepend pose code to outgoing text (if any). Returns wire text.
    // Leading form: "(#GxxxExxxM1) text" — matches parse_pose_code leading handling + OEC leading form.
    let with_pose = |text: &str, pose: &str| -> String {
        let p = pose.trim();
        if p.is_empty() { text.to_string() } else { format!("{p} {text}") }
    };

    loop {
        tokio::select! {
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(IrcCommand::SendMessage { channel, text, pose_code }) => {
                        crate::log::debug("irc", format!("OUT SendMessage to {channel} pose={pose_code:?} preview={:?}", crate::log::preview_redacted(&text)));
                        // Step 1 (0.3.17, review 3.9) defense in depth: app.rs already
                        // blocks unknown slash commands, but SendMessage must never
                        // send one as PRIVMSG even if a future caller bypasses app.rs.
                        // Only "/me <text>" and "/join <channel>" are understood here.
                        // Anything else starting with '/' is dropped without logging
                        // its args (they may contain a password) and without emitting
                        // a Message event.
                        let slash_blocked = text.starts_with('/') && {
                            let cmd_name = text.split_whitespace().next().unwrap_or("/");
                            let has_arg = text.splitn(2, ' ').nth(1).map(|a| !a.trim().is_empty()).unwrap_or(false);
                            !((cmd_name.eq_ignore_ascii_case("/me") || cmd_name.eq_ignore_ascii_case("/join")) && has_arg)
                        };
                        if slash_blocked {
                            crate::log::warn("irc", format!("SendMessage: blocked unknown slash command {:?} — not sent", text.split_whitespace().next().unwrap_or("/")));
                        } else if text.eq_ignore_ascii_case("/me") { /* bare /me — ignore */ } else if let Some(action) = me_arg(&text) {
                            let wire = with_pose(action, &pose_code);
                            send_line(&mut writer, &format!("PRIVMSG {channel} :\x01ACTION {wire}\x01")).await;
                            send_ev(IrcEvent::Action { server: server_name.clone(), channel, nick: nick.clone(), text: wire, is_me: true });
                        } else if let Some(join) = join_arg(&text) {
                            // join_arg is case-insensitive; normalize handles empty/bare -> ignore.
                            if let Some(ch) = normalize_join_channel(join) {
                                join_sent.push(ch.clone());
                                send_line(&mut writer, &format!("JOIN {ch}")).await;
                            }
                        } else {
                            let wire = with_pose(&text, &pose_code);
                            send_line(&mut writer, &format!("PRIVMSG {channel} :{wire}")).await;
                            send_ev(IrcEvent::Message { server: server_name.clone(), channel, nick: nick.clone(), text: wire, is_me: true, kind: crate::comic::MessageKind::Say });
                        }
                    }
                    Some(IrcCommand::Think { channel, text, pose_code }) => {
                        crate::log::debug("irc", format!("OUT Think to {channel} pose={pose_code:?} preview={:?}", crate::log::preview(&text)));
                        let wire = with_pose(&format!("*thinks* {text}"), &pose_code);
                        send_line(&mut writer, &format!("PRIVMSG {channel} :{wire}")).await;
                        send_ev(IrcEvent::Message { server: server_name.clone(), channel, nick: nick.clone(), text: wire, is_me: true, kind: crate::comic::MessageKind::Think });
                    }
                    Some(IrcCommand::Whisper { target, text, pose_code }) => {
                        crate::log::debug("irc", format!("OUT Whisper to {target} pose={pose_code:?} preview={:?} — private, not to channel", crate::log::preview(&text)));
                        let wire = with_pose(&text, &pose_code);
                        send_line(&mut writer, &format!("PRIVMSG {target} :{wire}")).await;
                        send_ev(IrcEvent::Message { server: server_name.clone(), channel: target.clone(), nick: nick.clone(), text: format!("(whisper to {target}) {wire}"), is_me: true, kind: crate::comic::MessageKind::Whisper });
                    }
                    Some(IrcCommand::Action { channel, text, pose_code }) => {
                        crate::log::debug("irc", format!("OUT Action (CTCP) to {channel} pose={pose_code:?} preview={:?}", crate::log::preview(&text)));
                        let wire = with_pose(&text, &pose_code);
                        send_line(&mut writer, &format!("PRIVMSG {channel} :\x01ACTION {wire}\x01")).await;
                        send_ev(IrcEvent::Action { server: server_name.clone(), channel, nick: nick.clone(), text: wire, is_me: true });
                    }
                    Some(IrcCommand::AppearsAs { channel, line }) => {
                        crate::log::debug("irc", format!("OUT AppearsAs to {channel}: preview={:?} — tells peers our AVB character + URL", crate::log::preview(&line)));
                        send_line(&mut writer, &format!("PRIVMSG {channel} :{line}")).await;
                        // Echo locally as an AppearsAs event so our own character updates in UI
                        if let Some((character, url)) = parse_appears_as(&line) {
                            send_ev(IrcEvent::AppearsAs { server: server_name.clone(), channel, nick: nick.clone(), character, url });
                        }
                    }
                    Some(IrcCommand::BackgroundAs { channel, line }) => {
                        crate::log::debug("irc", format!("OUT BackgroundAs to {channel}: preview={:?}", crate::log::preview(&line)));
                        send_line(&mut writer, &format!("PRIVMSG {channel} :{line}")).await;
                        if let Some((bg, url)) = parse_background_as(&line) {
                            send_ev(IrcEvent::BackgroundAs { server: server_name.clone(), channel, nick: nick.clone(), background: bg, url });
                        }
                    }
                    Some(IrcCommand::Join { channel }) => { crate::log::info("irc", format!("OUT JOIN {channel} on {}", server_name)); send_line(&mut writer, &format!("JOIN {channel}")).await; }
                    Some(IrcCommand::Part { channel }) => { crate::log::info("irc", format!("OUT PART {channel} on {}", server_name)); send_line(&mut writer, &format!("PART {channel}")).await; }
                    Some(IrcCommand::Quit { reason }) => { crate::log::info("irc", format!("OUT QUIT on {} reason={:?}", server_name, crate::log::preview(&reason))); send_line(&mut writer, &format!("QUIT :{reason}")).await; break; }
                    Some(IrcCommand::SetAway { message }) => {
                        // Bundle: standard AWAY to the server + Microsoft
                        // CTCP AWAY to every joined channel (CoffeeCupAway.mrc
                        // sendallchan). Back = bare AWAY / bare CTCP AWAY.
                        crate::log::info("irc", format!("OUT SetAway on {} message={:?} channels={:?}", server_name, message.as_deref().map(crate::log::preview), joined_channels));
                        self_away = message.clone();
                        match &message {
                            Some(m) => send_line(&mut writer, &format!("AWAY :{m}")).await,
                            None => send_line(&mut writer, "AWAY").await,
                        }
                        for ch in &joined_channels {
                            match &message {
                                Some(m) => send_line(&mut writer, &format!("PRIVMSG {ch} :\x01AWAY {m}\x01")).await,
                                None => send_line(&mut writer, &format!("PRIVMSG {ch} :\x01AWAY\x01")).await,
                            }
                        }
                    }
                    Some(IrcCommand::VersionQuery { target }) => {
                        crate::log::info("irc", format!("OUT VersionQuery to {target} on {}", server_name));
                        send_line(&mut writer, &format!("PRIVMSG {target} :\x01VERSION\x01")).await;
                    }
                    None => break,
                }
            }
            line = lines.next_line() => {
                let line = match line { Ok(Some(l)) => l, Ok(None) => { crate::log::warn("irc", format!("IN {}: server closed connection (EOF) — disconnecting", server_name)); break; }, Err(e) => { crate::log::error("irc", format!("IN {}: read error {e} — disconnecting", server_name)); break; } };
                crate::log::info("irc", format!("IN {} raw preview={:?}", server_name, crate::log::preview(&line)));
                if let Some(token) = line.strip_prefix("PING ") { crate::log::trace("irc", format!("IN PING token={:?} — replying PONG", crate::log::preview(token))); send_line(&mut writer, &format!("PONG {token}")).await; continue; }
                if line.contains(" 353 ") {
                    if let Some(after) = line.split(" :").last() {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 5 {
                            let channel = parts[4].to_string();
                            // NAMES prefixes: @ = op (host), + % ~ & = other
                            // modes. Ops are tracked separately so the
                            // member list can draw the hammer icon.
                            let mut members: Vec<String> = Vec::new();
                            let mut ops: Vec<String> = Vec::new();
                            for n in after.split_whitespace() {
                                let bare = n.trim_start_matches(['@', '+', '%', '~', '&']).to_string();
                                if n.starts_with('@') { ops.push(bare.clone()); }
                                members.push(bare);
                            }
                            send_ev(IrcEvent::Members { server: server_name.clone(), channel, members, ops });
                        }
                    }
                    continue;
                }
                if line.contains(" JOIN ") {
                    if let Some((prefix, rest)) = line.split_once(' ') {
                        let join_nick = prefix.trim_start_matches(':').split('!').next().unwrap_or("").to_string();
                        let channel = rest.split_whitespace().nth(1).unwrap_or("").trim_start_matches(':').to_string();
                        // Our own JOIN echo, even if the server truncated /
                        // changed our nick: we sent JOIN for this channel and
                        // have not yet seen our own echo, and the echo nick
                        // is a prefix of our configured nick (truncation).
                        let is_own_join = join_nick == my_nick
                            || (join_sent.contains(&channel) && !joined_channels.contains(&channel) && my_nick.starts_with(&join_nick));
                        if is_own_join {
                            // Server changed our nick (truncation). Adopt it.
                            if join_nick != my_nick {
                                crate::log::info("irc", format!("IN JOIN echo on {} — server assigned nick {join_nick:?} (configured {my_nick:?})", server_name));
                                nick = join_nick.clone();
                                my_nick = join_nick.clone();
                                send_ev(IrcEvent::MyNick { server: server_name.clone(), nick: join_nick.clone() });
                            }
                            join_sent.retain(|c| c != &channel);
                            if !joined_channels.contains(&channel) { joined_channels.push(channel.clone()); }
                            // mIRC Away.mrc on-me-join: re-announce away
                            // to the channel so newcomers see the cup.
                            if let Some(msg) = &self_away {
                                send_line(&mut writer, &format!("PRIVMSG {channel} :\x01AWAY {msg}\x01")).await;
                            }
                            send_ev(IrcEvent::Joined { server: server_name.clone(), channel: channel.clone() });
                        }
                        else {
                            // mIRC Away.mrc on-join: if WE are away, tell
                            // the joiner privately so they see our cup.
                            if let Some(msg) = &self_away {
                                send_line(&mut writer, &format!("PRIVMSG {join_nick} :\x01AWAY {msg}\x01")).await;
                            }
                            send_ev(IrcEvent::MemberJoined { server: server_name.clone(), channel, nick: join_nick });
                        }
                    }
                    continue;
                }
                if line.contains(" PART ") {
                    if let Some((prefix, rest)) = line.split_once(' ') {
                        let part_nick = prefix.trim_start_matches(':').split('!').next().unwrap_or("").to_string();
                        let channel = rest.split_whitespace().nth(1).unwrap_or("").to_string();
                        if part_nick == my_nick { joined_channels.retain(|c| c != &channel); send_ev(IrcEvent::Parted { server: server_name.clone(), channel: channel.clone() }); }
                        else { send_ev(IrcEvent::MemberLeft { server: server_name.clone(), channel, nick: part_nick }); }
                    }
                    continue;
                }
                if line.contains(" QUIT ") {
                    if let Some((prefix, _)) = line.split_once(' ') {
                        let quit_nick = prefix.trim_start_matches(':').split('!').next().unwrap_or("").to_string();
                        if quit_nick != my_nick { send_ev(IrcEvent::MemberLeft { server: server_name.clone(), channel: String::new(), nick: quit_nick }); }
                    }
                    continue;
                }
                // NICK — someone (or us) renamed. mIRC Away.mrc moves the
                // away entry to the new nick; the app does the same for
                // members / ops / away / character entries.
                if line.contains(" NICK ") {
                    if let Some((prefix, rest)) = line.split_once(' ') {
                        let old_nick = prefix.trim_start_matches(':').split('!').next().unwrap_or("").to_string();
                        let new_nick = rest.split_whitespace().nth(1).unwrap_or("").trim_start_matches(':').to_string();
                        if !old_nick.is_empty() && !new_nick.is_empty() {
                            if old_nick == my_nick { my_nick = new_nick.clone(); nick = new_nick.clone(); send_ev(IrcEvent::MyNick { server: server_name.clone(), nick: new_nick }); }
                            else { send_ev(IrcEvent::MemberRenamed { server: server_name.clone(), old_nick, new_nick }); }
                        }
                    }
                    continue;
                }
                // MODE +o / -o — channel operator (host) change. Only the
                // single-mode form is handled; multi-mode lines (e.g.
                // +ov nick nick) are ignored rather than mis-parsed.
                if line.contains(" MODE ") {
                    if let Some((_, rest)) = line.split_once(" MODE ") {
                        let mut parts = rest.split_whitespace();
                        if let (Some(channel), Some(modes), Some(target)) = (parts.next(), parts.next(), parts.next()) {
                            match modes {
                                "+o" => send_ev(IrcEvent::OpChanged { server: server_name.clone(), channel: channel.to_string(), nick: target.to_string(), is_op: true }),
                                "-o" => send_ev(IrcEvent::OpChanged { server: server_name.clone(), channel: channel.to_string(), nick: target.to_string(), is_op: false }),
                                _ => {}
                            }
                        }
                    }
                    continue;
                }
                // 301 RPL_AWAY — standard IRC away notice: someone we
                // messaged (or WHOISed) is away. Treated like a private
                // CTCP AWAY: the app applies it to every channel we
                // share with that nick.
                if line.contains(" 301 ") {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 4 {
                        let target = parts[3].to_string();
                        let msg = line.splitn(2, " :").nth(1).unwrap_or("").to_string();
                        send_ev(IrcEvent::Away { server: server_name.clone(), channel: String::new(), nick: target, message: Some(msg) });
                    }
                    continue;
                }
                if line.contains("CAP * LS") && cfg.sasl.is_some() && !sasl_started {
                    if line.contains("sasl") { send_line(&mut writer, "CAP REQ :sasl").await; } else { send_line(&mut writer, "CAP END").await; }
                    continue;
                }
                if line.contains("CAP * ACK :sasl") { send_line(&mut writer, "AUTHENTICATE PLAIN").await; sasl_started = true; continue; }
                if line.trim() == "AUTHENTICATE +" {
                    if let Some(sasl) = &cfg.sasl {
                        // Password is NEVER logged, and the base64 blob is never logged either
                        // (it decodes to \0user\0pass). Log only that we are sending it.
                        crate::log::debug("irc", format!("SASL: server sent AUTHENTICATE + — sending AUTHENTICATE PLAIN for user={:?} (password not logged, base64 not logged)", sasl.user));
                        let raw = format!("\0{}\0{}", sasl.user, sasl.pass);
                        let b64 = base64_encode(raw.as_bytes());
                        send_line(&mut writer, &format!("AUTHENTICATE {b64}")).await;
                    } else {
                        crate::log::warn("irc", format!("SASL: server sent AUTHENTICATE + but no SASL config for {} — ignoring", server_name));
                    }
                    continue;
                }
                if line.contains("903") || line.contains("904") || line.contains("905") { send_line(&mut writer, "CAP END").await; continue; }
                if line.contains("CAP * ACK") && !line.contains("sasl") { send_line(&mut writer, "CAP END").await; continue; }
                // CTCP ACTION incoming
                if line.contains("PRIVMSG") && line.contains("\x01ACTION") {
                    if let Some((prefix, rest)) = line.split_once(" PRIVMSG ") {
                        let nick2 = prefix.trim_start_matches(':').split('!').next().unwrap_or("").to_string();
                        let mut parts = rest.splitn(2, " :");
                        let channel2 = parts.next().unwrap_or("").to_string();
                        let payload = parts.next().unwrap_or("").trim_end_matches("\x01").trim_start_matches("\x01ACTION ").to_string();
                        send_ev(IrcEvent::Action { server: server_name.clone(), channel: channel2, nick: nick2, text: strip_irc_formatting(&payload), is_me: false });
                    }
                    continue;
                }
                // NOTICE — CTCP VERSION replies arrive as NOTICE, not PRIVMSG.
                // `:nick!user@host NOTICE <target> :\x01VERSION <version>\x01`
                // Other NOTICEs (server notices, etc.) are ignored.
                if line.contains(" NOTICE ") && line.contains("\x01VERSION") {
                    if let Some((prefix, rest)) = line.split_once(" NOTICE ") {
                        let sender = prefix.trim_start_matches(':').split('!').next().unwrap_or("").to_string();
                        if let Some(payload) = rest.splitn(2, " :").nth(1) {
                            let payload = payload.trim().trim_matches('\x01');
                            if let Some(version) = payload.strip_prefix("VERSION ").or(if payload == "VERSION" { Some("") } else { None }) {
                                send_ev(IrcEvent::VersionReply { server: server_name.clone(), nick: sender, version: version.to_string() });
                            }
                        }
                    }
                    continue;
                }
                // PRIVMSG — check Appears as / Background as FIRST, before treating as speech
                if let Some((target, sender, payload, is_ctcp)) = parse_privmsg(&line) {
                    crate::log::trace("irc", format!("IN PRIVMSG from {sender} to {target} on {} preview={:?}", server_name, crate::log::preview(&payload)));
                    let trimmed = payload.trim();
                    if let Some((character, url)) = parse_appears_as(trimmed) {
                        send_ev(IrcEvent::AppearsAs { server: server_name.clone(), channel: target.clone(), nick: sender, character, url });
                        continue;
                    }
                    if let Some((bg, url)) = parse_background_as(trimmed) {
                        send_ev(IrcEvent::BackgroundAs { server: server_name.clone(), channel: target.clone(), nick: sender, background: bg, url });
                        continue;
                    }
                    // CTCP VERSION — direct query only (target is our nick).
                    // A VERSION sent to a channel would make every client
                    // reply at once, so channel-targeted VERSION is ignored.
                    // Reply is a NOTICE with the exact string
                    // `Open Comic Chat (Version 0.3.17)` from crate::version_reply_with().
                    if is_ctcp && target == my_nick && (trimmed == "VERSION" || trimmed.starts_with("VERSION ")) {
                        crate::log::debug("irc", format!("IN CTCP VERSION from {sender} on {} — replying NOTICE with version reply", server_name));
                        send_line(&mut writer, &format!("NOTICE {sender} :\x01VERSION {}\x01", crate::version_reply_with(ctcp_version.as_deref()))).await;
                        continue;
                    }
                    // CTCP AWAY (Microsoft Comic Chat, nonstandard) —
                    // see CoffeeCupAway.mrc. `AWAY <msg>` = away, bare
                    // `AWAY` = back. Channel-targeted applies to that
                    // channel; private (target is our nick) applies to
                    // every channel we share with the sender (the app
                    // resolves it). Must come before the whisper branch
                    // or a private AWAY would show up as a whisper.
                    if is_ctcp && (trimmed == "AWAY" || trimmed.starts_with("AWAY ")) {
                        let msg = trimmed.strip_prefix("AWAY ").map(|s| s.to_string());
                        let ch = if target == my_nick { String::new() } else { target.clone() };
                        crate::log::debug("irc", format!("IN CTCP AWAY from {sender} on {} channel={ch:?} message={msg:?} — coffee cup", server_name));
                        send_ev(IrcEvent::Away { server: server_name.clone(), channel: ch, nick: sender, message: msg });
                        continue;
                    }
                    // Think marker
                    if trimmed.starts_with("*thinks* ") {
                        // Keep the *thinks* marker in the event text so comic.rs strips it for display
                        // but the kind is Think.
                        send_ev(IrcEvent::Message { server: server_name.clone(), channel: target, nick: sender, text: strip_irc_formatting(&payload), is_me: false, kind: crate::comic::MessageKind::Think });
                        continue;
                    }
                    // If target is our nick, it's a whisper (DM) — show as Whisper in that DM view
                    if target == my_nick {
                        send_ev(IrcEvent::Message { server: server_name.clone(), channel: sender.clone(), nick: sender, text: strip_irc_formatting(&payload), is_me: false, kind: crate::comic::MessageKind::Whisper });
                        continue;
                    }
                    send_ev(IrcEvent::Message { server: server_name.clone(), channel: target, nick: sender, text: strip_irc_formatting(&payload), is_me: false, kind: crate::comic::MessageKind::Say });
                    continue;
                }
                if (line.contains(" 432 ") || line.contains(" 433 ")) && !welcomed {
                    let retry = format!("{}_", nick.chars().take(8).collect::<String>());
                    crate::log::info("irc", format!("IN {} nick rejected (432/433) for {nick} - retrying as {retry}", server_name));
                    nick = retry.clone(); my_nick = retry.clone();
                    send_line(&mut writer, &format!("NICK {nick}")).await;
                    continue;
                }
                if line.contains(" 001 ") && !welcomed {
                    // Learn our ACTUAL nick from the welcome (:server 001 <nick> :...).
                    // The server may have truncated / changed the configured nick
                    // (e.g. a 9-char limit: oarion-tests -> oarion-te). Adopt it
                    // as our identity so the app never shows us twice.
                    if let Some(actual) = line.split_whitespace().nth(2).map(|s| s.to_string()) {
                        if actual != my_nick {
                            crate::log::info("irc", format!("IN 001 welcome on {} — server assigned nick {actual:?} (configured {my_nick:?})", server_name));
                            nick = actual.clone();
                            my_nick = actual.clone();
                            send_ev(IrcEvent::MyNick { server: server_name.clone(), nick: actual });
                        }
                    }
                    crate::log::info("irc", format!("IN 001 welcome on {} — registered as {nick}, joining {} pending channel(s) {:?}", server_name, pending_joins.len(), pending_joins));
                    welcomed = true;
                    send_ev(IrcEvent::Connected { server: server_name.clone() });
                    for ch in pending_joins.drain(..) {
                        join_sent.push(ch.clone());
                        send_line(&mut writer, &format!("JOIN {ch}")).await;
                        send_ev(IrcEvent::Joined { server: server_name.clone(), channel: ch });
                    }
                    continue;
                }
                if line.contains(" JOIN ") {
                    if let Some(ch) = line.split(" JOIN ").nth(1).map(|s| s.trim().trim_start_matches(':').to_string()) {
                        send_ev(IrcEvent::Joined { server: server_name.clone(), channel: ch });
                    }
                    continue;
                }
            }
        }
    }
    crate::log::info("irc", format!("run_server ending for {server_name} — connection closed, event Disconnected will be sent"));
    send_ev(IrcEvent::Disconnected { server: server_name, reason: "connection closed".into() });
}

fn parse_privmsg(line: &str) -> Option<(String, String, String, bool)> {
    let rest = match line.strip_prefix(':') { Some(r) => r, None => { return None; } };
    let (prefix, after) = rest.split_once(' ')?;
    let nick = prefix.split('!').next()?.to_string();
    let after = after.strip_prefix("PRIVMSG ")?;
    let (target, text) = after.split_once(" :")?;
    // CTCP = payload wrapped in \x01 ... \x01. Report it so callers can
    // tell a real CTCP AWAY / VERSION from plain chat that happens to
    // say "AWAY ..." or "VERSION".
    let is_ctcp = text.starts_with('\x01') && text.ends_with('\x01');
    let text = text.strip_prefix('\x01').and_then(|t| t.strip_suffix('\x01')).map(|t| t.strip_prefix("ACTION ").unwrap_or(t).to_string()).unwrap_or_else(|| text.to_string());
    Some((target.to_string(), nick, text, is_ctcp))
}
fn base64_encode(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 { T[((n >> 6) & 63) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[(n & 63) as usize] as char } else { '=' });
    }
    out
}


/// Normalize a /join argument: trim, None if empty, keep channel prefix
/// (# & + !) if present, else prepend #.
pub fn normalize_join_channel(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if raw.starts_with('#') || raw.starts_with('&') || raw.starts_with('+') || raw.starts_with('!') {
        Some(raw.to_string())
    } else {
        Some(format!("#{raw}"))
    }
}

#[cfg(test)]
mod normalize_join_channel_tests {
    use super::normalize_join_channel;

    #[test]
    fn bare_name_gets_hash() {
        assert_eq!(normalize_join_channel("comic"), Some("#comic".to_string()));
    }

    #[test]
    fn hash_prefix_unchanged() {
        assert_eq!(normalize_join_channel("#comic"), Some("#comic".to_string()));
    }

    #[test]
    fn other_prefixes_unchanged() {
        assert_eq!(normalize_join_channel("&comic"), Some("&comic".to_string()));
        assert_eq!(normalize_join_channel("+comic"), Some("+comic".to_string()));
        assert_eq!(normalize_join_channel("!comic"), Some("!comic".to_string()));
    }

    #[test]
    fn empty_and_whitespace_none() {
        assert_eq!(normalize_join_channel(""), None);
        assert_eq!(normalize_join_channel("   "), None);
    }

    #[test]
    fn trims_whitespace() {
        assert_eq!(normalize_join_channel("  comic  "), Some("#comic".to_string()));
    }
}

/// Sanitize an outgoing IRC line: CR, LF, NUL -> space, so one message
/// can never split into two wire lines or truncate on a NUL.
pub fn sanitize_outgoing_line(s: &str) -> String {
    s.replace(|c| c == '\r' || c == '\n' || c == '\0', " ")
}

#[cfg(test)]
mod sanitize_outgoing_line_tests {
    use super::sanitize_outgoing_line;

    #[test]
    fn strips_cr_lf_nul() {
        assert_eq!(sanitize_outgoing_line("a\rb"), "a b");
        assert_eq!(sanitize_outgoing_line("a\nb"), "a b");
        assert_eq!(sanitize_outgoing_line("a\0b"), "a b");
        assert_eq!(sanitize_outgoing_line("a\r\nb"), "a  b");
    }

    #[test]
    fn normal_text_unchanged() {
        assert_eq!(sanitize_outgoing_line("hello world"), "hello world");
        assert_eq!(sanitize_outgoing_line(""), "");
    }
}

/// Extract /join argument case-insensitively, preserving channel case.
/// "/join #comic" -> Some("#comic"), "/JOIN #comic" -> Some("#comic"),
/// bare "/join" -> Some("") (empty -> ignored via normalize_join_channel),
/// anything else -> None.
pub fn join_arg(text: &str) -> Option<&str> {
    let prefix = text.get(..5)?;
    if !prefix.eq_ignore_ascii_case("/join") {
        return None;
    }
    if text.len() == 5 {
        return Some("");
    }
    if text.as_bytes()[5] != b' ' {
        return None;
    }
    Some(&text[6..])
}

#[cfg(test)]
mod join_arg_tests {
    use super::join_arg;

    #[test]
    fn lowercase_and_uppercase() {
        assert_eq!(join_arg("/join #comic"), Some("#comic"));
        assert_eq!(join_arg("/JOIN #comic"), Some("#comic"));
        assert_eq!(join_arg("/Join #comic"), Some("#comic"));
        assert_eq!(join_arg("/jOiN #comic"), Some("#comic"));
    }

    #[test]
    fn preserves_channel_case() {
        assert_eq!(join_arg("/JOIN #Comic"), Some("#Comic"));
    }

    #[test]
    fn bare_join_is_empty() {
        assert_eq!(join_arg("/join"), Some(""));
        assert_eq!(join_arg("/JOIN"), Some(""));
    }

    #[test]
    fn not_join_is_none() {
        assert_eq!(join_arg("/msg hi"), None);
        assert_eq!(join_arg("/joinx #comic"), None);
        assert_eq!(join_arg("join #comic"), None);
        assert_eq!(join_arg(""), None);
    }
}

/// Extract /me argument case-insensitively, preserving action case.
/// "/me waves" -> Some("waves"), "/ME waves" -> Some("waves"),
/// bare "/me" -> None, anything else -> None.
pub fn me_arg(text: &str) -> Option<&str> {
    let prefix = text.get(..3)?;
    if !prefix.eq_ignore_ascii_case("/me") {
        return None;
    }
    if text.len() == 3 {
        return None;
    }
    if text.as_bytes()[3] != b' ' {
        return None;
    }
    Some(&text[4..])
}

#[cfg(test)]
mod me_arg_tests {
    use super::me_arg;

    #[test]
    fn lowercase_and_uppercase() {
        assert_eq!(me_arg("/me waves"), Some("waves"));
        assert_eq!(me_arg("/ME waves"), Some("waves"));
        assert_eq!(me_arg("/Me waves"), Some("waves"));
        assert_eq!(me_arg("/mE waves"), Some("waves"));
    }

    #[test]
    fn preserves_action_case() {
        assert_eq!(me_arg("/ME Waves Hello"), Some("Waves Hello"));
    }

    #[test]
    fn bare_me_is_none() {
        assert_eq!(me_arg("/me"), None);
        assert_eq!(me_arg("/ME"), None);
    }

    #[test]
    fn not_me_is_none() {
        assert_eq!(me_arg("/msg hi"), None);
        assert_eq!(me_arg("/mex hi"), None);
        assert_eq!(me_arg("/meow hi"), None);
        assert_eq!(me_arg("me waves"), None);
        assert_eq!(me_arg(""), None);
    }
}

/// Extract /nick argument case-insensitively, preserving nick case.
/// Returns first whitespace-separated word only (nicks can't contain
/// spaces). Bare "/nick" or whitespace-only -> None.
#[allow(dead_code)]
pub fn nick_arg(text: &str) -> Option<&str> {
    let prefix = text.get(..5)?;
    if !prefix.eq_ignore_ascii_case("/nick") {
        return None;
    }
    if text.len() == 5 {
        return None;
    }
    if text.as_bytes()[5] != b' ' {
        return None;
    }
    let arg = text[6..].trim();
    if arg.is_empty() {
        return None;
    }
    arg.split_whitespace().next()
}

#[cfg(test)]
mod nick_arg_tests {
    use super::nick_arg;

    #[test]
    fn lowercase_and_uppercase() {
        assert_eq!(nick_arg("/nick Bob"), Some("Bob"));
        assert_eq!(nick_arg("/NICK Bob"), Some("Bob"));
        assert_eq!(nick_arg("/Nick Bob"), Some("Bob"));
    }

    #[test]
    fn preserves_nick_case() {
        assert_eq!(nick_arg("/NICK BobSmith"), Some("BobSmith"));
    }

    #[test]
    fn bare_or_blank_none() {
        assert_eq!(nick_arg("/nick"), None);
        assert_eq!(nick_arg("/NICK"), None);
        assert_eq!(nick_arg("/nick   "), None);
    }

    #[test]
    fn first_word_only() {
        assert_eq!(nick_arg("/nick Bob Smith"), Some("Bob"));
        assert_eq!(nick_arg("/nick   Bob   "), Some("Bob"));
    }

    #[test]
    fn not_nick_none() {
        assert_eq!(nick_arg("/msg hi"), None);
        assert_eq!(nick_arg("/nickx Bob"), None);
        assert_eq!(nick_arg("nick Bob"), None);
        assert_eq!(nick_arg(""), None);
    }
}

/// Extract /msg target + message case-insensitively, preserving case.
/// "/msg Bob hi" -> Some(("Bob","hi")), "/MSG Bob hi" -> same.
/// No message, bare "/msg", or whitespace-only -> None.
#[allow(dead_code)]
#[allow(dead_code)]
pub fn msg_arg(text: &str) -> Option<(&str, &str)> {
    let prefix = text.get(..4)?;
    if !prefix.eq_ignore_ascii_case("/msg") {
        return None;
    }
    if text.len() == 4 {
        return None;
    }
    if text.as_bytes()[4] != b' ' {
        return None;
    }
    let rest = text[5..].trim_start();
    if rest.is_empty() {
        return None;
    }
    let mut parts = rest.splitn(2, char::is_whitespace);
    let target = parts.next()?;
    let msg = parts.next()?.trim_start();
    if target.is_empty() || msg.is_empty() {
        return None;
    }
    Some((target, msg))
}

#[cfg(test)]
mod msg_arg_tests {
    use super::msg_arg;

    #[test]
    fn lowercase_and_uppercase() {
        assert_eq!(msg_arg("/msg Bob hi"), Some(("Bob", "hi")));
        assert_eq!(msg_arg("/MSG Bob hi"), Some(("Bob", "hi")));
        assert_eq!(msg_arg("/Msg Bob hi"), Some(("Bob", "hi")));
    }

    #[test]
    fn preserves_case_and_multiword() {
        assert_eq!(msg_arg("/MSG BobSmith Hello World"), Some(("BobSmith", "Hello World")));
    }

    #[test]
    fn no_message_none() {
        assert_eq!(msg_arg("/msg"), None);
        assert_eq!(msg_arg("/MSG"), None);
        assert_eq!(msg_arg("/msg Bob"), None);
        assert_eq!(msg_arg("/msg   "), None);
        assert_eq!(msg_arg("/msg Bob   "), None);
    }

    #[test]
    fn not_msg_none() {
        assert_eq!(msg_arg("/me waves"), None);
        assert_eq!(msg_arg("/msgx Bob hi"), None);
        assert_eq!(msg_arg("msg Bob hi"), None);
        assert_eq!(msg_arg(""), None);
    }
}

/// Rest after "/msg " case-insensitively, preserving case.
/// "/msg Bob hi" -> Some("Bob hi"), "/MSG Bob hi" -> same.
#[allow(dead_code)]
/// Bare "/msg" or whitespace-only -> None. Caller still splits
/// target / message itself.
#[allow(dead_code)]
pub fn msg_rest(text: &str) -> Option<&str> {
    let prefix = text.get(..4)?;
    if !prefix.eq_ignore_ascii_case("/msg") {
        return None;
    }
    if text.len() == 4 {
        return None;
    }
    if text.as_bytes()[4] != b' ' {
        return None;
    }
    let rest = text[5..].trim_start();
    if rest.is_empty() {
        return None;
    }
    Some(rest)
}

#[cfg(test)]
mod msg_rest_tests {
    use super::msg_rest;

    #[test]
    fn lowercase_and_uppercase() {
        assert_eq!(msg_rest("/msg Bob hi"), Some("Bob hi"));
        assert_eq!(msg_rest("/MSG Bob hi"), Some("Bob hi"));
        assert_eq!(msg_rest("/Msg Bob hi"), Some("Bob hi"));
    }

    #[test]
    fn preserves_case() {
        assert_eq!(msg_rest("/MSG BobSmith Hello World"), Some("BobSmith Hello World"));
    }

    #[test]
    fn bare_or_blank_none() {
        assert_eq!(msg_rest("/msg"), None);
        assert_eq!(msg_rest("/MSG"), None);
        assert_eq!(msg_rest("/msg   "), None);
    }

    #[test]
    fn not_msg_none() {
        assert_eq!(msg_rest("/me waves"), None);
        assert_eq!(msg_rest("/msgx Bob hi"), None);
        assert_eq!(msg_rest("msg Bob hi"), None);
        assert_eq!(msg_rest(""), None);
    }
}

/// Extract /part argument case-insensitively, preserving channel case.
/// Returns None if not a /part command, Some(None) for bare "/part"
#[allow(dead_code)]
/// (leave current channel), Some(Some(channel)) for "/part #chan"
/// (first word only, rest is reason).
#[allow(dead_code)]
pub fn part_arg(text: &str) -> Option<Option<&str>> {
    let prefix = text.get(..5)?;
    if !prefix.eq_ignore_ascii_case("/part") {
        return None;
    }
    if text.len() == 5 {
        return Some(None);
    }
    if text.as_bytes()[5] != b' ' {
        return None;
    }
    let arg = text[6..].trim();
    if arg.is_empty() {
        return Some(None);
    }
    Some(Some(arg.split_whitespace().next()?))
}

#[cfg(test)]
mod part_arg_tests {
    use super::part_arg;

    #[test]
    fn lowercase_and_uppercase() {
        assert_eq!(part_arg("/part #chan"), Some(Some("#chan")));
        assert_eq!(part_arg("/PART #chan"), Some(Some("#chan")));
        assert_eq!(part_arg("/Part #chan"), Some(Some("#chan")));
    }

    #[test]
    fn bare_is_some_none() {
        assert_eq!(part_arg("/part"), Some(None));
        assert_eq!(part_arg("/PART"), Some(None));
        assert_eq!(part_arg("/part   "), Some(None));
    }

    #[test]
    fn first_word_only() {
        assert_eq!(part_arg("/part #chan bye"), Some(Some("#chan")));
        assert_eq!(part_arg("/part   #Chan   bye"), Some(Some("#Chan")));
    }

    #[test]
    fn not_part_none() {
        assert_eq!(part_arg("/msg hi"), None);
        assert_eq!(part_arg("/partx #chan"), None);
        assert_eq!(part_arg("part #chan"), None);
        assert_eq!(part_arg(""), None);
    }
}

/// Extract /quit reason case-insensitively, preserving reason case.
#[allow(dead_code)]
/// Returns None if not a /quit command, Some(None) for bare "/quit",
/// Some(Some(reason)) for "/quit bye".
#[allow(dead_code)]
pub fn quit_arg(text: &str) -> Option<Option<&str>> {
    let prefix = text.get(..5)?;
    if !prefix.eq_ignore_ascii_case("/quit") {
        return None;
    }
    if text.len() == 5 {
        return Some(None);
    }
    if text.as_bytes()[5] != b' ' {
        return None;
    }
    let arg = text[6..].trim();
    if arg.is_empty() {
        return Some(None);
    }
    Some(Some(arg))
}

#[cfg(test)]
mod quit_arg_tests {
    use super::quit_arg;

    #[test]
    fn lowercase_and_uppercase() {
        assert_eq!(quit_arg("/quit bye"), Some(Some("bye")));
        assert_eq!(quit_arg("/QUIT bye"), Some(Some("bye")));
        assert_eq!(quit_arg("/Quit bye"), Some(Some("bye")));
    }

    #[test]
    fn bare_is_some_none() {
        assert_eq!(quit_arg("/quit"), Some(None));
        assert_eq!(quit_arg("/QUIT"), Some(None));
        assert_eq!(quit_arg("/quit   "), Some(None));
    }

    #[test]
    fn preserves_reason() {
        assert_eq!(quit_arg("/QUIT See You Later"), Some(Some("See You Later")));
        assert_eq!(quit_arg("/quit   bye   "), Some(Some("bye")));
    }

    #[test]
    fn not_quit_none() {
        assert_eq!(quit_arg("/msg hi"), None);
        assert_eq!(quit_arg("/quitx bye"), None);
        assert_eq!(quit_arg("quit bye"), None);
        assert_eq!(quit_arg(""), None);
    }
}

/// Extract /away reason case-insensitively, preserving reason case.
#[allow(dead_code)]
/// Returns None if not a /away command, Some(None) for bare "/away"
/// (back from away), Some(Some(reason)) for "/away busy".
#[allow(dead_code)]
pub fn away_arg(text: &str) -> Option<Option<&str>> {
    let prefix = text.get(..5)?;
    if !prefix.eq_ignore_ascii_case("/away") {
        return None;
    }
    if text.len() == 5 {
        return Some(None);
    }
    if text.as_bytes()[5] != b' ' {
        return None;
    }
    let arg = text[6..].trim();
    if arg.is_empty() {
        return Some(None);
    }
    Some(Some(arg))
}

#[cfg(test)]
mod away_arg_tests {
    use super::away_arg;

    #[test]
    fn lowercase_and_uppercase() {
        assert_eq!(away_arg("/away busy"), Some(Some("busy")));
        assert_eq!(away_arg("/AWAY busy"), Some(Some("busy")));
        assert_eq!(away_arg("/Away busy"), Some(Some("busy")));
    }

    #[test]
    fn bare_is_some_none() {
        assert_eq!(away_arg("/away"), Some(None));
        assert_eq!(away_arg("/AWAY"), Some(None));
        assert_eq!(away_arg("/away   "), Some(None));
    }

    #[test]
    fn preserves_reason() {
        assert_eq!(away_arg("/AWAY Out To Lunch"), Some(Some("Out To Lunch")));
    }

    #[test]
    fn not_away_none() {
        assert_eq!(away_arg("/msg hi"), None);
        assert_eq!(away_arg("/awayx busy"), None);
        assert_eq!(away_arg("away busy"), None);
        assert_eq!(away_arg(""), None);
    }
}

/// Extract /topic argument case-insensitively, preserving topic case.
#[allow(dead_code)]
/// Returns None if not a /topic command, Some(None) for bare "/topic"
/// (show topic), Some(Some(topic)) for "/topic New Topic".
#[allow(dead_code)]
pub fn topic_arg(text: &str) -> Option<Option<&str>> {
    let prefix = text.get(..6)?;
    if !prefix.eq_ignore_ascii_case("/topic") {
        return None;
    }
    if text.len() == 6 {
        return Some(None);
    }
    if text.as_bytes()[6] != b' ' {
        return None;
    }
    let arg = text[7..].trim();
    if arg.is_empty() {
        return Some(None);
    }
    Some(Some(arg))
}

#[cfg(test)]
mod topic_arg_tests {
    use super::topic_arg;

    #[test]
    fn lowercase_and_uppercase() {
        assert_eq!(topic_arg("/topic New Topic"), Some(Some("New Topic")));
        assert_eq!(topic_arg("/TOPIC New Topic"), Some(Some("New Topic")));
        assert_eq!(topic_arg("/Topic New Topic"), Some(Some("New Topic")));
    }

    #[test]
    fn bare_is_some_none() {
        assert_eq!(topic_arg("/topic"), Some(None));
        assert_eq!(topic_arg("/TOPIC"), Some(None));
        assert_eq!(topic_arg("/topic   "), Some(None));
    }

    #[test]
    fn not_topic_none() {
        assert_eq!(topic_arg("/msg hi"), None);
        assert_eq!(topic_arg("/topicx hi"), None);
        assert_eq!(topic_arg("topic hi"), None);
        assert_eq!(topic_arg(""), None);
    }
}

/// Rest after "/kick " case-insensitively, preserving case.
#[allow(dead_code)]
/// "/kick Bob spamming" -> Some("Bob spamming"), "/KICK Bob" -> Some("Bob").
/// Bare "/kick" or whitespace-only -> None. Caller still splits target / reason.
#[allow(dead_code)]
pub fn kick_rest(text: &str) -> Option<&str> {
    let prefix = text.get(..5)?;
    if !prefix.eq_ignore_ascii_case("/kick") {
        return None;
    }
    if text.len() == 5 {
        return None;
    }
    if text.as_bytes()[5] != b' ' {
        return None;
    }
    let rest = text[6..].trim_start();
    if rest.is_empty() {
        return None;
    }
    Some(rest)
}

#[cfg(test)]
mod kick_rest_tests {
    use super::kick_rest;

    #[test]
    fn lowercase_and_uppercase() {
        assert_eq!(kick_rest("/kick Bob spamming"), Some("Bob spamming"));
        assert_eq!(kick_rest("/KICK Bob spamming"), Some("Bob spamming"));
        assert_eq!(kick_rest("/Kick Bob"), Some("Bob"));
    }

    #[test]
    fn bare_or_blank_none() {
        assert_eq!(kick_rest("/kick"), None);
        assert_eq!(kick_rest("/KICK"), None);
        assert_eq!(kick_rest("/kick   "), None);
    }

    #[test]
    fn not_kick_none() {
        assert_eq!(kick_rest("/msg hi"), None);
        assert_eq!(kick_rest("/kickx Bob"), None);
        assert_eq!(kick_rest("kick Bob"), None);
        assert_eq!(kick_rest(""), None);
    }
}

#[allow(dead_code)]
/// Rest after "/notice " case-insensitively, preserving case.
/// "/notice Bob hi" -> Some("Bob hi"), "/NOTICE Bob hi" -> same.
/// Bare "/notice" or whitespace-only -> None. Caller splits target/message.
#[allow(dead_code)]
pub fn notice_rest(text: &str) -> Option<&str> {
    let prefix = text.get(..7)?;
    if !prefix.eq_ignore_ascii_case("/notice") {
        return None;
    }
    if text.len() == 7 {
        return None;
    }
    if text.as_bytes()[7] != b' ' {
        return None;
    }
    let rest = text[8..].trim_start();
    if rest.is_empty() {
        return None;
    }
    Some(rest)
}

#[cfg(test)]
mod notice_rest_tests {
    use super::notice_rest;

    #[test]
    fn lowercase_and_uppercase() {
        assert_eq!(notice_rest("/notice Bob hi"), Some("Bob hi"));
        assert_eq!(notice_rest("/NOTICE Bob hi"), Some("Bob hi"));
    }

    #[test]
    fn bare_or_blank_none() {
        assert_eq!(notice_rest("/notice"), None);
        assert_eq!(notice_rest("/notice   "), None);
    }

    #[test]
    fn not_notice_none() {
        assert_eq!(notice_rest("/msg hi"), None);
        assert_eq!(notice_rest("/noticex Bob hi"), None);
        assert_eq!(notice_rest(""), None);
    }
}

#[allow(dead_code)]
/// Extract /whois target case-insensitively, preserving nick case.
/// "/whois Bob" -> Some("Bob"), "/WHOIS Bob" -> same. First word only.
/// Bare "/whois" or whitespace-only -> None.
#[allow(dead_code)]
pub fn whois_arg(text: &str) -> Option<&str> {
    let prefix = text.get(..6)?;
    if !prefix.eq_ignore_ascii_case("/whois") {
        return None;
    }
    if text.len() == 6 {
        return None;
    }
    if text.as_bytes()[6] != b' ' {
        return None;
    }
    let arg = text[7..].trim();
    if arg.is_empty() {
        return None;
    }
    arg.split_whitespace().next()
}

#[cfg(test)]
mod whois_arg_tests {
    use super::whois_arg;

    #[test]
    fn lowercase_and_uppercase() {
        assert_eq!(whois_arg("/whois Bob"), Some("Bob"));
        assert_eq!(whois_arg("/WHOIS Bob"), Some("Bob"));
    }

    #[test]
    fn preserves_case_first_word() {
        assert_eq!(whois_arg("/WHOIS BobSmith extra"), Some("BobSmith"));
    }

    #[test]
    fn bare_or_blank_none() {
        assert_eq!(whois_arg("/whois"), None);
        assert_eq!(whois_arg("/whois   "), None);
    }

    #[test]
    fn not_whois_none() {
        assert_eq!(whois_arg("/msg hi"), None);
        assert_eq!(whois_arg("/whoisx Bob"), None);
        assert_eq!(whois_arg(""), None);
    }
}

#[allow(dead_code)]
/// Rest after "/invite " case-insensitively, preserving case.
/// "/invite Bob #chan" -> Some("Bob #chan"), "/INVITE Bob #chan" -> same.
/// Bare "/invite" or whitespace-only -> None. Caller splits target/channel.
#[allow(dead_code)]
pub fn invite_rest(text: &str) -> Option<&str> {
    let prefix = text.get(..7)?;
    if !prefix.eq_ignore_ascii_case("/invite") {
        return None;
    }
    if text.len() == 7 {
        return None;
    }
    if text.as_bytes()[7] != b' ' {
        return None;
    }
    let rest = text[8..].trim_start();
    if rest.is_empty() {
        return None;
    }
    Some(rest)
}

#[cfg(test)]
mod invite_rest_tests {
    use super::invite_rest;

    #[test]
    fn lowercase_and_uppercase() {
        assert_eq!(invite_rest("/invite Bob #chan"), Some("Bob #chan"));
        assert_eq!(invite_rest("/INVITE Bob #chan"), Some("Bob #chan"));
    }

    #[test]
    fn bare_or_blank_none() {
        assert_eq!(invite_rest("/invite"), None);
        assert_eq!(invite_rest("/invite   "), None);
    }

    #[test]
    fn not_invite_none() {
        assert_eq!(invite_rest("/msg hi"), None);
        assert_eq!(invite_rest("/invitex Bob #chan"), None);
        assert_eq!(invite_rest(""), None);
    }
}

/// Rest after "/mode " case-insensitively, preserving case.
/// "/mode #chan +o Bob" -> Some("#chan +o Bob"), "/MODE #chan +o Bob" -> same.
/// Bare "/mode" or whitespace-only -> None. Caller parses modes.
#[allow(dead_code)]
pub fn mode_rest(text: &str) -> Option<&str> {
    let prefix = text.get(..5)?;
    if !prefix.eq_ignore_ascii_case("/mode") {
        return None;
    }
    if text.len() == 5 {
        return None;
    }
    if text.as_bytes()[5] != b' ' {
        return None;
    }
    let rest = text[6..].trim_start();
    if rest.is_empty() {
        return None;
    }
    Some(rest)
}

#[cfg(test)]
mod mode_rest_tests {
    use super::mode_rest;

    #[test]
    fn lowercase_and_uppercase() {
        assert_eq!(mode_rest("/mode #chan +o Bob"), Some("#chan +o Bob"));
        assert_eq!(mode_rest("/MODE #chan +o Bob"), Some("#chan +o Bob"));
    }

    #[test]
    fn bare_or_blank_none() {
        assert_eq!(mode_rest("/mode"), None);
        assert_eq!(mode_rest("/mode   "), None);
    }

    #[test]
    fn not_mode_none() {
        assert_eq!(mode_rest("/msg hi"), None);
        assert_eq!(mode_rest("/modex #chan"), None);
        assert_eq!(mode_rest(""), None);
    }
}

/// Rest after "/query " case-insensitively, preserving case.
/// "/query Bob hi" -> Some("Bob hi"), "/QUERY Bob hi" -> same.
/// Bare "/query" or whitespace-only -> None. Caller splits target/message.
#[allow(dead_code)]
pub fn query_rest(text: &str) -> Option<&str> {
    let prefix = text.get(..6)?;
    if !prefix.eq_ignore_ascii_case("/query") {
        return None;
    }
    if text.len() == 6 {
        return None;
    }
    if text.as_bytes()[6] != b' ' {
        return None;
    }
    let rest = text[7..].trim_start();
    if rest.is_empty() {
        return None;
    }
    Some(rest)
}

#[cfg(test)]
mod query_rest_tests {
    use super::query_rest;

    #[test]
    fn lowercase_and_uppercase() {
        assert_eq!(query_rest("/query Bob hi"), Some("Bob hi"));
        assert_eq!(query_rest("/QUERY Bob hi"), Some("Bob hi"));
    }

    #[test]
    fn bare_or_blank_none() {
        assert_eq!(query_rest("/query"), None);
        assert_eq!(query_rest("/query   "), None);
    }

    #[test]
    fn not_query_none() {
        assert_eq!(query_rest("/msg hi"), None);
        assert_eq!(query_rest("/queryx Bob"), None);
        assert_eq!(query_rest(""), None);
    }
}

/// Extract /op target case-insensitively, preserving nick case.
/// "/op Bob" -> Some("Bob"), "/OP Bob" -> same. First word only.
/// Bare "/op" or whitespace-only -> None.
#[allow(dead_code)]
pub fn op_arg(text: &str) -> Option<&str> {
    let prefix = text.get(..3)?;
    if !prefix.eq_ignore_ascii_case("/op") {
        return None;
    }
    if text.len() == 3 {
        return None;
    }
    if text.as_bytes()[3] != b' ' {
        return None;
    }
    let arg = text[4..].trim();
    if arg.is_empty() {
        return None;
    }
    arg.split_whitespace().next()
}

#[cfg(test)]
mod op_arg_tests {
    use super::op_arg;

    #[test]
    fn lowercase_and_uppercase() {
        assert_eq!(op_arg("/op Bob"), Some("Bob"));
        assert_eq!(op_arg("/OP Bob"), Some("Bob"));
    }

    #[test]
    fn preserves_case_first_word() {
        assert_eq!(op_arg("/OP BobSmith extra"), Some("BobSmith"));
    }

    #[test]
    fn bare_or_blank_none() {
        assert_eq!(op_arg("/op"), None);
        assert_eq!(op_arg("/op   "), None);
    }

    #[test]
    fn not_op_none() {
        assert_eq!(op_arg("/msg hi"), None);
        assert_eq!(op_arg("/opx Bob"), None);
        assert_eq!(op_arg(""), None);
    }
}

/// Extract /voice target case-insensitively, preserving nick case.
/// "/voice Bob" -> Some("Bob"), "/VOICE Bob" -> same. First word only.
/// Bare "/voice" or whitespace-only -> None.
#[allow(dead_code)]
pub fn voice_arg(text: &str) -> Option<&str> {
    let prefix = text.get(..6)?;
    if !prefix.eq_ignore_ascii_case("/voice") {
        return None;
    }
    if text.len() == 6 {
        return None;
    }
    if text.as_bytes()[6] != b' ' {
        return None;
    }
    let arg = text[7..].trim();
    if arg.is_empty() {
        return None;
    }
    arg.split_whitespace().next()
}

#[cfg(test)]
mod voice_arg_tests {
    use super::voice_arg;

    #[test]
    fn lowercase_and_uppercase() {
        assert_eq!(voice_arg("/voice Bob"), Some("Bob"));
        assert_eq!(voice_arg("/VOICE Bob"), Some("Bob"));
    }

    #[test]
    fn preserves_case_first_word() {
        assert_eq!(voice_arg("/VOICE BobSmith extra"), Some("BobSmith"));
    }

    #[test]
    fn bare_or_blank_none() {
        assert_eq!(voice_arg("/voice"), None);
        assert_eq!(voice_arg("/voice   "), None);
    }

    #[test]
    fn not_voice_none() {
        assert_eq!(voice_arg("/msg hi"), None);
        assert_eq!(voice_arg("/voicex Bob"), None);
        assert_eq!(voice_arg(""), None);
    }
}

/// Extract /deop target case-insensitively, preserving nick case.
/// "/deop Bob" -> Some("Bob"), "/DEOP Bob" -> same. First word only.
/// Bare "/deop" or whitespace-only -> None.
#[allow(dead_code)]
pub fn deop_arg(text: &str) -> Option<&str> {
    let prefix = text.get(..5)?;
    if !prefix.eq_ignore_ascii_case("/deop") {
        return None;
    }
    if text.len() == 5 {
        return None;
    }
    if text.as_bytes()[5] != b' ' {
        return None;
    }
    let arg = text[6..].trim();
    if arg.is_empty() {
        return None;
    }
    arg.split_whitespace().next()
}

#[cfg(test)]
mod deop_arg_tests {
    use super::deop_arg;

    #[test]
    fn lowercase_and_uppercase() {
        assert_eq!(deop_arg("/deop Bob"), Some("Bob"));
        assert_eq!(deop_arg("/DEOP Bob"), Some("Bob"));
    }

    #[test]
    fn bare_or_blank_none() {
        assert_eq!(deop_arg("/deop"), None);
        assert_eq!(deop_arg("/deop   "), None);
    }

    #[test]
    fn not_deop_none() {
        assert_eq!(deop_arg("/msg hi"), None);
        assert_eq!(deop_arg("/deopx Bob"), None);
        assert_eq!(deop_arg(""), None);
    }
}

/// Extract /devoice target case-insensitively, preserving nick case.
/// "/devoice Bob" -> Some("Bob"), "/DEVOICE Bob" -> same. First word only.
/// Bare "/devoice" or whitespace-only -> None.
#[allow(dead_code)]
pub fn devoice_arg(text: &str) -> Option<&str> {
    let prefix = text.get(..8)?;
    if !prefix.eq_ignore_ascii_case("/devoice") {
        return None;
    }
    if text.len() == 8 {
        return None;
    }
    if text.as_bytes()[8] != b' ' {
        return None;
    }
    let arg = text[9..].trim();
    if arg.is_empty() {
        return None;
    }
    arg.split_whitespace().next()
}

#[cfg(test)]
mod devoice_arg_tests {
    use super::devoice_arg;

    #[test]
    fn lowercase_and_uppercase() {
        assert_eq!(devoice_arg("/devoice Bob"), Some("Bob"));
        assert_eq!(devoice_arg("/DEVOICE Bob"), Some("Bob"));
    }

    #[test]
    fn bare_or_blank_none() {
        assert_eq!(devoice_arg("/devoice"), None);
        assert_eq!(devoice_arg("/devoice   "), None);
    }

    #[test]
    fn not_devoice_none() {
        assert_eq!(devoice_arg("/msg hi"), None);
        assert_eq!(devoice_arg("/devoicex Bob"), None);
        assert_eq!(devoice_arg(""), None);
    }
}

/// Rest after "/ban " case-insensitively, preserving case.
/// "/ban Bob" -> Some("Bob"), "/BAN Bob" -> same.
/// Bare "/ban" or whitespace-only -> None. Caller parses target/mask.
#[allow(dead_code)]
pub fn ban_rest(text: &str) -> Option<&str> {
    let prefix = text.get(..4)?;
    if !prefix.eq_ignore_ascii_case("/ban") {
        return None;
    }
    if text.len() == 4 {
        return None;
    }
    if text.as_bytes()[4] != b' ' {
        return None;
    }
    let rest = text[5..].trim_start();
    if rest.is_empty() {
        return None;
    }
    Some(rest)
}

#[cfg(test)]
mod ban_rest_tests {
    use super::ban_rest;

    #[test]
    fn lowercase_and_uppercase() {
        assert_eq!(ban_rest("/ban Bob"), Some("Bob"));
        assert_eq!(ban_rest("/BAN Bob"), Some("Bob"));
    }

    #[test]
    fn bare_or_blank_none() {
        assert_eq!(ban_rest("/ban"), None);
        assert_eq!(ban_rest("/ban   "), None);
    }

#[allow(dead_code)]
    #[test]
    fn not_ban_none() {
        assert_eq!(ban_rest("/msg hi"), None);
        assert_eq!(ban_rest("/banx Bob"), None);
        assert_eq!(ban_rest(""), None);
    }
}

/// Rest after "/unban " case-insensitively, preserving case.
/// "/unban Bob" -> Some("Bob"), "/UNBAN Bob" -> same.
/// Bare "/unban" or whitespace-only -> None. Caller parses target/mask.
#[allow(dead_code)]
pub fn unban_rest(text: &str) -> Option<&str> {
    let prefix = text.get(..6)?;
    if !prefix.eq_ignore_ascii_case("/unban") {
        return None;
    }
    if text.len() == 6 {
        return None;
    }
    if text.as_bytes()[6] != b' ' {
        return None;
    }
    let rest = text[7..].trim_start();
    if rest.is_empty() {
        return None;
    }
    Some(rest)
}

#[cfg(test)]
mod unban_rest_tests {
    use super::unban_rest;

    #[test]
    fn lowercase_and_uppercase() {
        assert_eq!(unban_rest("/unban Bob"), Some("Bob"));
        assert_eq!(unban_rest("/UNBAN Bob"), Some("Bob"));
    }

    #[test]
    fn bare_or_blank_none() {
        assert_eq!(unban_rest("/unban"), None);
        assert_eq!(unban_rest("/unban   "), None);
    }

#[allow(dead_code)]
    #[test]
    fn not_unban_none() {
        assert_eq!(unban_rest("/msg hi"), None);
        assert_eq!(unban_rest("/unbanx Bob"), None);
        assert_eq!(unban_rest(""), None);
    }
}

/// Extract /ignore target case-insensitively, preserving nick case.
/// "/ignore Bob" -> Some("Bob"), "/IGNORE Bob" -> same. First word only.
/// Bare "/ignore" or whitespace-only -> None.
#[allow(dead_code)]
pub fn ignore_arg(text: &str) -> Option<&str> {
    let prefix = text.get(..7)?;
    if !prefix.eq_ignore_ascii_case("/ignore") {
        return None;
    }
    if text.len() == 7 {
        return None;
    }
    if text.as_bytes()[7] != b' ' {
        return None;
    }
    let arg = text[8..].trim();
    if arg.is_empty() {
        return None;
    }
    arg.split_whitespace().next()
}

#[cfg(test)]
mod ignore_arg_tests {
    use super::ignore_arg;

    #[test]
    fn lowercase_and_uppercase() {
        assert_eq!(ignore_arg("/ignore Bob"), Some("Bob"));
        assert_eq!(ignore_arg("/IGNORE Bob"), Some("Bob"));
    }

    #[test]
    fn bare_or_blank_none() {
        assert_eq!(ignore_arg("/ignore"), None);
        assert_eq!(ignore_arg("/ignore   "), None);
    }

    #[test]
    fn not_ignore_none() {
        assert_eq!(ignore_arg("/msg hi"), None);
        assert_eq!(ignore_arg("/ignorex Bob"), None);
        assert_eq!(ignore_arg(""), None);
    }
}

/// Extract /unignore target case-insensitively, preserving nick case.
/// "/unignore Bob" -> Some("Bob"), "/UNIGNORE Bob" -> same. First word only.
/// Bare "/unignore" or whitespace-only -> None.
#[allow(dead_code)]
pub fn unignore_arg(text: &str) -> Option<&str> {
    let prefix = text.get(..9)?;
    if !prefix.eq_ignore_ascii_case("/unignore") {
        return None;
    }
    if text.len() == 9 {
        return None;
    }
    if text.as_bytes()[9] != b' ' {
        return None;
    }
    let arg = text[10..].trim();
    if arg.is_empty() {
        return None;
    }
    arg.split_whitespace().next()
}

#[cfg(test)]
mod unignore_arg_tests {
    use super::unignore_arg;

    #[test]
    fn lowercase_and_uppercase() {
        assert_eq!(unignore_arg("/unignore Bob"), Some("Bob"));
        assert_eq!(unignore_arg("/UNIGNORE Bob"), Some("Bob"));
    }

    #[test]
    fn bare_or_blank_none() {
        assert_eq!(unignore_arg("/unignore"), None);
        assert_eq!(unignore_arg("/unignore   "), None);
    }

    #[test]
    fn not_unignore_none() {
        assert_eq!(unignore_arg("/msg hi"), None);
        assert_eq!(unignore_arg("/unignorex Bob"), None);
        assert_eq!(unignore_arg(""), None);
    }
}

/// Rest after "/list " case-insensitively, preserving case.
/// "/list #chan" -> Some("#chan"), "/LIST #chan" -> same.
/// Bare "/list" or whitespace-only -> None. Caller parses channel/mask.
#[allow(dead_code)]
pub fn list_rest(text: &str) -> Option<&str> {
    let prefix = text.get(..5)?;
    if !prefix.eq_ignore_ascii_case("/list") {
        return None;
    }
    if text.len() == 5 {
        return None;
    }
    if text.as_bytes()[5] != b' ' {
        return None;
    }
    let rest = text[6..].trim_start();
    if rest.is_empty() {
        return None;
    }
    Some(rest)
}

#[cfg(test)]
mod list_rest_tests {
    use super::list_rest;

    #[test]
    fn lowercase_and_uppercase() {
        assert_eq!(list_rest("/list #chan"), Some("#chan"));
        assert_eq!(list_rest("/LIST #chan"), Some("#chan"));
    }

    #[test]
    fn bare_or_blank_none() {
        assert_eq!(list_rest("/list"), None);
        assert_eq!(list_rest("/list   "), None);
    }

    #[test]
    fn not_list_none() {
        assert_eq!(list_rest("/msg hi"), None);
        assert_eq!(list_rest("/listx #chan"), None);
        assert_eq!(list_rest(""), None);
    }
}

/// Rest after "/names " case-insensitively, preserving case.
/// "/names #chan" -> Some("#chan"), "/NAMES #chan" -> same.
/// Bare "/names" or whitespace-only -> None. Caller parses channel.
#[allow(dead_code)]
pub fn names_rest(text: &str) -> Option<&str> {
    let prefix = text.get(..6)?;
    if !prefix.eq_ignore_ascii_case("/names") {
        return None;
    }
    if text.len() == 6 {
        return None;
    }
    if text.as_bytes()[6] != b' ' {
        return None;
    }
    let rest = text[7..].trim_start();
    if rest.is_empty() {
        return None;
    }
    Some(rest)
}

#[cfg(test)]
mod names_rest_tests {
    use super::names_rest;

    #[test]
    fn lowercase_and_uppercase() {
        assert_eq!(names_rest("/names #chan"), Some("#chan"));
        assert_eq!(names_rest("/NAMES #chan"), Some("#chan"));
    }

    #[test]
    fn bare_or_blank_none() {
        assert_eq!(names_rest("/names"), None);
        assert_eq!(names_rest("/names   "), None);
    }

    #[test]
    fn not_names_none() {
        assert_eq!(names_rest("/msg hi"), None);
        assert_eq!(names_rest("/namesx #chan"), None);
        assert_eq!(names_rest(""), None);
    }
}

#[cfg(test)]
mod version_freeze_tests {
    #[test]
    fn version_stays_0_3_x() {
        // Version line frozen at 3.3 / 0.3.x — never 0.4 / 1.0.
        let v = env!("CARGO_PKG_VERSION");
        assert!(v.starts_with("0.3."), "version must stay 0.3.x, got: {v}");
    }
}

/// Resolve /part target: explicit arg wins, bare /part uses current channel.
/// Returns None if no channel (bare + empty current). Normalizes bare names
/// via normalize_join_channel ("foo" -> "#foo", "#foo" stays).
pub fn part_target(text: &str, current_channel: &str) -> Option<String> {
    let arg = text.strip_prefix("/part").unwrap_or(text).trim();
    // Bare "/part" or "/part " with nothing after
    if arg.is_empty() {
        let cur = current_channel.trim();
        if cur.is_empty() { return None; }
        return Some(cur.to_string());
    }
    // First token only — "/part #foo extra" -> "#foo"
    let first = arg.split_whitespace().next().unwrap_or("");
    if first.is_empty() { return None; }
    normalize_join_channel(first)
}

#[cfg(test)]
mod part_target_tests {
    use super::part_target;
    #[test]
    fn bare_uses_current() {
        assert_eq!(part_target("/part", "#general"), Some("#general".to_string()));
        assert_eq!(part_target("/part ", "#general"), Some("#general".to_string()));
    }
    #[test]
    fn bare_no_current_is_none() {
        assert_eq!(part_target("/part", ""), None);
        assert_eq!(part_target("/part", "   "), None);
    }
    #[test]
    fn explicit_keeps_hash() {
        assert_eq!(part_target("/part #foo", "#general"), Some("#foo".to_string()));
    }
    #[test]
    fn explicit_bare_name_gets_hash() {
        assert_eq!(part_target("/part foo", "#general"), Some("#foo".to_string()));
    }
    #[test]
    fn first_token_only() {
        assert_eq!(part_target("/part #foo extra", "#general"), Some("#foo".to_string()));
    }
}

/// Unified slash-command parse (pure, testable).
/// `current_channel` is used for bare /part. Returns NotSlash if text
/// does not start with '/'. Bare commands that need an arg and have none
/// return BareIgnored (caller should ignore, not send as chat).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlashAction {
    Join { channel: String },
    Part { channel: String },
    Me { text: String },
    Msg { target: String, text: String },
    Nick { nick: String },
    Quit { reason: String },
    Away { message: Option<String> },
    Back,
    NotSlash,
    Unknown { command: String },
    BareIgnored { command: String },
}

pub fn parse_slash_command(text: &str, current_channel: &str) -> SlashAction {
    let t = text.trim();
    if !t.starts_with('/') { return SlashAction::NotSlash; }
    // Split "/cmd" + rest (case-insensitive cmd, rest preserved)
    let without_slash = &t[1..];
    let (cmd_raw, rest) = match without_slash.find(|c: char| c.is_whitespace()) {
        Some(idx) => (&without_slash[..idx], without_slash[idx..].trim()),
        None => (without_slash, ""),
    };
    let cmd = cmd_raw.to_ascii_lowercase();
    match cmd.as_str() {
        "join" => {
            if rest.is_empty() { return SlashAction::BareIgnored { command: "join".into() }; }
            let first = rest.split_whitespace().next().unwrap_or("");
            // normalize_join_channel returns Option<String> in this tree
            match normalize_join_channel(first) {
                Some(ch) => SlashAction::Join { channel: ch },
                None => SlashAction::BareIgnored { command: "join".into() },
            }
        }
        "part" => {
            match part_target(t, current_channel) {
                Some(ch) => SlashAction::Part { channel: ch },
                None => SlashAction::BareIgnored { command: "part".into() },
            }
        }
        "me" => {
            if rest.is_empty() { return SlashAction::BareIgnored { command: "me".into() }; }
            SlashAction::Me { text: rest.to_string() }
        }
        "msg" => {
            let mut it = rest.splitn(2, char::is_whitespace);
            let target = it.next().unwrap_or("").trim();
            let msg = it.next().unwrap_or("").trim();
            if target.is_empty() || msg.is_empty() { return SlashAction::BareIgnored { command: "msg".into() }; }
            SlashAction::Msg { target: target.to_string(), text: msg.to_string() }
        }
        "nick" => {
            if rest.is_empty() { return SlashAction::BareIgnored { command: "nick".into() }; }
            SlashAction::Nick { nick: rest.split_whitespace().next().unwrap_or("").to_string() }
        }
        "quit" => {
            // Bare /quit quits with default reason — not ignored.
            let reason = if rest.is_empty() { "Quit".to_string() } else { rest.to_string() };
            SlashAction::Quit { reason }
        }
        "away" => {
            if rest.is_empty() {
                // Bare /away toggles — caller decides toggle using away state.
                // We encode as Away{None} meaning "toggle / back if away".
                // To distinguish toggle from explicit back, caller checks rest.
                // Here: bare -> Away{None} with toggle semantics documented in app.rs.
                SlashAction::Away { message: None }
            } else {
                SlashAction::Away { message: Some(rest.to_string()) }
            }
        }
        "back" => SlashAction::Back,
        _ => SlashAction::Unknown { command: cmd },
    }
}

#[cfg(test)]
mod slash_action_tests {
    use super::{parse_slash_command, SlashAction};
    #[test] fn not_slash() { assert_eq!(parse_slash_command("hello", "#c"), SlashAction::NotSlash); }
    #[test] fn join_explicit() { assert_eq!(parse_slash_command("/join #foo", "#c"), SlashAction::Join { channel: "#foo".into() }); }
    #[test] fn join_bare_name() { assert_eq!(parse_slash_command("/join foo", "#c"), SlashAction::Join { channel: "#foo".into() }); }
    #[test] fn join_bare_ignored() { assert_eq!(parse_slash_command("/join", "#c"), SlashAction::BareIgnored { command: "join".into() }); }
    #[test] fn join_case_insensitive() { assert_eq!(parse_slash_command("/JOIN #foo", "#c"), SlashAction::Join { channel: "#foo".into() }); }
    #[test] fn part_bare_uses_current() { assert_eq!(parse_slash_command("/part", "#cur"), SlashAction::Part { channel: "#cur".into() }); }
    #[test] fn part_explicit() { assert_eq!(parse_slash_command("/part #foo", "#cur"), SlashAction::Part { channel: "#foo".into() }); }
    #[test] fn me_ok() { assert_eq!(parse_slash_command("/me waves", "#c"), SlashAction::Me { text: "waves".into() }); }
    #[test] fn me_bare_ignored() { assert_eq!(parse_slash_command("/me", "#c"), SlashAction::BareIgnored { command: "me".into() }); }
    #[test] fn msg_ok() { assert_eq!(parse_slash_command("/msg bob hi there", "#c"), SlashAction::Msg { target: "bob".into(), text: "hi there".into() }); }
    #[test] fn msg_bare_ignored() { assert_eq!(parse_slash_command("/msg bob", "#c"), SlashAction::BareIgnored { command: "msg".into() }); }
    #[test] fn nick_ok() { assert_eq!(parse_slash_command("/nick NewNick", "#c"), SlashAction::Nick { nick: "NewNick".into() }); }
    #[test] fn quit_bare_default() { assert_eq!(parse_slash_command("/quit", "#c"), SlashAction::Quit { reason: "Quit".into() }); }
    #[test] fn quit_with_reason() { assert_eq!(parse_slash_command("/quit bye", "#c"), SlashAction::Quit { reason: "bye".into() }); }
    #[test] fn unknown() { assert_eq!(parse_slash_command("/frobnicate x", "#c"), SlashAction::Unknown { command: "frobnicate".into() }); }
}

/// CTCP helpers (wire: \x01CMD args\x01).
/// parse_ctcp("\x01VERSION\x01") == Some(("VERSION", ""))
/// parse_ctcp("\x01PING 123\x01") == Some(("PING", "123"))
/// parse_ctcp("hello") == None
pub fn is_ctcp_command(s: &str) -> bool { parse_ctcp(s).is_some() }
pub fn parse_ctcp(s: &str) -> Option<(String, String)> {
    let t = s.trim();
    if !t.starts_with('\u{1}') || !t.ends_with('\u{1}') || t.len() < 3 { return None; }
    let inner = &t[1..t.len() - 1];
    let (cmd, rest) = match inner.find(|c: char| c.is_whitespace()) {
        Some(idx) => (&inner[..idx], inner[idx..].trim()),
        None => (inner, ""),
    };
    if cmd.is_empty() { return None; }
    Some((cmd.to_ascii_uppercase(), rest.to_string()))
}

#[cfg(test)]
mod ctcp_tests {
    use super::{is_ctcp_command, parse_ctcp};
    #[test] fn version() { assert_eq!(parse_ctcp("\u{1}VERSION\u{1}"), Some(("VERSION".into(), "".into()))); }
    #[test] fn ping_with_arg() { assert_eq!(parse_ctcp("\u{1}PING 12345\u{1}"), Some(("PING".into(), "12345".into()))); }
    #[test] fn not_ctcp() { assert_eq!(parse_ctcp("hello"), None); assert!(!is_ctcp_command("hello")); }
    #[test] fn empty_inner() { assert_eq!(parse_ctcp("\u{1}\u{1}"), None); }
    #[test] fn lower_is_uppercased() { assert_eq!(parse_ctcp("\u{1}version\u{1}"), Some(("VERSION".into(), "".into()))); }
}

/// IRC formatting stripping (wire: \x03 color, \x02 bold, \x1F underline,
/// \x1D italic, \x0F reset, \x16 reverse). Display path strips these so
/// a colored message from a real client does not leak codes into balloons.
pub fn strip_irc_color_codes(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x03' {
            for _ in 0..2 {
                if let Some(&d) = chars.peek() {
                    if d.is_ascii_digit() { chars.next(); } else { break; }
                } else { break; }
            }
            if let Some(&',') = chars.peek() {
                chars.next();
                for _ in 0..2 {
                    if let Some(&d) = chars.peek() {
                        if d.is_ascii_digit() { chars.next(); } else { break; }
                    } else { break; }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}
pub fn strip_irc_formatting(s: &str) -> String {
    let no_color = strip_irc_color_codes(s);
    no_color.chars().filter(|&c| !matches!(c, '\x02' | '\x1F' | '\x1D' | '\x0F' | '\x16')).collect()
}
/// CTCP VERSION reply, always from Cargo version (frozen 0.3.17).
/// Uses env! so it cannot drift from Cargo.toml even if mod wiring differs.
pub fn ctcp_version_reply() -> String {
    format!("Open Comic Chat (Version {})", env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod irc_formatting_tests {
    use super::{ctcp_version_reply, strip_irc_color_codes, strip_irc_formatting};
    #[test] fn color_bare() { assert_eq!(strip_irc_color_codes("hi\x03there"), "hithere"); }
    #[test] fn color_with_num() { assert_eq!(strip_irc_color_codes("hi\x034there"), "hithere"); }
    #[test] fn color_fg_bg() { assert_eq!(strip_irc_color_codes("a\x034,12b"), "ab"); }
    #[test] fn bold_underline() { assert_eq!(strip_irc_formatting("\x02bold\x02 \x1Funder\x1F"), "bold under"); }
    #[test] fn formatting_keeps_text() { assert_eq!(strip_irc_formatting("hello world"), "hello world"); }
    #[test] fn version_reply_frozen() { assert_eq!(ctcp_version_reply(), "Open Comic Chat (Version 0.3.17)"); }
}

/// Raw IRC line parse (wire: [:prefix] COMMAND [params] [:trailing]).
/// parse_irc_line(":bob!u@h PRIVMSG #x :hi") -> prefix Some("bob!u@h"),
/// command "PRIVMSG", params ["#x"], trailing Some("hi").
/// parse_irc_line("PING :server") -> prefix None, command "PING",
/// params [], trailing Some("server").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawIrcLine {
    pub prefix: Option<String>,
    pub command: String,
    pub params: Vec<String>,
    pub trailing: Option<String>,
}
pub fn parse_irc_line(line: &str) -> Option<RawIrcLine> {
    let mut rest = line.trim_end_matches(|c| c == '\r' || c == '\n');
    if rest.is_empty() { return None; }
    let prefix = if let Some(stripped) = rest.strip_prefix(':') {
        match stripped.find(' ') {
            Some(idx) => { let p = &stripped[..idx]; rest = stripped[idx..].trim_start(); Some(p.to_string()) }
            None => return None,
        }
    } else { None };
    let (command, after_cmd) = match rest.find(' ') {
        Some(idx) => (&rest[..idx], rest[idx..].trim_start()),
        None => (rest, ""),
    };
    if command.is_empty() { return None; }
    let mut params: Vec<String> = Vec::new();
    let mut trailing: Option<String> = None;
    let mut cur = after_cmd;
    while !cur.is_empty() {
        if let Some(t) = cur.strip_prefix(':') {
            trailing = Some(t.to_string());
            break;
        }
        match cur.find(' ') {
            Some(idx) => { params.push(cur[..idx].to_string()); cur = cur[idx..].trim_start(); }
            None => { params.push(cur.to_string()); break; }
        }
    }
    Some(RawIrcLine { prefix, command: command.to_ascii_uppercase(), params, trailing })
}
/// Channel: starts # or &, 2..=50 chars, no space / comma / \x07.
pub fn is_channel_name(s: &str) -> bool {
    let t = s.trim();
    t.len() >= 2 && t.len() <= 50 && (t.starts_with('#') || t.starts_with('&'))
        && !t.contains(' ') && !t.contains(',') && !t.contains('\x07')
}
/// Nick: 1..=30, starts letter or _[]\\`^|}{, rest alnum or _-[]\\`^|}{, no space.
pub fn is_valid_nick(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() || t.len() > 30 { return false; }
    let first = t.chars().next().unwrap();
    if !(first.is_ascii_alphabetic() || matches!(first, '_' | '[' | ']' | '\\' | '`' | '^' | '|' | '{' | '}')) { return false; }
    t.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '[' | ']' | '\\' | '`' | '^' | '|' | '{' | '}'))
}

#[cfg(test)]
mod raw_irc_line_tests {
    use super::{is_channel_name, is_valid_nick, parse_irc_line};
    #[test] fn privmsg() {
        let l = parse_irc_line(":bob!u@h PRIVMSG #x :hi there").unwrap();
        assert_eq!(l.prefix.as_deref(), Some("bob!u@h"));
        assert_eq!(l.command, "PRIVMSG");
        assert_eq!(l.params, vec!["#x".to_string()]);
        assert_eq!(l.trailing.as_deref(), Some("hi there"));
    }
    #[test] fn ping() {
        let l = parse_irc_line("PING :server").unwrap();
        assert_eq!(l.prefix, None);
        assert_eq!(l.command, "PING");
        assert_eq!(l.trailing.as_deref(), Some("server"));
    }
    #[test] fn join_no_trailing() {
        let l = parse_irc_line(":bob!u@h JOIN #x").unwrap();
        assert_eq!(l.command, "JOIN");
        assert_eq!(l.params, vec!["#x".to_string()]);
        assert_eq!(l.trailing, None);
    }
    #[test] fn empty_none() { assert_eq!(parse_irc_line(""), None); assert_eq!(parse_irc_line("   "), None); }
    #[test] fn command_uppercased() { assert_eq!(parse_irc_line("privmsg #x :hi").unwrap().command, "PRIVMSG"); }
    #[test] fn channel_ok() { assert!(is_channel_name("#foo")); assert!(is_channel_name("&foo")); assert!(!is_channel_name("foo")); assert!(!is_channel_name("#")); assert!(!is_channel_name("#a b")); assert!(!is_channel_name("#a,b")); }
    #[test] fn nick_ok() { assert!(is_valid_nick("bob")); assert!(is_valid_nick("Bob_123")); assert!(is_valid_nick("[bob]")); assert!(!is_valid_nick("")); assert!(!is_valid_nick("9bob")); assert!(!is_valid_nick("bob bob")); assert!(!is_valid_nick("a".repeat(31).as_str())); }
}

/// Wire helpers: prefix "bob!user@host" -> "bob". No '!' -> whole prefix.
/// is_privmsg_to_channel("#x", my_nick) == true, ("bob", my_nick) == false.
pub fn extract_nick_from_prefix(prefix: &str) -> String {
    prefix.split('!').next().unwrap_or(prefix).to_string()
}
pub fn is_privmsg_to_channel(target: &str, my_nick: &str) -> bool {
    target != my_nick && is_channel_name(target)
}

#[cfg(test)]
mod prefix_helper_tests {
    use super::{extract_nick_from_prefix, is_privmsg_to_channel};
    #[test] fn nick_from_prefix() { assert_eq!(extract_nick_from_prefix("bob!u@h"), "bob"); assert_eq!(extract_nick_from_prefix("bob"), "bob"); assert_eq!(extract_nick_from_prefix(""), ""); }
    #[test] fn privmsg_channel() { assert!(is_privmsg_to_channel("#x", "me")); assert!(!is_privmsg_to_channel("me", "me")); assert!(!is_privmsg_to_channel("bob", "me")); }
}

/// Wire helper: split prefix "bob!user@host" -> (nick "bob", user Some("user"), host Some("host")).
/// "bob" -> ("bob", None, None). "bob@host" -> ("bob", None, Some("host")).
/// format_privmsg("#x", "hi") -> "PRIVMSG #x :hi".
pub fn parse_irc_prefix(prefix: &str) -> (String, Option<String>, Option<String>) {
    let (nick_part, host_part) = match prefix.find('@') {
        Some(idx) => (&prefix[..idx], Some(&prefix[idx + 1..])),
        None => (prefix, None),
    };
    let (nick, user) = match nick_part.find('!') {
        Some(idx) => (nick_part[..idx].to_string(), Some(nick_part[idx + 1..].to_string())),
        None => (nick_part.to_string(), None),
    };
    (nick, user, host_part.map(|s| s.to_string()))
}
pub fn format_privmsg(target: &str, text: &str) -> String {
    format!("PRIVMSG {} :{}", target, text)
}

#[cfg(test)]
mod irc_prefix_tests {
    use super::{format_privmsg, parse_irc_prefix};
    #[test] fn full() { assert_eq!(parse_irc_prefix("bob!u@h"), ("bob".into(), Some("u".into()), Some("h".into()))); }
    #[test] fn nick_only() { assert_eq!(parse_irc_prefix("bob"), ("bob".into(), None, None)); }
    #[test] fn nick_host() { assert_eq!(parse_irc_prefix("bob@h"), ("bob".into(), None, Some("h".into()))); }
    #[test] fn format() { assert_eq!(format_privmsg("#x", "hi"), "PRIVMSG #x :hi"); }
}

/// Numeric replies: command is exactly 3 ASCII digits ("001", "353", "433").
/// is_error_numeric: 400-599 (e.g. 401, 433, 464, 475).
pub fn is_numeric_reply(cmd: &str) -> bool {
    cmd.len() == 3 && cmd.chars().all(|c| c.is_ascii_digit())
}
pub fn is_error_numeric(cmd: &str) -> bool {
    if !is_numeric_reply(cmd) { return false; }
    matches!(cmd.chars().next(), Some('4') | Some('5'))
}

#[cfg(test)]
mod numeric_tests {
    use super::{is_error_numeric, is_numeric_reply};
    #[test] fn numeric() { assert!(is_numeric_reply("001")); assert!(is_numeric_reply("353")); assert!(!is_numeric_reply("PRIVMSG")); assert!(!is_numeric_reply("01")); assert!(!is_numeric_reply("")); }
    #[test] fn error() { assert!(is_error_numeric("401")); assert!(is_error_numeric("433")); assert!(!is_error_numeric("001")); assert!(!is_error_numeric("353")); assert!(!is_error_numeric("PRIVMSG")); }
}

/// Wire format helpers: "JOIN #x", "PART #x", "QUIT :bye".
/// format_quit("") -> "QUIT :Quit" (default reason).
pub fn format_join(channel: &str) -> String { format!("JOIN {}", channel) }
pub fn format_part(channel: &str) -> String { format!("PART {}", channel) }
pub fn format_quit(reason: &str) -> String {
    let r = if reason.trim().is_empty() { "Quit" } else { reason.trim() };
    format!("QUIT :{}", r)
}

#[cfg(test)]
mod format_join_tests {
    use super::{format_join, format_part, format_quit};
    #[test] fn join() { assert_eq!(format_join("#x"), "JOIN #x"); }
    #[test] fn part() { assert_eq!(format_part("#x"), "PART #x"); }
    #[test] fn quit() { assert_eq!(format_quit("bye"), "QUIT :bye"); assert_eq!(format_quit(""), "QUIT :Quit"); assert_eq!(format_quit("   "), "QUIT :Quit"); }
}

/// Away wire: "AWAY :msg" or bare "AWAY" for back. 301 is the away numeric.
pub fn format_away(msg: Option<&str>) -> String {
    match msg {
        Some(m) if !m.trim().is_empty() => format!("AWAY :{}", m.trim()),
        _ => "AWAY".to_string(),
    }
}
pub fn format_back() -> String { "AWAY".to_string() }
pub fn is_away_numeric(cmd: &str) -> bool { cmd == "301" }

#[cfg(test)]
mod away_format_tests {
    use super::{format_away, format_back, is_away_numeric};
    #[test] fn away_with_msg() { assert_eq!(format_away(Some("brb")), "AWAY :brb"); }
    #[test] fn away_none() { assert_eq!(format_away(None), "AWAY"); assert_eq!(format_away(Some("")), "AWAY"); assert_eq!(format_away(Some("   ")), "AWAY"); }
    #[test] fn back() { assert_eq!(format_back(), "AWAY"); }
    #[test] fn numeric() { assert!(is_away_numeric("301")); assert!(!is_away_numeric("302")); assert!(!is_away_numeric("PRIVMSG")); }
}

/// Wire format: TOPIC / KICK / NOTICE. Topic numerics 332 (topic) + 333 (topic setter).
pub fn format_topic(channel: &str, topic: &str) -> String { format!("TOPIC {} :{}", channel, topic) }
pub fn format_kick(channel: &str, nick: &str, reason: &str) -> String {
    let r = if reason.trim().is_empty() { nick } else { reason.trim() };
    format!("KICK {} {} :{}", channel, nick, r)
}
pub fn format_notice(target: &str, text: &str) -> String { format!("NOTICE {} :{}", target, text) }
pub fn is_topic_numeric(cmd: &str) -> bool { cmd == "332" || cmd == "333" }

#[cfg(test)]
mod topic_kick_tests {
    use super::{format_kick, format_notice, format_topic, is_topic_numeric};
    #[test] fn topic() { assert_eq!(format_topic("#x", "hello"), "TOPIC #x :hello"); }
    #[test] fn kick() { assert_eq!(format_kick("#x", "bob", "spam"), "KICK #x bob :spam"); assert_eq!(format_kick("#x", "bob", ""), "KICK #x bob :bob"); }
    #[test] fn notice() { assert_eq!(format_notice("bob", "hi"), "NOTICE bob :hi"); }
    #[test] fn numeric() { assert!(is_topic_numeric("332")); assert!(is_topic_numeric("333")); assert!(!is_topic_numeric("331")); }
}

/// MODE wire: "MODE #x +o bob" or "MODE #x +nt" (no target). 324 is MODE numeric.
pub fn format_mode(channel: &str, modes: &str, target: &str) -> String {
    if target.trim().is_empty() {
        format!("MODE {} {}", channel, modes)
    } else {
        format!("MODE {} {} {}", channel, modes, target.trim())
    }
}
pub fn is_mode_numeric(cmd: &str) -> bool { cmd == "324" }

#[cfg(test)]
mod mode_tests {
    use super::{format_mode, is_mode_numeric};
    #[test] fn mode_with_target() { assert_eq!(format_mode("#x", "+o", "bob"), "MODE #x +o bob"); }
    #[test] fn mode_no_target() { assert_eq!(format_mode("#x", "+nt", ""), "MODE #x +nt"); }
    #[test] fn numeric() { assert!(is_mode_numeric("324")); assert!(!is_mode_numeric("332")); }
}

/// WHOIS / WHO wire. WHOIS numerics: 311 (user), 312 (server), 313 (operator), 318 (end), 319 (channels).
pub fn format_whois(nick: &str) -> String { format!("WHOIS {}", nick.trim()) }
pub fn format_who(channel: &str) -> String { format!("WHO {}", channel.trim()) }
pub fn is_whois_numeric(cmd: &str) -> bool { matches!(cmd, "311" | "312" | "313" | "318" | "319") }

#[cfg(test)]
mod whois_tests {
    use super::{format_who, format_whois, is_whois_numeric};
    #[test] fn whois() { assert_eq!(format_whois("bob"), "WHOIS bob"); assert_eq!(format_whois("  bob "), "WHOIS bob"); }
    #[test] fn who() { assert_eq!(format_who("#x"), "WHO #x"); }
    #[test] fn numeric() { assert!(is_whois_numeric("311")); assert!(is_whois_numeric("319")); assert!(!is_whois_numeric("332")); }
}

/// INVITE / LIST wire. LIST numerics: 322 (list), 323 (end of list).
pub fn format_invite(nick: &str, channel: &str) -> String { format!("INVITE {} {}", nick.trim(), channel.trim()) }
pub fn format_list() -> String { "LIST".to_string() }
pub fn is_list_numeric(cmd: &str) -> bool { cmd == "322" || cmd == "323" }



pub fn format_names(channel: &str) -> String { format!("NAMES {}", channel.trim()) }



pub fn format_ban(channel: &str, mask: &str) -> String { format!("MODE {} +b {}", channel.trim(), mask.trim()) }



pub fn format_unban(channel: &str, mask: &str) -> String { format!("MODE {} -b {}", channel.trim(), mask.trim()) }



pub fn format_op(channel: &str, nick: &str) -> String { format!("MODE {} +o {}", channel.trim(), nick.trim()) }



pub fn format_deop(channel: &str, nick: &str) -> String { format!("MODE {} -o {}", channel.trim(), nick.trim()) }



pub fn format_voice(channel: &str, nick: &str) -> String { format!("MODE {} +v {}", channel.trim(), nick.trim()) }



pub fn format_devoice(channel: &str, nick: &str) -> String { format!("MODE {} -v {}", channel.trim(), nick.trim()) }



pub fn format_nick(new_nick: &str) -> String { format!("NICK {}", new_nick.trim()) }



pub fn is_names_numeric(cmd: &str) -> bool { cmd == "353" || cmd == "366" }



pub fn is_ban_numeric(cmd: &str) -> bool { cmd == "367" || cmd == "368" }



pub fn is_nick_numeric(cmd: &str) -> bool { matches!(cmd, "431" | "432" | "433" | "436") }



pub fn format_ping(server: &str) -> String { format!("PING {}", server.trim()) }



pub fn format_pong(server: &str) -> String { format!("PONG {}", server.trim()) }



pub fn is_ping_command(cmd: &str) -> bool { cmd.eq_ignore_ascii_case("PING") }



pub fn is_pong_command(cmd: &str) -> bool { cmd.eq_ignore_ascii_case("PONG") }



pub fn parse_prefix_nick(prefix: &str) -> String { let t = prefix.trim().trim_start_matches(':'); let before_bang = t.split('!').next().unwrap_or(t); let before_at = before_bang.split('@').next().unwrap_or(before_bang); before_at.to_string() }



pub fn parse_prefix_host(prefix: &str) -> String { let t = prefix.trim().trim_start_matches(':'); match t.rfind('@') { Some(i) => t[i+1..].to_string(), None => "".to_string() } }



pub fn is_privmsg_command(cmd: &str) -> bool { cmd.eq_ignore_ascii_case("PRIVMSG") }



pub fn is_notice_command(cmd: &str) -> bool { cmd.eq_ignore_ascii_case("NOTICE") }



pub fn is_join_command(cmd: &str) -> bool { cmd.eq_ignore_ascii_case("JOIN") }



pub fn is_part_command(cmd: &str) -> bool { cmd.eq_ignore_ascii_case("PART") }



pub fn is_quit_command(cmd: &str) -> bool { cmd.eq_ignore_ascii_case("QUIT") }



pub fn is_nick_command(cmd: &str) -> bool { cmd.eq_ignore_ascii_case("NICK") }



pub fn format_ctcp_action(target: &str, text: &str) -> String { let d = 1 as char; format!("PRIVMSG {} :{}ACTION {}{}", target.trim(), d, text, d) }



pub fn is_ctcp_action_text(s: &str) -> bool { s.contains(1 as char) && s.to_ascii_uppercase().contains("ACTION") }



pub fn strip_irc_bold(s: &str) -> String { s.chars().filter(|&c| c != 2 as char).collect() }



pub fn strip_irc_underline(s: &str) -> String { s.chars().filter(|&c| c != 31 as char).collect() }



pub fn strip_irc_reverse(s: &str) -> String { s.chars().filter(|&c| c != 22 as char).collect() }



pub fn strip_irc_italic(s: &str) -> String { s.chars().filter(|&c| c != 29 as char).collect() }



pub fn strip_irc_reset(s: &str) -> String { s.chars().filter(|&c| c != 15 as char).collect() }



pub fn is_irc_formatted(s: &str) -> bool { s.chars().any(|c| matches!(c as u32, 2 | 3 | 15 | 22 | 29 | 31)) }



pub fn parse_oec_numbers(s: &str) -> Option<(u32, u32)> { let start = s.find("(OEC:")?; let rest = &s[start + 5..]; let m_pos = rest.find('M')?; let n_str = &rest[..m_pos]; let after_m = &rest[m_pos + 1..]; let end = after_m.find(')')?; let m_str = &after_m[..end]; let n: u32 = n_str.parse().ok()?; let m: u32 = m_str.parse().ok()?; Some((n, m)) }



pub fn is_oec_message(s: &str) -> bool { s.contains("(OEC:") }



pub fn is_ctcp_message(s: &str) -> bool { s.starts_with(1 as char) && s.ends_with(1 as char) && s.len() >= 2 }



pub fn strip_ctcp(s: &str) -> String { s.trim_matches(1 as char).to_string() }



pub fn parse_ctcp_command(s: &str) -> String { if !is_ctcp_message(s) { return "".to_string(); } let inner = s.trim_matches(1 as char); inner.split_whitespace().next().unwrap_or("").to_ascii_uppercase() }



pub fn is_action_message(s: &str) -> bool { if !is_ctcp_message(s) { return false; } let inner = s.trim_matches(1 as char); inner.starts_with("ACTION") }



pub fn parse_action_text(s: &str) -> String { if !is_action_message(s) { return "".to_string(); } let inner = s.trim_matches(1 as char); let after = inner.strip_prefix("ACTION").unwrap_or(&inner); after.trim_start().to_string() }



pub fn is_ping_message(s: &str) -> bool { s.trim_start().to_ascii_uppercase().starts_with("PING") }



pub fn is_pong_message(s: &str) -> bool { s.trim_start().to_ascii_uppercase().starts_with("PONG") }



pub fn parse_ping_token(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); if parts.len() < 2 { return "".to_string(); } parts[1].trim_start_matches(':').to_string() }



pub fn is_numeric_message(s: &str) -> bool { if let Some(first) = s.split_whitespace().next() { return first.len() == 3 && first.chars().all(|c| c.is_ascii_digit()); } false }



pub fn parse_numeric_code(s: &str) -> Option<u32> { if !is_numeric_message(s) { return None; } let first = s.split_whitespace().next()?; first.parse::<u32>().ok() }



pub fn parse_privmsg_target(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); if parts.len() >= 3 && parts[1] == "PRIVMSG" { return parts[2].to_string(); } "".to_string() }



pub fn parse_privmsg_text(s: &str) -> String { if let Some(pos) = s.find(" :") { return s[pos + 2..].to_string(); } "".to_string() }



pub fn is_join_message(s: &str) -> bool { s.contains(" JOIN ") }



pub fn parse_join_channel(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "JOIN" && i + 1 < parts.len() { return parts[i + 1].trim_start_matches(':').to_string(); } } "".to_string() }



pub fn is_part_message(s: &str) -> bool { s.contains(" PART ") }



pub fn parse_part_channel(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "PART" && i + 1 < parts.len() { return parts[i + 1].trim_start_matches(':').to_string(); } } "".to_string() }



pub fn is_quit_message(s: &str) -> bool { s.contains(" QUIT ") || s.contains(" QUIT:") }



pub fn parse_quit_reason(s: &str) -> String { if let Some(pos) = s.find("QUIT :") { return s[pos + 6..].to_string(); } if let Some(pos) = s.find("QUIT ") { return s[pos + 5..].trim_start_matches(':').to_string(); } "".to_string() }



pub fn is_nick_message(s: &str) -> bool { s.contains(" NICK ") }



pub fn parse_new_nick(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "NICK" && i + 1 < parts.len() { return parts[i + 1].trim_start_matches(':').to_string(); } } "".to_string() }



pub fn is_topic_message(s: &str) -> bool { s.contains(" TOPIC ") || s.contains(" 332 ") }



pub fn parse_notice_target(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); if parts.len() >= 3 && parts[1] == "NOTICE" { return parts[2].to_string(); } "".to_string() }



pub fn parse_notice_text(s: &str) -> String { if let Some(pos) = s.find(" :") { return s[pos + 2..].to_string(); } "".to_string() }



pub fn is_mode_message(s: &str) -> bool { s.contains(" MODE ") }



pub fn parse_mode_target(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "MODE" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } "".to_string() }



pub fn parse_mode_flags(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "MODE" && i + 2 < parts.len() { return parts[i + 2].to_string(); } } "".to_string() }



pub fn is_kick_message(s: &str) -> bool { s.contains(" KICK ") }



pub fn parse_kick_channel(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "KICK" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } "".to_string() }



pub fn parse_kick_nick(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "KICK" && i + 2 < parts.len() { return parts[i + 2].to_string(); } } "".to_string() }



pub fn is_invite_message(s: &str) -> bool { s.contains(" INVITE ") }



pub fn parse_invite_nick(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "INVITE" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } "".to_string() }



pub fn parse_invite_channel(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "INVITE" && i + 2 < parts.len() { return parts[i + 2].trim_start_matches(':').to_string(); } } "".to_string() }



pub fn parse_nick_from_prefix(s: &str) -> String { if !s.starts_with(':') { return "".to_string(); } let rest = &s[1..]; let prefix = rest.split_whitespace().next().unwrap_or(""); if let Some(bang) = prefix.find('!') { return prefix[..bang].to_string(); } if let Some(at) = prefix.find('@') { return prefix[..at].to_string(); } prefix.to_string() }



pub fn parse_user_from_prefix(s: &str) -> String { if !s.starts_with(':') { return "".to_string(); } let rest = &s[1..]; let prefix = rest.split_whitespace().next().unwrap_or(""); if let Some(bang) = prefix.find('!') { let after = &prefix[bang + 1..]; if let Some(at) = after.find('@') { return after[..at].to_string(); } return after.to_string(); } "".to_string() }



pub fn parse_host_from_prefix(s: &str) -> String { if !s.starts_with(':') { return "".to_string(); } let rest = &s[1..]; let prefix = rest.split_whitespace().next().unwrap_or(""); if let Some(at) = prefix.find('@') { return prefix[at + 1..].to_string(); } "".to_string() }



pub fn is_server_message(s: &str) -> bool { if !s.starts_with(':') { return false; } let rest = &s[1..]; if let Some(prefix) = rest.split_whitespace().next() { return !prefix.contains('!'); } false }



pub fn is_error_message(s: &str) -> bool { s.contains(" ERROR ") || s.trim_start().starts_with("ERROR") }



pub fn parse_error_text(s: &str) -> String { if let Some(pos) = s.find("ERROR :") { return s[pos + 7..].to_string(); } if let Some(pos) = s.find("ERROR ") { return s[pos + 6..].trim_start_matches(':').to_string(); } "".to_string() }



pub fn is_welcome_message(s: &str) -> bool { s.contains(" 001 ") }



pub fn is_names_message(s: &str) -> bool { s.contains(" 353 ") }



pub fn parse_names_list(s: &str) -> Vec<String> { if let Some(pos) = s.find(" :") { return s[pos + 2..].split_whitespace().map(|w| w.to_string()).collect(); } Vec::new() }



pub fn is_who_message(s: &str) -> bool { s.contains(" 352 ") }



pub fn is_ban_message(s: &str) -> bool { s.contains(" MODE ") && s.contains("+b") }



pub fn is_op_message(s: &str) -> bool { s.contains(" MODE ") && s.contains("+o") }



pub fn parse_quit_text(s: &str) -> String { if let Some(pos) = s.find(" :") { return s[pos + 2..].to_string(); } "".to_string() }



pub fn parse_topic_channel(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "TOPIC" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } "".to_string() }



pub fn parse_topic_text(s: &str) -> String { if let Some(pos) = s.find(" :") { return s[pos + 2..].to_string(); } "".to_string() }



pub fn irc_is_ctcp(s: &str) -> bool { s.contains(char::from(1)) }



pub fn irc_parse_ctcp_command(s: &str) -> String { if let Some(start) = s.find(char::from(1)) { let rest = &s[start + 1..]; let end = rest.find(|c| c == ' ' || c == char::from(1)).unwrap_or(rest.len()); return rest[..end].to_string(); } "".to_string() }



pub fn irc_parse_ctcp_args(s: &str) -> String { if let Some(start) = s.find(char::from(1)) { let rest = &s[start + 1..]; if let Some(sp) = rest.find(' ') { let after = &rest[sp + 1..]; let end = after.find(char::from(1)).unwrap_or(after.len()); return after[..end].to_string(); } } "".to_string() }



pub fn irc_is_action(s: &str) -> bool { s.contains("ACTION") && s.contains(char::from(1)) }



pub fn irc_parse_action_text(s: &str) -> String { if let Some(pos) = s.find("ACTION ") { let rest = &s[pos + 7..]; let end = rest.find(char::from(1)).unwrap_or(rest.len()); return rest[..end].to_string(); } "".to_string() }



pub fn irc_is_privmsg(s: &str) -> bool { s.contains(" PRIVMSG ") }



pub fn irc_parse_privmsg_target(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "PRIVMSG" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } "".to_string() }



pub fn irc_parse_privmsg_text(s: &str) -> String { if let Some(pos) = s.find(" :") { return s[pos + 2..].to_string(); } "".to_string() }



pub fn irc_is_pong(s: &str) -> bool { s.trim_start().starts_with("PONG") }



pub fn irc_parse_pong_token(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); if parts.len() >= 2 { return parts[1].trim_start_matches(':').to_string(); } "".to_string() }



pub fn irc_is_whois(s: &str) -> bool { s.contains(" WHOIS ") }



pub fn irc_parse_whois_nick(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "WHOIS" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } "".to_string() }



pub fn irc_is_away(s: &str) -> bool { s.contains(" AWAY ") }



pub fn irc_parse_away_text(s: &str) -> String { if let Some(pos) = s.find(" :") { return s[pos + 2..].to_string(); } "".to_string() }



pub fn irc_is_back(s: &str) -> bool { s.contains(" BACK ") }



pub fn irc_is_list(s: &str) -> bool { s.contains(" LIST ") }



pub fn irc_parse_list_channel(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "LIST" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } "".to_string() }



pub fn irc_is_end_of_names(s: &str) -> bool { s.contains(" 366 ") }



pub fn irc_is_motd(s: &str) -> bool { s.contains(" 372 ") }



pub fn irc_parse_motd_text(s: &str) -> String { if let Some(pos) = s.find(" :") { return s[pos + 2..].to_string(); } "".to_string() }



pub fn irc_is_luser(s: &str) -> bool { s.contains(" 251 ") }



pub fn irc_is_version_msg(s: &str) -> bool { s.contains(" VERSION ") }



pub fn irc_parse_version_nick(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "VERSION" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } "".to_string() }



pub fn irc_is_time_msg(s: &str) -> bool { s.contains(" TIME ") }



pub fn irc_is_end_of_motd(s: &str) -> bool { s.contains(" 376 ") }



pub fn irc_is_no_motd(s: &str) -> bool { s.contains(" 422 ") }



pub fn irc_is_topic_set(s: &str) -> bool { s.contains(" 333 ") }



pub fn irc_is_welcome(s: &str) -> bool { s.contains(" 001 ") }



pub fn irc_is_your_host(s: &str) -> bool { s.contains(" 002 ") }



pub fn irc_is_created(s: &str) -> bool { s.contains(" 003 ") }



pub fn irc_is_myinfo(s: &str) -> bool { s.contains(" 004 ") }



pub fn irc_is_bounce(s: &str) -> bool { s.contains(" 005 ") }



pub fn irc_parse_welcome_nick(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "001" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_kill(s: &str) -> bool { s.contains(" KILL ") }



pub fn irc_parse_kill_nick(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "KILL" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_error_msg(s: &str) -> bool { s.contains(" ERROR ") }



pub fn irc_is_oper(s: &str) -> bool { s.contains(" OPER ") }



pub fn irc_parse_oper_nick(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "OPER" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_squit(s: &str) -> bool { s.contains(" SQUIT ") }



pub fn irc_parse_squit_server(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "SQUIT" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_rehash(s: &str) -> bool { s.contains(" REHASH ") || s.trim_end().ends_with(" REHASH") }



pub fn irc_is_restart(s: &str) -> bool { s.contains(" RESTART ") || s.trim_end().ends_with(" RESTART") }



pub fn irc_is_wallops(s: &str) -> bool { s.contains(" WALLOPS ") }



pub fn irc_parse_wallops_text(s: &str) -> String { if let Some(pos) = s.find(" :") { return s[pos + 2..].to_string(); } String::new() }



pub fn irc_is_globops(s: &str) -> bool { s.contains(" GLOBOPS ") }



pub fn irc_parse_globops_text(s: &str) -> String { if let Some(pos) = s.find(" :") { return s[pos + 2..].to_string(); } String::new() }



pub fn irc_is_admin(s: &str) -> bool { s.contains(" ADMIN ") }



pub fn irc_parse_admin_nick(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "ADMIN" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_stats(s: &str) -> bool { s.contains(" STATS ") }



pub fn irc_parse_stats_letter(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "STATS" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_links(s: &str) -> bool { s.contains(" LINKS ") }



pub fn irc_parse_links_server(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "LINKS" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_trace(s: &str) -> bool { s.contains(" TRACE ") }



pub fn irc_parse_trace_target(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "TRACE" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_users(s: &str) -> bool { s.contains(" USERS ") }



pub fn irc_is_summon(s: &str) -> bool { s.contains(" SUMMON ") }



pub fn irc_is_userhost(s: &str) -> bool { s.contains(" USERHOST ") }



pub fn irc_parse_userhost_nick(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "USERHOST" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_ison(s: &str) -> bool { s.contains(" ISON ") }



pub fn irc_parse_ison_nicks(s: &str) -> String { if let Some(pos) = s.find(" ISON ") { return s[pos + 6..].to_string(); } String::new() }



pub fn irc_is_silence(s: &str) -> bool { s.contains(" SILENCE ") }



pub fn irc_parse_silence_target(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "SILENCE" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_watch(s: &str) -> bool { s.contains(" WATCH ") }



pub fn irc_parse_watch_nick(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "WATCH" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_accept(s: &str) -> bool { s.contains(" ACCEPT ") }



pub fn irc_parse_accept_nick(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "ACCEPT" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_knock(s: &str) -> bool { s.contains(" KNOCK ") }



pub fn irc_parse_knock_channel(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "KNOCK" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_help(s: &str) -> bool { s.contains(" HELP ") }



pub fn irc_parse_help_topic(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "HELP" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_info(s: &str) -> bool { s.contains(" INFO ") }



pub fn irc_parse_info_target(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "INFO" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_squery(s: &str) -> bool { s.contains(" SQUERY ") }



pub fn irc_parse_squery_target(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "SQUERY" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_servlist(s: &str) -> bool { s.contains(" SERVLIST ") }



pub fn irc_parse_servlist_mask(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "SERVLIST" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_parse_rehash_target(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "REHASH" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_parse_restart_target(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "RESTART" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_die(s: &str) -> bool { s.contains(" DIE ") }



pub fn irc_parse_die_target(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "DIE" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_parse_wallops_message(s: &str) -> String { if let Some(pos) = s.find(" :") { return s[pos + 2..].to_string(); } String::new() }



pub fn irc_parse_globops_message(s: &str) -> String { if let Some(pos) = s.find(" :") { return s[pos + 2..].to_string(); } String::new() }



pub fn irc_is_chatops(s: &str) -> bool { s.contains(" CHATOPS ") }



pub fn irc_parse_chatops_message(s: &str) -> String { if let Some(pos) = s.find(" :") { return s[pos + 2..].to_string(); } String::new() }



pub fn irc_is_locops(s: &str) -> bool { s.contains(" LOCOPS ") }



pub fn irc_parse_locops_message(s: &str) -> String { if let Some(pos) = s.find(" :") { return s[pos + 2..].to_string(); } String::new() }



pub fn irc_parse_admin_target(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "ADMIN" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_parse_time_target(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "TIME" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_parse_oper_name(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "OPER" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_pass(s: &str) -> bool { s.contains(" PASS ") }



pub fn irc_parse_pass_value(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "PASS" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_nickserv(s: &str) -> bool { s.contains("NickServ") || s.contains("NICKSERV") }



pub fn irc_parse_nickserv_command(s: &str) -> String { if let Some(pos) = s.find(" :") { let rest = &s[pos + 2..]; let parts: Vec<&str> = rest.split_whitespace().collect(); if !parts.is_empty() { return parts[0].to_string(); } } String::new() }



pub fn irc_is_chanserv(s: &str) -> bool { s.contains("ChanServ") || s.contains("CHANSERV") }



pub fn irc_parse_chanserv_command(s: &str) -> String { if let Some(pos) = s.find(" :") { let rest = &s[pos + 2..]; let parts: Vec<&str> = rest.split_whitespace().collect(); if !parts.is_empty() { return parts[0].to_string(); } } String::new() }



pub fn irc_is_memoserv(s: &str) -> bool { s.contains("MemoServ") || s.contains("MEMOSERV") }



pub fn irc_parse_memoserv_command(s: &str) -> String { if let Some(pos) = s.find(" :") { let rest = &s[pos + 2..]; let parts: Vec<&str> = rest.split_whitespace().collect(); if !parts.is_empty() { return parts[0].to_string(); } } String::new() }



pub fn irc_is_hostserv(s: &str) -> bool { s.contains("HostServ") || s.contains("HOSTSERV") }



pub fn irc_parse_hostserv_command(s: &str) -> String { if let Some(pos) = s.find(" :") { let rest = &s[pos + 2..]; let parts: Vec<&str> = rest.split_whitespace().collect(); if !parts.is_empty() { return parts[0].to_string(); } } String::new() }



pub fn irc_parse_user_name(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "USER" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_server(s: &str) -> bool { s.contains(" SERVER ") }



pub fn irc_parse_server_name(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "SERVER" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_connect(s: &str) -> bool { s.contains(" CONNECT ") }



pub fn irc_parse_connect_target(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "CONNECT" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_disconnect(s: &str) -> bool { s.contains(" DISCONNECT ") }



pub fn irc_parse_disconnect_target(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "DISCONNECT" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_reconnect(s: &str) -> bool { s.contains(" RECONNECT ") }



pub fn irc_parse_reconnect_target(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "RECONNECT" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_parse_away_message(s: &str) -> String { if let Some(pos) = s.find(" :") { return s[pos + 2..].to_string(); } String::new() }



pub fn irc_parse_pong_server(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "PONG" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_cap(s: &str) -> bool { s.contains(" CAP ") }



pub fn irc_parse_cap_subcommand(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "CAP" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_authenticate(s: &str) -> bool { s.contains(" AUTHENTICATE ") }



pub fn irc_parse_authenticate_data(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "AUTHENTICATE" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_sasl(s: &str) -> bool { s.contains(" SASL ") }



pub fn irc_parse_sasl_mechanism(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "SASL" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_certfp(s: &str) -> bool { s.contains(" CERTFP ") || s.contains("certfp") }



pub fn irc_parse_certfp_fingerprint(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "CERTFP" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_account(s: &str) -> bool { s.contains(" ACCOUNT ") }



pub fn irc_parse_account_name(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "ACCOUNT" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_parse_who_target(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "WHO" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_parse_whois_target(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "WHOIS" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_whowas(s: &str) -> bool { s.contains(" WHOWAS ") }



pub fn irc_parse_whowas_target(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "WHOWAS" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_names(s: &str) -> bool { s.contains(" NAMES ") }



pub fn irc_parse_names_channel(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "NAMES" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_parse_topic_channel(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "TOPIC" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_invite(s: &str) -> bool { s.contains(" INVITE ") }



pub fn irc_parse_invite_nick(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "INVITE" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_kick(s: &str) -> bool { s.contains(" KICK ") }



pub fn irc_parse_kick_nick(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "KICK" && i + 2 < parts.len() { return parts[i + 2].to_string(); } } String::new() }



pub fn irc_is_ban(s: &str) -> bool { s.contains(" BAN ") }



pub fn irc_parse_ban_mask(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "BAN" && i + 2 < parts.len() { return parts[i + 2].to_string(); } } String::new() }



pub fn irc_is_unban(s: &str) -> bool { s.contains(" UNBAN ") }



pub fn irc_parse_unban_mask(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "UNBAN" && i + 2 < parts.len() { return parts[i + 2].to_string(); } } String::new() }



pub fn irc_is_mode(s: &str) -> bool { s.contains(" MODE ") }



pub fn irc_parse_mode_target(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "MODE" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_time(s: &str) -> bool { s.contains(" TIME ") }



pub fn irc_parse_time_server(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "TIME" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_version(s: &str) -> bool { s.contains(" VERSION ") }



pub fn irc_parse_version_server(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "VERSION" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_parse_admin_server(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "ADMIN" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_parse_info_server(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "INFO" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_parse_motd_server(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "MOTD" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_lusers(s: &str) -> bool { s.contains(" LUSERS ") }



pub fn irc_parse_lusers_server(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "LUSERS" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_parse_trace_server(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "TRACE" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_parse_silence_mask(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "SILENCE" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_unwatch(s: &str) -> bool { s.contains(" UNWATCH ") }



pub fn irc_parse_unwatch_nick(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "UNWATCH" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_chghost(s: &str) -> bool { s.contains(" CHGHOST ") }



pub fn irc_parse_chghost_ident(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "CHGHOST" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_chgname(s: &str) -> bool { s.contains(" CHGNAME ") }



pub fn irc_parse_chgname_nick(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "CHGNAME" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_setname(s: &str) -> bool { s.contains(" SETNAME ") }



pub fn irc_parse_setname_name(s: &str) -> String { if let Some(pos) = s.find(" :") { return s[pos + 2..].to_string(); } String::new() }



pub fn irc_is_chgident(s: &str) -> bool { s.contains(" CHGIDENT ") }



pub fn irc_parse_chgident_ident(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "CHGIDENT" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_chgswhois(s: &str) -> bool { s.contains(" CHGSWHOIS ") }



pub fn irc_parse_chgswhois_nick(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "CHGSWHOIS" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_svsjoin(s: &str) -> bool { s.contains(" SVSJOIN ") }



pub fn irc_parse_svsjoin_channel(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "SVSJOIN" && i + 2 < parts.len() { return parts[i + 2].to_string(); } } String::new() }



pub fn irc_is_svspart(s: &str) -> bool { s.contains(" SVSPART ") }



pub fn irc_parse_svspart_channel(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "SVSPART" && i + 2 < parts.len() { return parts[i + 2].to_string(); } } String::new() }



pub fn irc_is_svsmode(s: &str) -> bool { s.contains(" SVSMODE ") }



pub fn irc_parse_svsmode_nick(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "SVSMODE" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_svsnick(s: &str) -> bool { s.contains(" SVSNICK ") }



pub fn irc_parse_svsnick_nick(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "SVSNICK" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_svsnoop(s: &str) -> bool { s.contains(" SVSNOOP ") }



pub fn irc_parse_svsnoop_nick(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "SVSNOOP" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_svskill(s: &str) -> bool { s.contains(" SVSKILL ") }



pub fn irc_parse_svskill_nick(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "SVSKILL" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_gline(s: &str) -> bool { s.contains(" GLINE ") }



pub fn irc_parse_gline_mask(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "GLINE" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_ungline(s: &str) -> bool { s.contains(" UNGLINE ") }



pub fn irc_parse_ungline_mask(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "UNGLINE" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_kline(s: &str) -> bool { s.contains(" KLINE ") }



pub fn irc_parse_kline_mask(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "KLINE" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_unkline(s: &str) -> bool { s.contains(" UNKLINE ") }



pub fn irc_parse_unkline_mask(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "UNKLINE" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_zline(s: &str) -> bool { s.contains(" ZLINE ") }



pub fn irc_parse_zline_mask(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "ZLINE" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_unzline(s: &str) -> bool { s.contains(" UNZLINE ") }



pub fn irc_parse_unzline_mask(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "UNZLINE" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_shun(s: &str) -> bool { s.contains(" SHUN ") }



pub fn irc_parse_shun_mask(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "SHUN" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_unshun(s: &str) -> bool { s.contains(" UNSHUN ") }



pub fn irc_parse_unshun_mask(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "UNSHUN" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_sajoin(s: &str) -> bool { s.contains(" SAJOIN ") }



pub fn irc_parse_sajoin_channel(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "SAJOIN" && i + 2 < parts.len() { return parts[i + 2].to_string(); } } String::new() }



pub fn irc_is_sapart(s: &str) -> bool { s.contains(" SAPART ") }



pub fn irc_parse_sapart_channel(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "SAPART" && i + 2 < parts.len() { return parts[i + 2].to_string(); } } String::new() }



pub fn irc_is_samode(s: &str) -> bool { s.contains(" SAMODE ") }



pub fn irc_parse_samode_nick(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "SAMODE" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_sanick(s: &str) -> bool { s.contains(" SANICK ") }



pub fn irc_parse_sanick_nick(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "SANICK" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_sakick(s: &str) -> bool { s.contains(" SAKICK ") }



pub fn irc_parse_sakick_nick(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "SAKICK" && i + 2 < parts.len() { return parts[i + 2].to_string(); } } String::new() }



pub fn irc_parse_connect_server(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "CONNECT" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_map(s: &str) -> bool { s.contains(" MAP ") }



pub fn irc_parse_map_server(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "MAP" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_deoper(s: &str) -> bool { s.contains(" DEOPER ") }



pub fn irc_parse_deoper_nick(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "DEOPER" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_parse_locops_text(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "LOCOPS" && i + 1 < parts.len() { return parts[i + 1..].join(" ").trim_start_matches(':').to_string(); } } String::new() }



pub fn irc_parse_chatops_text(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "CHATOPS" && i + 1 < parts.len() { return parts[i + 1..].join(" ").trim_start_matches(':').to_string(); } } String::new() }



pub fn irc_is_unsilence(s: &str) -> bool { s.contains(" UNSILENCE ") }



pub fn irc_parse_unsilence_mask(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "UNSILENCE" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_parse_whowas_nick(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "WHOWAS" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_parse_users_nick(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if *p == "USERS" && i + 1 < parts.len() { return parts[i + 1].to_string(); } } String::new() }



pub fn irc_is_dcc(s: &str) -> bool { s.contains("DCC ") }



pub fn irc_parse_dcc_type(s: &str) -> String { let parts: Vec<&str> = s.split_whitespace().collect(); for (i, p) in parts.iter().enumerate() { if p.contains("DCC") && i + 1 < parts.len() { return parts[i + 1].trim_matches(char::from(1)).to_string(); } } String::new() }



pub fn irc_is_ctcp_version(s: &str) -> bool { s.contains("VERSION") }



pub fn irc_parse_ctcp_version_text(s: &str) -> String { if let Some(pos) = s.find("VERSION") { let rest = &s[pos + 7..]; return rest.trim().trim_matches(char::from(1)).trim_start_matches(':').trim().to_string(); } String::new() }



pub fn irc_is_ctcp_time(s: &str) -> bool { s.contains("TIME") }



pub fn irc_parse_ctcp_time_text(s: &str) -> String { if let Some(pos) = s.find("TIME") { let rest = &s[pos + 4..]; return rest.trim().trim_matches(char::from(1)).trim_start_matches(':').trim().to_string(); } String::new() }



pub fn irc_is_ctcp_ping(s: &str) -> bool { s.contains("PING") && s.contains(char::from(1)) }



pub fn irc_parse_ctcp_ping_text(s: &str) -> String { if let Some(pos) = s.find("PING") { let rest = &s[pos + 4..]; return rest.trim().trim_matches(char::from(1)).trim_start_matches(':').trim().to_string(); } String::new() }



pub fn irc_is_001(s: &str) -> bool { s.contains(" 001 ") }



pub fn irc_parse_001_text(s: &str) -> String { if let Some(pos) = s.find(" 001 ") { let rest = &s[pos + 5..]; if let Some(cpos) = rest.find(" :") { return rest[cpos + 2..].trim().to_string(); } return rest.trim().to_string(); } String::new() }



pub fn irc_is_002(s: &str) -> bool { s.contains(" 002 ") }



pub fn irc_parse_002_text(s: &str) -> String { if let Some(pos) = s.find(" 002 ") { let rest = &s[pos + 5..]; if let Some(cpos) = rest.find(" :") { return rest[cpos + 2..].trim().to_string(); } return rest.trim().to_string(); } String::new() }



pub fn irc_is_003(s: &str) -> bool { s.contains(" 003 ") }



pub fn irc_parse_003_text(s: &str) -> String { if let Some(pos) = s.find(" 003 ") { let rest = &s[pos + 5..]; if let Some(cpos) = rest.find(" :") { return rest[cpos + 2..].trim().to_string(); } return rest.trim().to_string(); } String::new() }



pub fn irc_is_004(s: &str) -> bool { s.contains(" 004 ") }



pub fn irc_parse_004_text(s: &str) -> String { if let Some(pos) = s.find(" 004 ") { let rest = &s[pos + 5..]; if let Some(cpos) = rest.find(" :") { return rest[cpos + 2..].trim().to_string(); } let rest = rest.trim(); if let Some(sp) = rest.find(' ') { return rest[sp + 1..].trim().to_string(); } return rest.to_string(); } String::new() }



pub fn irc_is_005(s: &str) -> bool { s.contains(" 005 ") }



pub fn irc_parse_005_text(s: &str) -> String { if let Some(pos) = s.find(" 005 ") { let rest = &s[pos + 5..]; if let Some(cpos) = rest.find(" :") { return rest[cpos + 2..].trim().to_string(); } return rest.trim().to_string(); } String::new() }



pub fn irc_is_433(s: &str) -> bool { s.contains(" 433 ") }



pub fn irc_parse_433_text(s: &str) -> String { if let Some(pos) = s.find(" 433 ") { let rest = &s[pos + 5..]; if let Some(cpos) = rest.find(" :") { return rest[cpos + 2..].trim().to_string(); } return rest.trim().to_string(); } String::new() }


