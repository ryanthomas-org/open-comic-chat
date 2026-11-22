//! Panel grouping, title card, cast, layout.
//! Codifies panel.cpp/.h, chatdoc.cpp, histent.cpp/.h, pageview.cpp,
//! and the panel logic in comic-chat-rs 0.3.17 src/comic.rs + src/app.rs.
//!
//! Panel model (authoritative):
//! * A panel is a horizontal strip. It holds 1..N entries (messages /
//!   actions / joins / parts). A panel is "full" when the next entry
//!   would exceed the panel width or the max-entries rule — see
//!   `PanelLayout::is_full`. The original did NOT put one message per
//!   panel; it packed until full, then started a new panel.
//! * The title card is a panel, not a banner: it appears ONLY in the
//!   very first frame when a room is joined, is static like other comic
//!   panes (it does not float / stay pinned), and scrolls into history
//!   with the rest. 0.3.17 at one point rendered it as a pinned header
//!   — wrong. `TitleCard` + `should_show_title_card` encode the rule.
//! * Establishing / establishing-shot: the first panel after joining
//!   may include a wide background + cast lineup. See `is_establishing`.
//! * Cast: the ordered list of avatars present in the room, in join
//!   order. The panel layout places avatars left-to-right in cast order
//!   for the speakers in that panel, not in message order.
//! * Zoom: panel layout is computed at zoom 1.0 then scaled. `zoom`
//!   multiplies every px value. Layout must be zoom-independent in
//!   structure (same grouping at any zoom) — grouping uses unscaled
//!   widths, then scales for display.
//! * Scroll: panels are appended; the view scrolls to the bottom on
//!   new panel unless the user has scrolled up (then it stays).
//!   `ScrollState` encodes that rule.

/// One entry inside a panel.
#[derive(Debug, Clone, PartialEq)]
pub struct PanelEntry {
    pub speaker: String,
    /// Avatar name (from `Appears as`), empty = speaker's default.
    pub avatar: String,
    /// Balloon style as a string: "speech"|"thought"|"shout"|"whisper"|"narrative".
    pub style: String,
    /// Measured text width at zoom 1.0 (px), including padding.
    pub width: f64,
    /// Measured height at zoom 1.0 (px).
    pub height: f64,
    /// True for joins/parts/system lines (narrative, no balloon tail).
    pub is_system: bool,
}

/// Layout constants. Panel width 640 at zoom 1.0 in the original
/// (the 0.3.17 app.rs used 640 as the reference width).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelLayout {
    pub panel_width: f64,
    pub panel_gap: f64,
    pub entry_gap: f64,
    pub max_entries: usize,
    pub max_panel_height: f64,
}
impl Default for PanelLayout {
    fn default() -> Self { Self { panel_width: 640.0, panel_gap: 8.0, entry_gap: 12.0, max_entries: 4, max_panel_height: 480.0 } }
}
impl PanelLayout {
    /// Would adding `entry` to a panel that already holds `current`
    /// exceed width / count / height? Uses unscaled widths.
    pub fn is_full(&self, current: &[PanelEntry], entry: &PanelEntry) -> bool {
        if current.len() >= self.max_entries { return true; }
        let used: f64 = current.iter().map(|e| e.width).sum::<f64>() + self.entry_gap * current.len().saturating_sub(1) as f64;
        if used + self.entry_gap + entry.width > self.panel_width { return true; }
        let h = current.iter().map(|e| e.height).fold(0.0f64, f64::max).max(entry.height);
        if h > self.max_panel_height { return true; }
        false
    }
}

/// Group a stream of entries into panels, in order.
/// System entries (joins/parts) do NOT force a new panel on their own;
/// they pack like any entry, except a join that starts a new cast
/// member's first appearance may start a new panel if `break_on_join`
/// is set (the original did this for the establishing shot only).
pub fn group_into_panels(entries: &[PanelEntry], layout: &PanelLayout, break_on_join: bool) -> Vec<Vec<PanelEntry>> {
    let mut panels: Vec<Vec<PanelEntry>> = Vec::new();
    let mut cur: Vec<PanelEntry> = Vec::new();
    for e in entries {
        if !cur.is_empty() && layout.is_full(&cur, e) { panels.push(std::mem::take(&mut cur)); }
        if break_on_join && e.is_system && !cur.is_empty() {
            // Join/leave after at least one entry: start fresh panel so the
            // new cast member gets a clean establishing position.
            panels.push(std::mem::take(&mut cur));
        }
        cur.push(e.clone());
    }
    if !cur.is_empty() { panels.push(cur); }
    panels
}

/// Title card: room name + topic + member list, rendered as the first panel.
/// Appears ONLY when `is_first_frame_after_join` is true. Never re-appears
/// on scroll, resize, or reconnect to the same room (reconnect = new join
/// only if the client actually left the room).
#[derive(Debug, Clone, PartialEq)]
pub struct TitleCard { pub room: String, pub topic: String, pub members: Vec<String> }
pub fn should_show_title_card(is_first_frame_after_join: bool, already_shown: bool) -> bool {
    is_first_frame_after_join && !already_shown
}
pub fn title_card_panel(card: &TitleCard) -> PanelEntry {
    PanelEntry { speaker: card.room.clone(), avatar: String::new(), style: "narrative".to_string(), width: 600.0, height: 120.0, is_system: true }
}

/// Establishing shot: first panel after join, wide background + cast.
/// True only for the first panel of a newly joined room.
pub fn is_establishing(panel_index: usize, joined_this_session: bool) -> bool {
    panel_index == 0 && joined_this_session
}

/// Cast: ordered list of (nick, avatar) in join order.
#[derive(Debug, Clone, Default)]
pub struct Cast { pub members: Vec<(String, String)> }
impl Cast {
    pub fn join(&mut self, nick: &str, avatar: &str) { if !self.members.iter().any(|(n,_)| n == nick) { self.members.push((nick.to_string(), avatar.to_string())); } }
    pub fn part(&mut self, nick: &str) { self.members.retain(|(n,_)| n != nick); }
    pub fn set_avatar(&mut self, nick: &str, avatar: &str) { if let Some(m) = self.members.iter_mut().find(|(n,_)| n == nick) { m.1 = avatar.to_string(); } }
    /// Avatar x-positions for the speakers in a panel, left-to-right in
    /// cast order (not message order). Returns (nick, x) with x centred
    /// in that speaker's slot.
    pub fn avatar_positions(&self, speakers: &[String], panel_width: f64) -> Vec<(String, f64)> {
        let mut ordered: Vec<String> = Vec::new();
        for (n, _) in &self.members { if speakers.contains(n) && !ordered.contains(n) { ordered.push(n.clone()); } }
        for s in speakers { if !ordered.contains(s) { ordered.push(s.clone()); } }
        if ordered.is_empty() { return vec![]; }
        let slot = panel_width / ordered.len() as f64;
        ordered.iter().enumerate().map(|(i, n)| (n.clone(), slot * (i as f64 + 0.5))).collect()
    }
}

/// Scroll rule: auto-scroll to bottom on new panel UNLESS user scrolled up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollState { pub user_scrolled_up: bool }
impl ScrollState {
    pub fn should_autoscroll(&self) -> bool { !self.user_scrolled_up }
    /// Call when the user scrolls: if they are at (or near) the bottom,
    /// clear the flag so autoscroll resumes.
    pub fn on_user_scroll(&mut self, at_bottom: bool) { self.user_scrolled_up = !at_bottom; }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(speaker: &str, w: f64) -> PanelEntry { PanelEntry { speaker: speaker.into(), avatar: String::new(), style: "speech".into(), width: w, height: 60.0, is_system: false } }
    #[test] fn packs_until_full() { let l = PanelLayout::default(); let es = vec![entry("a", 300.0), entry("b", 300.0), entry("c", 300.0)]; let ps = group_into_panels(&es, &l, false); assert_eq!(ps.len(), 2); assert_eq!(ps[0].len(), 2); }
    #[test] fn title_card_only_first_frame() { assert!(should_show_title_card(true, false)); assert!(!should_show_title_card(true, true)); assert!(!should_show_title_card(false, false)); }
    #[test] fn cast_order_not_message_order() { let mut c = Cast::default(); c.join("bob", "Bob"); c.join("alice", "Alice"); let pos = c.avatar_positions(&vec!["alice".into(), "bob".into()], 640.0); assert_eq!(pos[0].0, "bob"); assert_eq!(pos[1].0, "alice"); }
    #[test] fn max_entries() { let l = PanelLayout { max_entries: 2, ..Default::default() }; let es = vec![entry("a", 10.0), entry("b", 10.0), entry("c", 10.0)]; assert_eq!(group_into_panels(&es, &l, false).len(), 2); }
}
