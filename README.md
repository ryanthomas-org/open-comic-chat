# Comic Chat RS — 0.3.17 — AVB / BGB / Pose codes / Click-to-include

> **Config is Lua-only (0.3.17).** Lua is authoritative and immutable except by you.
> The GUI never writes `config.lua`. There is no Options window, no Servers editor,
> no Save-settings button. All usage is in the GUI (chat, connect, whisper, away,
> Save Conversation); all configuration is done exclusively in Lua — edit
> `config.lua` and restart. See `config.lua.example` (it includes the About-logo
> reproduction steps for Help > About).

Microsoft Comic Chat (1996) reimagined in Rust + Lua config, Linux desktop, modern IRC with TLS, multi-server.

## Versioning — locked to 0.3.x

This release is **0.3.17**. The version line is **0.3.x**: do not bump to
0.4 / 1.0. Patch increments only (0.3.17, 0.3.17, ...). `Cargo.toml`
`version` and `VERSION` in `src/main.rs` are the single source of truth;
the window title, the startup log, and the CTCP VERSION reply all read
from that constant, so they can never drift apart.

History note: early turns of this project called the same codebase
3.3 / 3.3.1 / 3.4 / 3.4.1. Those numbers now map to the 0.3.x line
(3.3.1 = 0.3.17, 3.4 = 0.3.17). Old comments saying "v3.3.1" / "v3.4"
have been rewritten to 0.3.17 / 0.3.17. Microsoft's own versions (1.0,
2.1, 2.5) are unchanged — they are the original client's versions,
not ours.

## Original source — MIT licensed, and what we take from it

The full original Microsoft Comic Chat source is now MIT licensed
(July 16, 2026) at **github.com/microsoft/comic-chat** — snapshots from
a 1996 pre-release through 2.5 beta, plus the SDK and docs. This Rust
client is clean-room, but it uses that source as its style guide:

- **Toolbar:** one row of literal, flat, chunky buttons, each a picture
  of what it does. Our icons keep that look (see Icons below).
- **Balloons:** speech / thought / whisper / action shapes, ALL-CAPS
  text, tails to the speaker (SIGGRAPH 96, §5).
- **Panels:** 5-character cap, one balloon per character per panel,
  narration bars for actions, halo around characters (SIGGRAPH 96, §6).
- **Font:** Comic Sans everywhere (it shipped the font).

What we do NOT copy: the original BMP bitmap strip, MFC code, or
Microsoft's font file. Our icons are SVGs, our code is Rust, and
Comic Sans is loaded from a copy already on your machine
(see Font below).

## CTCP VERSION

A direct CTCP VERSION query (`PRIVMSG <us> :\x01VERSION\x01`) is
answered with a NOTICE carrying exactly:

```
Open Comic Chat (Version 0.3.17)
```

The reply comes from `version_reply()` in `src/main.rs`, which formats
`VERSION`, so it always matches the release. VERSION sent to a channel
is ignored — replying there would make every client answer at once.

## 0.3.17 — Comic Sans font lookup via fontconfig

0.3.17's font search checked only flat hardcoded paths
(`~/.fonts/comic.ttf`) and never asked fontconfig anything, so a real
install in `~/.fonts/Comic Sans MS/comic.ttf` was missed. 0.3.17 resolves
the **family** `"Comic Sans MS"` to a **file**: `ui.font_path` (realpath)
first, then `fc-list "Comic Sans MS" --format=%{file}:%{style}` (Regular
face preferred), then the
hardcoded candidates (now including the `Comic Sans MS/comic.ttf`
subdir forms). The `config.lua` comments now state plainly that
`ui.font` is a family name, `ui.font_path` is a file path inside the
`ui = { ... }` table, and the PostScript name `ComicSansMS` is never
used. Default changed from `"Comic Sans"` to the exact family
`"Comic Sans MS"`.

## 0.3.17 — Hand-drawn balloons, facing, Whisper box, sounds, static title card

- **Balloons are drawn, not framed (PDF §5.3).** `draw_balloon` replaces
  the egui Frame: a painter-drawn outline with a hand-drawn wobble
  (outward-only displacement, low-frequency wave + ripple), so it reads
  as drawn, not machine-made. Speech = white fill, thin black outline.
  Thought = cloud (bigger wobble, lavender fill). Whisper = dashed
  outline under a thick white halo + italic text (PDF §5.1, §5.5).
  Shout = thick red outline; Question = blue outline. No "Whisper" /
  "Thought" text label — the outline style is the signal, as in the
  original. Balloon text is ALL-CAPS, CENTERED, every line.
- **Character facing (PDF §4.3).** AVB art faces right, so the first
  character in a panel faces right (toward the rest) and every later
  character is flipped horizontally to face left (toward them). Only
  flipped when needed — a lone character, or a title card, is never
  flipped. Listeners in the cast row face left toward the speakers.
  The full Facing + Neighbors greedy ordering is still not implemented
  (positions are message order, not the paper's stable ordering), but
  the flip itself is correct for the common two-person case.
- **Whisper box.** Private messages from people OUTSIDE the room go to
  a text Whisper box — the original's behaviour. Toolbar button with
  an unread badge; the box opens itself on a new outside whisper; a
  reply goes to the last other party. Whispers from people IN the
  room become dashed whisper bubbles in the room's comic. Fixed along
  the way: a DM no longer creates a phantom channel named after the
  sender (the old code filed every DM as a channel).
- **Sounds play.** `(ding.wav)` / `(ding)` messages play from the Sound
  Search Path (`comic.sound_path`, default
  `~/.config/comic-chat/Sounds`, set in config.lua as `comic.sound_path`). Play Sound plays
  locally too; incoming (sound) messages play on receipt. The status
  line under Play Sound shows the resolved path or the miss. Players
  tried in order: pw-play, paplay, aplay, ffplay.
- **No DCC file transfers, no NetMeeting features.** Neither existed
  in the original Comic Chat and neither is in this build — the
  original had no DCC file transfers and no NetMeeting integration,
  and this client does not add them.
- **Title card is static.** The 0.3.17 "living title card" (re-ranked on
  every message by activity) is reverted: the card is computed once
  per join from NAMES order (you + the first three others) and never
  changes — it is the first frame only and scrolls into history as
  panels arrive (scroll up to see it). It also fires exactly once per
  join (a second NAMES no longer re-emits it). The `activity` table
  is gone.
- **Long messages spill over (PDF §6.1).** A message longer than ~280
  characters is split at word boundaries into ~280-char chunks, and
  each chunk becomes its own message in the panel sequence — so a long
  message flows across additional panels with clean, neatly drawn
  bubbles. The Plain Text view shows the original single message.
  The 5-char cap / one-balloon-per-character rule still applies per
  panel.
- **Emotion wheel is a colour-chooser disc (PDF Fig 1).** The wheel now
  draws one wedge per emotion in `Emotion::all()` order, clockwise
  from the top, in the emotion's colour (translucent), around a grey
  Neutral centre. Interaction is unchanged: click/drag sets emotion +
  strength, emoji buttons + slider still work.

## 0.3.17 — Balloon tails, camera zoom, living title card

Final-touch pass against the paper and the real screenshots, closing
four gap-list items (6, 9, 10, 14 — see below):

- **Balloon tails (PDF §5.4).** Every balloon now has a tail from under
  its bottom line of text to its speaker: a white triangle with a black
  outline for speech, three descending ovals for thought. The old "▲"
  marker above each head is gone — the tail does that job. Character
  columns share the balloon columns' width so each tail lands on its
  speaker. Routing channels (tails never crossing a balloon) are still
  not implemented — with one row of balloons per panel, tails are short
  and vertical, so there is nothing to cross.
- **Camera zoom is applied (PDF §6.2).** `Panel.zoom` was computed but
  ignored; now the scene scales with it — characters at `72 * zoom`,
  background strip taller for wide shots, shorter for tight ones.
  Balloons deliberately do NOT scale: the paper says "Word balloons are
  unaffected by the virtual zoom factor." About every 15th panel is an
  establishing shot (zoom 0.8, full-width row), and the title card gets
  a full-width row too, so the Starring pane spans the strip like the
  original's first pane.
- **Balloon text is centered (PDF §5.3).** Woodring centers every line;
  the old build left-aligned it.
- **The title card follows activity (PDF §8).** The Starring cast is no
  longer frozen at join-time NAMES order: it is you + the three most
  active others, re-ranked on every message (ties keep first-seen
  order), with the Plain Text `*** TITLE - starring ...` line rewritten
  to match. At join time nobody has spoken, so it still starts as
  NAMES order. Your own messages count (the server echoes them back);
  offline-demo messages count too. "MEANWHILE..." stays skipped. Reverted in 0.3.17: the card is static (computed once per join from NAMES order) — the paper's "most active participants" describes a finished strip; a live chat's title card is fixed at entry.
- **Linux app launcher.** `comic-chat.desktop` in the repo root (see
  Build & run above).

Deliberately NOT done in 0.3.17: character facing / ordering (gap 11) —
flipping sprites without the paper's Facing + Neighbors evaluation
would point characters at the wrong people, which looks worse than no
facing. 0.3.17 implements the flip (first character faces right, the
rest face left) but not the full Facing + Neighbors greedy ordering —
see 0.3.17 above.

## 0.3.17 — Room-entry title cards (Starring pane)

On entering a room, the first panel is now a title card, like the
original's first pane (screenshots: "JUST US CHUMPS STARRING
ProperDave, nobody"). One title is picked at random per entry from the
built-ins (JUST US CHUMPS, DEEPEST DARKEST DESIRES, YOU SHOULDA BEEN
THERE, NETWORKED NERDS) plus the config's custom `comic.titles`, which
are added on top, never a replacement. The cast is the connecting user
+ up to 3 others in the room, with their AVB icons (silhouette when an
AVB has not arrived yet). The card is local-only, never sent to the
server, and appears first in both the comic view and Plain Text
(`*** {title} - starring ...`).

The card is emitted when NAMES arrives (the first moment the member
list is known), and inserted before that join's "Now chatting in
<channel>" line. The paper's title panel listed the most active
participants (SIGGRAPH §8); at join time nobody has been active yet,
so the joining user + first others is the stand-in.

Also: the `fc-list` format in the font docs now shows the trailing
`\n` — without it fc-list concatenates the records into one line,
which no line parser can split.

## 0.3.17 — Away (coffee cup), member icons, "Now chatting in..."

### "Now chatting in <channel>"

When you join a channel, three things happen: a `System` line
`Now chatting in #channel` is added to the text log, the server status
reads `Now chatting in #channel`, and comics mode shows a System panel
with the same line. Join fires twice (001 pending-join + server JOIN
echo), so the line is deduped — it appears once per join.

### Away — coffee cup (FAQ: "How can I let people know I will be AFK?")

The FAQ's away system is visual: set yourself away and a coffee cup
appears next to your nickname in everyone's nickname list; come back
and it is removed. This build implements it end to end:

- **Set away:** click the coffee-cup **Away** button on the toolbar and
  type a message in the field next to it, or type `/away <message>`
  in the compose box. Bare `/away` toggles: away -> back (the FAQ's
  "type: /away" to return), not away -> away with the default message.
- **Default away message:** `Gone fishing.`
- **Back:** click the button again (it reads **Back** while you are
  away), or type `/away` / `/back`.
- **Member list:** a coffee cup sits next to an away nick in comics
  mode and text mode; hovering shows the away message
  (`Away: Gone fishing.`). Your own cup shows too.

### How away works on the wire — Microsoft's nonstandard CTCP AWAY

Microsoft did not use standard IRC AWAY for the coffee cup. It sent a
**CTCP AWAY** — `PRIVMSG <channel> :\x01AWAY <message>\x01` to every
channel, and bare `\x01AWAY\x01` to come back. This build bundles
that with standard IRC behaviour:

- **Out:** `SetAway` sends standard `AWAY :<message>` (bare `AWAY` to
  clear) to the server **and** CTCP AWAY to every joined channel.
- **In:** CTCP AWAY from a peer (`AWAY <msg>` = away, bare `AWAY` =
  back) and numeric **301 RPL_AWAY** (standard IRC away notice) both
  update the member list. A private CTCP AWAY (sent to us when we
  join while someone is away) applies to every channel we share with
  that nick.
- **While away:** anyone who joins a channel we are in gets a private
  CTCP AWAY from us, so they see our cup; when we join a channel
  while away, we re-announce CTCP AWAY to it. That is the
  `CoffeeCupAway.mrc` (Woodsman, 2000) behaviour — see
  `## Sources` — implemented natively, not as an mIRC script.
- **NICK while away:** the away entry follows the new nick
  (members / ops / away / character / background entries all move).

### Member list icons in comics mode

- **Avatar:** each member's AVB avatar (first decoded frame — the same
  texture the comic panels use) sits next to their name in comics
  mode. Text mode keeps names plain.
- **Hammer:** a hammer sits next to a **channel operator (host)** —
  `@` in the NAMES list, `MODE +o`. Hover reads
  `Channel operator (host)`. `MODE -o` removes it.
- **Coffee cup:** see Away above.

### FAQ audit (Sniffles' Place FAQ, mermeliz.com/sniffles/faq.html)

The FAQ text is visible; its screenshots come through as placeholders,
so the hammer could not be confirmed from a FAQ screenshot — it is
implemented from the standard `@` operator + the request. FAQ
features vs. this build: Play Sound (speaker icon) ✓ via the
Play Sound button, but `/sound off` / ctrl+H — not yet; send file
(right-click a name) — not yet; whisper ✓ via the compose Whisper
kind + member Whisper button, but `/whisper` / ctrl+W — not yet;
`/nick` — not yet,
panels across (max 4, set in config.lua as `ui.panels_per_row`) ✓ via
`panels_per_row`, Comic Chat vs IRC ✓, away ✓ (above), moof
(involuntary disconnect) — shown as a disconnect + reconnect via
Connect All. ChatCheck (panel width 5) is deliberately not rebuilt.

## Font — Comic Sans, loaded locally

`ui.font = "Comic Sans MS"` in `config.lua` (the default). The original
client drew everything in Comic Sans, so this build does too when it
can: on startup it loads a Comic Sans `.ttf` already on your machine.
The font file is NOT bundled (it is a Microsoft font, not MIT).

**What goes where — family vs. file vs. PostScript name:**

| Setting | Holds | Example |
| --- | --- | --- |
| `ui.font` | FAMILY name (`family:` in `fc-list -v`) | `"Comic Sans MS"` |
| `ui.font_path` | FILE path to a `.ttf` | `"/home/ryan/.fonts/Comic Sans MS/comic.ttf"` |
| neither | PostScript name (`postscriptname:`) — never use it here | `"ComicSansMS"` (no space) is NOT a family name |

**How the file is found, in order:** (1) `ui.font_path`, if set —
that exact file, canonicalized (realpath), wins; it goes INSIDE the
`ui = { ... }` table, and a leading `~/` is expanded. (2) `fc-list` —
the program runs `fc-list "Comic Sans MS" --format=%{file}:%{style}`
and reads the file fontconfig reports for that family, preferring the
Regular face. This finds installs in a subdir named after the family
(`~/.fonts/Comic Sans MS/comic.ttf`), which the old hardcoded flat-path
search missed. `fc-list` is used, NOT `fc-match`: `fc-match`
substitutes a different font when the family is absent, while `fc-list`
returns nothing unless the family really matches. (3) Hardcoded
candidates, including the `Comic Sans MS/comic.ttf` subdir forms. Only
the regular face is loaded (`comic.ttf`, not `comicbd.ttf` Bold). If
nothing is found, egui's built-in default is used and a warning is
logged listing everything checked. Set `font = "default"` to skip the
search.

**Check your install:** `fc-list | grep -i "comic sans"` to inspect,
`fc-list "Comic Sans MS" --format=%{file}` to see what the program
will load.

## 0.3.17 fixes (over 0.3)

1. **Pose codes**: v2.5 sends NO pose code. `pose_style = "none"` is now the default. The invented `(#G2:8E0:8M1)` colon form is never sent (still parsed for compat). `CAST:...` is never sent — cast is local-only. Only `pose_style = "old"` sends `(#G090E090M1)` (legacy 1.0–2.1).
2. **Idle log spam**: `parse_pose_code`, `appears_as_line`, `background_as_line`, `analyze`, texture loads no longer log per call / per frame. Failed textures are negative-cached (cleared on Rescan).
3. **Appears-as on join**: our `Appears as` line is now re-sent whenever a new user joins, so they learn our character (0.3 only sent it on our own JOIN).
4. **AVB preview**: chunked v2.5 decoder — real AVBs store raw pixels (not a full DIB) after gesture header + BGR palette + BMP header. Name chunk is null-terminated (was misread as length-prefixed). Config-dir `ComicArt/` now wins over `./ComicArt/` (stub no longer shadows real files).
5. **Panels**: MS Comic Chat layout — thick black border, white fill, full-bleed background, balloons in a row above characters with tail markers, silhouette (not emoji rectangle) when no sprite, no pose code in balloons, click character button to toggle cast.
6. **Debug log file**: `log::init` now uses a separate `started` flag (was `if guard.enabled { return; }` which returned on first init before opening the file). File is now actually opened.
7. **Config is Lua-only**: there is no `save()`. The GUI never writes `config.lua`. Edit Lua, restart. First run creates the file from the example; a file that fails to parse is left untouched and defaults are used.

## Debug log (0.3.17) — optional, off if absent

```lua
-- in config.lua — OFF if this line is absent (no file is created)
debug_log = { enabled = true, level = "debug" }
-- also works: debug_log = true  (level=debug)  or  debug_log = "trace"
-- optional: file = "/tmp/comic.log"  (default: ~/.config/comic-chat/debug.log)
-- level: error | warn | info | debug | trace (trace = most verbose)
```

- Verbose on every level: `config` (path, parse result, server count), `irc` (DNS, TCP, TLS, NICK/USER, JOIN/PART, PING/PONG, PRIVMSG in/out, SASL, disconnect reason), `avb` (ComicArt scan, header/version, Name/URL, image count, PNG status), `dib` (zlib streams, inflate, DIB header/bit depth/palette/dimensions, mask pairing, anchors), `pose` (pose-code parse + why it failed, Appears/Background parse), `comic` (text analysis -> emotion/balloon/gesture/strength, panel grouping), `app` (texture load DIB vs PNG vs placeholder, send, connect/disconnect, save).
- Each line explains *why*: e.g. `DIB decode failed: header_size 12 not 40/12`, `TCP connect failed: all addresses tried`, `no DIB image and no PNG sidecar — panel will draw placeholder`.
- Privacy: SASL password is **never** logged, and the base64 SASL blob is never logged. Chat text is logged only as a 120-char preview (newlines escaped), not a full transcript.
- GUI Save (`Servers` panel) preserves `debug_log` — it writes it back to `config.lua` instead of dropping it.

## What v0.3 adds (your request)

### 1. Character = AVB file, Background = BGB file — `ComicArt/` (authoritative)

```
ComicArt/
  Trekkie2018.AVB   + Trekkie2018.png   (PNG = preview for rendering)
  Frank.AVB         + Frank.png
  Cafe.BGB          + Cafe.png          (BGB = v2.5 compressed background)
  Cafe.BMP                              (BMP = manual background, not auto-downloadable)
  README.txt
```

- Same flat layout as original: `C:\Program Files\Microsoft Chat\COMICART` (and `... (x86)\...\COMICART`).
- Also scanned: `~/.config/comic-chat/ComicArt/` and legacy `assets/` (for backward compat).
- AVB/BGB are the **authoritative identity + URL**. The binary contains a Name chunk (`01 00`) and an embedded URL (plain ASCII `http://.../Name.AVB`).
- This client parses header (6 bytes: `81 81 ?? 00 02 00` for v2.5), version, Name, embedded URL, and fully decodes the DIBs (zlib-inflate v2.5, `BITMAPINFOHEADER`/`COREHEADER`, 1/4/8/24/32-bit + palette, bottom-up/top-down, row padding) + 1-bit halo masks + face/mouth anchors (see `src/dib.rs`). Decoded AVB is primary for rendering; PNG sidecar is only fallback. If neither exists, the character renders as a placeholder with its AVB name + pose, so the comic still works.
- Character / Background are set in config.lua (`comic.character` / `comic.background`). The GUI shows the current ones in Selfview; it has no picker. Rescan ComicArt still lists counts.

### 2. Send and interpret `Appears as` — the character line

```
# Appears as Trekkie2018.http://www.rebased.io/characters_for_comic_chat/Trekkie2018.AVB
# Appears as TIKI
```

- Sent **once** on join (auto-announce) and when you click **Announce character in channel** in Compose, or **Copy Appears-as line** to paste manually (dedoky / rebased / web-chat flow).
- Incoming `PRIVMSG` starting with `# Appears as ` is **not** a balloon. It updates the per-nick character map (`nick → AVB name + URL`) and appears as a system line in Plain Text: `* Alice appears as Trekkie2018 http://...`.
- If `auto_download = true` and the AVB is not in `ComicArt/`, the text log gives you the exact curl: `curl -o ComicArt/Trekkie2018.AVB "http://..."` (no silent HTTP download in this build — no HTTP client dep).
- Background sharing is the same shape: `# Background as Cafe.http://.../Cafe.BGB`

### 3. Send and interpret pose / character codes — `(#G...E...M...)`

Two wire forms, **both parsed**, one generated (config `pose_style`):

| Style | Example | Meaning |
|---|---|---|
| Old (1.0–2.1, angle) | `(#G090E090M1)` | G = gesture angle 090°, E = expression angle 090°, M = mouth 1 |
| New (2.5, indexed) | `(#G2:8E0:8M1)` | G2:8 = gesture index 2, strength 8 / E0:8 = expression index 0, strength 8 / M1 |
| Cast extension | `(#G2:8E0:8M1 CAST:Alice,Bob)` | This panel should include Alice + Bob (this client, backward-compatible — old clients ignore the trailing token) |

- **Send:** Every Say/Think/Whisper/Action appends the pose code: `Hello (#G2:8E0:8M1)`. The code is built from the Emotion wheel (or text analysis) + strength. Compose shows `Will send: Hello (#G2:8E0:8M1)` live, with a **Copy code** button.
- **Receive:** Trailing `(#G...E...M...)` is stripped for display in Comic view, stored as `PoseCode` on the message, and used to pose the character (emotion/gesture/strength). Plain Text view shows the raw code if `show_pose_codes_in_text = true` (original text clients saw the code as "garbage"), or the clean text if false.
- **Balloon kind is NOT in the pose code.** It is carried separately:
  - Say: `PRIVMSG #ch :text (#G...)`
  - Think: `PRIVMSG #ch :*thinks* text (#G...)` — visible as "thinks" to text clients
  - Whisper: `PRIVMSG <nick> :text (#G...)` — private, not to channel
  - Action: `PRIVMSG #ch :\x01ACTION text (#G...)\x01` — CTCP ACTION, `/me`

### 4. Click a user to include them in the frame — same corresponding code

- **Member list (right):** Click a member to toggle them in `panel_cast` (next panel only). Ctrl-click / click multiple members for multi-cast (original: hold Ctrl). Click again to remove. The sidebar shows `Next panel will include: Alice, Bob` + the exact code that will be sent: `(#G2:8E0:8M1 CAST:Alice,Bob)` + **Copy pose code**.
- **In a panel:** Each character has a `+ Include <nick>` button — same effect.
- **On send:** The cast is embedded in the pose code (`CAST:...`) and stored on the local message. The panel renders cast members as **silent listeners** (character + "(in panel — listening)") even if they have not spoken in that panel. After sending, the cast clears (next panel only, like original).
- **On receive:** If a message carries `CAST:Alice,Bob`, those nicks are added as listeners to that panel.

### 5. Original speaking / thinking / bubbles / action panes — same shapes

- **Say:** Normal rounded balloon, tail to character.
- **Think:** Cloud balloon (light blue fill) + `💭 Thought` label. Sent as `*thinks* text (#G...)`.
- **Whisper:** Light/dashed-look balloon + `🤫 Whisper` label, smaller. Sent as private `PRIVMSG <nick>`.
- **Action:** Caption box across the top of the panel (`* Nick waves`), not a balloon. Sent as CTCP ACTION.
- **Shout / Question / Exclaim:** Red / blue balloon variants from the original (all-caps / `!!!` → Shout, `?` → Question).
- **Play Sound:** `(sound.wav)` — receivers play it from their own Sound Search Path if they have the same filename (original behaviour).

## Build & run

```bash
cargo run --release
# or: cargo run --release -- --config ./config.lua
```

Linux app launcher: `comic-chat.desktop` is in the repo root. Copy it to
`~/.local/share/applications/` and it shows up in the launcher. It runs
`comic-chat` from PATH (e.g. `~/.cargo/bin` after `cargo install --path .`);
edit the `Exec=` line if your binary lives elsewhere.

Config: `~/.config/comic-chat/config.lua` (or `./config.lua` in cwd). See `config.lua` for the `comic.character`, `character_url`, `background`, `pose_style`, `auto_download` fields.

## Files

- `src/pose.rs` — pose code + Appears-as parsing/generation (with unit tests for `(#G090E090M1)`, `(#G2:8E0:8M1)`, `Appears as Trekkie2018.http://...`)
- `src/avb.rs` — AVB/BGB parser + `ComicArt/` scanner
- `src/irc.rs` — IRC + Comic Chat wire protocol (Appears-as, pose codes, Think/Whisper/Action)
- `src/comic.rs` — emotion/gesture/balloon rules, panel grouping, pose-aware message creation
- `src/app.rs` — UI: ComicArt pickers, member click-to-include, Selfview, Emotion wheel, Compose with live pose preview
- `ComicArt/` — authoritative art folder (flat, with sample `Trekkie2018.AVB` + PNG preview)

## Sources

- `microsoft/comic-chat` (MIT, 2026 open-source release) — repo structure, `COMICART` layout, modernized build notes.
- Mermaid Elizabeth How2 — `.AVB` = character, `.BGB` = compressed background (v2.5, with embedded URL), `.BMP` = manual background; both live in `Comicart/`.
- `comic.dedoky.com` Character Guide — copy-and-paste `Appears as` line flow for web chat / IRC.
- Kurlander et al., *Comic Chat: From Research to Product* — 8 emotions, gesture/expression wheels, 4 balloon types (speech / thought / whisper / action).
- Hacker News discussion of the 2026 open-source release — Comic Chat extended IRC with proprietary extensions hidden inside CTCP / in-band `(#G...)` codes.
- Sniffles' Place FAQ (mermeliz.com/sniffles/faq.html) — user-facing
  feature overview: Play Sound, send file, whisper, caps/shouting,
  nick in use, panels across (max 4), Comic Chat vs IRC, away (coffee
  cup), moof. Screenshots on that page come through as placeholders
  in this client, so the hammer icon could not be confirmed from
  them — it is implemented from `@` operator + MODE +o.
- `CoffeeCupAway.mrc` (Woodsman, 3/30/2000) — mIRC script simulating
  the MS Chat coffee-cup away algorithms: CTCP AWAY to every channel
  on away/back, private CTCP AWAY to joiners while away, CTCP AWAY
  re-announced on own JOIN, away entry follows NICK changes. This
  build implements that behaviour natively (see `## 0.3.17`).

## DIB / mask decoder — now in this build (v0.3.17)

- `src/dib.rs` fully decodes AVB/BGB: zlib-inflate v2.5 payloads (`AVB_ZLIB_BMP_HEADER` — `BITMAPFILEHEADER` removed, `BITMAPINFOHEADER` kept, pixel data zlib-compressed), parse `BITMAPINFOHEADER` (40-byte) + `BITMAPCOREHEADER` (12-byte), 1/4/8/24/32-bit + palette, bottom-up / top-down, row padding.
- Halo mask: each colour DIB is paired with a following 1-bit DIB of the same size. Mask luminance > 128 = opaque, else transparent. Without it you get a rectangle; with it you get the character shape.
- Face / mouth anchors: parsed from the chunk stream (`POINT` = 2 x `i32`), centre fallback if absent.
- Rendering: decoded AVB image is primary in `char_texture` / `bg_texture`. PNG sidecar is only fallback if no DIB decodes.
- Auto-download gives you the exact `curl` command; it does not fetch over HTTPS silently.
- Whisper balloons are simulated with a light stroke (egui has no dashed stroke).

## 0.3.17 — Colorful SVG icons + Comic Chat PDF corrections (SIGGRAPH 96)

### Icons — `icons/*.svg` (21 files, 0.3.17 adds `emotion-wheel`, 0.3.17 adds `coffee` + `hammer`)

SVGs drawn as a 90s toolbar row, following the original toolbar in the
MIT source: flat fills, thick outlines, each button a literal picture
of what it does. The original used one fixed-size BMP bitmap strip;
SVG keeps the same look but scales cleanly on HiDPI instead of going
blurry like a stretched BMP. SVG is kept deliberately — it is the
format that scales — and the 90s look is intentional, not a placeholder.

**Audit rule: no icon serves no purpose.** The point of adding icons is
GUI feedback. Every key below is attached to a visible button /
selectable that does something observable and reports it via tooltip +
status line. If you add a key, wire a button for it in `app.rs` or
delete the key. The full audit lives in `src/icons.rs`.

| Key | SVG | Used for |
| --- | --- | --- |
| `connect` | green arrow into a yellow door | Connect All, Servers → Connect |
| `disconnect` | red arrow out of a gray door | Servers → Disconnect |
| `say` | blue speech bubble | Compose → Say |
| `think` | purple thought cloud + dots | Compose → Think |
| `whisper` | dashed teal bubble, "shh" | Compose → Whisper, member Whisper |
| `action` | **orange rectangle, "ACTION"** — not a bubble | Compose → Action |
| `character` | smiling face on a checkerboard | Selfview shows current character (set in config.lua) |
| `background` | sky / sun / hills / wall | Share background, Copy Background-as line (character/background themselves are set in config.lua) |
| `comic` | 4-panel grid with bubbles | Top → Comic Strip |
| `text` | page with "Aa" | Top → Plain Text |
| `save` | blue floppy + green check | Save Conversation |
| `copy` | two overlapping pages | Copy pose code, Copy code, Copy Appears-as, Copy Background-as |
| `sound` | yellow bell + blue waves | Play Sound |
| `send` | green paper plane | Compose → Send |
| `settings` | orange/blue gear | (no GUI use in 0.3.17 — config is Lua-only; key still accepted) |
| `refresh` | blue circular arrows + magnifier | Rescan ComicArt |
| `members` | two heads | Servers… |
| `announce` | red megaphone + yellow waves | Announce character |
| `emotion-wheel` | circular wheel, 8 emotion faces on rim, Neutral centre, black handle dot | Emotion wheel button (toggles Freeze) |
| `coffee` | white cup, brown coffee, steam | Away button (toolbar); coffee cup next to an away nick in the member list |
| `hammer` | gray hammer, brown handle | Hammer next to a channel operator (host) in the member list |

In the egui UI each button shows `[glyph] Label` (egui cannot rasterise
SVG natively) and its tooltip names the SVG file it stands for. See
`src/icons.rs`.

**Override any or all icons in `config.lua`:**

```lua
icons = {
  say = "icons/my-say.svg",              -- relative to the project dir
  action = "/home/anon/icons/my-action.svg", -- absolute path also works
  ["emotion-wheel"] = "icons/my-wheel.svg", -- brackets: see below
}
```

Omit the `icons` table, omit a key, or point a key at a file that does
not exist, and that icon falls back to `icons/<key>.svg` / the built-in
glyph. GUI Save preserves your `icons` table (it is a known key, not an
unknown line that gets dropped).

**Brackets:** hyphenated keys MUST use bracket syntax in Lua.
`["emotion-wheel"] = "icons/my-wheel.svg"` is valid;
bare `emotion-wheel = "icons/my-wheel.svg"` is parsed as
`emotion - wheel` (a subtraction) and the whole config fails to load.
All other keys are plain identifiers, so bare `say = ...` is fine.
GUI Save writes bracket form automatically for hyphenated keys, so a
a file the GUI wrote is no longer possible — the GUI never writes config. The same note is in the example
`config.lua` comments.

### PDF corrections — what the SIGGRAPH paper says vs. what 0.3.17 did

Source: Kurlander, Skelly, Salesin, "Comic Chat", SIGGRAPH 96, pp. 225–236
(`ComicChatSiggraph.pdf`). Figures 7–9 in the PDF are real auto-generated
output; Figures 1–6 are diagrams / mockups showing the *rules* (emotion
wheel, routing channels, semantic elements). Real screenshots of the
shipped client (e.g. the "OARION71 RUNS AWAY" panel) agree with the PDF
on the points below, and disagree with 0.3.17 on the ones marked FIXED.

**FIXED in 0.3.17**

1. **Actions are narration bars, not balloons (PDF §8).** The PDF: "IRC
   clients typically support a special kind of message to indicate what
   a participant is doing. Comic Chat looks for such messages, and
   places them in narration boxes." Real screenshot: a white rectangle
   with a black border across the very top of the panel, black bold
   ALL-CAPS text "OARION71 RUNS AWAY", no tail, no bubble, character
   standing below. 0.3.17 rendered `/me` as a yellow caption box *and*
   as `"* nick text"` inside a balloon Frame — both wrong. 0.3.17:
   `Panel.actions` (separate from `Panel.messages`), drawn as a white
   rect / black border / bold ALL-CAPS bar at the top; the actor still
   appears in the character row. The yellow caption box is kept only
   for narration (join/leave), never for `/me`.
2. **Balloon text is ALL-CAPS (PDF §5.5).** "Most comic artists draw
   balloon text in all-caps... We also display the text in all-caps, no
   matter how it was typed." 0.3.17 showed mixed case in balloons.
   0.3.17 uppercases balloon display only; stored text and Plain Text
   view keep the original case.
3. **Panel breaks (PDF §6.1).** 0.3.17 broke panels only by "N messages
   per panel". The PDF breaks when: the balloon layout cannot fit
   another balloon; adding a speaker would exceed **5 characters** per
   panel; a character would get a **second balloon** in the same panel;
   or a character's expression would change in a way one panel cannot
   show. 0.3.17 implements the 5-char cap, the one-balloon-per-character
   rule, and the per-panel cap in `group_into_panels`. Actions do not
   count toward the balloon cap, but their actor counts toward the
   5-char cap.
4. **Halo (PDF §4.4).** "Comic artists... leave a little white space
   around the character. This makes the character much more visible,
   and makes it pop out of the background." 0.3.17 drew sprites /
   silhouettes with no halo. 0.3.17 draws a white frame behind sprites
   and a white underlay behind silhouette heads/bodies.

**STILL OPEN — proposed, not yet implemented (bigger than one turn)**

5. **Emotion wheel is a wheel (PDF §4.1).** 8 emotions on a circle
   perimeter (coy, happy, laughing, shouting, angry, sad, scared,
   bored), centre = neutral, distance from centre = intensity, one
   mouse drag sets type + intensity. DONE in 0.3.17: `draw_emotion_wheel`
   is a draggable circular wheel keeping the same `wheel_emotion` /
   `wheel_strength` / `wheel_frozen` state, and `icons/emotion-wheel.svg`
   draws the same wheel (8 faces on the rim in `Emotion::all()` order,
   Neutral centre, handle dot = intensity). DONE in 0.3.17: the wheel
   now draws the colour-chooser disc (Fig 1) — one wedge per emotion
   in `Emotion::all()` order, clockwise from the top, in the emotion's
   colour (translucent), around a grey Neutral centre. Interaction
   unchanged.
6. **Balloon layout: routing channels + reading order (PDF §5.2, §5.4).**
   Balloons read top-down, then left-to-right; tails are routed in
   disjoint channels so they never cross a balloon or another tail,
   and end above the speaker's face. DONE in 0.3.17 for the tail half:
   every balloon has a tail to its speaker (triangle / thought ovals),
   columns aligned so tails land on their characters, and the "▲"
   marker is gone. Routing channels still open — with one balloon row
   per panel the tails are short and vertical, so there is nothing to
   cross; full `PlaceBalloons` placement is still future work.
7. **Balloon bodies flow around text (PDF §5.3).** B-spline outlines
   with margin, no inward dips, small low-frequency waves on long
   segments. 0.3.17 uses rounded rectangles. PARTLY done in 0.3.17: the
   text inside is now centered on every line, as Woodring draws it.
   DONE in 0.3.17: balloons are painter-drawn with a hand-drawn wobble
   outline (outward-only displacement, low-frequency wave + ripple),
   no inward dips — the rounded frames are gone. The B-spline outline
   itself is still future work; the wobble polyline is the drawn
   approximation.
8. **Whisper: dashed outline + halo (PDF §5.1, §5.5).** Whisper balloons
   have a dashed outline made visible by a halo (thick white pen, then
   thin black dashed pen) and italic text. 0.3.17 has italic text + a
   thin grey outline (egui Frame has no dashed stroke). DONE in 0.3.17:
   whisper outlines are drawn with the painter's dashed path + white
   halo, italic text, no "Whisper" label. Whispers from people
   outside the room go to the text Whisper box instead (see 0.3.17).
9. **Thought: cloud / oval tail (PDF §5.1).** Thought tails are ovals
   forming an arc/line, not a solid triangle. DONE in 0.3.17: the tail
   row draws three descending ovals for thought balloons. DONE in
   0.3.17: the "Thought ○ ○" text label is gone — the cloud outline
   is the signal, as in the original.
10. **Camera zoom is computed but unused (PDF §6.2).** "Pull in to the
    tightest shot possible" subject to: never cut at the neck, never cut
    an important character at the sides, never cut at the ankles;
    establishing shot on entry and about every 15 panels. DONE in 0.3.17:
    characters scale by `panel.zoom`, background strip height follows
    the zoom, balloons deliberately do not scale (paper: "Word
    balloons are unaffected by the virtual zoom factor"), and every
    15th panel is a full-width establishing shot (zoom 0.8). The
    neck/ankle cut rules are still open — characters are never cut at
    all in this build, which is the safe side of the rule.
11. **Character facing / ordering (PDF §4.3).** Speakers face the
    person they address; positions are kept stable panel-to-panel.
    0.3.17 draws speakers left-to-right in message order, never flipped.
    DONE in 0.3.17 for the flip: AVB art faces right, so the first
    character faces right and every later character is flipped to face
    left; listeners face left toward the speakers. The full Facing +
    Neighbors greedy ordering (positions stable panel-to-panel) is
    still future work — positions are message order.
12. **Shout: jagged outline (PDF §5.1).** The paper itself says the
    jagged shout balloon was "yet to be implemented" in 1996, so
    0.3.17's thick red rounded frame is faithful to the paper — note it
    as a known gap, not a regression. Still a thick red frame in
    0.3.17 (now painter-drawn, not an egui Frame). Jagged polyline
    outline is future work.
13. **Semantic elements (PDF §6.3).** Keyword → background change /
    scene object / "Greek Chorus" meta-character. 0.3.17 has none.
    Proposal: keyword table (place, team, pet, job) → background swap
    for one panel, behind a config flag, never for private whispers.
14. **Title panel / "MEANWHILE..." (PDF §8, §9).** Figure 7's title
    panel lists the most active participants; future work suggests
    "MEANWHILE..." panels showing other conversations. 0.3.17 implements the title panel: first panel on entry, random title
    (built-ins + config `comic.titles`), cast = you + up to 3 others.
    0.3.17 made the cast follow activity (re-ranked per message).
    Corrected in 0.3.17: the cast is static — you + the first three
    others in NAMES order, computed once per join. The paper's "most
    active participants" describes a finished strip; a live chat's
    title card is fixed at entry, and it is the first frame only.
    "MEANWHILE..." still skipped until privacy is addressed.

**Mockups vs. pixel-perfect renders in the PDF — how to tell them apart**

- **Pixel-perfect / real output:** Figures 7, 8, 9 (full pages of
  auto-generated panels, with title panel, narration boxes, and the
  "OARION71"-style action bars in Fig. 9). These are the ground truth
  for layout, caps, and narration bars.
- **Diagrams / mockups:** Figures 1 (emotion wheel + three bodies),
  2 (four panels showing good/bad composition), 3 (halo on/off),
  4 (three balloon types), 5 (routing channels), 6 (Ohio semantic
  elements). These illustrate rules, not a single shipped frame —
  use them for the rule, not for pixel positions.
- **Real client screenshots** (outside the PDF) agree with Figs. 7–9
  on: white action bar at the top, ALL-CAPS balloon text, characters
  standing on a floor line with a halo, backgrounds full-bleed.
  Where a screenshot and a diagram disagree, trust the screenshot /
  Figs. 7–9.
