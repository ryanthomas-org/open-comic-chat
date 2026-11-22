# Microsoft Comic Chat — Technical Specification

<!-- STAY-ON-TRACK -->
> **Stay on track:** Read `GOALS.md` + this spec first, before coding. If code and docs
> disagree, stop. Fix docs + code together, do not silently drift. After every
> change, update both files: `GOALS.md` status + this spec if a rule changed.
> `GOALS.md` is the goal list. This spec is the wire / rendering truth.
> When they conflict, the Microsoft source wins, then this spec, then `GOALS.md`.
<!-- /STAY-ON-TRACK -->


**Document status:** Preliminary technical document for `comic-chat-rs`.
**Target:** `comic-chat-rs 0.3.17` (frozen `3.3` line). Do not bump version without consent.
**Authority rule:** Where Part B differs from Part A or from the code Part A is based on, Part A and the `crates/` libraries win. Part B is contextualisation, not correction. If a Part A detail is found wrong against the Microsoft source, fix the library first, then this spec — never the reverse.

**Sources for Part A:** Direct analysis of the Microsoft Chat MIT-licensed open-source code (C++ / MFC Comic Chat client): `chat.cpp`, `chatdoc.cpp`, `panel.cpp/.h`, `avatar.cpp/.h`, `bodycam.cpp/.h`, `balloon.cpp/.h`, `arc.cpp`, `traj.cpp`, `spline.cpp`, `vector2d.cpp/.h`, `textpose.cpp`, `textview.cpp/.h`, `semantic.cpp`, `format` / `chatprot`, `ircproto.h`, `ircsock.cpp`, `protsupp.cpp`, `userinfo.cpp/.h`, `histent.cpp/.h`, `pageview.cpp`, DIB / AVB loader, `chatver.h`, `bbox.h`, `pe.h` — plus the Rust rewrites in `crates/` in this bundle, which codify the same findings. Every claim in Part A traces to one of those two. Claims in Part B are marked as supplemented.
Real AVB (verified Trekkie2018.AVB, 27 gestures in 0x0C list) is typed u16 records, not [tag][len] chunks; no POSE chunk exists - pose code is gesture entry unk1 (bytes 12-13): 1=Happy, 2=Coy, 3=Bored, 4=Scared, 5=Sad, 6=Angry, 7=Shout, 8=Laugh, 9=Neutral, 10=Wave, 11=Point, 12=Gesture; face anchor is bytes 15/17; falling back by DIB order showed Happy for Coy.

**How to read this document:** §1–§9 are Part A (authoritative, code-derived). §10 is the assessment of the existing `comic-chat-rs 0.3.17` file set and the changes needed. §11–§16 are Part B (supplemented full-spec material). Appendix A is the file / symbol map. Appendix B lists what was *not* established by the code analysis and must be re-checked against the Microsoft source before being relied on.

---

# Part A — Findings from the code analysis (authoritative)

## 1. Architecture: what the Microsoft client actually is

### 1.1 Layering

The client is a classic MFC document/view application, not a single-window chat box:

| Layer | Files | Job |
|---|---|---|
| App / frame | `chat.cpp`, `chatdoc.cpp`, `pageview.cpp` | App object, main frame, document, view switching (Text ↔ Comics), menus/toolbars, status bar |
| Protocol | `ircsock.cpp`, `ircproto.h`, `protsupp.cpp`, `chatprot` | Socket, IRC line parser, IRCX extensions, CTCP, slash commands, flood control |
| Model | `chatdoc.cpp`, `histent.cpp/.h`, `userinfo.cpp/.h` | Room state, history entries, user info, cast, topic, member list |
| Panel / layout | `panel.cpp/.h`, `pageview.cpp` | Group entries into panels, title card, establishing shot, avatar placement, zoom, scroll |
| Rendering | `balloon.cpp/.h`, `avatar.cpp/.h`, `bodycam.cpp/.h`, `arc.cpp`, `traj.cpp`, `spline.cpp`, `vector2d.cpp/.h`, `textpose.cpp`, `textview.cpp/.h`, `bbox.h` | Balloon outlines, avatar compositing, geometry |
| Text / format | `format`, `semantic.cpp`, `textpose.cpp` | Per-byte formatting, pose codes, OEC, semantic classification |
| Assets | DIB / AVB / BGB loader, `chatver.h` | Avatar + background files, version |

**Consequence for the Rust rewrite:** the Rust crates in this bundle mirror this layering exactly (`comic-protocol`, `comic-panel`, `comic-balloon`, `comic-avatar`, `comic-geometry`, `comic-text`). Do not collapse layers (e.g. do not put balloon math in the IRC module, do not put pose parsing in the avatar module). The original's bugs were mostly *layer* bugs — one layer guessing another layer's job — and the Rust rewrite must not repeat them.

### 1.2 Document / view split

- `CChatDoc` owns the room state: current channel, topic, member list, history (`CHistEnt` list), cast, and the "appears as" / "background as" selections. It does **not** own pixels.
- `CPageView` / panel view owns layout + rendering. It asks the document for entries and lays them out.
- History entries (`CHistEnt`) are the unit of record: one entry = one message / action / join / part / system line, with speaker, timestamp, text, pose, balloon style, avatar, background. Panels are a *view* over history, not the storage. **Never store panels as the history** — regrouping (e.g. on resize / zoom change) must be possible from history alone. The 0.3.17 `src/comic.rs` stored panel groupings in a way that made regrouping lossy; see §10.

### 1.3 Text vs Comics mode

- The client has two views over the same document: Text view (scrolling log) and Comics view (panels). Switching views does not re-fetch history and does not re-parse messages.
- Comics-mode peers negotiate via IRCX / CTCP (see §7). A peer that does not negotiate Comics mode receives plain text (pose codes stripped, formatting stripped to plain text). **Sending pose codes to a non-Comics peer is a protocol violation** — the original gated pose/OEC encoding on the peer's negotiated mode. The 0.3.17 code sent pose codes unconditionally; see §10.
- The "Say" window (message input) is shared. The balloon-style selector (Speech / Thought / Shout / Whisper) and the emotion wheel are Comics-view controls; they are inert in Text view but their state persists across view switches.

### 1.4 Versioning

- `chatver.h` defines the client version string used in CTCP VERSION replies and in the About dialog. The 0.3.17 Rust `src/version.rs` carries its own version; the two must agree for a given release (see §10: they did not in the supplied file set — `version.rs` was a stub).
- Version `2.5` is the last Microsoft Comic Chat release and the wire-compatibility target. Earlier versions (1.0, 2.0) are **not** wire targets; where they differ, 2.5 wins.

---

## 2. Geometry — `vector2d`, `traj`, `arc`, `spline`, `bbox`

Codified in `crates/comic-geometry`.

### 2.1 Coordinates and angles (read this before touching any drawing code)

- Points are integer device points in panel space in the original (`CPoint` / `DPOINT`, `long` / `int`). The Rust crates use `f64` internally and round only at the rasteriser, because B-spline / arc subdivision is not integer-stable. Layout must match the integer original to within 1 device px.
- **Y grows down** (GDI / screen space). Angles are in degrees, 0 = +X (east), increasing **clockwise** in screen space (toward +Y). This is the `vector2d` convention with `atan2(y, x)` on screen coordinates. Do **not** "fix" it to maths convention (counter-clockwise, Y up) — every tail, arc, and spike flips vertically if you do.
- Angles are always normalised to `[0, 360)` on input to arc / angle helpers (`normalise_angle_deg`). Callers must not pass unnormalised angles and expect wraparound elsewhere.
- `VectorAdd`, `ScalarMult`, `Magn` are the primitive operations. The original did **not** guard division by zero magnitude — callers dividing by magnitude got NaN. The Rust `Vector::normalised` returns zero for a zero vector instead of NaN. This is a deliberate, documented deviation (NaN is never the right tail direction).

### 2.2 `vector2d` — observed operations

| Original | Rust | Semantics |
|---|---|---|
| `VectorAdd` | `Vector::add` | Component-wise add |
| `ScalarMult` | `Vector::scale` | Scale by scalar |
| `Magn` / `Magnitude` | `Vector::magnitude` | Euclidean length |
| `VectorToAngle` | `vector_to_angle` | `atan2(dy, dx)` → degrees, normalised |
| `AngleToVector` | `angle_to_vector` | Unit vector at angle |
| `AngleBetween` | `angle_between` | Smallest absolute difference, `[0,180]` |
| `Rotate` | `rotate` | Rotate clockwise in screen space |

Dot and cross products are used for tail / spike orientation (cross sign = clockwise vs counter-clockwise in screen space). They are in the Rust crate as `dot` / `cross` even though the original inlined them in several places.

### 2.3 Arcs — `arc.cpp`

- `ScanArcAux` walked an arc in fixed angular steps and emitted points on the ellipse. `DrawArc2` drew the resulting polyline. `DashArc2` drew it dashed by skipping runs.
- The Rust crate splits this into two functions so fill and stroke share geometry: `scan_arc` (polyline) + `dash_polyline` (split into "on" runs). Dashing is measured **along the polyline**, the first dash starts "on", and a trailing partial dash is **kept**, not dropped. (The 0.3.17 `app.rs` dropped trailing partial dashes — wrong for whisper outlines.)
- Default arc step: 3 degrees when the caller passes `<= 0`. The original used a step that kept chord error small for the radii in use; 3° is the value observed for balloon-sized arcs.

### 2.4 Trajectories — `traj.cpp`

- `traj.cpp` provided the path helpers used for tails and think-trails: quadratic Bezier evaluation (control-point → tip) and arc-length parameterisation. The Rust `tail_polygon` / `think_trail` use the same quadratic Bezier (mid → control → tip, sampled at 6 steps for tails, 3 circles for trails). **Do not replace the Bezier with a straight line** — bent tails are a visible part of the original look.
- Arc-length parameterisation is used for waves / spikes so the wave count is resolution-independent (same wave count regardless of B-spline subdivision). The 0.3.17 `app.rs` parameterised waves by *point index*, so changing `per_seg` changed the wave count — wrong.

### 2.5 B-splines — `spline.cpp`

- Balloon outlines are closed uniform cubic B-splines through control points (see §4). The Rust `bspline_closed` uses the standard uniform cubic basis. It is closed: the last point joins the first. Control points are corners + edge midpoints of the text rect (8 points). `per_seg` = subdivisions per span; the 0.3.17 `app.rs` used 8.
- The B-spline stays inside the convex hull of its control points. This is why balloon padding works: the outline never overshoots the control rect by more than the spline's smoothing.

### 2.6 Bounding boxes — `bbox.h`

- `BBOX` / `CRect`-like use: axis-aligned boxes for hit-testing, clipping, and layout. `BBox::from_points`, `contains`, `inflate`, `centre` are in the Rust crate. Hit-testing a balloon uses its bbox first (cheap reject), then the polyline (exact).

---

## 3. Text, pose codes, OEC, formatting, semantic analysis

Codified in `crates/comic-text`.

### 3.1 Pose codes — the single most-misunderstood wire detail

**Canonical v2.5 wire form (authoritative, from the Microsoft source):**

```
(#G3:1E0:1M1) hello
```

Breakdown:

- `(#` — literal open.
- `G3:` — gesture 3, followed by a colon. **One space after `#`? No.** The exact bytes are `(#G3:1E0:1M1)`: no space after `#`, no space after `G3:`, one space after the closing `)` before the message text.
- `1E0` — intensity digit `1`, letter `E`, expression digit `0` (Neutral). The intensity digit is `1` in practice for user-selected poses; `0` for default. It is **not** the gesture.
- `:` — colon separator between expression and mouth.
- `1M1` — intensity digit `1`, letter `M`, mouth digit `1`.
- `)` — literal close, then a single space, then the message text.

**What the 0.3.17 `src/pose.rs` got wrong (and what to do):**

| 0.3.17 behaviour | Correct behaviour |
|---|---|
| Generated `(#G2:8E0:8M1)` — colon between gesture and expression, intensity `8` | Generate `(#G3:1E0:1M1)` — colon only after `G<n>` and between `E` and `M` groups, intensity `1` |
| Defaulted to a gesture and emitted a pose code on every message | `PoseStyle::None` is the default: a plain message sends **no** pose code at all |
| Parsed only its own invented form | Parse canonical + bare `(#1E0:1M1)` + colon form (compat only — never emit colon form) |

**Parsing rule:** `parse_pose_code` accepts all three forms, returns `(PoseCode, rest)`. Generation uses **only** the canonical form. This asymmetry is deliberate: be liberal in what you accept, strict in what you send.

### 3.2 Emotion wheel — 8 emotions + Neutral centre, fixed cyclic order

<!-- CORRECTED-32-2026-11-21 -->

**Real MS Chat wheel (help file + Kurlander):** 8 peripheral emotions + Neutral at the centre.

- Peripheral, clockwise from top: **Shout, Laugh, Happy, Coy, Bored, Scared, Sad, Angry**.
- Centre: **Neutral**. Neutral is not a wheel position. `comic.rs` `Emotion::all()` returning
  the 8 peripheral emotions, excluding Neutral, matches the help file's "eight different emotions".
- **Struck:** Disgusted, Contemptuous, Surprised, Afraid are NOT MS Chat wheel emotions.
  `Scared` is the MS name, not `Afraid`. `Laugh` is real. `Shout` is both a wheel emotion
  and a balloon style.
- Cyclic order in `comic.rs` `angle_deg` (Happy, Coy, Bored, Scared, Sad, Angry, Shout, Laugh)
  is the same cycle as above, just rotated. Do not "fix" the order by re-sorting.

**E-digit mapping — UNTRUSTED:** Do NOT assume `E0`..`E7` = wheel index in the order above.
The old spec claimed that. It was wrong about the wheel set, so its E-digit claim is
untrusted too. Re-derive the `E` digit mapping from `textpose.cpp` / `avatar.h` in the
Microsoft open-source tree before relying on it for wire encode / decode. Until then,
treat `E` digits as opaque in the spec, and keep `PoseCode` encode/decode covered by
tests against captured real-client bytes, not by this paragraph.

**CORRECTED-2026-11-21 — verified against live MS Chat 2.5 + Trekkie2018 AVB PoseEntry table:** OldAngle `(#G045E000M1)` is NOT parsed by MS Chat 2.5 — it falls back to Neutral, while this client parsed it locally, so local showed Coy/Wave and MS Chat showed Neutral. Canonical v2.5 `(#G<g>:1E<e>:1M1)` IS parsed. `E` = face_index, `G` = torso_index, same indices local rendering uses for `PoseEntry`: E Neutral 0, Happy 1, Coy 2, Bored 3, Scared 4, Sad 5, Angry 6, Shout 7, Laugh 8 / G Neutral 0, Wave 1, PointSelf 2, PointOther 3, DoublePoint 4, Shrug 5, Walk3Q 6, SideWalk 7, Walk3QF 8. Neutral/Neutral sends no code. `Contemptuous` / `Disgusted` / `Surprised` / `Afraid` are NOT MS Chat emotions — the `comic-text` crate table using them mis-maps E2 (Coy) as Sad and drops E8 (Laugh); parse E digit directly as face_index, do not go through that table.

### 3.3 `Appears as` / `Background as`

- Sent as plain lines: `Appears as <name>` / `Background as <name>` (from `protsupp` / `chatdoc`). They are **not** pose codes, not CTCP, not slash commands on the wire (the client-side `/appears` / `/background` slash commands *produce* these lines).
- Parsing is case-insensitive on the prefix, case-sensitive on the name. Leading/trailing whitespace on the name is trimmed.
- `Appears as` changes the avatar for subsequent messages from that speaker. `Background as` changes the panel background for subsequent panels. Both persist until changed again (they are sticky state, not per-message).

### 3.4 OEC — Outgoing Encoding Control

- OEC is the `\x01`-style prefix the client prepends to encode formatting / colour / font for Comics-mode peers. The exact byte is version-dependent; v2.5 used `0x01` (SOH) as the escape.
- `oec_encode` prefixes `0x01 + <format bytes> + 0x01` when a non-default format is present, else returns the text unchanged. Format bytes: `B` (bold), `I` (italic), `U` (underline), `C<RRGGBB>` (colour). `oec_decode` reverses it.
- **OEC is only sent to Comics-mode peers.** To a Text-mode peer, send plain text (formatting stripped). See §7.
- **What was NOT established:** the exact OEC byte sequence for every format combination (bold+colour+font together) was not fully pinned down by the code analysis. The Rust crate encodes the observed subset (`B`/`I`/`U`/`C`). Font-name encoding is marked as "to be verified" in Appendix B. Do not invent font encoding — verify against the source before emitting font OEC.

### 3.5 Formatting — `format`

- Formatting is stored per-character in the original (`format.cpp`), with at most one formatting change per byte of payload (`MAX_FORMATTINGPERBYTE`). The Rust crate exposes a per-run view (`FormatRun`: bold/italic/underline/colour/font) because the renderer works in runs, not per-character.
- `MAX_INPUTLEN = 512` — input line limit. The Say window enforces it; the protocol layer also enforces it (see §7, 512-byte line limit including CRLF).
- Colour is RGB. Font is a font-name string. The original's font handling was Windows `LOGFONT`-based; the Rust rewrite must map font names to Linux fonts (see §10 — font mapping is a Linux porting task, not a wire task).

### 3.6 Semantic analysis — `semantic.cpp`

`CSemantic` classified a message to pick the *default* balloon / pose when the user did not pick one. Observable rules (Rust `classify_message`):

- `/me ...` or `* ... *` (wrapped in asterisks) → Action (narrative, no balloon tail to speaker in some renderings).
- Leading `name:` / `name,` where `name` is a member → Directed.
- Help-verified triggers (MS Chat help file / Kurlander) — these are real, not invented:
  - `LOL`, `ROTFL` → Laughing. `:-)` / `:)` → Happy. `:-(` / `:(` → Sad. `;-)` → Coy.
  - All capitals → Shouting. `!!!` → Shouting.
  - `I`, `I'm`, `I will`, `I'll`, `I am` at sentence start → point to self.
  - `You`, `are you`, `will you`, `did you`, `aren't you`, `don't you` → point to other.
  - `Hello`, `Hi`, `Bye`, `Welcome`, `Howdy` → Wave.
  - When no emotion is specified and none can be inferred, cycle between multiple neutral gestures.
  - **An emotion hint does NOT override an explicit pose.** It only fills in a pose when the user sent none.
- Otherwise → Plain.

**What is UNVERIFIED (do not treat as spec):** extra word lists in `comic.rs` `analyze()`
(`yay`, `sad` / `cry`, `bored` / `zzz`, `scared` / `afraid` / `eek` / `help`,
`angry` / `grr` / `wtf` / `damn`, `hmm` / `thinking`, `?`, `!` / `wow` / `omg`)
and `Question` / `Exclaim` balloon types. The help file lists speech / thought /
whisper balloons + action boxes only. Keep or drop the extras only after checking
`semantic.cpp` / `textpose.cpp`. Do not invent additional emoticons.
<!-- CORRECTED-36-2026-11-21 -->

---

## 4. Balloons — `balloon.cpp`, `bodycam.cpp`

Codified in `crates/comic-balloon` + `comic-geometry`.

### 4.1 The balloon is NOT a rounded rect

Construction (authoritative):

1. **Text rect** = measured text + padding.
2. **Control points** = corners + edge midpoints of that rect (8 points, clockwise from top-left corner).
3. **Closed B-spline** through those control points (`bspline_closed`, `per_seg = 8`).
4. **Style transform:** Speech = smooth; Whisper = wavy + dashed outline; Thought = wavy + think-trail, no tail; Shout = spiked.
5. **Tail** (Speech / Shout / Whisper) from balloon edge to speaker's head anchor, with a bend. Thought uses a trail, not a tail.

**The 0.3.17 `app.rs` drew balloons as rounded rects with a triangular tail — wrong.** It must be replaced with the B-spline construction above. The rounded-rect look is visibly different (corners too sharp, edges too straight).

### 4.2 Metrics (device px at 96 DPI, zoom 1.0)

| Metric | Value | Notes |
|---|---|---|
| `pad_x` | 12 | Horizontal padding, each side |
| `pad_y` | 8 | Vertical padding, each side |
| `min_w` | 48 | Minimum balloon width |
| `min_h` | 32 | Minimum balloon height |
| `tail_half_width` | 10 | Half-width where tail meets balloon |
| `tail_bend` | 6 | Lateral offset at tip control point |
| `dash_on` / `dash_off` | 6 / 4 | Whisper dashed outline |
| `trail_radius` | 8 | Thought-trail base circle radius |

Multiply by zoom for other zooms. **Do not bake zoom into the constants.**

### 4.3 Tails

- Tail base: the point on the balloon outline closest to the speaker's head anchor, offset along the outline so the tail base straddles it. The base is perpendicular to the direction from balloon centre to head.
- Tail polygon: quad (base_left → base_right → tip) with a quadratic Bezier outer edge for bent tails (`tail_polygon`). The base edge is *inside* the balloon body and is **not** stroked separately — stroking it draws a line across the balloon interior (a 0.3.17 bug).
- Tail bend: lateral offset at the tip control point. `bend = 0` = straight tail.

### 4.4 Think-trails

- Thought balloons have **no tail**. They have a trail: **3 circles**, geometrically shrinking toward the head, along the same quadratic Bezier as a tail would follow.
- Radii: `base_radius * (1 - t * 0.7)` for `t = 1/4, 2/4, 3/4`. The 0.3.17 `app.rs` drew 2 circles — wrong.
- Each trail circle is filled with balloon fill + stroked with balloon outline.

### 4.5 Shout / whisper

- **Shout:** spikes added to the B-spline outline. Spikes are triangular: every `pitch = 18` px along the outline, push outward by `depth = 8` px along the local normal, with a triangular profile (peak at the spike centre, base on the outline). Points between spikes stay on the base curve. The 0.3.17 `app.rs` offset *every* point outward — that makes a bigger balloon, not a spiked one.
- **Whisper:** wavy outline (`add_waves`, amplitude 3 px, 12 waves) + dashed outline (`dash_on = 6`, `dash_off = 4`). The wave phase is a function of **arc length**, not point index (see §2.4).
- **Thought:** wavy outline (same as whisper) + think-trail, no tail, no dashes.

### 4.6 Colours

- Balloon fill = speaker's chosen balloon colour; default white `(255,255,255)`. Outline = black `(0,0,0)`. Whisper used a grey outline in some versions — keep black unless the theme says otherwise. Do not invent per-style colours.

---

## 5. Avatars — `avatar.cpp`, `bodycam.cpp`, DIB / AVB / BGB

Codified in `crates/comic-avatar`.

### 5.1 AVB file format

- AVB (avatar, v2.5) is a chunked file. Each chunk: `[4-byte tag][u32 little-endian length][payload]`.
- Known tags: `AVB ` / header, `DIB ` (one DIB image per pose part), `POSE` (pose table), `FACE` (face/mouth anchor table), `NAME`, `DESC`. **Unknown tags MUST be skipped by length, not treated as error.** The 0.3.17 `src/avb.rs` treated an unknown tag as fatal — wrong, and it broke on real AVB files that contain vendor tags.
- Chunks are **not** padded. Do not align to 4 bytes between chunks.

### 5.2 BGB file format

- BGB (background) uses the same chunk framing. Payload is a single DIB (the background image) plus `BGND` / size chunks.
- **The 0.3.17 bug:** BGB loaded (DIB decoded) but AVBs did not display, and BGBs did not display either — because the BGB path decoded the DIB but never registered the image as a background. The Rust `load_bgb` returns the decoded image explicitly. The caller must *use* the return value. This is a "return value ignored" bug, not a decoding bug.

### 5.3 DIB decoding

- DIB = Windows `BITMAPINFOHEADER` (40 bytes) + palette (if ≤8bpp) + pixel data.
- Supported: 1/4/8bpp palettised + 24bpp RGB, bottom-up (positive height) and top-down (negative height).
- Compression: `BI_RGB` (0) only in practice. `BI_RLE8` / `BI_RLE4` are **not** supported by the original and must be **rejected**, not silently mis-decoded. The 0.3.17 `src/dib.rs` attempted RLE and produced garbage — remove that path.
- Rows are padded to 4-byte boundaries. Palette entries are BGRA (B, G, R, reserved) — convert to RGBA. Palette entry count = `colours_used` if non-zero, else `2^bpp`.
- The original converted to a DDB for display; the Rust crate decodes to RGBA (row-major, top-down). The renderer (egui) takes RGBA.

### 5.4 Halo / mask

- AVB body images carry a 1-bit mask (the "halo") used to composite the avatar over the background without a rectangular edge. The mask is stored as a second DIB (same dimensions) or as the alpha channel in 32bpp variants.
- `apply_mask`: where mask pixel is black (0), set alpha to 0. Where white, opaque. Do not threshold at 128 for the *mask* itself — the mask is 1-bit; the Rust crate thresholds at 128 only because the mask arrives as RGBA (black vs white). If the mask arrives as 1bpp, decode it first, then apply.

### 5.5 Pose selection — fallback order (authoritative)

A pose is (gesture, expression, mouth). The avatar file lists, for each gesture, the image indices for each expression; the mouth is a separate overlay image anchored at the face anchor. If the exact (gesture, expression) pair is missing, fall back in this order:

1. Same gesture, Neutral expression.
2. Gesture 0 (standing), requested expression.
3. Gesture 0, Neutral.
4. (Last resort) First pose in the table.

**Never fall back to a random gesture.** The 0.3.17 code fell back to index 0 unconditionally — wrong, it showed the wrong pose for missing combinations.

### 5.6 Face / mouth anchors

- `FACE` chunks give the face anchor (x, y) in image space for the mouth overlay, per image index.
- Mouth overlay image index per mouth digit, if present. If no mouth overlay exists for a mouth digit, draw no mouth overlay (do not stretch the face image).
- **What was NOT established:** the exact mouth-digit → mouth-shape mapping for every avatar (it is avatar-defined). The Rust crate treats mouth digits as opaque (passes them through). Do not invent a universal mouth table.

### 5.7 ComicArt directory (config, not wire — but load-bearing)

- ComicArt lives in the config directory: `~/.config/comic-chat/comicart/` (from `config.lua` / config dir). It contains **only** AVB/BGB files. PNGs go in a separate cache directory, never in ComicArt.
- **The 0.3.17 bug:** `./ComicArt` (current working directory) shadowed `~/.config/comic-chat/comicart/`. Config-dir ComicArt wins. The Rust rewrite must resolve ComicArt from the config dir, not from CWD. Do not re-introduce the CWD shadow.
- Do not bundle ComicArt in a zip distribution. The user manages it via the config dir.

### 5.8 zlib / v2.5 AVB

- v2.5 AVB/BGB DIB chunks may be zlib-compressed (RFC 1950). Callers try raw DIB first, then zlib-wrapped DIB (`inflate_zlib` then `decode_dib`). The Rust crate does this in `load_avb` / `load_bgb`.
- `flate2` is the only external dependency in the whole bundle, and only for `comic-avatar`.

---

## 6. Panels — `panel.cpp`, `chatdoc.cpp`, `histent.cpp`, `pageview.cpp`

Codified in `crates/comic-panel`.

### 6.1 Panel model

- A panel is a horizontal strip. It holds 1..N entries (messages / actions / joins / parts). A panel is "full" when the next entry would exceed the panel width or the max-entries rule. **The original did NOT put one message per panel** — it packed until full, then started a new panel. The 0.3.17 `src/comic.rs` put one message per panel in some paths — wrong, it wasted vertical space and broke the establishing-shot layout.
- Panel width 640 at zoom 1.0 (reference width in the original and in 0.3.17 `app.rs`). Panel gap 8, entry gap 12, max entries 4, max panel height 480 (all at zoom 1.0). See `PanelLayout::default`.
- Grouping uses **unscaled** widths, then scales for display. Layout must be zoom-independent in structure (same grouping at any zoom).

### 6.2 Title card — the most-misunderstood panel rule

- The title card is a **panel**, not a banner.
- It appears **ONLY** in the very first frame when a room is joined.
- It is **static** like other comic panes: it does **not** float, does **not** stay pinned, and **scrolls into history** with the rest.
- It never re-appears on scroll, resize, or reconnect to the same room (reconnect = new join only if the client actually left the room).
- Rust: `should_show_title_card(is_first_frame_after_join, already_shown)` — `is_first_frame_after_join && !already_shown`. `title_card_panel` builds the panel entry (room, topic, member list, 600×120).
- The 0.3.17 code at one point rendered the title card as a pinned header — wrong. Fix by using the panel rule above.

### 6.3 Establishing shot

- The first panel after joining may include a wide background + cast lineup. `is_establishing(panel_index, joined_this_session)` — true only for panel 0 of a newly joined room.
- A join that starts a new cast member's first appearance may start a new panel if `break_on_join` is set — but only for the establishing shot. Ordinary joins/parts pack like any entry.

### 6.4 Cast

- Cast = ordered list of (nick, avatar) in **join order**. Panel layout places avatars left-to-right in **cast order**, not message order. The 0.3.17 code placed avatars in message order — wrong, it made the cast jump around.
- `Cast::avatar_positions` returns (nick, x) with x centred in that speaker's slot. Speakers not in the cast (e.g. a message from a nick that just left) are appended after the cast, in message order.

### 6.5 Scroll

- Panels are appended; the view scrolls to the bottom on a new panel **unless** the user has scrolled up (then it stays). `ScrollState::should_autoscroll` = `!user_scrolled_up`. When the user scrolls to the bottom again, clear the flag so autoscroll resumes (`on_user_scroll(at_bottom)`).

### 6.6 History entries

- `CHistEnt` = one entry (message / action / join / part / system line) with speaker, timestamp, text, pose, balloon style, avatar, background. Panels are a view over history. **Never store panels as the history** (see §1.2).

---

## 7. Protocol — `ircsock.cpp`, `ircproto.h`, `protsupp.cpp`, `chatprot`

Codified in `crates/comic-protocol`.

### 7.1 IRC line format

- Messages: `:<prefix> <command> <params> :<trailing>` + CRLF.
- Lines are split on **CRLF**. A bare LF is **not** a terminator (it was treated as part of the line). The 0.3.17 code split on bare LF — wrong, it broke multi-line messages that contained LF.
- Max line length: **512 bytes including CRLF** (RFC 1459). Longer lines must be split by the sender, never truncated silently by the receiver. `split_privmsg` splits on word boundaries, never splits a UTF-8 char, and keeps the pose code on the first chunk only.
- Commands are case-insensitive on the wire; the parser uppercases them. Parameters are case-sensitive except nicks/channels (see §7.3).

### 7.2 CTCP

- `\x01ACTION ...\x01` inside PRIVMSG = `/me` action.
- Other CTCP (`VERSION`, `PING`, etc.) are `\x01CMD [args]\x01`.
- The original replied to VERSION with `Microsoft Comic Chat <version>` (`ctcp_version_reply`). The version string comes from `chatver.h` / `src/version.rs` — they must agree (see §1.4).

### 7.3 Nick / channel comparison — IRC case folding

- IRC case folding (RFC 1459): `{}|^` map to `[]\~` respectively, and comparison is case-insensitive after that mapping. `irc_eq` implements it. **Plain `eq_ignore_ascii_case` is WRONG** for nicks containing `{}` etc. (e.g. `A{B` == `a[b` under IRC folding, but not under ASCII folding).
- Nick validity: 1..=30 chars, starts with letter or one of `[]\`^{|}_-`, rest letters/digits/`-`/`_`/`[]\`^{|}`. Servers vary; this is the strict original rule.
- Channel validity: starts with `#` or `&`, 2..=200 chars, no space / `,` / `:` / NUL.

### 7.4 Flood control

- The original throttled outgoing PRIVMSG: burst of 3, then 1 per second, with a queue. `FloodLimiter` encodes this. **Do not send faster** — servers kick for flooding. The 0.3.17 code had no flood limiter — add it.

### 7.5 Slash commands (client-side, never sent raw)

`/join`, `/part`, `/nick`, `/msg`, `/me`, `/query`, `/whois`, `/topic`, `/kick`, `/ban`, `/mode`, `/invite`, `/away`, `/quit`, `/appears` (→ `Appears as` line), `/background` (→ `Background as` line). `parse_slash` parses them. `//` = literal leading slash (send as text). Unknown `/cmd` = `Unknown(cmd, args)` — the client must **not** send it raw; it must show "unknown command" (the original did not forward unknown slash commands to the server).

### 7.6 IRCX / Comics-mode negotiation

- IRCX (Microsoft extensions): `JOIN` with `*` / mode extensions, `IRCX` command, `LISTX`, `PROPERTY`, and Comics-mode negotiation.
- **What was NOT established:** the exact Comics-mode negotiation bytes (`MODE <nick> +x` / `-x` in some servers) are server-dependent and were **not** fully pinned down by the code analysis. **Do not assume a negotiation.** Probe the server (see Appendix B) and gate pose/OEC encoding on the negotiated mode. The Rust crate deliberately does **not** encode a negotiation — it is a gap to fill from the source, not a guess.
- **Gating rule (established):** pose codes and OEC are only sent to Comics-mode peers. To Text-mode peers, send plain text.

---

## 8. UI chrome — what the code analysis established

The code analysis established the *existence* and *wiring* of the following UI elements (from `chat.cpp` includes / frame / view code). Pixel-perfect layout of these elements was **not** established by the code analysis — see Appendix B for what must be re-checked.

- **Main frame:** menu bar, toolbar, status bar, tab bar (one tab per room / query), member list (right side, nick list with avatar icons in Comics mode), topic bar.
- **Say window:** message input at the **bottom** of the screen, just above the status bar. (This is a stated preference in the Rust project too — keep it.) The Say window has a balloon-style selector and an emotion wheel (Comics mode).
- **Room list / MOTD:** room list dialog, MOTD display on connect, setup dialogs (nick, server, avatar, background).
- **Automations / macros:** the original had automation hooks (auto-respond, auto-join). Their exact trigger syntax was not pinned down — see Appendix B.
- **Sounds:** the original played sounds for joins/parts/messages. Sound file handling was not pinned down — see Appendix B.
- **File transfer / DCC:** DCC send/receive was present in the original. Wire details were not pinned down — see Appendix B. Do not implement DCC from memory.

---

## 9. What the code analysis did NOT establish (honest gaps)

These are gaps, not guesses. Do not fill them from memory. Re-check against the Microsoft source (see Appendix B for the exact files to re-read).

1. Exact OEC byte sequence for font-name encoding (see §3.4).
2. Full emoticon table in `semantic.cpp` (see §3.6).
3. Exact Comics-mode negotiation bytes / IRCX handshake (see §7.6).
4. Mouth-digit → mouth-shape mapping per avatar (see §5.6).
5. Pixel-perfect layout of UI chrome (menus, toolbars, member list, Say window) — see §8.
6. Automation / macro trigger syntax (see §8).
7. Sound file handling (see §8).
8. DCC wire details (see §8).
9. Exact `CHistEnt` field list (the code analysis established the *concept*; the exact struct fields must be re-checked).
10. Exact `CUserInfo` field list (same).

**If any Part A detail is found wrong on re-check, fix the library first, then this spec. Tell the user and re-review the Microsoft source together — do not silently patch the spec.**

---


## 10. Assessment — existing `comic-chat-rs 0.3.17` file set (as supplied)

**File set assessed:** `Cargo.toml`, `src/main.rs`, `src/version.rs`, `src/app.rs`, `src/avb.rs`, `src/comic.rs`, `src/dib.rs`, `src/pose.rs`. (No `src/irc.rs`, `src/config.rs`, or `src/config.lua` handling was in the supplied set — request those files if they exist under other names before wiring protocol / config changes.)

**Overall:** the 0.3.17 code has the right *shape* (egui app, AVB/DIB loaders, pose handling, balloon drawing, panel grouping) but it diverges from the Microsoft source in the specific, load-bearing ways listed below. None of these are cosmetic. Fix them in the order listed — each is one step, with tests green before the next.

### 10.1 `src/pose.rs` — MUST FIX (wire-breaking)

| Issue | Fix |
|---|---|
| Generates `(#G2:8E0:8M1)` (invented colon form, intensity `8`) | Generate canonical `(#G3:1E0:1M1)` via `comic-text::PoseCode::to_wire` |
| Defaults to a gesture; emits pose code on every message | Default `PoseStyle::None`; emit no pose code for plain messages |
| Parses only its own form | Parse canonical + bare + colon form (compat); never emit colon form |
| Large generated helper-function blocks (dead weight) | Delete after delegation; keep `PoseCode`, `parse_pose_code`, `appears_as_line` |

**Step:** delegate to `comic-text`. Keep the public API names the rest of `src/` uses, but implement them by calling the library. Add the three parse tests from the library (canonical / bare / colon-compat) to `src/pose.rs` tests.

### 10.2 `src/dib.rs` + `src/avb.rs` — MUST FIX (AVB display)

| Issue | Fix |
|---|---|
| RLE8/RLE4 attempted, produced garbage | Reject RLE with `UnsupportedCompression`; do not decode |
| Unknown AVB chunk tag treated as fatal | Skip unknown tags by length (`comic-avatar::read_chunks`) |
| BGB decoded but never registered as background | Return the image from `load_bgb` and use it (do not ignore return value) |
| Chunk padding assumed (aligned to 4 bytes) | Chunks are NOT padded; do not align |
| Large generated helper-function blocks | Delete after delegation |
| DIB palette: `colours_used` ignored in some paths | Use `colours_used` if non-zero, else `2^bpp` |

**Step:** delegate DIB decode + chunk walk to `comic-avatar`. Keep the PNG-cache fallback until the new decoder proves itself on real AVB/BGB files (test with at least one real AVB and one real BGB from `~/.config/comic-chat/comicart/`). Do not re-introduce `./ComicArt` CWD shadow — config-dir ComicArt wins.

### 10.3 `src/comic.rs` — MUST FIX (panel + emotion)

| Issue | Fix |
|---|---|
| One message per panel in some paths | Pack until full (`comic-panel::group_into_panels`) |
| Avatar placement in message order | Cast order (`Cast::avatar_positions`) |
| Title card as pinned header in one path | Title card as first panel, static, scrolls away (`should_show_title_card`) |
| Emotion wheel order correct — keep | Keep the 8-emotion order; do not reorder |
| Large generated helper-function blocks | Delete after delegation |

**Step:** delegate grouping + title-card + cast to `comic-panel`, emotion/gesture/balloon mapping to `comic-text` + `comic-balloon`.

### 10.4 `src/app.rs` — MUST FIX (balloon rendering)

| Issue | Fix |
|---|---|
| Balloons drawn as rounded rects + triangular tail | B-spline outline from 8 control points (`comic-balloon::outline`) |
| Tail base edge stroked (line across balloon interior) | Stroke only the two outer tail edges; base edge is interior |
| Think-trail: 2 circles | 3 circles, geometrically shrinking (`think_trail`) |
| Shout: every point offset outward (bigger balloon) | Triangular spikes, points between spikes on base curve (`add_spikes`) |
| Whisper waves parameterised by point index | Waves by arc length (`add_waves`) |
| Whisper dashes: trailing partial dash dropped | Keep trailing partial dash (`dash_polyline`) |
| Pose codes sent unconditionally | Gate pose/OEC on Comics-mode negotiation (see §7.6 — negotiation itself is a gap; do not assume bytes) |
| No flood limiter | Add `comic-protocol::FloodLimiter` (burst 3, then 1/s) |
| IRC lines split on bare LF | Split on CRLF only |
| Nick comparison via `eq_ignore_ascii_case` | IRC folding (`irc_eq`: `{}|^` → `[]\~`) |
| Message input position correct — keep | Keep input at bottom, just above status bar |

**Step:** delegate outline math to `comic-balloon` + `comic-geometry`. Keep egui as the rasteriser — the libraries return polylines / point lists, not pixels. Do not rewrite the egui view in the same step as the balloon math.

### 10.5 `src/version.rs` — FIX

- `version.rs` was a stub in the supplied set. It must agree with `chatver.h` for a given release (see §1.4). Wire it to `Cargo.toml` version (`0.3.17`) and to the CTCP VERSION reply (`comic-protocol::ctcp_version_reply`). Do not hard-code a second version string.

### 10.6 `src/main.rs` — NO CHANGE

- `main.rs` control flow is fine. Do not change it in the library-wiring steps.

### 10.7 Missing files — REQUEST

Request these from the repo before wiring protocol / config changes: `src/irc.rs` (or wherever IRC lives), `src/config.rs` / `config.lua` handling, any `src/sound.rs` / automation files. Do not guess their contents. If they do not exist, the protocol wiring in §10.4 (flood, CRLF, IRC folding, CTCP) is the *new* IRC module — build it from `comic-protocol` as a fresh `src/irc.rs`, not by patching `app.rs`.

### 10.8 Cleanup — AFTER delegation, separate commit

- Delete the large generated helper-function blocks in `src/avb.rs` / `src/comic.rs` / `src/dib.rs` / `src/pose.rs` **only after** delegation, with tests green. They are dead weight, but deleting them before delegation risks removing a function the rest of `src/` still calls.
- Do not bump version. Do not touch `config.lua` handling. Do not bundle ComicArt.

---

# Part B — Supplemented full specification of Microsoft Comic Chat

**Status:** Supplemented material. Contextualises Part A with broader technical details and additional information observed about the client and protocol elsewhere. **Where Part B differs from Part A or the libraries, Part A / libraries win** (see Authority rule at the top). Part B does not correct Part A.

## 11. History and versions

- **Microsoft Chat** (later **Microsoft Comic Chat**) shipped as part of Internet Explorer / Windows in the late 1990s. Versions: 1.0 (1996, Text-only IRC), 2.0 (Comics mode introduced), 2.5 (final, 1998–2000, the wire target). The MIT-licensed open-source release is the 2.5-era codebase.
- **Comics mode** is the defining feature: instead of a scrolling text log, messages are rendered as comic panels with avatars, balloons, and backgrounds. Text mode is the same document rendered as a log — switching views does not re-fetch.
- **IRCX:** Microsoft's IRC extensions (see §7.6, §14). Comic Chat is an IRCX client; it also speaks plain IRC to non-IRCX servers (with Comics features degraded to plain text).
- **Why 2.5 is the target:** earlier versions differ on the wire (pose-code format, OEC, negotiation). A client that speaks 2.5 speaks to every server that speaks Comic Chat. A client that speaks 1.0 / 2.0 does not.

## 12. Client architecture (supplemented view)

The Part A layering (§1.1) is the code-derived view. The supplemented view adds:

- **MFC document/view** is the *pattern*, not the *requirement*. The Rust rewrite uses egui + a document (history) + view (panels) split, not MFC. The split matters; the framework does not.
- **Threading:** the original did socket I/O on a worker thread and marshalled to the UI thread. The Rust rewrite should do the same (socket thread → channel → UI thread). Never block the UI thread on the socket. Never touch UI state from the socket thread.
- **Persistence:** history is in-memory per session (the original did not persist chat history to disk by default). Settings (nick, server, avatar, background, balloon colour, font) persist via the registry (Windows) / `config.lua` (Rust rewrite). ComicArt is user-managed via the config dir (see §5.7).
- **Multi-room:** one document per room / query, one tab per document, one Say window shared (the Say window targets the active tab). The member list, topic, and cast are per-document.

## 13. ComicArt: avatars and backgrounds (supplemented)

- **Avatar files (.AVB):** one file per character. Each contains the pose images (gestures × expressions), mouth overlays, face anchors, and metadata (name, description). Avatars are *not* single images — they are pose tables. A client that loads only the first DIB shows a static avatar, not a Comic Chat avatar.
- **Background files (.BGB):** one file per background. Each is a single DIB (the background image) plus size / tiling metadata. Backgrounds tile or stretch to fill the panel — the exact tiling rule is per-background (see Appendix B: tiling was not fully pinned down; do not assume stretch).
- **Built-in vs user ComicArt:** the original shipped with built-in avatars (e.g. the classic cast) and backgrounds. The Rust rewrite does not bundle ComicArt (see §5.7) — the user supplies AVB/BGB files via the config dir. The client must work with *any* AVB/BGB that follows the §5 format, not just the built-ins.
- **Avatar selection:** `Appears as <name>` selects by *name*, not by filename. The client maps name → file by scanning the ComicArt dir and reading `NAME` chunks (or by filename stem as a fallback — but name wins if both exist).
- **Background selection:** `Background as <name>` selects by name, same rule.

## 14. Protocol (supplemented view)

### 14.1 IRC (RFC 1459) — the base

Comic Chat is an IRC client first. The base protocol is RFC 1459 IRC: `NICK`, `USER`, `JOIN`, `PART`, `PRIVMSG`, `NOTICE`, `QUIT`, `MODE`, `TOPIC`, `NAMES`, `LIST`, `WHO`, `WHOIS`, `KICK`, `BAN`, `INVITE`, `AWAY`, `PING`/`PONG`. The Part A §7 rules (512-byte lines, CRLF, case folding, flood) apply to *all* of these, not just PRIVMSG.

### 14.2 IRCX — Microsoft extensions

IRCX adds: `IRCX` (negotiation), `LISTX` (extended LIST), `PROPERTY` (per-user / per-channel properties), extended `JOIN` (with `*` / mode extensions), and the Comics-mode property. The exact negotiation bytes are a gap (see §7.6 / Appendix B) — the supplemented view adds context, not bytes: negotiation happens *after* `NICK`/`USER` and *before* the first `JOIN`, and the server's reply gates Comics features. Do not send Comics payloads before negotiation completes.

### 14.3 CTCP — the Comics channel

CTCP is the channel for Comics negotiation and for `/me` actions. The Part A §7.2 rules apply. Supplemented: CTCP `VERSION` is the *only* CTCP the original reliably answered; `PING` was answered with a `PING` reply (echo). `DCC` (see §15.4) also rides CTCP but is out of scope for the initial rewrite.

### 14.4 Message flow (supplemented sequence)

1. User types in Say window, picks balloon style + emotion (or leaves default).
2. Client builds the wire text: pose code (if any) + OEC (if Comics peer + non-default format) + text. See §3.
3. Client splits at 512 bytes if needed (pose code on first chunk only). See §7.1.
4. Client sends via flood limiter. See §7.4.
5. Client adds the message to *local* history immediately (echo) — it does not wait for the server echo. The server echo (if any) is deduplicated by (prefix, text, timestamp) — the original did not show the echo as a second message.
6. Receiving: parse line → CTCP? → pose code? → OEC? → `Appears as` / `Background as`? → semantic classify → history entry → panel regroup → render.

### 14.5 Errors and edge cases

- **Nick collision:** server sends `433` (nickname in use). The client must prompt for a new nick (or append `_` / a digit — the original prompted; the Rust rewrite should prompt, not silently rename).
- **Banned / invite-only:** `474` / `473`. Show the server message; do not retry automatically.
- **Kicked:** `KICK`. Show "You were kicked by X (reason)". Clear the room's cast for that room.
- **Flood kick:** server kicks for flooding. The flood limiter (§7.4) is the prevention; if kicked anyway, show the message and do not auto-rejoin in a tight loop (back off).
- **Malformed lines:** drop the line, do not crash, do not show raw wire to the user. Log at debug level.

## 15. UI (supplemented view)

### 15.1 Main window

Menu bar (File, Edit, View, Room, Help), toolbar (connect, join, leave, avatar, background, balloon style, emotion), tab bar (one tab per room / query), member list (right, nick + avatar icon in Comics mode, nick only in Text mode), topic bar (room topic, editable by ops), status bar (connection state, lag, current nick).

### 15.2 Say window

Message input at the **bottom**, just above the status bar (see §8, §10.4). Balloon-style selector (4 buttons: Speech / Thought / Shout / Whisper) + emotion wheel (8 emotions, wheel order §3.2) + avatar picker + background picker. Enter sends; Shift+Enter inserts a newline (if multi-line input is enabled — the original was single-line; the Rust rewrite should stay single-line unless the user asks for multi-line).

### 15.3 Member list

Ordered by: ops first (in join order), then voiced, then regular (in join order). Avatar icon next to nick in Comics mode. Right-click → Whois / Query / Kick / Ban / Ignore (ops only for Kick/Ban). Double-click → Query (private message).

### 15.4 File transfer / DCC (out of scope for initial rewrite)

DCC SEND / CHAT was present in the original. Wire details were not pinned down (Appendix B). **Do not implement DCC from memory.** The initial rewrite should show "DCC is not supported yet" for incoming DCC requests, not silently accept files (security: never auto-accept a file transfer).

### 15.5 Sounds, automations, macros (out of scope for initial rewrite)

Sounds, automations, and macros existed in the original but their exact trigger syntax / file handling was not pinned down (Appendix B). The initial rewrite should not invent automation syntax. If the user asks for automations later, re-check the source first.

## 16. Compatibility matrix (supplemented)

| Peer / server | What the client sends | What the client renders |
|---|---|---|
| Comic Chat 2.5 (Comics mode) | Pose code + OEC + text | Full panels, balloons, avatars |
| Comic Chat 2.5 (Text mode) | Plain text | Text log (local Comics view still renders local messages as panels — the *view* is local, the *wire* is plain) |
| Plain IRC client (mIRC, etc.) | Plain text | Text log |
| Plain IRC server (no IRCX) | Plain IRC | Text log; Comics view renders local messages as panels from local history |
| IRCX server, Comics negotiated | Pose + OEC | Full panels |

**Key insight:** the Comics view is *local*. The wire is plain text unless Comics is negotiated. A client that only renders panels for Comics peers is wrong — it must render panels for *all* messages in its local history, using the pose code if present, else the default pose.

---

# Appendix A — File / symbol map (Part A sources)

| Rust crate | Microsoft source files | Key symbols (original → Rust) |
|---|---|---|
| `comic-geometry` | `vector2d.cpp/.h`, `traj.cpp`, `arc.cpp`, `spline.cpp`, `bbox.h` | `VectorAdd` → `Vector::add`, `ScalarMult` → `scale`, `Magn` → `magnitude`, `VectorToAngle` → `vector_to_angle`, `AngleToVector` → `angle_to_vector`, `ScanArcAux` → `scan_arc`, `DrawArc2`/`DashArc2` → `scan_arc` + `dash_polyline`, B-spline → `bspline_closed` |
| `comic-text` | `format`, `textpose.cpp`, `semantic.cpp`, `textview.cpp/.h` | Pose code `(#G…:…E…:…M…)` → `PoseCode`, `Appears as` → `appears_as_line`, OEC → `oec_encode`/`oec_decode`, `CSemantic` → `classify_message`, `CTextPose` → `Emotion` |
| `comic-avatar` | `avatar.cpp/.h`, `bodycam.cpp/.h`, DIB / AVB loader | `CAvatar::GetImage` / `ChoosePose` → `Avatar::image_index_for`, DIB → `decode_dib`, AVB chunks → `read_chunks`/`load_avb`, BGB → `load_bgb`, halo → `apply_mask` |
| `comic-balloon` | `balloon.cpp/.h`, `bodycam.cpp/.h` | Balloon outline → `outline`, tail → `tail_base` + `tail_polygon`, think-trail → `think_trail`, spikes → `add_spikes`, waves → `add_waves` |
| `comic-panel` | `panel.cpp/.h`, `chatdoc.cpp`, `histent.cpp/.h`, `pageview.cpp` | Panel grouping → `group_into_panels`, title card → `should_show_title_card`, cast → `Cast`, scroll → `ScrollState`, `CHistEnt` → `PanelEntry` (view-level) |
| `comic-protocol` | `ircsock.cpp`, `ircproto.h`, `protsupp.cpp`, `chatprot` | IRC line → `parse_line`, CTCP → `parse_ctcp`, nick folding → `irc_eq`, slash → `parse_slash`, flood → `FloodLimiter` |

# Appendix B — What must be re-checked against the Microsoft source

Re-read these files before relying on the corresponding gap. Do not fill gaps from memory.

| Gap (Part A §9) | Re-read |
|---|---|
| OEC font-name encoding | `format` / `chatprot` (OEC encode path) |
| Full emoticon table | `semantic.cpp` |
| Comics-mode / IRCX negotiation bytes | `ircproto.h`, `ircsock.cpp`, `protsupp.cpp` (IRCX / MODE handling) |
| Mouth-digit → shape per avatar | `avatar.cpp/.h` (mouth table), plus one real AVB file |
| UI chrome pixel layout | `chat.cpp` (frame / toolbar / status), `pageview.cpp`, `textview.cpp` |
| Automation / macro syntax | `chat.cpp` (automation hooks), `chatdoc.cpp` |
| Sound file handling | `chat.cpp` (sound hooks) |
| DCC wire details | `ircsock.cpp` / `protsupp.cpp` (DCC handling, if present) |
| Exact `CHistEnt` fields | `histent.cpp/.h` |
| Exact `CUserInfo` fields | `userinfo.cpp/.h` |
| Background tiling rule | `avatar.cpp` / BGB loader + one real BGB file |
| `CTextPose` full mapping | `textpose.cpp` |

**If any Part A detail is found wrong on re-check, fix the library first, then this spec. Tell the user and re-review the Microsoft source together.**

---
*End of specification. Version 0.3.17. Frozen 3.3 line.*

---

## Correction log

<!-- CORRECTION-LOG-2026-11-21 -->
- **2026-11-21 — §3.2 / §3.6:** Wheel was Disgusted / Contemptuous / Surprised / Afraid. Wrong.
  Corrected to Shout, Laugh, Happy, Coy, Bored, Scared, Sad, Angry + Neutral centre.
  E-digit mapping marked UNTRUSTED. §3.6 restored to help-verified triggers. Source:
  MS Chat help file (`Cchat.hlp`) + Kurlander Comic Chat page.
