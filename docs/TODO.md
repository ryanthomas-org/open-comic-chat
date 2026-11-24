# GOALS — comic-chat-rs / open-comic-chat

Status: kept, 2026-11-23. Pose wire section updated from verified v2.5-beta-1 source (Batch 1 + Batch 2). Other goals unchanged.

## 1. Pose wire — must agree: preview == local panel == MS Chat 2.5

- [ ] `to_wire_string` is per-AVB table lookup, not a formula. See `COMIC_CHAT_SPEC.md` §6.
- [ ] `parse_pose` is exact inverse of `to_wire_string` (round-trip on all 20 verified wires).
- [ ] Preview uses same `GetBodyFromEmotion` / `GetIndices` / `GetEmotions` path as send. No preview-only mapping.
- [ ] Neutral cycling is `SetBodyNeutral`: rotation through `EM_NEUTRAL` + `0.0` records in file order from `m_lastBody`.
- [ ] `R` = frozen (`freeze != AF_UNFROZEN`). Do not hard-code `R = wheel`.
- [ ] Same-AVB: use indices. Substitute-AVB (`OTHERMAPPED`): use emotion + intensity via `SetEmotions`.
- [ ] Per-AVB tables live in `docs/pose-tables/` (JSON) and are loaded by a `pose.rs` test. No tables only in chat.
- [ ] Open test: wheel Coy, then plain message without touching wheel, check if `R` persists. Do not hard-code `R` until tested.
- [ ] Open: `SM_*` numeric mode values (need header). `CGESTUREPREFIX` etc. defines (need header). Keyword strings (need resource strings). Full `E` part for Geordi neutrals (need `debug.log` full wire).

## 2. Rendering — pixel-perfect MS Chat

- [ ] AVB decoding and panel rendering match original Microsoft Chat before adding new features.
- [ ] AVB loader: index = file order, `emotion = emFloats[nEmotion]`, `intensity = byIntensity/255`, ditto poseID reuse. See SPEC §3.
- [ ] Balloon: clouded rounded bubbles, tail strokes not extending into bubble, multiple bubbles from different users in same panel handled.
- [ ] Avatar: white background, mask inversion correct, pose mapping via canonical v2.5 pose code (per-AVB table), wheel emotions mapped to AVB image index (Coy etc. distinguished from spec emotions vs crate extensions).
- [ ] Title card pane: only in very first frame when a room is joined, static like other panes, scrolls into history.
- [ ] Message input at bottom, just above status bar.
- [ ] Scrollbar visible once exists, stick to bottom on new pane if already at bottom.

## 3. Config + distribution

- [ ] Lua config at `~/.config/comic-chat/config.lua` is authoritative and immutable except by user. All configuration in Lua, no config controls in GUI.
- [ ] `comicart` directory contains only AVB/BGB files. PNGs go to separate cache directory. Do not bundle ComicArt in zip distribution. Config dir manages ComicArt.
- [ ] Version frozen at `0.3.17` across `Cargo.toml` and `src/version.rs`. Only bump when floored by improvements and with explicit consent. Version consistency throughout stack.

## 4. Workflow

- [ ] Patch scripts: `Apply-*.py` in repo root, idempotent, run via `python3 "$(ls -t Apply-*.py | head -1)" && cargo test`, paste `cargo test` output.
- [ ] One logical fix per commit. Short commit messages when requested (<=64 chars).
- [ ] Extensive combined commits where possible, not hundreds of small steps.
- [ ] Answers: concise, 1-2 steps at a time. Single definitive answer, no choose-between-options.
- [ ] Do not offer frequent breaks while working. Keep going on ok/go/proceed.
- [ ] Pose tables, SPEC, GOALS are saved persistently in repo files, not only in chat. See `docs/pose-tables/README.md`.

## 5. Withdrawn — do not reintroduce

- Colon-delimited wire codes. `G000`-to-`G010` patch. Angle-form canonical wire. `G`'s `10` / `E`'s `0` as magic constants. `R = wheel-selected` as definition. Hard-coded keyword list from `#if 0` code. Hard-coded mode numbers 1/2/3/5 as verified.


## 6. Corrections to the 2026-11-23 draft that preceded this file — that draft was WRONG

- Neutral wire is `(1, 0)`, NOT `(9, 0)`. See SPEC §1.3 + §8.1 + §8.6. Revert any patch that made `single_face_emo()` return `(9, 0)` for wire encoding.
- `M1` NOT verified. Do NOT hard-code mode numbers. See SPEC §5.
- `hello -> Wave` / `lol -> Laugh` NOT verified keywords. Do NOT hard-code keyword list. See SPEC §2.2.
- `R` = freeze only, NOT strength. See SPEC §1.7.
- AppearsAs: split name + `.url`, do NOT strip trailing `.` / `?`. See SPEC §8.3.
- Table selection (`nEmotion 9` for Neutral) vs wire encoding (`1` for `0.0`) are two levels. Do NOT conflate them.

## 7. New tasks from implementation (2026-11-23, after the v2.5 SPEC)

- [ ] Revert Neutral wire `(9, 0)` -> `(1, 0)` in `single_face_emo()`; keep table selection on `nEmotion 9`. Verify: Neutral wire `G010E010` (face `1`/`0`), Happy wire `G310E019` (face `1`/`9`) — distinguished by body + intensity.
- [ ] Fix AppearsAs parser to split name + `.url` via `APPEARSPREFIX`, not strip `.` / `?`.
- [ ] Wire `should_send_appears_as()` at actual send sites (own join `peer=""` public once, peer join whisper-only once per peer per character) — helper exists, call sites still open.
- [ ] Live verify: Trekkie then Geordi, `cargo run --release`, send plain / Coy / Happy / Wave / PointOther / `hello` / `lol`; check OUT `(#G...) text` once, preview == local == MS Chat, neutrals cycle in file order from `m_lastBody`, AppearsAs 1x, no duplicate whisper, panels 320 / 4 at ~1450 / 3 at ~1000.
- [ ] Open test from SPEC §1.7: wheel Coy, then plain message without touching wheel, check if `R` persists.
- [ ] Open from SPEC §5: `SM_*` numbers (need header), `CGESTUREPREFIX` defines (need header), keyword strings (need resources), full `E` part for Geordi neutrals (need `debug.log` full wire), `M` value for 20 live codes (shorthand omitted `M`).

