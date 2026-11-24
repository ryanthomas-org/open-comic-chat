# COMIC_CHAT_SPEC — Verified from Microsoft v2.5-beta-1 source

Status: VERIFIED 2026-11-23 from Batch 1 + Batch 2.
Batch 1: `protsupp.cpp`, `protsupp.h`, `avatario.cpp`, `avatario.h` (5475 lines, output1_of_2.txt)
Batch 2: `avatar.cpp`, `avatar.h`, `textpose.cpp`, `histent.cpp`, `histent.h`, `avbfile.cpp`, `avbfile.h` (5693 lines, output2_of_2.txt)
Live wire: 20 distinct wire strings from MS Chat 2.5 sessions (see `docs/pose-tables/wire-codes-verified.md`).

This SPEC supersedes all earlier angle-form, colon-form, and G000-to-G010 claims. Those are withdrawn below.

## 1. Wire format — VERIFIED

### 1.1 Encoder: `bInsertAnnotations` (protsupp.cpp:3028 in Batch 1 concatenation)

```c
av->GetIndices(faceIndex, torsoIndex, bbRequested);
av->GetEmotions(face, torso);
BYTE faceIndexByte = IndexToByte(faceIndex);
BYTE torsoIndexByte = IndexToByte(torsoIndex);
BYTE modeByte = IndexToByte(BM2SM(uModes));
EmotionToBytes(face, faceEmotion, faceIntensity);
EmotionToBytes(torso, torsoEmotion, torsoIntensity);
sprintf(szBuff, "%s#%c%c%c%c%c%c%c%c%s%c%c",
    bIncludeParenthesis ? "(" : "",
    CGESTUREPREFIX, torsoIndexByte, torsoEmotion, torsoIntensity,
    CEXPRESSIONPREFIX, faceIndexByte, faceEmotion, faceIntensity,
    bbRequested ? "R" : "",
    CMODEPREFIX, modeByte);
// + optional "T..." talk-tos, then ") " if parenthesis
```

Wire on the wire is therefore:

```
(# G <torsoIdx> <torsoEmo> <torsoInt> E <faceIdx> <faceEmo> <faceInt> [R] M <mode> [T...] )
```

- `G` = torso / gesture slot. `E` = face / expression slot. `R` = requested/frozen. `M` = mode. `T` = talk-tos.
- No colons. No separators. No angle form. No `G000` form.
- Every value byte is a single character.

### 1.2 Byte codec — VERIFIED

```c
BYTE IndexToByte(BYTE b) { return b + '0'; }
BYTE ByteToIndex(BYTE b) { return b - '0'; }
```

So `'0'` = 0, `'1'` = 1, `'9'` = 9, `':'` = 10, `';'` = 11, `'<'` = 12, `'='` = 13, `'>'` = 14, `'?'` = 15, `'@'` = 16, `'A'` = 17, `'B'` = 18, `'G'` = 23, `'H'` = 24, `'I'` = 25, `'J'` = 26, etc.
`'2'` = 2, `'@'` = 16, `'J'` = 26 are body indices in the live codes, not ASCII art.

### 1.3 Emotion codec — VERIFIED

`emFloats[]` (avatario.cpp, Batch 1):

| Wire index (ByteToIndex) | `emFloats[]` value | Name |
| --- | --- | --- |
| 0 | 0.0 | placeholder 0.0 |
| 1 | 0.0 | EM_HAPPY |
| 2 | 1 * 2PI / 8 = 0.785... | EM_COY |
| 3 | 2 * 2PI / 8 = 1.571... | EM_BORED |
| 4 | 3 * 2PI / 8 = 2.356... | EM_SCARED |
| 5 | 4 * 2PI / 8 = 3.142... | EM_SAD |
| 6 | 5 * 2PI / 8 = 3.927... | EM_ANGRY |
| 7 | 6 * 2PI / 8 = 4.712... | EM_SHOUT |
| 8 | 7 * 2PI / 8 = 5.498... | EM_LAUGH |
| 9 | 0.0 | EM_NEUTRAL |
| 10 | 1001.0 | EM_WAVE |
| 11 | 1002.0 | EM_POINTOTHER |
| 12 | 1003.0 | EM_POINTSELF |
| 13 | 1004.0 | EM_DOUBLEPOINT |
| 14 | 1005.0 | EM_SHRUG |
| 15 | 1006.0 | EM_3QRWALK |
| 16 | 1007.0 | EM_SIDEWALK |
| 17 | 1008.0 | EM_3QFWALK |

Encoder `EmotionToBytes` (avatario.cpp, Batch 1):

```c
void EmotionToBytes(CEmotion &em, BYTE &emotion, BYTE &intensity) {
  BYTE emVal = 9;  // neutral always safe
  int n = sizeof(emFloats)/sizeof(float);
  for (int i = 1; i < n; i++)
    if (emFloats[i] == em.m_emotion) { emVal = (BYTE)i; break; }
  BYTE inVal = (BYTE)(em.m_intensity * 10);
  emotion = IndexToByte(emVal);
  intensity = IndexToByte(inVal);
}
```

Critical consequence, VERIFIED: `EM_HAPPY` (0.0) and `EM_NEUTRAL` (0.0) are the same float. The loop starts at `i=1`, so 0.0 encodes as wire index `1`, never `9`. Default is `9` only if no match.
Wire intensity = `intensity * 10`, truncated to byte. `0.9` -> `'9'`, `0.8` -> `'8'`, `0.0` -> `'0'`.

Decoder `BytesToEmotion`:

```c
void BytesToEmotion(CEmotion &em, BYTE emIndex, BYTE inIndex) {
  if (emIndex >= sizeof(emFloats)/sizeof(float)) em.m_emotion = EM_NEUTRAL;
  else em.m_emotion = emFloats[emIndex];
  em.m_intensity = (float)(inIndex / 10.0);
}
```

So `'1'` decodes to 0.0, `'2'` to COY, `'6'` to ANGRY, `':'` (10) to WAVE, `';'` (11) to POINTOTHER.

### 1.4 Decoder: `ProcessSay` / `ProcessUDIData` (protsupp.cpp, Batch 1) — VERIFIED

Both parse the same prefix:

```
if "(#" + ") " present:
  if 'G': Gest = ByteToIndex, GestE = ByteToIndex, GestI = ByteToIndex
  if 'E': Expr = ByteToIndex, ExprE = ByteToIndex, ExprI = ByteToIndex
  if 'R': bbReq = 1
  if 'M': uModes = SM2BM(ByteToIndex)
  if 'T': GetTalkTos(...)
  if GestI != -1 and ExprI != -1: bbCooked = 1, advance past ") "
```

`m_udi` fields in this code are: `m_chGest` = torso index, `m_chGestE` = torso emotion index, `m_chGestI` = torso intensity index, `m_chExpr` = face index, `m_chExprE` = face emotion index, `m_chExprI` = face intensity index, `m_bbReq` = R, `m_uModes`, `m_bbCooked`.

`BM2SM` / `SM2BM` (protsupp.cpp, Batch 1) map between `BM_*` (say/whisper/think/action/sound) and `SM_*` wire modes. The numeric values of `SM_*` were NOT in the two batches (they live in a header not sent). Do NOT hard-code mode numbers 1/2/3/5 as verified. `M` is `IndexToByte(BM2SM(uModes))`; the header with `SM_*` values must be checked before claiming numbers.

Prefix letters `G`/`E`/`R`/`M`/`T` are confirmed by live wire + `SayEntry::FormatOtherArgs` (`"(G:%d %d %d E:%d %d %d R:%d M:%d"`). The `#define CGESTUREPREFIX` etc. values live in a header not in the two batches; do not cite a line for them.

### 1.5 `SayEntry::Execute` (histent.cpp, Batch 2) — VERIFIED

```c
if (m_udi.m_bbCooked) {
  if (!(av->m_flags & OTHERMAPPED))
    av->SetIndices(m_udi.m_chExpr, m_udi.m_chGest, m_udi.m_bbReq);
  else {
    BytesToEmotion(expr, m_udi.m_chExprE, m_udi.m_chExprI);
    BytesToEmotion(gest, m_udi.m_chGestE, m_udi.m_chGestI);
    av->SetEmotions(expr, gest);
  }
}
```

- Same AVB: use indices directly. No lookup. Both sides load the same `.avb`, so index means the same record.
- Substitute AVB (`OTHERMAPPED`): ignore indices, match on emotion + intensity via `SetEmotions`. That is why BOTH index and emotion+intensity are sent.

### 1.6 `GetIndices` / `SetIndices` / `GetEmotions` / `SetEmotions` (avatar.cpp, Batch 2) — VERIFIED

`CAvatarSimple` (single-body characters: Trekkie, Geordi):

```c
GetIndices: chTorsoIndex = bodyRec - bRec; chFaceIndex = 0; bbRequested = (m_freeze != AF_UNFROZEN);
GetEmotions: face = bodyRec emotion/intensity; torso = 0.0 / 0.0;
SetIndices: if (torsoIndex in range) bodyRec = bRec + torsoIndex; requested = bbRequested; // face index ignored
SetEmotions: cycle from after current body, pick record with face emotion == requested emotion and closest intensity; else SetBodyNeutral;
```

`CAvatarComplex` (separate face + torso):

```c
GetIndices: face = faceRec - fRec; torso = torsoRec - bRec; bbRequested = (freeze != UNFROZEN);
GetEmotions: face = faceRec; torso = torsoRec;
SetIndices: face = fRec + faceIndex if in range; torso = bRec + torsoIndex if in range;
SetEmotions: face: closest-intensity match on face emotion; torso: closest-intensity match on torso emotion; else neutral;
```

Consequence for Simple: on the wire, `G` always carries torso emotion `1` (`0.0`) + intensity `'0'`, and `E` always carries face index `'0'` + real emotion + intensity. Example: `G210E0:8` = torso 2 / 0.0 / 0.0, face 0 / WAVE / 0.8. Do NOT treat `G`'s `10` as a constant to copy; it is torso emotion 1 + torso intensity 0, derived from `GetEmotions`.

`AF_UNFROZEN = 1, AF_TEMPFROZEN = 2, AF_FROZEN = 3` (avatar.h, Batch 2).

### 1.7 `R` — VERIFIED as frozen/requested, not just wheel

`bbRequested = (m_freeze != AF_UNFROZEN)`.

`R` means the pose is frozen / requested, i.e. not auto-picked from text. `ChatPreSendText` returns early if `m_freeze != AF_UNFROZEN`, so a frozen pose persists across plain messages. The wheel presumably sets `m_freeze`, but the wheel-to-freeze mapping was NOT in the two batches. Do NOT hard-code `R = wheel-selected` as the definition. Definition is freeze state. Test: wheel Coy, then plain message without touching wheel, check if `R` persists. That test is still open.

### 1.8 Character binding — VERIFIED

`ChatAnnounceNewAvatar` (protsupp.cpp, Batch 1): sends `"# Appears as <name>[.url]"` (`"#%s%s.%s"` or `"#%s%s"` with `APPEARSPREFIX`). Receiver `ProcessComment` parses `APPEARSPREFIX`, extracts name + URL, calls `ChangeAvatarEntry` / `SetUserAvatarRealInfo`.

`GetAvatar3` (avatar.cpp, Batch 2): search by `OriginalName`, return unused original with `OTHERMAPPED` cleared; if in use, `DupAvatar`; else `LoadAvatar(name)`; if null and `bRandomIfNotFound`, pick next avatar name, recurse, set `OTHERMAPPED`. That flag is what triggers the `SetEmotions` fallback in `SayEntry::Execute`.

## 2. Sender: text -> emotion -> body — VERIFIED

### 2.1 `ChatPreSendText` (textpose.cpp, Batch 2)

```c
void ChatPreSendText(CString &str, int avID) {
  if (!comic view) return;
  av = GetAvatar(avID) or MyAvatar();
  if (!av || av->m_freeze != AF_UNFROZEN) return; // frozen: do not auto-pick
  GetEmotionsFromString(str, emo);
  CBody *newBody = av->GetBodyFromEmotion(emo);
  av->UpdateBody(newBody);
}
```

### 2.2 `GetEmotionsFromString` / `InitializeEmotionRules` (textpose.cpp, Batch 2)

Rules are loaded from string resources via `InitializeEmotionRules` -> `LoadCompositeRule` -> `LoadSingleRule` -> `RegisterRule`. Rule format in resources: `Function(args);Strength` lines, newline-separated. Functions: `AllCaps`, `FindString`, `FindString*`, `CheckWord`, `CheckWord*`, `CheckStart`, `CheckStart*`. `*` = case-insensitive (lowercased).

`GetEmotionsFromString` checks in order: caps (`CheckForUppers` + `capsStrength`), general (`FindString`), word (`CheckWord` via `CheckWord()` substring + word-boundary), sentence-start (`CheckStart` via `StartCompare2` at each sentence start). Each match does `emOpts.Add(emotion, 1.0, strength)`.

The old `CheckStarts` / `CheckForPoints` / `CheckForLaughs` code in the same file is `#if 0` disabled. Do NOT cite it as the live keyword list. The live keyword strings live in resources (`ruleIDs` / `ruleEMs`), not in `textpose.cpp`. `ruleEMs[]` order in Batch 2 is: SHOUT, LAUGH, HAPPY, SAD, POINTOTHER, POINTSELF, WAVE, COY, ANGRY, SCARED, BORED. The resource strings for those IDs were NOT in the two batches.

So: Wave / PointSelf / PointOther only by keyword is directionally correct (they are rule-driven), but the exact keyword list is NOT verified from these batches. Do not hard-code a keyword list in SPEC.

### 2.3 `GetBodyFromEmotion` (avatar.cpp, Batch 2) — VERIFIED

`CAvatarSimple::GetBodyFromEmotion(CEmotion &emotion)` (single emotion):
- Loop `index = (m_lastBody + 1 + i) % m_nBodies` — rotation, starts after last used.
- Skip if `bRec[index].emotion > 7` — skips WAVE/POINT/etc. (floats 1001+) because they are not wheel angles. Only wheel emotions (0.0..5.498) are considered here.
- If `angle < PI / NEMOTIONS` (22.5 deg) or first-neutral fallback, compute `delta_i = |emotion.intensity - bRec.intensity|`, pick smallest `delta_i`.
- `SetBody(body, nearestI)`.

`CAvatarSimple::GetBodyIndexFromEmotion` (used by the `CEmotionOpts` version):
- If `emotion <= 2*PI`: angle-nearest as above.
- Else (`> 2*PI`, i.e. WAVE/POINT/etc.): exact match `if (emotion == bRec[i].emotion) { bIndex = i; break; }`.

`CAvatarSimple::GetBodyFromEmotion(CEmotionOpts &emOpts)`:
- Loop priorities descending, `GetBodyIndexFromEmotion` for each opt, first found wins, `SetBody(body, bIndex)`, break. If none found, `SetBodyNeutral`.

`CAvatarComplex::GetBodyFromEmotion` does the same separately for face and torso.

### 2.4 Neutral cycling — VERIFIED

`CAvatarSimple::SetBodyNeutral`:

```c
int c = m_lastBody;
for (i in 0..m_nBodies) { c = (c+1) % m_nBodies; if (bRec[c].emotion == EM_NEUTRAL && bRec[c].intensity == 0.0) { SetBody(body, c); return; } }
SetBody(body, 0);
```

Same for `SetTorsoNeutral` / `SetFaceNeutral` in Complex. Neutral cycling is rotation through `EM_NEUTRAL` + intensity `0.0` records in file order, starting after last. It is NOT random, NOT first-neutral-only, NOT a formula. If no neutral record exists, fall back to index 0.

`SetNeutral` for Simple: `GetBodyFromEmotion(CEmotion(0.0, 0.0))`. For Complex: `SetTorsoNeutral` + `SetFaceNeutral`.

## 3. AVB loader — VERIFIED, index = file order

`CAvatarX::LoadAvatar` (avbfile.cpp, Batch 2): read header, verify magic, create `CAvatarSimple` vs `CAvatarComplex` by `avh.nType`, loop tags until `AK_STARTDATA`, dispatch to `HandleLoadTag`.

`CAvatarSimple::LoadBodyRecs` / `CAvatarComplex::LoadFaceRecs` / `LoadTorsoRecs`:

```c
Read16(nCount);
bRec = malloc(sizeof(*bRec) * nCount);
m_nBodies = nCount; // nFaces / nTorsos for Complex
for (i = 0; i < nCount; i++) {
  Read(bodydata);
  if (imageOffset != prev) { bRec[i].poseID = CreatePoseWithMask(...); }
  else { bRec[i].poseID = bRec[i-1].poseID; } // ditto case
  bRec[i].emotion = EmotionToFloat(nEmotion); // nEmotion is emFloats INDEX, not float
  bRec[i].intensity = byIntensity / 255.0;
}
```

- Index IS file order: `bRec[0]` is first body record in file, `bRec[16]` is 17th, etc.
- AVB `nEmotion` is an index into `emFloats[]`, converted via `EmotionToFloat`. `EmotionToFloat` returns 0.0 if out of range.
- AVB intensity is `byIntensity / 255.0` (0..1 in 1/255 steps). Wire intensity is `intensity * 10` truncated (0..1 in 0.1 steps). Do not confuse the two quantizations.
- Ditto case: if image offset repeats, poseID is reused from previous record. Two records can share a poseID but have different emotion/intensity.

## 4. Live wire decode — VERIFIED against §1-§3

Notation: `G<tIdx><tEmo><tInt> E<fIdx><fEmo><fInt> [R]` where each char is `ByteToIndex`. `tEmo`/`tInt` for Simple are always `1`/`0` (0.0/0.0). `fIdx` for Simple is always `0`.

### Geordi (Simple, per live logs)

| Shorthand in logs | Full (reconstructed, M not in shorthand) | Torso idx | Face emo | Face int | Reading |
| --- | --- | --- | --- | --- | --- |
| G010 | `(#G010E010M1)` — E part inferred, see note | 0 | 1 (0.0) | 0.0 | Neutral, body 0 |
| G110 | `(#G110E010M1)` — E part inferred | 1 | 1 (0.0) | 0.0 | Neutral, body 1 |
| G710 | `(#G710E010M1)` — E part inferred | 7 | 1 (0.0) | 0.0 | Neutral, body 7 |
| G310E019 | `(#G310E019M1)` | 3 | 1 (0.0) | 0.9 | Happy, body 3, intensity 0.9 — Happy vs Neutral distinguished by body index + intensity, not by emotion byte (both 0.0) |
| G610E069 | `(#G610E069M1)` | 6 | 6 (ANGRY) | 0.9 | Angry |
| G910E059 | `(#G910E059M1)` | 9 | 5 (SAD) | 0.9 | Sad |
| G510E039 | `(#G510E039M1)` | 5 | 3 (BORED) | 0.9 | Bored |
| G810E049 | `(#G810E049M1)` | 8 | 4 (SCARED) | 0.9 | Scared |
| G410E079 | `(#G410E079M1)` | 4 | 7 (SHOUT) | 0.9 | Shout |
| G210E089 | `(#G210E089M1)` | 2 | 8 (LAUGH) | 0.9 | Laugh |

Note on Geordi neutrals: logs preserved only the `G` part in shorthand. `E010` (face 0, emo 1, int 0) is inferred from `SetBodyNeutral` (neutral = EM_NEUTRAL 0.0 + intensity 0.0, which encodes as `1`/`0`). Re-check `debug.log` for the full `E` part before treating `E010` as observed.

### Trekkie2018 (Simple)

| Wire | Torso idx | Face emo | Face int | R | Reading |
| --- | --- | --- | --- | --- | --- |
| `G210E0:8` | 2 | 10 (WAVE) | 0.8 | — | Wave, body 2 |
| `GJ10E0;9` | 26 (`J`) | 11 (POINTOTHER) | 0.9 | — | PointOther, body 26 |
| `G@10E029R` | 16 (`@`) | 2 (COY) | 0.9 | R | Coy, body 16, frozen |
| `GH10E028R` | 24 (`H`) | 2 (COY) | 0.8 | R | Coy, body 24, frozen |
| `GI10E027R` | 25 (`I`) | 2 (COY) | 0.7 | R | Coy, body 25, frozen |
| `G610E019R` | 6 | 1 (0.0) | 0.9 | R | Happy, body 6, frozen |
| `GB10E016R` | 18 (`B`) | 1 (0.0) | 0.6 | R | Happy, body 18, frozen |
| `GB10E016R` / `G<10E015R` | 28 (`<`) | 1 (0.0) | 0.5 | R | Happy, body 28, frozen |
| `GA10E059R` | 17 (`A`) | 5 (SAD) | 0.9 | R | Sad, body 17, frozen |
| `G510E058R` | 5 | 5 (SAD) | 0.8 | R | Sad, body 5, frozen |

Full wire is `(#<above>M<mode>)`. `M` was not in the shorthand logs; `M1` (say) is assumed for these, not observed.

Why Happy/Coy/Sad have multiple bodies: `GetBodyIndexFromEmotion` / `GetBodyFromEmotion` pick by angle + closest intensity, with rotation from `m_lastBody`. Multiple records share the same emotion float but differ in intensity and poseID. That is why Coy appears as bodies 16/24/25 at 0.9/0.8/0.7, Happy as 6/18/28 at 0.9/0.6/0.5, Sad as 17/5 at 0.9/0.8.

## 5. Withdrawn / NOT verified — do not use

- **Withdrawn:** Colon-delimited codes (`G:...`, `E:...`). No colons in `bInsertAnnotations` or `ProcessSay`.
- **Withdrawn:** `G000`-to-`G010` patch. No such fix in source. `G010` is torso 0 / emo 1 / int 0, not a corrected `G000`.
- **Withdrawn:** Angle-form canonical wire. Wire is `'0'+index` + `'0'+emFloats-index` + `'0'+intensity*10`, not angle strings.
- **Withdrawn:** `G`'s `10` as a magic constant. It is torso emotion 1 + torso intensity 0, from `GetEmotions` for Simple.
- **Withdrawn:** `E`'s `0` as a magic constant. It is face index 0 for Simple.
- **Withdrawn:** `R = wheel-selected` as definition. `R = (freeze != UNFROZEN)`.
- **Withdrawn:** Face emotion universal / body per-AVB as the whole story. For Simple, face emotion = bodyRec emotion, torso emotion = 0.0. For Complex, face and torso have separate recs. Index + emotion + intensity are all sent because same-AVB uses index, substitute-AVB uses emotion.
- **NOT verified from these batches:** `SM_*` numeric mode values (need header with `SM_*` defines). `CGESTUREPREFIX` etc. `#define` values (need header). Exact keyword strings for `FindString`/`CheckWord`/`CheckStart` (need resource strings for `ruleIDs`). Wheel-to-freeze mapping. Full `E` part for Geordi neutrals (need `debug.log` full wire). `M` value for the 20 live codes (shorthand omitted `M`).

## 6. What to implement in Rust (comic-chat-rs / open-comic-chat)

- `PoseCode::to_wire_string` must be per-AVB table lookup, not a formula. Lookup: given current `bodyRec` (index, emotion, intensity) + freeze state, emit `(#G<torsoIdxByte><torsoEmoByte><torsoIntByte>E<faceIdxByte><faceEmoByte><faceIntByte>[R]M<modeByte>)` using `IndexToByte` (`+ '0'`) and `EmotionToBytes` (loop from 1, default 9, intensity*10). For Simple: torsoIdx = body index, torsoEmo = `1`, torsoInt = `'0'`, faceIdx = `'0'`, faceEmo/int from bodyRec.
- `parse_pose` must be the inverse: `ByteToIndex` (`- '0'`) for each of the 6 bytes, `R` -> `bbReq`, `M` -> `SM2BM`. Then `SetIndices` if not `OTHERMAPPED`, else `BytesToEmotion` + `SetEmotions`.
- Preview == sent at `pose_id` level: preview must use the same `GetBodyFromEmotion` / `GetIndices` / `GetEmotions` path as send, not a separate preview-only mapping. Test: for each of the 20 live codes, `to_wire_string(parse(wire)) == wire` (round-trip), and `parse(wire).body_index == wire body index` for same-AVB case.
- Neutral cycling must be `SetBodyNeutral`: cycle `(last+1)%n` through `EM_NEUTRAL` + `0.0` records in file order. Do NOT pick first neutral every time, do NOT randomize, do NOT use a formula.
- `GetBodyFromEmotion` must implement: rotation from `m_lastBody`, skip `emotion > 7` for single-emotion path, angle `< PI/8` + closest intensity, exact match for `> 2*PI` (WAVE/POINT/etc.), priority loop for `CEmotionOpts`, fallback to `SetBodyNeutral`.
- AVB loader must implement: `nCount` then sequential `bRec[i]` in file order, `emotion = emFloats[nEmotion]`, `intensity = byIntensity/255.0`, ditto poseID reuse. Do NOT treat AVB `nEmotion` as the float itself.
- Per-AVB tables: `docs/pose-tables/trekkie2018.json` + `geordi.json` are the source of truth for body indices. `pose.rs` test must load them and assert Coy/Happy/Sad/Wave/PointOther/neutral cycles from §4. See `docs/pose-tables/README.md`.

## 7. Tests to add (must pass before merge)

1. Round-trip: for each wire in `wire-codes-verified.md`, `parse(to_wire(parse(wire))) == wire` (with `M` normalized).
2. `EmotionToBytes`: `0.0` -> `'1'`, `COY` -> `'2'`, `WAVE` -> `':'`, intensity `0.9` -> `'9'`, `0.0` -> `'0'`.
3. `BytesToEmotion`: `'1'` -> 0.0, `':'` -> 1001.0, `'9'` intensity -> 0.9.
4. Simple `GetEmotions`: bodyRec COY 0.9 -> face COY 0.9, torso 0.0/0.0.
5. Simple `GetIndices`: body 16 -> torso 16, face 0, `bbRequested` = freeze != UNFROZEN.
6. Neutral cycle: Trekkie/Geordi neutral records in file order, rotation from last, fallback to 0.
7. `SetEmotions` fallback: OTHERMAPPED, COY 0.8 on a different AVB picks closest-intensity COY record.
8. Preview == sent: same body index for same input text + freeze state via `ChatPreSendText` path.


---

## 8. Implementation addenda — 2026-11-23, post-Batch-1/2 (code, not v2.5 source)

These are implementation learnings from `open-comic-chat` code + live `debug.log`. They do NOT override §1-§7. Where they conflicted with §1-§7 in an earlier draft, §1-§7 wins — see §8.6 corrections.

### 8.1 Two levels: table selection vs wire encoding — do not conflate them

- Table selection uses AVB `nEmotion` (index into `emFloats[]`). Trekkie Neutral bodies have `nEmotion == 9` (`EM_NEUTRAL`), Happy bodies have `nEmotion == 1` (`EM_HAPPY`), Coy `2`, etc.
- Wire encoding uses `EmotionToBytes(float)`: `0.0` -> wire index `1`, never `9`. So a Neutral body (`nEmotion 9`, float `0.0`) encodes on the wire as face emo `1`, int `0`: `E010`. A Happy body (`nEmotion 1`, float `0.0`) encodes as face emo `1`, int `9`: `E019`. They are distinguished on the wire by body index + intensity, not by emotion byte (§1.3, §4).
- `pose_lookup::body_for_pose_cycled()` therefore matches on `nEmotion` (the table's 2nd field, `u16`, compare as `face_emo as u16` — `E0308` fix) to pick the body, then `wire_for_body()` returns the table wire verbatim for that body (including `R` when the table has it), not a recomputed wire from the lookup key.
- `PoseCode::single_face_emo()` must return the *wire* pair, not the table key. For Neutral it must return `(1, 0)`, not `(9, 0)`. The `(9, 0)` value is the table-selection key, not the wire pair.

### 8.2 Neutral cycling — implementation

- `PoseCode::to_code_string_for_avb_cycled(avb_name, cycle)` and `to_wire_string_for_avb_cycled(pose_style, avb_name, cycle)` take an explicit `cycle`.
- All 3 send sites in `app.rs` (`send_current` empty `<Chr>`, `send_current` text, third `SendMessage` site) call `to_wire_string_for_avb_cycled(..., self.neutral_cycle)`.
- Capture `__cycle_should_advance` before any pose is used: `pose.emotion() == Neutral && !wheel_frozen`. Advance `neutral_cycle` once, after send, so preview == wire == local. Do NOT use a global / thread-local counter in `body_for_pose` — preview would advance it and desync preview from wire.
- Cycling order is file order through `EM_NEUTRAL` + `0.0` records from `m_lastBody` (§2.4), not a hardcoded list. Trekkie file order happens to be bodies `0, 4, 8, 19` (`G010, G410, G810, GC10`); Geordi `0, 1, 7` (`G010, G110, G710`). Do not hard-code those as the algorithm.

### 8.3 AppearsAs — implementation

- `ComicChatApp::appears_as_sent: HashSet<String>`, key `peer|char_name`. Helper `should_send_appears_as(peer, char_name) -> bool` returns `false` if already sent, else inserts and returns `true`.
- Own join: public `# Appears as <name>` once, `peer = ""`. Peer join: whisper-only reply to that peer, once per peer per character.
- `E0609` / `E0124` / `E0063` fix: the field belongs in `struct ComicChatApp`, the initializer belongs in `let mut app = Self { ... }` constructor. Inserting `appears_as_sent: HashSet::new(),` into the struct definition causes `E0124` (duplicate) + return-type-notation error. Inserting after the struct's `neutral_cycle:` field (a type, not a value) causes the same. Insert only after the constructor's `neutral_cycle:` value line.
- Parser: format is `# Appears as <name>[.url]` (§1.8, `APPEARSPREFIX`). Split name + optional `.url`. Do NOT blindly strip trailing `.` / `?` — the trailing dot may be the `.url` delimiter. `clean_appears_as_name()` in an earlier draft that stripped `.` / `?` is superseded by name+URL split. Receiver must apply as `char_map[peer] = name`, not a whisper panel.

### 8.4 Transport / panels / <Chr> — implementation

- `irc.rs::with_pose` is the single wrapper producing `(#G...) text`. Do not wrap in `app.rs`. OUT must be `(#G...) text` exactly once.
- IRC line-based: sanitize pasted `\r` / `\n` to spaces, never split one PRIVMSG.
- Pose-only Enter = `<Chr>`: build pose BEFORE clearing `panel_cast`; clearing first made `pose.cast` empty, wire `<Chr>` had no `Tnick`, peers never added the included user. Local echo via `pending_chr_cast` masked the wire bug.
- Panels (implementation choice, not v2.5 source): `normal_panel_w = 320.0` fixed, `per_row = floor(available_width / 320)`, at least 1. At ~1450 wide, 4 per row; at ~1000, 3 per row. Do not grow panels to fill row.
- Wheel (implementation): `wheel_gesture` buttons for Wave / PointSelf / PointOther set `Gesture::Wave / PointSelf / PointOther`. All AVB poses must be reachable from wheel, not only via typed triggers.

### 8.5 What `cargo test` now covers

- `E0308`: `avb_emo` is `u16`, `face_emo` is `u8` — compare as `*avb_emo == face_emo as u16`.
- `E0609` / `E0124` / `E0063`: `appears_as_sent` field + constructor init (see §8.3).
- After `Apply-Fix-AppearsAsSent-Field3.py`, `cargo test` passes.

### 8.6 Corrections to the 2026-11-23 draft that preceded this file — that draft was WRONG, this file wins

1. **Neutral wire is `(1, 0)`, NOT `(9, 0)`.** The earlier draft claimed fixing Neutral to `(9, 0)` fixed a Neutral/Happy collapse. Per §1.3 `EmotionToBytes`, `0.0` encodes as `1`, never `9`. `9` is the table `nEmotion` index for `EM_NEUTRAL`, not the wire byte. The earlier patch that changed `single_face_emo()` Neutral to `(9, 0)` must be reverted for wire encoding; table selection still uses `9`.
2. **`M1` is NOT verified.** The earlier draft said `M1` for normal / say. Per §1.1 / §5, `SM_*` numeric values were NOT in the two batches. Do NOT hard-code `M1` / `M2` / etc. as verified.
3. **`hello -> Wave`, `lol -> Laugh`, etc. are NOT verified keywords.** The earlier draft listed them as verified triggers. Per §2.2, keyword strings live in resources, `#if 0` code is disabled, `ruleEMs[]` order is known but strings are not. Do NOT hard-code a keyword list in SPEC.
4. **`R` is freeze only, NOT strength.** The earlier draft said `R` when frozen OR full-strength. Per §1.7, `R = (freeze != AF_UNFROZEN)`. Strength does not set `R`.
5. **AppearsAs clean that strips `.` / `?` is superseded.** See §8.3: split name + `.url`, do not strip.
6. **Single CEmotion only is incomplete.** The earlier draft said no Complex. Per §1.6, `CAvatarComplex` has separate face + torso recs with separate `GetIndices` / `SetEmotions`. Simple vs Complex must both be handled.

