# docs/pose-tables — README

Source of truth for pose wire. Chat context is ephemeral; these files are not.

- `trekkie2018.json` — wire-observed bodies for Trekkie2018 (Simple). 10 entries.
- `geordi.json` — wire-observed bodies for Geordi (Simple). 10 entries.
- `wire-codes-verified.md` — human-readable decode of the 20 wires.
- `COMIC_CHAT_SPEC.md` (repo root / docs) — verified wire spec from v2.5-beta-1 Batch 1 + Batch 2.

## How to fill the rest

`Dump-Gesture2.py` says Trekkie has 27 bodies. `Dump-POSE.py` dumps AVB records.
Modify those scripts to write here directly, instead of only printing:

- Each entry: `index` (= file order, `bRec[i]`), `body_index`, `torso_emotion_index`, `face_emotion_index`, `face_intensity`, `wire_shorthand`, `label`.
- AVB `nEmotion` is an `emFloats[]` index. Convert via `emFloats[nEmotion]`, do not use `nEmotion` as the float.
- AVB intensity is `byIntensity / 255.0`. Wire intensity is `intensity * 10` truncated.
- Do not invent bodies not in the AVB or not in live wire. Mark inferred fields in `wire_full_note`.

## How to use in Rust

`pose.rs` test loads `trekkie2018.json` + `geordi.json` and asserts:
Coy / Happy / Sad / Wave / PointOther / neutral cycles from `COMIC_CHAT_SPEC.md` §4.
If the test fails, the tables are wrong or the code drifted. Fix the code or the tables in the repo, not in chat.
