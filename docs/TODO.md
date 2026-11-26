# TODO.md — open-comic-chat v0.3.17
# Rewritten 2026-11-25 after Trekkie Neutral verified. Corrects all prior drafts.

## DONE (verified)
- [x] Trekkie Neutral wire/panel/preview agree: G110/G410/G810/GB10, cycling, panel == preview.
- [x] Delete hard-coded AVB tables: pose_tables.rs, pose_lookup.rs.
- [x] Delete pose.rs hard-coded AVB functions (to_code_string_for_avb*, to_wire_string_for_avb*).
- [x] wire_for_pose looks up character, not pose_style.
- [x] Panels small (~320), pack per row, full body, no torso-up crop.
- [x] Title card first frame only, static, scrolls into history.
- [x] Balloon: clouded rounded bubble, tail points to avatar, no stroke into bubble.

## IMMANENT OBJECTIVES (in order)
1. **Geordi verification** — Switch character to Geordi. Send Neutral x4.
   Confirm wire/panel/preview agree. Confirm Geordi neutral face in the wire:
   Geordi.json infers face 1 (E010); generic path must be checked against the
   AVB POSE table, not the old hard-coded table. Verify the full Geordi.json
   table against the AVB (not yet confirmed).
2. **Non-Neutral emotions** — Send Coy, Happy, Wave, PointSelf, PointOther,
   `hello`, `lol`. Confirm wire/panel/preview agree, OUT once each,
   no icon or Scared bleed into other emotions.
3. **AppearsAs 1x** — Currently sends 2x (duplicate lines in OUT log).
   Find the second send site, remove the duplicate. Verify OUT AppearsAs
   appears once per join.
4. **MS Chat peer verification** — Peer oarion7 (real MS Chat) confirms
   Trekkie/Geordi poses render correctly: no code showing, no icon,
   correct pose for each emotion/gesture.
5. **Git push** — Confirm commit b210adf pushed to origin/dev. If not, push.
6. **Outbound variant cap + non-Neutral symbolic codes** — Verify:
   max 10 poses per emotion, unlimited neutrals; non-Neutral symbolic wire
   codes (e.g. Coy) map correctly in both directions (AVB -> wire, wire -> AVB).
7. **Spec/TODO replace** — User reviews NEW_COMIC_CHAT_SPEC.md / NEW_TODO.md.
   If accepted, replace COMIC_CHAT_SPEC.md / TODO.md. Do not replace without
   user permission.

## NOTES (do not regress)
- AVB file is the source of truth. Never hard-code AVB tables into Rust.
- pose_style is a style selector, not an AVB name. AVB lookup uses character.
- Per-avatar overrides are unacceptable.
- Spec/goals updates only on user permission at a clean stopping point.
- If the next 2 patches do not return great progress, the project is cancelled.
  (Trekkie Neutral fix counts as great progress; the project continues.)
