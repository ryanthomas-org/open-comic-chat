mod about;
mod g_pose_index;
pub mod version;
mod app;
mod icons;
mod avb;
mod log;
mod dib;
mod comic;
mod config;
mod irc;
mod pose;

use app::ComicChatApp;
use config::{default_config_path, load_or_create, ServerConfig};
use irc::{run_server, IrcCommand, IrcEvent};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

pub type HandleMap = Arc<Mutex<HashMap<String, mpsc::UnboundedSender<IrcCommand>>>>;

/// This project's version. Single source of truth: CTCP VERSION, the
/// window title, and the startup log all read from here, so the reply
/// can never drift from the release number.
/// Version line: 0.3.x — do not bump to 0.4 / 1.0. Patch increments only (0.3.17, 0.3.17, ...).
pub const VERSION: &str = "0.3.17";

/// CTCP VERSION reply: exactly `Open Comic Chat (Version 0.3.17)`.
/// See irc.rs — a direct CTCP VERSION query gets a NOTICE with this.
/// CTCP VERSION reply. The numerical version is concatenated here
/// from `VERSION` (the crate version), so the reply can never drift
/// from the release number. `override_text`, when set in config
/// (`comic.ctcp_version`), replaces the whole string — use it to
/// identify as another client, or to hide the version.
pub fn version_reply_with(override_text: Option<&str>) -> String {
    match override_text {
        Some(v) if !v.trim().is_empty() => v.to_string(),
        _ => format!("Open Comic Chat (Version {VERSION})"),
    }
}

// FIX: GUI Save used to be fire-and-forget + blocked by `spawned` HashSet, so editing
// a server never reconnected until restart. Control allows Re-connect of the SAME name.
#[derive(Debug, Clone, PartialEq)]
pub enum Control { Connect(ServerConfig), Disconnect(String) }
pub type ControlTx = mpsc::UnboundedSender<Control>;

fn main() -> eframe::Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("tokio runtime");
    let rt_handle = rt.handle().clone();
    let cfg = load_or_create(&default_config_path());
    // Debug log: off if `debug_log` is absent in config.lua. Init here, before
    // any IRC / GUI work, so all later logs are captured. If disabled, init
    // creates no file and every log call is a no-op.
    crate::log::init(cfg.debug_log.enabled, cfg.debug_log.file.clone(), &cfg.debug_log.level);
    crate::log::info("main", format!("Comic Chat RS {VERSION} starting, config={} me.nick={} servers={} character={} background={} debug_log enabled={} level={} file={:?}",
        cfg.path.display(), cfg.me.nick, cfg.servers.len(), cfg.comic.character, cfg.comic.background, cfg.debug_log.enabled, cfg.debug_log.level, cfg.debug_log.file));
    if !cfg.debug_log.enabled {
        // No file is created when disabled — this eprintln is the only hint.
        // (Do not log to file here: that would create the file we promised not to.)
        eprintln!("[comic-chat] debug log is off (debug_log absent or enabled=false in config.lua). Add `debug_log = {{ enabled = true, level = \"debug\" }}` to enable it.");
    }
    let me_nick = cfg.me.nick.clone();
    let (event_tx, event_rx) = mpsc::unbounded_channel::<IrcEvent>();
    let (control_tx, mut control_rx) = mpsc::unbounded_channel::<Control>();
    let handles: HandleMap = Arc::new(Mutex::new(HashMap::new()));
    let handles_rt = handles.clone();
    let event_tx_rt = event_tx.clone();
    let me_rt = me_nick.clone();
    let cfg_rt = cfg.clone();
    rt_handle.spawn(async move {
        let mut tasks: HashMap<String, tokio::task::JoinHandle<()>> = HashMap::new();
        let spawn = |tasks: &mut HashMap<String, tokio::task::JoinHandle<()>>, handles: &HandleMap, s: ServerConfig, me: String, ev: mpsc::UnboundedSender<IrcEvent>| {
            if let Some(old) = tasks.remove(&s.name) { old.abort(); }
            if let Ok(mut m) = handles.lock() { m.remove(&s.name); }
            let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
            if let Ok(mut m) = handles.lock() { m.insert(s.name.clone(), cmd_tx); }
            let ctcp_version = cfg_rt.comic.ctcp_version.clone();
            let h = tokio::spawn(run_server(s.clone(), me, cmd_rx, ev, ctcp_version));
            tasks.insert(s.name.clone(), h);
        };
        for s in &cfg_rt.servers {
            if s.autoconnect {
                crate::log::info("main", format!("autoconnect: starting server {} host={} port={} tls={} channels={:?}", s.name, s.host, s.effective_port(), s.tls, s.channels));
                spawn(&mut tasks, &handles_rt, s.clone(), me_rt.clone(), event_tx_rt.clone());
            } else {
                crate::log::debug("main", format!("autoconnect: skipping server {} (autoconnect=false)", s.name));
            }
        }
        while let Some(cmd) = control_rx.recv().await {
            match cmd {
                Control::Connect(s) => {
                    crate::log::info("main", format!("control: Connect requested for server {} host={} port={} tls={}", s.name, s.host, s.effective_port(), s.tls));
                    spawn(&mut tasks, &handles_rt, s, me_rt.clone(), event_tx_rt.clone());
                }
                Control::Disconnect(name) => {
                    if let Some(old) = tasks.remove(&name) { old.abort(); }
                    if let Ok(mut m) = handles_rt.lock() { m.remove(&name); }
                }
            }
        }
    });
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1180.0_f32, 760.0_f32]).with_title(format!("Comic Chat RS {VERSION}")),
        ..Default::default()
    };
    let result = eframe::run_native(
        "Comic Chat RS",
        options,
        Box::new(move |cc| {
            // Comic Sans first: the original client drew everything in it
            // (it shipped the font). ui.font / ui.font_path in config.lua.
            app::install_font(&cc.egui_ctx, &cfg.ui);
            Box::new(ComicChatApp::new(cfg, event_tx, event_rx, handles, control_tx)) as Box<dyn eframe::App>
        }),
    );
    drop(rt);
    result
}

