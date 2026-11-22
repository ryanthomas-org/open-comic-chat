// AVB / BGB — Microsoft Comic Chat character / background files.
//
// Authoritative facts (ArchiveTeam Just Solve the File Format Problem,
// Microsoft comic-chat repo, Mermaid Elizabeth How2):
// - Characters: .AVB, Backgrounds: .BGB (v2.5, zlib-compressed) + .BMP (manual backgrounds)
// - Both live FLAT in one folder: COMICART
//     C:\Program Files\Microsoft Chat\COMICART
//     C:\Program Files (x86)\Microsoft Chat\COMICART
// - 6-byte header, then tagged chunks, data section between tags 06 00 and 07 00
// - v2.1: version byte 01, images are BMP (not always standard BMP)
// - v2.5: version byte 02, image data is zlib-compressed, file carries an embedded URL
//     so other clients can auto-download it (Comics View: "automatically download
//     characters/backgrounds you do not have")
// - Magic:
//     AVB v2.1: 81 00 ?? 00 01 00   (?? = 01 or 02)
//     AVB v2.5: 81 81 ?? 00 02 00   (?? = 01 or 02)
//     BGB v2.5: 81 81 03 00 02 00
// - All files normally end with 07 00
//
// What this Rust client does (v0.3.17 — full decoder):
// - Parses header + version + kind, extracts Name (chunk 01 00) and any embedded
//   http(s) URL.
// - Fully decodes the DIBs inside the file via crate::dib (see src/dib.rs):
//   zlib-inflate v2.5 payloads, parse BITMAPINFOHEADER / COREHEADER,
//   1/4/8/24/32-bit + palette, bottom-up / top-down, row padding,
//   then pair each colour DIB with its 1-bit halo mask and composite to RGBA.
// - Face / mouth anchors are parsed from the chunk stream (POINTs) with a
//   centre fallback.
// - For rendering, the decoded AVB image is now primary. A PNG sidecar with
//   the SAME stem is only a fallback if the AVB has no decodable DIB:
//     ComicArt/Trekkie2018.AVB  (decoded)  else  ComicArt/Trekkie2018.png

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvbKind { Character, Background }

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub struct AvbFile {
    pub name: String,          // e.g. Trekkie2018 — from Name chunk, or file stem
    pub file_name: String,      // e.g. Trekkie2018.AVB
    pub path: PathBuf,
    pub kind: AvbKind,
    pub version: u8,            // 1 = v2.1, 2 = v2.5
    pub url: Option<String>,    // embedded URL, if any
    pub image_count: usize,
    pub byte_len: usize,
    pub preview_png: Option<PathBuf>, // same-stem .png / .bmp if present
}

pub fn comicart_dirs() -> Vec<PathBuf> {
    // ORDER MATTERS: scan_comicart() keeps the FIRST file for a given
    // stem. The user's config-dir ComicArt must win over ./ComicArt,
    // which may contain the bundled 24-byte Frank.AVB stub or be the
    // wrong CWD when launched from a desktop file. v0.3.17 had ./ComicArt
    // first, so a stub shadowed the user's real AVB and preview broke.
    let mut dirs = Vec::new();
    // Linux per-user location, mirroring ~/.config/comic-chat/config.lua — FIRST.
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".config/comic-chat/ComicArt"));
        dirs.push(home.join(".config/comic-chat/comicart"));
    }
    // CWD ComicArt (repo / install dir).
    dirs.push(PathBuf::from("ComicArt"));
    dirs.push(PathBuf::from("comicart"));
    // Legacy aliases from v0.2 (kept for backward compat, not authoritative)
    dirs.push(PathBuf::from("assets/characters"));
    dirs.push(PathBuf::from("assets/backgrounds"));
    dirs.push(PathBuf::from("assets"));
    dirs
}

pub fn parse_avb_bytes(bytes: &[u8], path: &Path) -> Option<AvbFile> {
    if bytes.len() < 6 { crate::log::debug("avb", format!("parse_avb_bytes: {} is only {} bytes (<6) — too short for AVB header, skipping. Fix: file is truncated or not an AVB.", path.display(), bytes.len())); return None; }
    // Header: [0]=0x81, [4]=version (01 or 02), [5]=0x00
    if bytes[0] != 0x81 { crate::log::debug("avb", format!("parse_avb_bytes: {} first byte is 0x{:02x} not 0x81 — not an AVB/BGB, skipping. Fix: check extension, file may be a plain BMP/PNG.", path.display(), bytes[0])); return None; }
    let version = bytes[4];
    if version != 1 && version != 2 { crate::log::debug("avb", format!("parse_avb_bytes: {} version byte is {} not 1 (v2.1) or 2 (v2.5) — unknown AVB version, skipping", path.display(), version)); return None; }
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_uppercase();
    let kind = match ext.as_str() {
        "AVB" => AvbKind::Character,
        "BGB" => AvbKind::Background,
        "BMP" => AvbKind::Background,
        _ => return None,
    };
    // For BGB, byte[2] is 03; for AVB, 01/02 — don't reject on it, just record.
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Unknown").to_string();
    // Name chunk: tag 01 00 at offset 6 in most files. Try to read it as UTF-16LE or ASCII.
    let name = extract_name(bytes).unwrap_or_else(|| stem.clone());
    let url = extract_url(bytes);
    // Image count: count "BM" + zlib headers
    let mut image_count = 0;
    for w in bytes.windows(2) { if w == b"BM" { image_count += 1; } }
    for w in bytes.windows(2) { if w == [0x78, 0x9C] { image_count += 1; } }
    // Preview sidecar: same stem .png, .PNG, .bmp (for backgrounds)
    let mut preview_png = None;
    for cand_ext in ["png", "PNG", "bmp", "BMP", "jpg", "JPG"] {
        let cand = path.with_extension(cand_ext);
        if cand.exists() && cand != path { preview_png = Some(cand); break; }
    }
    crate::log::debug("avb", format!("parse_avb_bytes: {} parsed OK — name={:?} kind={:?} version={} bytes={} image_count~{} url={:?} preview_png={:?}",
        path.display(), name, kind, version, bytes.len(), image_count, url, preview_png));
    Some(AvbFile {
        name,
        file_name: path.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string(),
        path: path.to_path_buf(),
        kind, version, url,
        image_count, byte_len: bytes.len(),
        preview_png,
    })
}

fn extract_name(bytes: &[u8]) -> Option<String> {
    // Real format (AVBuster / Microsoft loader): after the 6-byte header,
    // chunk type 1 is a NULL-TERMINATED string, NOT length-prefixed.
    // v0.3.17 read bytes[8..10] as a length, which is actually the first
    // two chars of the name ("Tr" -> len 29268) — so it always failed
    // and fell back to the file stem. That fallback usually saved it,
    // but a file whose internal Name differs from its stem (renamed file)
    // was then looked up under the wrong name and preview broke.
    // Try null-terminated first, then the old length-prefixed form.
    if bytes.len() > 10 && bytes[6] == 0x01 && bytes[7] == 0x00 {
        // Null-terminated: bytes[8..] up to first 0x00.
        if let Some(end) = bytes[8..].iter().position(|&b| b == 0) {
            if end > 0 && end < 256 {
                let raw = &bytes[8..8 + end];
                if let Ok(s) = std::str::from_utf8(raw) {
                    let s = s.trim_matches('\0').trim().to_string();
                    if !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == ' ' || c == '.') {
                        // Strip a trailing .AVB if the Name chunk includes it.
                        let s = s.trim_end_matches(".AVB").trim_end_matches(".avb").to_string();
                        if !s.is_empty() { return Some(s); }
                    }
                }
            }
        }
        // Fallback: old length-prefixed interpretation.
        let len = u16::from_le_bytes([bytes[8], bytes[9]]) as usize;
        if len > 0 && len < 128 && 10 + len <= bytes.len() {
            let raw = &bytes[10..10 + len];
            if raw.contains(&0) {
                let units: Vec<u16> = raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                if let Ok(s) = String::from_utf16(&units) {
                    let s = s.trim_matches('\0').trim().to_string();
                    if !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == ' ') { return Some(s); }
                }
            } else if let Ok(s) = std::str::from_utf8(raw) {
                let s = s.trim_matches('\0').trim().to_string();
                if !s.is_empty() { return Some(s); }
            }
        }
    }
    None
}

fn extract_url(bytes: &[u8]) -> Option<String> {
    // Scan for printable ASCII, look for http:// or https:// ending in .AVB/.BGB/.avb/.bgb or /
    let text = bytes.iter().map(|b| if *b >= 32 && *b < 127 { *b as char } else { '\n' }).collect::<String>();
    for token in text.split(|c: char| c.is_whitespace() || c == '\0') {
        if token.starts_with("http://") || token.starts_with("https://") {
            let clean = token.trim_matches(|c| c == '"' || c == '\'' || c == '<' || c == '>').to_string();
            if clean.len() > 10 { return Some(clean); }
        }
    }
    None
}

pub fn scan_comicart() -> (Vec<AvbFile>, Vec<AvbFile>) {
    crate::log::info("avb", format!("scan_comicart: scanning {} ComicArt dir(s) {:?} — looking for .AVB/.BGB/.BMP + PNG-only fallbacks", comicart_dirs().len(), comicart_dirs()));
    let mut chars = Vec::new();
    let mut bgs = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for dir in comicart_dirs() {
        crate::log::debug("avb", format!("scan_comicart: checking dir {}", dir.display()));
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for e in entries.flatten() {
                let p = e.path();
                let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("").to_uppercase();
                if ext != "AVB" && ext != "BGB" && ext != "BMP" { continue; }
                let key = p.file_name().and_then(|s| s.to_str()).unwrap_or("").to_uppercase();
                if seen.contains(&key) { continue; }
                seen.insert(key);
                if let Ok(bytes) = std::fs::read(&p) {
                    if let Some(f) = parse_avb_bytes(&bytes, &p) {
                        match f.kind { AvbKind::Character => chars.push(f), AvbKind::Background => bgs.push(f) }
                    } else {
                        crate::log::debug("avb", format!("scan_comicart: {} failed AVB header parse ({} bytes) — will try BMP fallback if .BMP, else skipped", p.display(), bytes.len()));
                        // .BMP backgrounds have no AVB header — still list them as backgrounds
                        if ext == "BMP" {
                            let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("Background").to_string();
                            bgs.push(AvbFile { name: stem.clone(), file_name: p.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string(), path: p.clone(), kind: AvbKind::Background, version: 0, url: None, image_count: 1, byte_len: bytes.len(), preview_png: Some(p.clone()) });
                        }
                    }
                }
            }
        }
    }
    // Also list PNG-only characters (sidecar without AVB) as characters so users
    // who only have a PNG can still pick it — marked as PNG-only.
    for dir in comicart_dirs() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for e in entries.flatten() {
                let p = e.path();
                let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("").to_uppercase();
                if ext != "PNG" { continue; }
                let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
                // If an AVB with same stem already listed, skip
                if chars.iter().any(|c| c.name.eq_ignore_ascii_case(&stem)) || bgs.iter().any(|b| b.name.eq_ignore_ascii_case(&stem)) { continue; }
                let key = format!("PNG:{stem}");
                if seen.contains(&key) { continue; }
                seen.insert(key);
                // Heuristic: PNG in ComicArt root is a character unless its name matches a known background name
                chars.push(AvbFile { name: stem.clone(), file_name: p.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string(), path: p.clone(), kind: AvbKind::Character, version: 0, url: None, image_count: 1, byte_len: std::fs::metadata(&p).map(|m| m.len() as usize).unwrap_or(0), preview_png: Some(p.clone()) });
            }
        }
    }
    chars.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    bgs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    crate::log::info("avb", format!("scan_comicart: done — {} character(s) {:?} + {} background(s) {:?}. If a character is missing, check: file is in ComicArt/ (flat, not subfolder), extension is .AVB/.BGB, and header starts 0x81.",
        chars.len(), chars.iter().map(|c| c.name.clone()).collect::<Vec<_>>(), bgs.len(), bgs.iter().map(|b| b.name.clone()).collect::<Vec<_>>()));
    (chars, bgs)
}

#[allow(dead_code)]
pub fn avb_url_for(name: &str, files: &[AvbFile]) -> Option<String> {
    files.iter().find(|f| f.name.eq_ignore_ascii_case(name)).and_then(|f| f.url.clone())
}

// Convenience: fully decode an AVB/BGB file to RGBA images (DIB + mask).
 #[allow(dead_code)]
 pub fn decode_file(path: &std::path::Path) -> Option<crate::dib::DecodedAvb> {
    crate::log::debug("avb", format!("decode_file: decoding {} via dib::decode_avb_file", path.display()));
    let r = crate::dib::decode_avb_file(path);
    match &r {
        Some(d) => crate::log::debug("avb", format!("decode_file: {} decoded OK — {} image(s), bg_image={} face={:?} mouth={:?}", path.display(), d.images.len(), d.bg_image.is_some(), d.face_anchor, d.mouth_anchor)),
        None => crate::log::warn("avb", format!("decode_file: {} failed to decode — no DIB found or all DIBs failed. Rendering will fall back to PNG sidecar if present, else placeholder. See dib logs for why.", path.display())),
    }
    r
 }






/// URL helpers: AVB = character, BGB = background. Case-insensitive suffix.
pub fn is_avb_url(url: &str) -> bool { url.trim().to_ascii_lowercase().ends_with(".avb") }
pub fn is_bgb_url(url: &str) -> bool { url.trim().to_ascii_lowercase().ends_with(".bgb") }



/// AVB helper: "http://x/Name.AVB?y=1" -> "Name" (filename without ext, no query).
pub fn avb_name_from_url(url: &str) -> String {
    let no_query = url.split(|c| c == '?' || c == '#').next().unwrap_or("");
    let file = no_query.rsplit('/').next().unwrap_or(no_query);
    if file.contains('.') {
        // Strip last extension: "Name.AVB" -> "Name", "a.b.avb" -> "a.b"
        let dot = file.rfind('.').unwrap();
        file[..dot].to_string()
    } else {
        file.to_string()
    }
}



/// BGB helper: mirror of avb_name_from_url — "http://x/Scene.BGB?y=1" -> "Scene".
pub fn bgb_name_from_url(url: &str) -> String {
    let no_query = url.split(|c| c == '?' || c == '#').next().unwrap_or("");
    let file = no_query.rsplit('/').next().unwrap_or(no_query);
    if file.contains('.') {
        let dot = file.rfind('.').unwrap();
        file[..dot].to_string()
    } else {
        file.to_string()
    }
}



/// Comicart dir rule: ONLY .AVB / .BGB files. PNGs go to separate cache.
/// is_comicart_file("Bob.AVB") == true, ("Bob.PNG") == false.
pub fn is_comicart_file(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    lower.ends_with(".avb") || lower.ends_with(".bgb")
}



/// AVB helper: character -> ".avb", background -> ".bgb".
/// avb_extension_for(true) == ".avb", (false) == ".bgb".
pub fn avb_extension_for(is_character: bool) -> &'static str {
    if is_character { ".avb" } else { ".bgb" }
}



/// Comicart helper: join dir + name + extension.
/// comicart_path_for("/cfg/comicart", "Bob", true) -> "/cfg/comicart/Bob.avb"
/// Trailing slash in dir is handled, name with ext is left as-is.
pub fn comicart_path_for(dir: &str, name: &str, is_character: bool) -> String {
    let d = dir.trim_end_matches('/');
    let ext = avb_extension_for(is_character);
    if name.to_ascii_lowercase().ends_with(".avb") || name.to_ascii_lowercase().ends_with(".bgb") {
        format!("{}/{}", d, name)
    } else {
        format!("{}/{}{}", d, name, ext)
    }
}











/// Background URL: base + "/" + name + ".bgb" (unless name already ends .bgb).
/// bgb_url_for("Scene", "http://x/bg") -> "http://x/bg/Scene.bgb"
pub fn bgb_url_for(name: &str, base: &str) -> String {
    let b = base.trim_end_matches('/');
    if name.to_ascii_lowercase().ends_with(".bgb") {
        format!("{}/{}", b, name)
    } else {
        format!("{}/{}.bgb", b, name)
    }
}







/// PNG cache rule: PNGs go to SEPARATE cache dir, never comicart.
/// avb_is_png_cache_file("Bob.png") == true, ("Bob.AVB") == false.
/// avb_cache_path_for("/cfg/cache", "Bob.AVB") -> "/cfg/cache/Bob.png"
pub fn avb_is_png_cache_file(name: &str) -> bool {
    name.trim().to_ascii_lowercase().ends_with(".png")
}
pub fn avb_cache_path_for(cache_dir: &str, name: &str) -> String {
    let d = cache_dir.trim_end_matches('/');
    let base = avb_name_from_url(name);
    format!("{}/{}.png", d, base)
}

#[cfg(test)]
mod cleanup_0317_avb_kept {
    use super::*;
    #[test]
    fn short_bytes_are_none() {
        let p = std::path::Path::new("X.AVB");
        assert!(parse_avb_bytes(b"", p).is_none());
        assert!(parse_avb_bytes(b"\x81", p).is_none());
    }
    #[test]
    fn url_lookup_on_empty_is_none() { assert_eq!(avb_url_for("missing", &[]), None); }
}
