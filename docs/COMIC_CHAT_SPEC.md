# COMIC_CHAT_SPEC.md — open-comic-chat v0.3.17
# Rewritten 2026-11-25 after Trekkie Neutral verified. Corrects all prior drafts.
# Status: Trekkie Neutral wire/panel/preview verified. Geordi + non-Neutral still open.

## 1. AVB file is the source of truth
- No hard-coded AVB tables in Rust. `pose_tables.rs` and `pose_lookup.rs` DELETED.
- `pose.rs` functions `to_code_string_for_avb`, `to_code_string_for_avb_cycled`,
  `to_wire_string_for_avb`, `to_wire_string_for_avb_cycled` DELETED.
- Pose data comes only from `decode_avb_file` -> `Dec.poses` (POSE table) and `Dec.images`.
- `PoseEntry { gesture, expression, image_index }`.
- For Trekkie2018, Neutral poses are `(gesture 0, expression 0, image_index != 0)`.
  `image_index 0` is the icon, excluded from Neutral cycle.

## 2. Wire format (OUT)
- Form: `(#G<body>10E<face><int>[R]M1) text`
- Body: single hex char (0-9, A-F) = `image_index` in AVB POSE table.
- Face: `1` for Neutral. (Prior hard-coded table used face 9, which collapsed
  Neutral and Happy to the same body. Wrong.)
- Int: `0` for Neutral; 0-9/A/B/C for other emotions.
- `R`: only when the Freeze box is checked at send time. Never from a table.
- `M1`: always.
- `pose_style` is a style selector (`none` / `2.5` / `old` / `angle`), NOT an AVB name.
  Never use it to look up an AVB. (Bug 2026-11-25: `wire_for_pose` looked up
  `pose_style`="old" in `avb_chars`, found nothing, fell back to bare Neutral
  `G010E010M1` = icon. Fixed by looking up `character` instead.)

## 3. Send path: `wire_for_pose`
- Looks up `self.cfg.comic.character` (e.g. `"Trekkie2018"`) in `avb_chars`.
- Calls `wire_from_poses`, which filters `Dec.poses` by emotion/gesture and
  cycles neutrals in POSE-table order, seeded by `neutral_cycle`.
- If the AVB is not found or cannot be decoded, logs `WIRE_POSE` and sends
  bare Neutral `G010E010M1` as a last resort. This is not the normal path.
- No fallback to hard-coded tables. They do not exist.

## 4. Neutral cycle — Trekkie2018, VERIFIED 2026-11-25
- Image indices `[1, 4, 8, 11]` -> wires `G110, G410, G810, GB10`, then repeat.
- OUT log confirms: `(#G110E010M1)`, `(#G410E010M1)`, `(#G810E010M1)`, `(#GB10E010M1)`.
- Preview (selfview), wire, and panel all read the same POSE table, so they agree.
- Preview: `char_pose_texture` / `pose_image_index_seeded` (strict filter).
- Panel: draws the wire body via the POSE table.
- Strict filter: `gesture == 0, expression == 0, image_index != 0`.

## 5. Panel rendering
- Full body. No torso-up crop (FIX 110).
- Small panels (~320px), pack per row: 4 at ~1450px width, 3 at ~1000px.
  Panels stay small and pack; they do not grow and spill early.
- Balloon: clouded rounded bubble. Tail points to avatar. Tail stroke does not
  extend into the bubble.
- Title card: displays only in the very first frame when a room is joined.
  Static like other panes, scrolls into history.
- One panel per message. Control messages never make panels.

## 6. AppearsAs (control message)
- Form: `# Appears as <Name>.<URL>` (e.g. `# Appears as Trekkie2018.http://.../Trekkie2018.AVB`).
- Control message: never makes a panel. Only updates `char_map`.
- One reply per peer per character.
- CURRENT BUG (open): sends 2x (duplicate lines in OUT log). Must become 1x.

## 7. Emotion wheel and triggers
- Emotions: Neutral, Happy, Coy, Bored, Scared, Sad, Angry, Shout, Laugh.
- Gestures: Neutral, Wave, PointSelf, PointOther.
- Wheel sets emotion/gesture for the next send. Freeze box holds the pose.
- Keyword `hello` triggers Wave pose, not Neutral.
- Freeze clears on send unless the Freeze box is checked.

## 8. Config
- Lua config at `/home/ryan/.config/comic-chat/config.lua` is authoritative and
  immutable except by the user. No config controls in the GUI.
- `comic.character`: AVB name (e.g. `"Trekkie2018"`). Used for AVB lookup.
- `comic.pose_style`: style selector only. Not an AVB name.
- ComicArt dir contains only AVB/BGB files. PNG cache lives in a separate dir.
- Message input at bottom of screen, just above the status bar.
- Client aims to look and operate as much like original Microsoft Comic Chat
  as possible.

## 9. Version freeze
- Freeze at 0.3.17. Only bump when floored by an improvement and the user consents.
- Version references stay consistent across `Cargo.toml` and `src/version.rs`.
- Repo: `https://github.com/ryanthomas-org/open-comic-chat`, branch `dev`.
- Commit `b210adf` made; push to `origin/dev` UNCONFIRMED (see TODO).

## 10. What was wrong before (do not regress)
- Hard-coded table listed Trekkie neutrals as `[0, 4, 8, 19]` — body 0 is the
  icon, body 19 is the Scared pose. Panel showed icon / neutral / neutral / scared.
- `wire_for_pose` looked up `pose_style` ("old") instead of `character`, so the
  generic path always failed and the send fell back to `G010E010M1` (icon).
- Preview used the AVB POSE table (correct); wire used the hard-coded fallback
  (wrong). That divergence is why preview was right and panel was wrong.
- Both are fixed. The hard-coded tables are deleted, not patched.
