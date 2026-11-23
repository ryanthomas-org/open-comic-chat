# Microsoft Comic Chat — Technical Specification (REVISED)

**Target:** `comic-chat-rs` 0.3.17 (frozen). Do not bump without explicit consent.
**Revision:** 2026-11-22, end-of-day correction pass.
**Authority rule:** User's live MS Chat 2.5 wire logs > Microsoft source files actually read > prior spec/GOALS claims. A claim presented earlier as "from the Microsoft source" that cannot be re-found in a file actually in hand is withdrawn below, not carried forward.

## What was wrong — withdrawn

### 1. "2.5 sends angle form `(#GdddEdddMd)`, G/E are degrees" — WITHDRAWN
GOALS (2026-11-22) said: correct form is `(#G090E090M1)`, G = gesture angle 000-359, E = emotion angle 000-359.

Disproved by your own MS Chat logs:
- Trekkie neutrals: `G010`, `G410`, `G810`, `GC10`, then back to `G010`. `E010`, `M1` unchanged.
- `G410`, `G810` exceed 359. `GC10` contains `C`, not a digit. None of these are degrees.
- The `45` in Coy's wheel angle (45 degrees) was conflated into wire `E045`. Wheel angle is local selection input. Wire `G/E` are not angles.

Do not generate or parse `(#GdddEdddMd)` as "the 2.5 form". Do not claim `(#G090E090M1)` is what 2.5 sends.

### 2. "Colon form `(#G3:1E0:1M1)` is banned, it comes from comic-text" — WITHDRAWN as a ban
The colon form is not verified as MS Chat 2.5 wire either. But the ban was also not verified against `textpose.cpp` / `ircproto.h` / `semantic.cpp`, which were not in the reviewed set. Remove the ban. Rule now: neither angle form nor colon form is canonical. Treat any incoming `(#...)` as opaque until it matches a row in the per-AVB table in §3.

### 3. "G = gesture-char + `10`" — WITHDRAWN
Last patch claimed `G010 / G410 / G810 / GC10` = gesture char + `"10"`, so `G000 -> G010` fixes Coy. That rule was inferred from public 2005/2009 IRC logs (MS Chat 1.0-2.1 era) + FAQ. The FAQ itself says 2.5 "no longer sends this type of information" — so 1.0-2.1 logs do not prove 2.5 behaviour. And it fails your Geordi data:
- Geordi neutrals: `G010`, `G110`, `G710` — steps 0, 1, 7, not 0, 4, 8, 12.
- Therefore `G` is per-AVB, not a universal formula. No formula fix. Table lookup only (§3).

### 4. "`avatar.h` / `avatar.cpp` define no pose string in PRIVMSG" — QUALIFIED, not a wire rule
Those two files alone do not define a PRIVMSG pose string. That does **not** mean 2.5 sends no pose. Your live MS Chat 2.5 logs show it **does** send `(#G...E...M...)` at the start of PRIVMSG. The wire format lives in the files not yet reviewed (`textpose.cpp`, `ircproto.h`, `chatprot`, `semantic.cpp`) + live logs. Until those are read, wire format is a GAP filled only by live logs, not by `avatar.h/cpp` silence.

### 5. "Generation uses no pose code for v2.5 Say" — WITHDRAWN as interop rule
Our client currently sends plain text (`pose=""`). MS Chat peers see that as no-pose / neutral, not as Coy. For interop, plain-text Say is not equivalent to MS Chat Say. Do not keep this as a goal.

### 6. Wheel order + AVB enum mapping — UNVERIFIED
Three conflicting wheel orders exist in doc history:
- GOALS 2026-11-22: Happy, Coy, Bored, Scared, Sad, Angry, Shout, Laugh
- Earlier: Coy, Happy, Laughing, Shouting, Angry, Sad, Scared, Bored (paper Fig 1)
- `avatar.h` `EM_*` order as quoted in SPEC §3.2: Happy 0°, Coy 45°, Bored 90°, Scared 135°, Sad 180°, Angry 225°, Shout 270°, Laugh 315°

No wheel function body for "select Coy" was re-retrieved in this session (GitHub file contents are not reachable via search here). So: **do not assert any wheel order as verified.** The function triggered by selecting Coy on the original wheel, and how it interacts with the AVB, is **UNVERIFIED** until `avatar.cpp` / `textpose.cpp` / `semantic.cpp` are read line-by-line. What is known is the model, not the function:
- Wheel selection sets a local `CEmotion` (angle + intensity) — it does not directly set wire bytes.
- That `CEmotion` picks a `BodyRec` / `FaceRec` from the **local** AVB.
- That pick yields a `pose_id` / table entry. Wire bytes, if any, must come from that entry, not from the angle.

Similarly withdrawn: "AVB file enum 1->Happy 0, 2->Coy 45, 3->Bored 90..." — that is angle, not AVB enum. Do not hard-code it.

### 7. "Coy preview shows Happy, sent code is Coy correctly, Coy = entries 16/24/25" — SUPERSEDED
Latest clean evidence is the opposite:
- Our client, Coy: preview shows Coy image (`picked idx=16`), wire `(#G000E045M1)`, MS Chat shows **neutral**.
- So preview and wire disagree, and wire is wrong. `E045` is not a valid MS Chat Coy code (or not sufficient alone). `G000` is not a valid MS Chat neutral (valid Trekkie neutrals are `G010/G410/G810/GC10`).

## High-confidence learnings (from your logs only)

1. **Neutrals cycle over `G`, not over gesture names.** Trekkie: `G010 -> G410 -> G810 -> GC10 -> G010`, `E010/M1` constant. One `G010` fix does not give cycling.
2. **`G` is per-AVB.** Trekkie steps 0/4/8/12. Geordi steps 0/1/7. No universal `+4` or `+"10"` rule. `G` must be read from that AVB's `BodyRec` table.
3. **`G` first char is hex-like, not decimal.** `C` = 12. Older logs also show `: ; < = > ? @ A` as `G/E` chars. Parser must accept `[0-9A-F:;<=>?@A]` at least, not `\d{3}`. Do not reject `GC10` as malformed.
4. **Wheel angle != wire code.** Coy wheel = 45° (if `EM_COY` order above is right) is selection input. It must never be formatted as `E045` on the wire.
5. **Preview == sent must hold at `pose_id` level.** Same `BodyRec/FaceRec` entry must be previewed and sent. Previewing by angle + sending by angle-derived decimal breaks this.
6. **Our current `to_wire_string` is wrong for both fields.** `G000` (invalid neutral) + `E045` (angle-as-wire). Do not patch it with another formula.

## Correct model (to verify against source + logs)

```
Wheel click / text trigger
  -> local CEmotion { angle, intensity }   [local only, never sent]
  -> GetBodyFromEmotion / GetHeadAndBodyFromEmotion
  -> BodyRec + FaceRec from THIS AVB
  -> pose_id
  -> wire (#G...E...M...) from pose_id table entry  [table from live MS Chat logs, §3]
  -> preview must show that same pose_id
```

## 3. Ground-truth table to capture (do not code until this exists)

For Trekkie2018 (and separately for Geordi), send from real MS Chat 2.5, in order, and record exact wire + what MS Chat displays:

| # | Input | Exact wire `(#G...E...M...)` | MS Chat shows |
|---|---|---|---|
| 1 | plain 1 | `G010` (expected) | |
| 2 | plain 2 | `G410` (expected) | |
| 3 | plain 3 | `G810` (expected) | |
| 4 | plain 4 | `GC10` (expected) | |
| 5 | plain 5 | `G010` again? | |
| 6 | wheel Coy | ? — NOT `E045` | |
| 7 | wheel Happy | ? | |
| 8 | wheel Sad | ? | |
| 9 | Wave | ? | |
| 10 | PointOther (`you`) | ? | |
| 11 | PointSelf (`I`) | ? | |

Only rows actually observed count. Empty rows stay empty. No filling from formula, no filling from other AVBs.

## What stays (frozen, unchanged)

- Version 0.3.17 frozen. Lua config authoritative, immutable except by user. No config controls in GUI.
- Message input at bottom, just above status bar. ComicArt holds only AVB/BGB. PNGs in separate cache. No bundling ComicArt in zip.
- Title card: only very first frame on join, static, scrolls into history.
- Pixel-perfect AVB decoding + panel rendering before new features. No new features until then.
- `Apply-*.py` patches, idempotent, refuse on drift. `cargo test` after each. Extensive combined commits.
- 1-2 steps at a time, one definitive answer, no frequent break offers.
- Diagnostics in `debug.log`, not stderr.

## Open gaps (explicit)

- Wheel Coy function body: not re-verified this session. Read `avatar.cpp`, `textpose.cpp`, `semantic.cpp` line-by-line before asserting it.
- Wire `G/E` encoding: not in `avatar.h/cpp`. Needs `textpose.cpp` / `ircproto.h` + live-log table.
- `CEmotionOpts` / two-emotion messages (face + gesture, e.g. Coy + PointOther): claimed in SPEC §3.2, not verified from live logs. Keep as hypothesis, not rule.
- Triggers (`;-)` -> Coy, `LOL` -> Laugh, etc.): help-verified only, not MS-source verified. Keep, but mark as such.
