# comic-chat-rs — GOALS.md (REVISED 2026-11-22, end of day)

> Read this + `COMIC_CHAT_SPEC.md` first, before coding. If code and docs disagree, stop. Fix docs + code together, do not silently drift.

## Goal

Reimplement Microsoft Comic Chat 2.5 in Rust + egui, to look and operate as much like the original as possible. Pixel-perfect AVB decoding and panel rendering before new features. Full reverse compatibility with Microsoft Chat 2.5 in comics mode.

## Frozen constraints

- **Version frozen at 0.3.17.** Keep references consistent across `Cargo.toml` and `src/version.rs`. Bump only when floored by an improvement and with explicit consent.
- **Lua config is authoritative and immutable except by user.** All configuration in `~/.config/comic-chat/config.lua`. No config controls in GUI.
- **Message input at bottom**, just above status bar.
- **ComicArt dir holds only AVB / BGB files.** PNGs go in a separate cache dir. Never bundle ComicArt in zip. User manages it via config dir.
- **Title card:** only in very first frame when a room is joined. Static, scrolls into history.
- **Fonts by default:** interface Comic Sans MS Regular, balloons Comic Sans MS Bold. Config may override.
- **Workflow:** `Apply-*.py` patch scripts, idempotent, refuse on drift, report OK / FAIL per item. Then `python3 "$(ls -t Apply-*.py | head -1)" && cargo test`. Combine commits into extensive batches. Work 1-2 steps at a time, one definitive answer. No frequent break offers. Keep going on ok / go / proceed.
- **Diagnostics:** use `debug.log`, not stderr.
- **Repo:** single development branch `dev`. Python patch files are git-ignored.

## Current focus: pose / wire — CORRECTED

### What we now know is wrong (do not re-introduce)

- **Do NOT send `(#GdddEdddMd)` as "angle form".** `G410 / G810 / GC10` are not degrees. `E045` is not Coy. Wheel angle 45° must never be formatted as wire `E045`.
- **Do NOT send `G000`.** Valid Trekkie neutrals are `G010 / G410 / G810 / GC10`. `G000` is invalid and MS Chat falls back to neutral.
- **Do NOT fix wire with a formula** (`G = char + "10"`, `G = torso*400+10`, `E = angle`). `G` is per-AVB (Trekkie steps 0/4/8/12, Geordi steps 0/1/7). Formula fixes will pass one AVB and break the next.
- **Do NOT assert a wheel order as verified.** Three conflicting orders exist in doc history. Wheel Coy function body was not re-retrieved this session. Mark it UNVERIFIED until `avatar.cpp` / `textpose.cpp` / `semantic.cpp` are read line-by-line.
- **Do NOT assert `(#G3:1E0:1M1)` is banned / is comic-text.** Neither angle nor colon form is canonical. Treat incoming `(#...)` as opaque until it matches the per-AVB table.
- **Do NOT claim `avatar.h/cpp` prove "no pose in PRIVMSG".** Those files alone do not define wire. Live MS Chat 2.5 logs show it **does** send `(#G...E...M...)` at start of PRIVMSG.

### Correct model

Wheel / trigger -> local `CEmotion` (angle + intensity) -> `BodyRec` / `FaceRec` from **this** AVB -> `pose_id` -> wire from `pose_id` table entry -> preview shows that same `pose_id`.

Rules that follow:
1. **Preview equals sent, at `pose_id` level.** Same entry previewed and sent. No re-derivation at send time.
2. **Cycle neutral poses only** unless a trigger fires. For Trekkie that means cycling `G010 / G410 / G810 / GC10`, not repeating `G010`.
3. **Empty Enter sends pose-only**, using the previewed `pose_id`.
4. **Interpret others' poses** by table lookup, not by angle formula.
5. **Never send pose empty and claim a pose was sent.** If a pose is shown, its table wire code is sent.
6. **Triggers (help-verified only, not MS-source verified):** `LOL`/`ROTFL` -> Laugh, `:-)` / `:)` -> Happy, `:-(` / `:(` -> Sad, `;-)` -> Coy, ALL CAPS -> Shout, `!!!` -> Shout, `I...` -> PointSelf, `You...` -> PointOther, `Hello`/`Hi`/`Bye`/`Welcome`/`Howdy` -> Wave. No trigger -> cycle neutrals. Explicit pose always wins over hint. `Coy + you` combination is hypothesis, not verified MS rule — do not hard-code it until the ground-truth table shows it.

### AVB loader

- Decode at each entry's own `gestureOffset`, using `unk1` mapping, not scan order or DIB order.
- Build `BodyRec` / `FaceRec` tables per AVB. Wire lookup comes from those tables + live-log table, never from wheel angle.
- Do NOT hard-code "file enum 2 -> Coy 45". That mapping is withdrawn.

## Rendering (unchanged)

- **Balloons:** Closed B-spline, outward-only displacement, no inward dips. Corners curvier so short `hey` is almost circular. Not rounded rect + wobble.
- **Avatars:** 3+ in frame show full-body, 1-2 show torso-up. Square panel. Tails slender/variable. Placeholder stays in column. No nick label under avatar, nick only on hover. Character stands on panel, not below it.
- **Panels:** One balloon per character per panel. 5-char cap. Balloons in row above characters, tails to speaker. Title card is its own panel.
- **Backgrounds:** `Background as` is sticky state, not per-message.

## Protocol / features (unchanged, except pose)

- **Character transfer:** Send / receive / HTTP download / catch from room. Currently not being caught — must be fixed.
- **Away:** Standard + non-standard CTCP away. Coffee-cup status, CTCP panes. Do not invent away bytes.
- **Pose / OEC gating:** Pose codes and OEC only to Comics-mode peers. To Text-mode peers, send plain text. Negotiation bytes are UNTRUSTED. Probe server, do not assume.
- **Appears as / Background as:** `# Appears as Name.URL` on join and announce. `#` is part of line.
- **Example config:** Must be valid, byte-identical meaning to `DEFAULT_CONFIG_LUA`, version 0.3.17 everywhere, fonts default.

## Status

- [ ] **Ground-truth table** (Trekkie + Geordi, 11 rows in spec §3) — FIRST, before any code change
- [ ] `to_wire_string` -> table lookup (per-AVB), not formula
- [ ] Neutral cycling via table (`G010/G410/G810/GC10` for Trekkie)
- [ ] Preview == sent at `pose_id`
- [ ] Balloon corners curvier, short-message circular, B-spline outward-only
- [ ] Avatar framing: 3+ full-body / 1-2 torso-up, square panel, slender tails
- [ ] Character transfer: send / receive / HTTP download / catch from room
- [ ] Away: CTCP coffee-cup + CTCP panes
- [ ] Example config version drift fixed
- [ ] Unused / deprecated scaffolding removed (only after verifying each item is unused)

## Done

- Balloon patch applied (B-spline base)
- Avatar sizing patch applied (base)
- 2026-11-22 correction pass: angle-form / `G000->G010` / `G=char+"10"` claims withdrawn; per-AVB table model adopted; docs frozen at revised versions

## Not doing

- No new features until pixel-perfect AVB / panel rendering is met.
- No bumping past 0.3.17 without explicit consent.
- No inventing emoticons, E-digit mappings, negotiation bytes, or wire formulas. Verify against Microsoft source + live MS Chat logs first, mark UNTRUSTED until then.
- No config controls in GUI. Lua only.
- No bundling ComicArt in zip.
- No merging a wire patch until its rows exist in the ground-truth table.

## How to update

After every change: check box above, move item to Done with date, and if a rule changed, update `COMIC_CHAT_SPEC.md` in same commit / patch. If a goal is found wrong against Microsoft source or live logs, fix spec first, then this file. Do not silently patch either file.
