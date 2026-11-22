# comic-chat-rs — GOALS.md

<!-- STAY-ON-TRACK -->
> **Stay on track:** Read this file + `COMIC_CHAT_SPEC.md` first, before coding.
> If code and docs disagree, stop. Fix docs + code together, do not silently drift.
> After every change, update both files: this file's status + the spec if a rule changed.
> This file is the goal list. `COMIC_CHAT_SPEC.md` is the wire / rendering truth.
> When they conflict, the Microsoft source wins, then the spec, then this file.
<!-- /STAY-ON-TRACK -->

## Goal

Reimplement Microsoft Comic Chat 2.5 in Rust + egui, in `comic-chat-rs`, to look and
operate as much like the original as possible. Pixel-perfect AVB decoding and panel
rendering before new features.

## Frozen constraints

- **Version frozen at 0.3.17.** Keep version references consistent across `Cargo.toml`
  and `src/version.rs`. Bump only when floored by an improvement and with explicit consent.
- **Lua config is authoritative and immutable except by user.** All configuration in
  `~/.config/comic-chat/config.lua`. No config controls in GUI.
- **Message input at bottom**, just above status bar.
- **ComicArt dir holds only AVB / BGB files.** PNGs go in a separate cache dir.
  Never bundle ComicArt in the zip distribution. User manages it via config dir.
- **Title card:** only in very first frame when a room is joined. Static, scrolls into history.
- **Workflow:** `Apply-*.py` patch scripts, idempotent, refuse on drift, report OK/FAIL per item.
  Then `cargo test`. Combine commits into extensive batches. 1-2 steps at a time.
  No frequent break offers. Keep going on ok / go / proceed.

## Current focus — pose / emotion / send (do this first)

1. **Cycle neutral poses only** unless a trigger fires. No random gesture fallback.
2. **Preview = sent pose.** The preview pane shows the exact pose about to be sent.
3. **Message sends that exact pose.** No re-derivation from text at send time.
4. **Empty Enter sends pose-only.** A pose with no text.
5. **Interpret others' poses.** A received pose code renders that exact pose.
6. **Triggers (help-verified only):**
   - `LOL`, `ROTFL` -> Laughing. `:-)` / `:)` -> Happy. `:-(` / `:(` -> Sad. `;-)` -> Coy.
   - All capitals -> Shouting. `!!!` -> Shouting.
   - `I`, `I'm`, `I will`, `I'll`, `I am` -> point to self.
   - `You`, `are you`, `will you`, `did you`, `aren't you`, `don't you` -> point to other.
   - `Hello`, `Hi`, `Bye`, `Welcome`, `Howdy` -> Wave.
   - No trigger -> cycle neutral poses. Explicit pose always wins over hint.

Emotion wheel (spec §3.2, corrected 2026-11-21): Shout, Laugh, Happy, Coy, Bored,
Scared, Sad, Angry around the wheel, Neutral at centre. `E`-digit mapping is UNTRUSTED
until re-derived from `textpose.cpp` / `avatar.h`. Do not use Disgusted / Contemptuous /
Surprised / Afraid — not MS Chat.

## Rendering

- **Balloons:** B-spline construction, not rounded rect. Corners curvier so a short
  message like `hey` is almost circular. No janky wavies, no excess side padding.
- **Avatars:** 3+ avatars full-body, 1-2 torso-up, square panel, slender variable tails,
  placeholder in column. No random gesture fallback (spec §5.5).
- **Panels:** One balloon per character per panel. Title card is its own panel.
- **Backgrounds:** `Background as` is sticky state, not per-message.

## Protocol / features

- **Character transfer:** Send / receive character data. Download missing characters
  over HTTP. Catch character data from a user already in the room.
- **Away:** Non-standard MS Chat away. CTCP "coffee cup" status. CTCP panes show
  full character on white, status in dedicated frame.
- **Pose / OEC gating:** Pose codes and OEC only to Comics-mode peers. To Text-mode
  peers, send plain text. Negotiation bytes are UNTRUSTED — probe server, do not assume.
- **Example config:** Fix version drift in example config / docs. Keep 0.3.17 everywhere.

## Status

- [ ] Pose cycle + preview = sent + empty-Enter pose-only (current focus)
- [ ] Balloon corners curvier, short-message circular
- [ ] Avatar framing: 3+ full-body / 1-2 torso-up, square panel, slender tails
- [ ] Character transfer: send / receive / HTTP download / catch from room
- [ ] Away: CTCP coffee-cup + CTCP panes
- [ ] Example config version drift fixed
- [ ] Unused / deprecated scaffolding removed (only after verifying each item is unused)

## Done

- Balloon patch applied (B-spline base)
- Avatar sizing patch applied (base)
- Spec §3.2 / §3.6 corrected 2026-11-21 (wheel + help-verified triggers)

## Not doing

- No new features until pixel-perfect AVB / panel rendering is met.
- No bumping past 0.3.17 without explicit consent.
- No inventing emoticons, E-digit mappings, or negotiation bytes. Verify against
  Microsoft source first, mark UNTRUSTED until then.
- No config controls in GUI. Lua only.
- No bundling ComicArt in zip.

## How to update this file

After every change: check the box above, move the item to Done with date, and if a
rule changed, update `COMIC_CHAT_SPEC.md` in the same commit / patch. If a goal is
found wrong against the Microsoft source, fix the spec first, then this file.
Do not silently patch either file.
