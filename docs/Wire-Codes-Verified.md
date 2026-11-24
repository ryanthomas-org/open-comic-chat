# wire-codes-verified.md

20 distinct wire strings, verified 2026-11-23 vs v2.5-beta-1 Batch 1 + Batch 2.
Format: `(#G<torsoIdx><torsoEmo><torsoInt>E<faceIdx><faceEmo><faceInt>[R]M<mode>)`, each value `'0'+n`, intensity `n/10`.
For Simple avatars: torsoEmo always `1` (0.0), torsoInt always `0`, faceIdx always `0`. FaceEmo is the real emotion.

## Trekkie2018

| Wire shorthand | Decode |
| --- | --- |
| G210E0:8 | torso 2, face WAVE (10), 0.8 |
| GJ10E0;9 | torso 26, face POINTOTHER (11), 0.9 |
| G@10E029R | torso 16, face COY (2), 0.9, R |
| GH10E028R | torso 24, face COY (2), 0.8, R |
| GI10E027R | torso 25, face COY (2), 0.7, R |
| G610E019R | torso 6, face 0.0 (HAPPY), 0.9, R |
| GB10E016R | torso 18, face 0.0 (HAPPY), 0.6, R |
| G<10E015R | torso 28, face 0.0 (HAPPY), 0.5, R |
| GA10E059R | torso 17, face SAD (5), 0.9, R |
| G510E058R | torso 5, face SAD (5), 0.8, R |

## Geordi

| Wire shorthand | Decode |
| --- | --- |
| G010 | torso 0, face 0.0, 0.0 — Neutral, E part inferred |
| G110 | torso 1, face 0.0, 0.0 — Neutral, E part inferred |
| G710 | torso 7, face 0.0, 0.0 — Neutral, E part inferred |
| G310E019 | torso 3, face 0.0 (HAPPY), 0.9 |
| G610E069 | torso 6, face ANGRY (6), 0.9 |
| G910E059 | torso 9, face SAD (5), 0.9 |
| G510E039 | torso 5, face BORED (3), 0.9 |
| G810E049 | torso 8, face SCARED (4), 0.9 |
| G410E079 | torso 4, face SHOUT (7), 0.9 |
| G210E089 | torso 2, face LAUGH (8), 0.9 |

## Notes

- `M` was not in the shorthand logs. `M1` (say) is assumed, not observed.
- Geordi neutrals: only `G` part preserved in shorthand. `E010` inferred from `SetBodyNeutral`.
- Happy vs Neutral: both 0.0 emotion byte `1`. Distinguished by body index + intensity.
- Full JSON: `trekkie2018.json`, `geordi.json` in this folder. Those are the source of truth for `pose.rs` tests.
