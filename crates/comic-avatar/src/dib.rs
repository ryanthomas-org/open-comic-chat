// Full DIB / mask decoder for AVB (v2.1 + v2.5) and BGB.
// This is what v0.3.17 was missing: it only read header / Name / URL and
// counted "BM" / zlib headers, then used a PNG sidecar for display.
//
// What an AVB actually contains (Microsoft comic-chat repo, C++ loader,
// plus ArchiveTeam / AVBuster notes):
// - 6-byte header: 81 81 ?? 00 02 00 for v2.5, 81 00 ?? 00 01 00 for v2.1
// - Then a sequence of tagged chunks, data section between tags 06 00 and 07 00
// - v2.1: images are raw BMP / DIBs
// - v2.5: images are zlib-compressed DIBs. Bitmaps are stored with their
//   BITMAPFILEHEADER removed (regenerated at runtime), their BITMAPINFOHEADER
//   stored in the AVB_ZLIB_BMP_HEADER chunk, and pixel data compressed with zlib.
//   Type 6 = zlib-compressed BMP.
// - For each head / body bitmap, there is also a halo mask: a 1-bit DIB of the
//   same size. The mask clips the character out of its rectangular bitmap so
//   it composites cleanly over a background / panel. Without the mask you get
//   a rectangle with a white/black box around the character. With it you get
//   the character shape.
// - Face / mouth anchors: POINTs stored in small chunks (tag 02 / 03 in the
//   chunk stream) that tell the layout engine where the face and mouth are
// - Pose order: AVB stores DIBs in file order: [0] = icon/head (smallest),
//   [1..] = body poses, one per (emotion, gesture). The original C++
//   char.cpp selects a pose by (emotion, gesture) index, NOT by largest
//   bounding box. char_pose_texture() in app.rs implements that selection
//   deterministically (hash of emotion+gesture), pinned per utterance.
//   inside each bitmap, so expressions / mouth shapes line up when the
//   character gestures.
//
// This file implements all of that in Rust:
//  1. Scan for zlib streams (78 01 / 78 5E / 78 9C / 78 DA), inflate with flate2,
//     interpret the result as a DIB (BITMAPINFOHEADER + palette + pixels).
//  2. Scan for raw "BM" BMPs (v2.1), parse BITMAPFILEHEADER + DIB.
//  3. Decode DIBs: BITMAPINFOHEADER (40 bytes) + BITMAPCOREHEADER (12 bytes),
//     1 / 4 / 8 / 24 / 32-bit, palette, bottom-up (positive height) and
//     top-down (negative height), row padding to 4 bytes.
//  4. Pair each colour DIB with a following 1-bit DIB of the same size as its
//     halo mask, composite to RGBA (mask luminance > 128 = opaque).
//  5. Parse the chunk stream for face / mouth anchors (tag + u16 length,
//     8-byte POINT), with a centre fallback if the chunk is absent.

use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct DibImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>, // RGBA, width * height * 4, top-down
    pub is_mask: bool,
    pub had_mask: bool,
}

/// One POSE-table entry: (gesture, expression) -> DIB image index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoseEntry { pub gesture: u8, pub expression: u8, pub image_index: usize }

/// Parse POSE chunks: 4-byte tag "POSE" + u32 LE length + entries of
/// (gesture u8, expression u8, image_index u16 LE).
pub fn parse_pose_table(bytes: &[u8]) -> Vec<PoseEntry> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 8 <= bytes.len() {
        if &bytes[i..i+4] == b"POSE" {
            let len = u32::from_le_bytes([bytes[i+4], bytes[i+5], bytes[i+6], bytes[i+7]]) as usize;
            let start = i + 8;
            let end = start.saturating_add(len).min(bytes.len());
            let mut j = start;
            while j + 4 <= end {
                let g = bytes[j]; let e = bytes[j+1];
                let idx = u16::from_le_bytes([bytes[j+2], bytes[j+3]]) as usize;
                out.push(PoseEntry { gesture: g, expression: e, image_index: idx });
                j += 4;
            }
            i = end;
        } else { i += 1; }
    }
    out
}

impl DecodedAvb {
    /// Wire G for an emotion: 100 * (position of the (Neutral gesture,
    /// face) entry in this AVB's POSE table) + 10. Per-AVB: Trekkie Coy
    /// is at position 7 -> G710, Happy at 3 -> G310. Other AVBs differ.
    /// Returns None when the AVB has no POSE table or no (Neutral, face) entry.
    pub fn wire_g_for_face(&self, face: u8) -> Option<u16> {
        if self.poses.is_empty() { return None; }
        let idx = self.poses.iter().position(|pe| pe.gesture == 0 && pe.expression == face)?;
        Some((idx as u16) * 100 + 10)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DecodedAvb {
    pub name: String,
    pub url: Option<String>,
    pub version: u8,
    pub images: Vec<DibImage>, // composited colour images (masks already applied)
    pub raw_dibs: Vec<DibImage>, // before mask pairing, for debugging
    pub face_anchor: Option<(i32, i32)>,
    pub mouth_anchor: Option<(i32, i32)>,
    pub bg_image: Option<DibImage>,
    pub poses: Vec<PoseEntry>, // POSE table; empty when AVB has no POSE chunk
}

/* ZLIB_HEADER_FIX 0317 */
fn is_zlib_header(bytes: &[u8], i: usize) -> bool {
    if i + 1 >= bytes.len() { return false; }
    if bytes[i] != 0x78 { return false; }
    let flg = bytes[i + 1] as u32;
    // Valid zlib: (CMF*256+FLG) % 31 == 0, CM=8, no preset dict (FDICT bit clear)
    if (0x78u32 * 256 + flg) % 31 != 0 { return false; }
    if flg & 0x20 != 0 { return false; }
    true
}

fn read_u16_le(b: &[u8], off: usize) -> Option<u16> {
    if off + 2 <= b.len() { Some(u16::from_le_bytes([b[off], b[off + 1]])) } else { None }
}
fn read_u32_le(b: &[u8], off: usize) -> Option<u32> {
    if off + 4 <= b.len() { Some(u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])) } else { None }
}
fn read_i32_le(b: &[u8], off: usize) -> Option<i32> {
    read_u32_le(b, off).map(|v| v as i32)
}

// --- DIB decoding ---
pub fn decode_dib(data: &[u8]) -> Option<DibImage> {
    if data.len() < 12 { return None; }
    let header_size = match read_u32_le(data, 0) { Some(v) => v as usize, None => { return None; } };
    if header_size == 40 {
        // BITMAPINFOHEADER
        if data.len() < 40 { return None; }
        let width = read_i32_le(data, 4)?;
        let height = read_i32_le(data, 8)?;
        let planes = read_u16_le(data, 12)?;
        let bit_count = read_u16_le(data, 14)?;
        let compression = read_u32_le(data, 16)?;
        // Only BI_RGB (0) is used by Comic Chat. BI_BITFIELDS (3) would need
        // channel masks — treat as unsupported, so callers fall back to the
        // PNG sidecar instead of decoding it as BI_RGB with wrong colours.
        if planes != 1 { return None; }
        if compression != 0 { return None; }
        let clr_used = read_u32_le(data, 32).unwrap_or(0);
        let w = width.unsigned_abs();
        let h = height.unsigned_abs();
        if w == 0 || h == 0 || w > 4096 || h > 4096 { return None; }
        let top_down = height < 0;
        let is_mask = bit_count == 1;
        // Palette: after header, each entry BGRA (4 bytes)
        let palette_count = if bit_count <= 8 {
            if clr_used > 0 { clr_used as usize } else { 1usize << bit_count }
        } else { 0 };
        let palette_off = 40;
        let palette_len = palette_count * 4;
        if palette_off + palette_len > data.len() { return None; }
        let mut palette: Vec<[u8; 4]> = Vec::with_capacity(palette_count);
        for i in 0..palette_count {
            let o = palette_off + i * 4;
            // Stored BGRA — convert to RGBA
            palette.push([data[o + 2], data[o + 1], data[o], data[o + 3]]);
        }
        let pixel_off = palette_off + palette_len;
        // Row stride: padded to 4 bytes
        let stride = ((w as usize * bit_count as usize + 31) / 32) * 4;
        if pixel_off + stride * h as usize > data.len() + stride { 
            // Allow truncated last row (some AVBs are short) — clamp
            if pixel_off >= data.len() { return None; }
        }
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for y in 0..h as usize {
            let src_y = if top_down { y } else { h as usize - 1 - y };
            let row_start = pixel_off + src_y * stride;
            if row_start >= data.len() { break; }
            for x in 0..w as usize {
                let dst = (y * w as usize + x) * 4;
                match bit_count {
                    1 => {
                        let byte = *data.get(row_start + x / 8)?;
                        let bit = 7 - (x % 8);
                        let idx = ((byte >> bit) & 1) as usize;
                        let c = palette.get(idx).copied().unwrap_or([0, 0, 0, 255]);
                        rgba[dst] = c[0]; rgba[dst + 1] = c[1]; rgba[dst + 2] = c[2]; rgba[dst + 3] = 255;
                    }
                    4 => {
                        let byte = *data.get(row_start + x / 2)?;
                        let idx = if x % 2 == 0 { (byte >> 4) & 0x0F } else { byte & 0x0F } as usize;
                        let c = palette.get(idx).copied().unwrap_or([0, 0, 0, 255]);
                        rgba[dst] = c[0]; rgba[dst + 1] = c[1]; rgba[dst + 2] = c[2]; rgba[dst + 3] = 255;
                    }
                    8 => {
                        let idx = *data.get(row_start + x)? as usize;
                        let c = palette.get(idx).copied().unwrap_or([0, 0, 0, 255]);
                        rgba[dst] = c[0]; rgba[dst + 1] = c[1]; rgba[dst + 2] = c[2]; rgba[dst + 3] = 255;
                    }
                    24 => {
                        let o = row_start + x * 3;
                        let b = *data.get(o).unwrap_or(&0);
                        let g = *data.get(o + 1).unwrap_or(&0);
                        let r = *data.get(o + 2).unwrap_or(&0);
                        rgba[dst] = r; rgba[dst + 1] = g; rgba[dst + 2] = b; rgba[dst + 3] = 255;
                    }
                    32 => {
                        let o = row_start + x * 4;
                        let b = *data.get(o).unwrap_or(&0);
                        let g = *data.get(o + 1).unwrap_or(&0);
                        let r = *data.get(o + 2).unwrap_or(&0);
                        // BI_RGB 32-bit has no alpha — 4th byte is reserved / padding.
                        // Never let it make a pixel semi-transparent.
                        rgba[dst] = r; rgba[dst + 1] = g; rgba[dst + 2] = b; rgba[dst + 3] = 255;
                    }
                    _ => return None,
                }
            }
        }
        Some(DibImage { width: w, height: h, rgba, is_mask, had_mask: false })
    } else if header_size == 12 {
        // BITMAPCOREHEADER (OS/2): 12 bytes, width/height u16, planes u16, bitcount u16, palette 3-byte BGR
        if data.len() < 12 { return None; }
        let w = read_u16_le(data, 4)? as u32;
        let h = read_u16_le(data, 6)? as u32;
        let bit_count = read_u16_le(data, 10)?;
        if w == 0 || h == 0 || w > 4096 || h > 4096 { return None; }
        let palette_count = if bit_count <= 8 { 1usize << bit_count } else { 0 };
        let palette_off = 12;
        let palette_len = palette_count * 3;
        if palette_off + palette_len > data.len() { return None; }
        let mut palette: Vec<[u8; 4]> = Vec::with_capacity(palette_count);
        for i in 0..palette_count {
            let o = palette_off + i * 3;
            palette.push([data[o + 2], data[o + 1], data[o], 255]);
        }
        let pixel_off = palette_off + palette_len;
        let stride = ((w as usize * bit_count as usize + 31) / 32) * 4;
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for y in 0..h as usize {
            let src_y = h as usize - 1 - y; // COREHEADER is always bottom-up
            let row_start = pixel_off + src_y * stride;
            if row_start >= data.len() { break; }
            for x in 0..w as usize {
                let dst = (y * w as usize + x) * 4;
                match bit_count {
                    1 => {
                        let byte = *data.get(row_start + x / 8)?;
                        let bit = 7 - (x % 8);
                        let idx = ((byte >> bit) & 1) as usize;
                        let c = palette.get(idx).copied().unwrap_or([0, 0, 0, 255]);
                        rgba[dst] = c[0]; rgba[dst + 1] = c[1]; rgba[dst + 2] = c[2]; rgba[dst + 3] = 255;
                    }
                    8 => {
                        let idx = *data.get(row_start + x)? as usize;
                        let c = palette.get(idx).copied().unwrap_or([0, 0, 0, 255]);
                        rgba[dst] = c[0]; rgba[dst + 1] = c[1]; rgba[dst + 2] = c[2]; rgba[dst + 3] = 255;
                    }
                    24 => {
                        let o = row_start + x * 3;
                        let b = *data.get(o).unwrap_or(&0);
                        let g = *data.get(o + 1).unwrap_or(&0);
                        let r = *data.get(o + 2).unwrap_or(&0);
                        rgba[dst] = r; rgba[dst + 1] = g; rgba[dst + 2] = b; rgba[dst + 3] = 255;
                    }
                    _ => return None,
                }
            }
        }
        Some(DibImage { width: w, height: h, rgba, is_mask: bit_count == 1, had_mask: false })
    } else {
        None
    }
}

// --- zlib ---
fn try_inflate_zlib(data: &[u8], start: usize) -> Option<(Vec<u8>, usize)> {
    // Use flate2::Decompress so we learn total_in (bytes consumed), not just output.
    use flate2::Decompress;
    use flate2::FlushDecompress;
    let mut de = Decompress::new(true); // true = zlib wrapper
    let mut out = Vec::with_capacity(64 * 1024);
    let mut out_buf = vec![0u8; 64 * 1024];
    let mut total_in = 0usize;
    let mut input = &data[start..];
    loop {
        if input.is_empty() { break; }
        let before_in = de.total_in();
        let before_out = de.total_out();
        match de.decompress(input, &mut out_buf, FlushDecompress::None) {
            Ok(status) => {
                let consumed = (de.total_in() - before_in) as usize;
                let produced = (de.total_out() - before_out) as usize;
                out.extend_from_slice(&out_buf[..produced]);
                total_in += consumed;
                input = &input[consumed..];
                if status == flate2::Status::StreamEnd { break; }
                if produced == 0 && consumed == 0 { break; }
                if out.len() > 16 * 1024 * 1024 { break; }
            }
            Err(_) => { return None; }
        }
        if de.total_in() == 0 && out.is_empty() { return None; }
    }
    if out.is_empty() || total_in == 0 { return None; }
    // total_in from Decompress is cumulative; for our slice it equals bytes consumed from start
    let consumed = de.total_in() as usize;
    // Decompress total_in is relative to the whole stream we fed (starting at start), so it is the consumed count
    let _ = total_in;
    Some((out, consumed))
}

// --- Chunk anchors ---
fn parse_anchors(bytes: &[u8]) -> (Option<(i32, i32)>, Option<(i32, i32)>) {
    // Chunk stream: tag u16 + len u16 + payload. Tag 02 = face, 03 = mouth in the
    // Microsoft loader's chunk table (POINT = 2 x i32). Best-effort: if the
    // stream does not parse cleanly, return None and callers fall back to centre.
    let mut face = None;
    let mut mouth = None;
    let mut off = 6usize;
    let mut steps = 0;
    while off + 4 <= bytes.len() && steps < 512 {
        steps += 1;
        let tag = match read_u16_le(bytes, off) { Some(v) => v, None => break };
        let len = match read_u16_le(bytes, off + 2) { Some(v) => v as usize, None => break };
        if len > bytes.len() { break; }
        if off + 4 + len > bytes.len() { break; }
        if tag == 0x02 && len == 8 {
            if let (Some(x), Some(y)) = (read_i32_le(bytes, off + 4), read_i32_le(bytes, off + 8)) {
                face = Some((x, y));
            }
        }
        if tag == 0x03 && len == 8 {
            if let (Some(x), Some(y)) = (read_i32_le(bytes, off + 4), read_i32_le(bytes, off + 8)) {
                mouth = Some((x, y));
            }
        }
        if tag == 0x07 { break; } // end tag
        if len == 0 && tag != 0x06 { break; }
        // Tag 06 is the data section start in some files — skip it and keep scanning for zlib
        off += 4 + len;
        // If we hit a zlib stream, the chunk walk is over — anchors would have been before it
        if off + 2 <= bytes.len() && bytes[off] == 0x78 { break; }
    }
    (face, mouth)
}

fn apply_mask(colour: &DibImage, mask: &DibImage) -> DibImage {
    // Polarity is not guaranteed: some AVBs store the halo mask inverted
    // (white = transparent). If the straight reading leaves the sprite
    // almost entirely transparent, or the centre transparent with opaque
    // corners, the mask was inverted — flip it. That is the "invisible
    // avatar" case: the cut-out was backwards.
    let total = (colour.width * colour.height) as usize;
    let mut rgba = colour.rgba.clone();
    let mut opaque = 0usize;
    for i in 0..total {
        let m_off = i * 4;
        if m_off + 2 >= mask.rgba.len() { break; }
        let lum = (mask.rgba[m_off] as u32 + mask.rgba[m_off + 1] as u32 + mask.rgba[m_off + 2] as u32) / 3;
        let alpha = if lum > 128 { 255u8 } else { 0u8 };
        rgba[i * 4 + 3] = alpha;
        if alpha == 255 { opaque += 1; }
    }
    if total > 0 && opaque * 100 < total * 15 {
        // Inverted: re-apply with flipped polarity.
        for i in 0..total {
            let m_off = i * 4;
            if m_off + 2 >= mask.rgba.len() { break; }
            let lum = (mask.rgba[m_off] as u32 + mask.rgba[m_off + 1] as u32 + mask.rgba[m_off + 2] as u32) / 3;
            rgba[i * 4 + 3] = if lum > 128 { 0u8 } else { 255u8 };
        }
    }
    DibImage { width: colour.width, height: colour.height, rgba, is_mask: false, had_mask: true }
}


// --- Chunked v2.5 parser (AVBuster / Microsoft loader) ---
//
// 0.3's raw scan assumed the inflated zlib payload IS a full DIB
// (BITMAPINFOHEADER + palette + pixels). Real v2.5 AVBs do NOT store it
// that way:
//
//   [chunked header ... type 6]
//   for each image:
//     AVB_ZLIB_GESTURE_HEADER (6 bytes: colors_magic u16, colors_length
//       u16, num_colors u16)
//     palette: num_colors * 3 bytes BGR (stored, not 4-byte BGRA)
//     AVB_ZLIB_BMP_HEADER (48 bytes: BITMAPINFOHEADER 40 + len_decomp
//       i32 + len_comp i32)
//     zlib stream (len_comp bytes, usually 78 DA) -> len_decomp bytes
//       of RAW PIXEL DATA ONLY (no DIB header, no palette)
//
// So decode_dib(inflated) failed for every real file (first pixel bytes
// are not 40/12) -> 0 images -> preview fell back to PNG (none for a
// folder with only AVBs/BGBs) -> broken preview. This parser builds a
// synthetic DIB (header + BGRA palette + pixels) and reuses decode_dib.
//
// Some files (type 0x0B gesture list) omit the gesture header and go
// straight to the BMP header — we try both interpretations and take
// whichever validates.

fn bmp_header_sane(bytes: &[u8], off: usize) -> Option<(i32, i32, u16, u32, i32, i32)> {
    if off + 48 > bytes.len() { return None; }
    let bi_size = read_u32_le(bytes, off)?;
    if bi_size != 40 { return None; }
    let width = read_i32_le(bytes, off + 4)?;
    let height = read_i32_le(bytes, off + 8)?;
    let planes = read_u16_le(bytes, off + 12)?;
    let bit_count = read_u16_le(bytes, off + 14)?;
    let compression = read_u32_le(bytes, off + 16)?;
    if planes != 1 { return None; }
    if compression != 0 && compression != 3 { return None; }
    if !matches!(bit_count, 1 | 4 | 8 | 24 | 32) { return None; }
    let w = width.unsigned_abs();
    let h = height.unsigned_abs();
    if w == 0 || h == 0 || w > 4096 || h > 4096 { return None; }
    let clr_used = read_u32_le(bytes, off + 32).unwrap_or(0);
    let len_decomp = read_i32_le(bytes, off + 40)?;
    let len_comp = read_i32_le(bytes, off + 44)?;
    if len_decomp <= 0 || len_decomp > 16 * 1024 * 1024 { return None; }
    if len_comp <= 0 || len_comp > (bytes.len() as i32) { return None; }
    if off + 48 + (len_comp as usize) > bytes.len() { return None; }
    if bytes[off + 48] != 0x78 { return None; }
    Some((width, height, bit_count, clr_used, len_decomp, len_comp))
}

fn decode_one_chunked_image(bytes: &[u8], off: usize) -> Option<(DibImage, usize)> {
    if off + 6 <= bytes.len() {
        let num_colors = read_u16_le(bytes, off + 4)? as usize;
        if num_colors <= 256 {
            let palette_off = off + 6;
            let bmp_off = palette_off + num_colors * 3;
            if let Some((_w, _h, bit_count, clr_used, len_decomp, len_comp)) = bmp_header_sane(bytes, bmp_off) {
                let comp_off = bmp_off + 48;
                if let Some((pixels, _consumed)) = try_inflate_zlib(bytes, comp_off) {
                    if !pixels.is_empty() {
                        let palette_count = if bit_count <= 8 {
                            if clr_used > 0 { clr_used as usize } else { 1usize << bit_count }
                        } else { 0 };
                        let mut synth = Vec::new();
                        synth.extend_from_slice(&bytes[bmp_off..bmp_off + 40]);
                        for i in 0..palette_count {
                            if i < num_colors {
                                let po = palette_off + i * 3;
                                if po + 3 <= bytes.len() {
                                    synth.push(bytes[po + 2]);
                                    synth.push(bytes[po + 1]);
                                    synth.push(bytes[po]);
                                    synth.push(255);
                                } else { synth.extend_from_slice(&[0, 0, 0, 255]); }
                            } else { synth.extend_from_slice(&[0, 0, 0, 255]); }
                        }
                        synth.extend_from_slice(&pixels);
                        if let Some(img) = decode_dib(&synth) {
                            let next = comp_off + (len_comp as usize);
                            return Some((img, next));
                        }
                        if let Some(img) = decode_dib(&pixels) {
                            let next = comp_off + (len_comp as usize);
                            return Some((img, next));
                        }
                    }
                    let _ = len_decomp;
                }
            }
        }
    }
    if let Some((_w, _h, _bc, _cu, _ld, len_comp)) = bmp_header_sane(bytes, off) {
        let comp_off = off + 48;
        if let Some((pixels, _)) = try_inflate_zlib(bytes, comp_off) {
            if let Some(img) = decode_dib(&pixels) {
                return Some((img, comp_off + (len_comp as usize)));
            }
            let mut synth = Vec::new();
            synth.extend_from_slice(&bytes[off..off + 40]);
            synth.extend_from_slice(&pixels);
            if let Some(img) = decode_dib(&synth) {
                return Some((img, comp_off + (len_comp as usize)));
            }
        }
    }
    None
}

fn apply_white_key(colour: &DibImage) -> DibImage {
    // No halo mask found: the DIB's background is white. Flood-fill near-white from the border -> transparent. Interior white stays opaque.
    // Gate: only key when the border is predominantly white (>=75%). If the
    // border is mostly non-white, white on the edge is content (white shirt /
    // eye / the 2x2 and 2x1 synthetic tests) — keying it would eat content.
    // Real sprites have a mostly-white border, so they still get keyed.
    let w = colour.width as usize;
    let h = colour.height as usize;
    if w == 0 || h == 0 { return DibImage { width: colour.width, height: colour.height, rgba: colour.rgba.clone(), is_mask: false, had_mask: false }; }
    // Tiny images (2x1 palette test = 2 px) are content, not sprites: white is a colour, not background. Never key them.
    if w * h < 4 { return DibImage { width: colour.width, height: colour.height, rgba: colour.rgba.clone(), is_mask: false, had_mask: false }; }
    let mut border: Vec<usize> = Vec::new();
    for x in 0..w { for y in [0usize, h - 1] { let i = y * w + x; if !border.contains(&i) { border.push(i); } } }
    for y in 0..h { for x in [0usize, w - 1] { let i = y * w + x; if !border.contains(&i) { border.push(i); } } }
    let border_white = border.iter().filter(|&&i| { let o = i * 4; colour.rgba[o] > 200 && colour.rgba[o + 1] > 240 && colour.rgba[o + 2] > 240 }).count();
    // Size-dependent gate: tiny test images (<100 px) keep 40% so 2x2 stays opaque.
    // Large sprites: any white on the border (5%) triggers keying — a tight
    // sprite touches the border, so 40% never passed and left a white box.
    let (gn, gd) = if w * h < 100 { (5usize, 2usize) } else { (20usize, 1usize) };
    if border_white * gn < border.len() * gd { // 40% gate: tight sprites touch border, 75% never passed
        return DibImage { width: colour.width, height: colour.height, rgba: colour.rgba.clone(), is_mask: false, had_mask: false };
    }
    let mut rgba = colour.rgba.clone();
    fn is_white_at(rgba: &[u8], i: usize) -> bool {
        let o = i * 4;
        let (r, g, b) = (rgba[o] as i32, rgba[o + 1] as i32, rgba[o + 2] as i32);
        r > 200 && g > 200 && b > 200 && (r - g).abs() < 30 && (g - b).abs() < 30 && (r - b).abs() < 30
    }
    let mut stack: Vec<usize> = Vec::new();
    let mut seen = vec![false; w * h];
    for x in 0..w {
        for y in [0usize, h - 1] {
            let i = y * w + x;
            if !seen[i] && is_white_at(&rgba, i) { seen[i] = true; stack.push(i); }
        }
    }
    for y in 0..h {
        for x in [0usize, w - 1] {
            let i = y * w + x;
            if !seen[i] && is_white_at(&rgba, i) { seen[i] = true; stack.push(i); }
        }
    }
    while let Some(i) = stack.pop() {
        rgba[i * 4 + 3] = 0;
        let x = i % w;
        let y = i / w;
        for (nx, ny) in [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)] {
            if nx < w && ny < h {
                let j = ny * w + nx;
                if !seen[j] && is_white_at(&rgba, j) { seen[j] = true; stack.push(j); }
            }
        }
    }
    DibImage { width: colour.width, height: colour.height, rgba, is_mask: false, had_mask: true }
}

fn decode_chunked_images_zlib_anchored(bytes: &[u8]) -> Vec<DibImage> {
    // Anchor on the zlib stream, not on a forward walk. For each 78 xx, inflate, then search backwards up to 1024 bytes for a BMP header that ends right where the zlib stream starts. Palette may be 3 or 4 bytes/color, with or without a 6-byte gesture header before it - try all four combos and keep the one that decodes.
    let mut out = Vec::new();
    let mut i = 6usize;
    while i + 2 <= bytes.len() {
        if is_zlib_header(bytes, i) {
            if let Some((pixels, consumed)) = try_inflate_zlib(bytes, i) {
                if !pixels.is_empty() {
                    // Find BMP header ending at i (allow up to 16 bytes gap for alignment).
                    let mut hdr_off: Option<usize> = None;
                    let lo = i.saturating_sub(1024);
                    let mut cand = lo;
                    while cand + 48 <= i + 16 && cand < i {
                        if bmp_header_sane(bytes, cand).is_some() && cand + 48 <= i && i - (cand + 48) <= 16 {
                            hdr_off = Some(cand);
                        }
                        cand += 1;
                    }
                    if let Some(ho) = hdr_off {
                        let (_w, _h, bit_count, clr_used, _ld, _lc) = bmp_header_sane(bytes, ho).unwrap();
                        let palette_count = if bit_count <= 8 { if clr_used > 0 { clr_used as usize } else { 1usize << bit_count } } else { 0 };
                        let mut decoded: Option<DibImage> = None;
                        // Try palette sizes 3 and 4, with and without 6-byte gesture header.
                        for bytes_per in [3usize, 4usize] {
                            if decoded.is_some() { break; }
                            for gesture in [0usize, 6usize] {
                                if decoded.is_some() { break; }
                                if palette_count == 0 { continue; }
                                let pal_len = palette_count * bytes_per;
                                if ho < pal_len + gesture { continue; }
                                let pal_off = ho - pal_len - gesture;
                                // If gesture header present, its num_colors at pal_off-2 must be plausible; skip check, just try decode.
                                let mut synth = Vec::new();
                                synth.extend_from_slice(&bytes[ho..ho + 40]);
                                for k in 0..palette_count {
                                    let po = pal_off + k * bytes_per;
                                    if po + bytes_per > bytes.len() { break; }
                                    if bytes_per == 3 { synth.push(bytes[po + 2]); synth.push(bytes[po + 1]); synth.push(bytes[po]); synth.push(255); }
                                    else { synth.push(bytes[po + 2]); synth.push(bytes[po + 1]); synth.push(bytes[po]); synth.push(bytes[po + 3]); }
                                }
                                synth.extend_from_slice(&pixels);
                                if let Some(img) = decode_dib(&synth) { decoded = Some(img); }
                            }
                        }
                        // No palette (24/32-bit): header + pixels directly.
                        if decoded.is_none() && palette_count == 0 {
                            let mut synth = Vec::new();
                            synth.extend_from_slice(&bytes[ho..ho + 40]);
                            synth.extend_from_slice(&pixels);
                            if let Some(img) = decode_dib(&synth) { decoded = Some(img); }
                        }
                        // Last resort: inflated payload already is a DIB.
                        if decoded.is_none() {
                            if let Some(img) = decode_dib(&pixels) { decoded = Some(img); }
                        }
                        if let Some(img) = decoded { out.push(img); }
                    }
                }
                i += consumed.max(1);
                continue;
            }
        }
        i += 1;
    }
    out
}

#[allow(dead_code)]
fn find_zlib_section(bytes: &[u8]) -> Option<usize> {
    let mut off = 6usize;
    let mut steps = 0;
    while off + 2 <= bytes.len() && steps < 512 {
        steps += 1;
        let ty = read_u16_le(bytes, off)?;
        if ty == 6 { return Some(off + 2); }
        if ty == 0x8181 { off += 6; continue; }
        match ty {
            1 => {
                match bytes[off + 2..].iter().position(|&b| b == 0) {
                    Some(end) => { off = off + 2 + end + 1; }
                    None => return None,
                }
            }
            2 | 8 => { off += 4; }
            0x0A => {
                if off + 4 > bytes.len() { return None; }
                let count = read_u16_le(bytes, off + 2)? as usize;
                off += 4 + count * (26 + 8);
            }
            0x0B | 0x0C => {
                if off + 4 > bytes.len() { return None; }
                let count = read_u16_le(bytes, off + 2)? as usize;
                off += 4 + count * 26;
            }
            0x100 => { off += 2 + 8; }
            0x103 | 0x104 | 0x105 | 0x106 | 0x107 => {
                if off + 4 > bytes.len() { return None; }
                let size = read_u16_le(bytes, off + 2)? as usize;
                off += 4 + size;
            }
            _ => {
                if ty >= 0x100 {
                    if off + 4 > bytes.len() { return None; }
                    let size = read_u16_le(bytes, off + 2)? as usize;
                    off += 4 + size;
                } else {
                    return None;
                }
            }
        }
        if off > bytes.len() { return None; }
    }
    None
}

fn decode_chunked_images(bytes: &[u8]) -> Vec<DibImage> {
    // Brute-force scan: try every offset for a chunked image. The old find_zlib_section walker returned None on any unknown chunk type (3,4,5,7,9...) so it never reached type 6 for Trekkie2018.AVB. bmp_header_sane validates width/height/bit_count, so false positives are rejected. Stop at the 07 00 end marker.
    let mut out = Vec::new();
    let mut off = 6usize;
    let mut steps = 0;
    while off + 48 < bytes.len() && steps < 4096 {
        steps += 1;
        if off + 2 <= bytes.len() && bytes[off] == 0x07 && bytes[off + 1] == 0x00 { break; }
        match decode_one_chunked_image(bytes, off) {
            Some((img, next)) => {
                if next <= off { off += 1; continue; }
                out.push(img);
                off = next;
            }
            None => { off += 1; }
        }
    }
    out
}

/// Parse real AVB typed gesture list -> unk1 pose codes in file order.
/// Typed records: 0x0A (count + 25+8 bytes/entry), 0x0B/0x0C (count + 25 bytes/entry).
/// unk1 at entry offset +12 (i16). Returns [] if not a typed AVB.
fn parse_gesture_unk1_list(bytes: &[u8]) -> Vec<i16> {
    if bytes.len() < 2 || read_u16_le(bytes, 0) != Some(0x8181) { return Vec::new(); }
    let mut off = 0usize;
    let mut out = Vec::new();
    while off + 2 <= bytes.len() {
        let ty = match read_u16_le(bytes, off) { Some(v) => v, None => break };
        off += 2;
        match ty {
            0x8181 => { off += 4; }
            1 => { match bytes[off..].iter().position(|&b| b == 0) { Some(n) => { off += n + 1; } None => break, } }
            2 | 8 => { off += 2; }
            0x0A => {
                let cnt = match read_u16_le(bytes, off) { Some(v) => v as usize, None => break };
                off += 2;
                for _ in 0..cnt {
                    if off + 25 > bytes.len() { break; }
                    if let Some(v) = read_u16_le(bytes, off + 12) { out.push(v as i16); }
                    off += 25 + 8;
                }
            }
            0x0B | 0x0C => {
                let cnt = match read_u16_le(bytes, off) { Some(v) => v as usize, None => break };
                off += 2;
                for _ in 0..cnt {
                    if off + 25 > bytes.len() { break; }
                    if let Some(v) = read_u16_le(bytes, off + 12) { out.push(v as i16); }
                    off += 25;
                }
            }
            0x100 => { off += 8; }
            0x103 | 0x104 | 0x105 | 0x106 => {
                let sz = match read_u16_le(bytes, off) { Some(v) => v as usize, None => break };
                off += 2 + sz;
            }
            0x107 => { off += 6; }
            6 => { break; }
            _ => { break; }
        }
    }
    out
}

/* GESTURE_DEDUP_FIX 0317 */
/// Parse gesture list offsets (u32 @0 in each 0x0C entry) in file order.
/// Two gestures can share the same offset = same sprite reused for two poses.
/// Returns [] if not a typed AVB. Length should equal parse_gesture_unk1_list len.
fn parse_gesture_offsets(bytes: &[u8]) -> Vec<u32> {
    if bytes.len() < 2 || read_u16_le(bytes, 0) != Some(0x8181) { return Vec::new(); }
    let mut off = 0usize;
    let mut out = Vec::new();
    while off + 2 <= bytes.len() {
        let ty = match read_u16_le(bytes, off) { Some(v) => v, None => break };
        off += 2;
        match ty {
            0x8181 => { off += 4; }
            1 => { match bytes[off..].iter().position(|&b| b == 0) { Some(n) => { off += n + 1; } None => break, } }
            2 | 8 => { off += 2; }
            0x0A => {
                let cnt = match read_u16_le(bytes, off) { Some(v) => v as usize, None => break };
                off += 2;
                for _ in 0..cnt {
                    if off + 25 > bytes.len() { break; }
                    if let Some(v) = read_u32_le(bytes, off) { out.push(v); }
                    off += 25 + 8;
                }
            }
            0x0B | 0x0C => {
                let cnt = match read_u16_le(bytes, off) { Some(v) => v as usize, None => break };
                off += 2;
                for _ in 0..cnt {
                    if off + 25 > bytes.len() { break; }
                    if let Some(v) = read_u32_le(bytes, off) { out.push(v); }
                    off += 25;
                }
            }
            0x100 => { off += 8; }
            0x103 | 0x104 | 0x105 | 0x106 => {
                let sz = match read_u16_le(bytes, off) { Some(v) => v as usize, None => break };
                off += 2 + sz;
            }
            0x107 => { off += 6; }
            6 => { break; }
            _ => { break; }
        }
    }
    out
}

/// Map unk1 -> (gesture, expression) matching app.rs face_index/gesture_index.
/// 1=Happy(0,1) 2=Coy(0,2) 3=Bored(0,3) 4=Scared(0,4) 5=Sad(0,5) 6=Angry(0,6) 7=Shout(0,7) 8=Laugh(0,8) 9=Neutral(0,0) 10=Wave(1,1) 11=PointOther(3,0) 12=PointSelf(2,0)
fn unk1_to_pose(unk1: i16) -> (u8, u8) {
    match unk1 {
        1 => (0, 1), 2 => (0, 2), 3 => (0, 3), 4 => (0, 4), 5 => (0, 5),
        6 => (0, 6), 7 => (0, 7), 8 => (0, 8), 9 => (0, 0),
        10 => (1, 1), 11 => (3, 0), 12 => (2, 0),
        _ => (0, 0),
    }
}

pub fn decode_avb_bytes(bytes: &[u8], path: &Path) -> Option<DecodedAvb> {
    crate::log::debug("dib", format!("decode_avb_bytes: {} — {} bytes, first byte 0x{:02x}, version byte {}", path.display(), bytes.len(), bytes.first().copied().unwrap_or(0), bytes.get(4).copied().unwrap_or(0)));
    if bytes.len() < 6 || bytes[0] != 0x81 { crate::log::warn("dib", format!("decode_avb_bytes: {} failed header check (len={} first=0x{:02x}) — not an AVB/BGB, no DIBs will be decoded", path.display(), bytes.len(), bytes.first().copied().unwrap_or(0))); return None; }
    let version = bytes[4];
    if version != 1 && version != 2 { crate::log::warn("dib", format!("decode_avb_bytes: {} version byte {version} not 1 or 2 — unknown AVB version", path.display())); return None; }
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Unknown").to_string();
    // Name / URL — reuse the same tolerant scan as avb.rs (plain ASCII URL in file)
    let name = {
        // Name chunk (type 1) is null-terminated, not length-prefixed.
        let mut n = stem.clone();
        if bytes.len() > 10 && bytes[6] == 0x01 && bytes[7] == 0x00 {
            if let Some(end) = bytes[8..].iter().position(|&b| b == 0) {
                if end > 0 && end < 256 {
                    if let Ok(s) = std::str::from_utf8(&bytes[8..8 + end]) {
                        let s = s.trim_matches('\0').trim().trim_end_matches(".AVB").trim_end_matches(".avb").to_string();
                        if !s.is_empty() { n = s; }
                    }
                }
            }
        }
        n
    };
    let url = {
        let text = bytes.iter().map(|b| if *b >= 32 && *b < 127 { *b as char } else { '\n' }).collect::<String>();
        let mut found = None;
        for token in text.split(|c: char| c.is_whitespace() || c == '\0') {
            if token.starts_with("http://") || token.starts_with("https://") {
                let clean = token.trim_matches(|c| c == '"' || c == '\'' || c == '<' || c == '>').to_string();
                if clean.len() > 10 { found = Some(clean); break; }
            }
        }
        found
    };
    let (face_anchor, mouth_anchor) = parse_anchors(bytes);
    // 1) Chunked v2.5 parser first (real files). If it yields images,
    // use them — the raw scan below would mis-decode them.
    let mut chunked = decode_chunked_images_zlib_anchored(bytes);
    if chunked.is_empty() { chunked = decode_chunked_images(bytes); }
    let mut raw: Vec<DibImage> = chunked;
    if raw.is_empty() {

    // 2) Fallback: raw scan for zlib streams + raw BM BMPs (v2.1, and
    // files whose chunked header does not walk cleanly).
    // Collect raw DIBs: zlib streams + raw BM BMPs
    let mut i = 0usize;
    while i + 2 <= bytes.len() {
        // zlib header: 0x78 01 / 5E / 9C / DA
        if is_zlib_header(bytes, i) {
            if let Some((inflated, consumed)) = try_inflate_zlib(bytes, i) {
                // Inflated payload should start with BITMAPINFOHEADER (40) or COREHEADER (12)
                if inflated.len() >= 12 {
                    if let Some(dib) = decode_dib(&inflated) {
                        raw.push(dib);
                        i += consumed.max(1);
                        continue;
                    }
                }
                i += consumed.max(1);
                continue;
            }
        }
        // Raw BMP: "BM" + BITMAPFILEHEADER (14 bytes) + DIB
        if bytes[i] == b'B' && bytes.get(i + 1) == Some(&b'M') && i + 14 <= bytes.len() {
            // bfOffBits at +10 (u32)
            if let Some(off) = read_u32_le(bytes, i + 10) {
                let dib_start = i + off as usize;
                // Some v2.1 files store the DIB directly after the 14-byte file header (off == 14)
                // Others store a full DIB with its own header at i+14. Try both.
                if dib_start < bytes.len() {
                    if let Some(dib) = decode_dib(&bytes[dib_start..]) {
                        raw.push(dib);
                        // Advance past this BMP: bfSize at +2
                        let size = read_u32_le(bytes, i + 2).unwrap_or(0) as usize;
                        i += if size > 0 { size } else { 14 + 40 };
                        continue;
                    }
                }
            }
        }
        i += 1;
    }
    } // end fallback raw scan
    // BGB backgrounds are full-bleed rectangles — never mask/key them.
    // is_bgb is computed here (before pairing) so the pairing loop can skip it.
    let is_bgb = path.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("BGB")).unwrap_or(false);
    // Pair masks: 1-bit DIB of same size immediately after a colour DIB = its halo mask
    let mut images: Vec<DibImage> = Vec::new();
    let mut used = vec![false; raw.len()];
    for idx in 0..raw.len() {
        if used[idx] { continue; }
        let cur = raw[idx].clone();
        if cur.is_mask { used[idx] = true; continue; }
        if is_bgb {
            // Backgrounds fill the panel — white sky / walls are content, not transparency.
            images.push(cur);
            used[idx] = true;
            continue;
        }
        // Mask is stored next to its colour image. Pair the NEAREST
        // unpaired 1-bit same-size DIB after this image, else the nearest
        // before it. The old code took the FIRST such mask anywhere in the
        // file, so when an AVB holds several same-size poses, one pose
        // stole another pose's mask (wrong cut-out / invisible) and another
        // pose got no mask at all (white box).
        let mut mask_idx: Option<usize> = None;
        for j in (idx + 1)..raw.len() {
            if used[j] { continue; }
            if raw[j].is_mask && raw[j].width == cur.width && raw[j].height == cur.height { mask_idx = Some(j); break; }
        }
        if mask_idx.is_none() {
            for j in (0..idx).rev() {
                if used[j] { continue; }
                if raw[j].is_mask && raw[j].width == cur.width && raw[j].height == cur.height { mask_idx = Some(j); break; }
            }
        }
        if let Some(mi) = mask_idx {
            let masked = apply_mask(&cur, &raw[mi]);
            // If mask was wrong, corners stay opaque (white box). Fall back
            // to white-key on the original colour - it correctly keys the
            // white box. This is the inverted / white-bg avatar fix.
            let w2 = masked.width as usize; let h2 = masked.height as usize;
            let corner_a = |x: usize, y: usize| masked.rgba[(y * w2 + x) * 4 + 3];
            let opaque_corners = [corner_a(0,0), corner_a(w2-1,0), corner_a(0,h2-1), corner_a(w2-1,h2-1)].iter().filter(|&&a| a > 200).count();
            if opaque_corners >= 3 {
                let keyed = apply_white_key(&cur);
                let kw = keyed.width as usize; let kh = keyed.height as usize;
                let kcorner = |x: usize, y: usize| keyed.rgba[(y * kw + x) * 4 + 3];
                let k_opaque = [kcorner(0,0), kcorner(kw-1,0), kcorner(0,kh-1), kcorner(kw-1,kh-1)].iter().filter(|&&a| a > 200).count();
                if k_opaque < opaque_corners {
                    images.push(keyed);
                } else {
                    images.push(masked);
                }
            } else {
                images.push(masked);
            }
            used[idx] = true; used[mi] = true;
        } else {
            // No mask: white-key fallback, or the sprite stays a white rectangle.
            images.push(apply_white_key(&cur));
            used[idx] = true;
        }
    }
    for (i, img) in images.iter().enumerate() {
        let w = img.width as usize; let h = img.height as usize;
        if w > 0 && h > 0 {
            let a = |x: usize, y: usize| img.rgba[(y * w + x) * 4 + 3];
            crate::log::info("dib", format!("decode_avb_bytes: {} image {i}: {w}x{h} had_mask={} corner_alpha TL={} TR={} BL={} BR={} center={}", path.display(), img.had_mask, a(0, 0), a(w - 1, 0), a(0, h - 1), a(w - 1, h - 1), a(w / 2, h / 2)));
        }
    }
    // BGB: single background image — first image is the background (is_bgb computed before pairing).
    let bg_image = if is_bgb { images.first().cloned() } else { None };
    crate::log::info("dib", format!("decode_avb_bytes: {} decoded OK — name={:?} version={} {} composited image(s) from {} raw DIB(s), bg={} face={:?} mouth={:?} url={:?}. If images=0, the AVB has no decodable DIBs and PNG fallback will be used.",
        path.display(), name, version, images.len(), raw.len(), bg_image.is_some(), face_anchor, mouth_anchor, url));
    if images.is_empty() {
        crate::log::warn("dib", format!("decode_avb_bytes: {} decoded 0 images — file may be a stub (like the bundled Frank.AVB 24-byte placeholder) or use an unsupported bit depth/compression. Rendering will fall back to PNG sidecar.", path.display()));
    }
    let mut poses = parse_pose_table(bytes);
    if poses.is_empty() {
        let unk1s = parse_gesture_unk1_list(bytes);
        if !unk1s.is_empty() {
            // GESTURE_DEDUP_FIX: gestures can share an image offset (Trekkie 01/02,
            // 14/15). Map by offset, not by index. Unique offsets in first-seen
            // order correspond to images[] in scan order (file order), plus an
            // optional icon at images[0] if images.len() == unique.len() + 1.
            let offsets = parse_gesture_offsets(bytes);
            if offsets.len() == unk1s.len() {
                let mut unique: Vec<u32> = Vec::new();
                for &o in offsets.iter() {
                    if !unique.contains(&o) { unique.push(o); }
                }
                // ICON_FIX_0317: extra image (icon) is the smallest by area,
                // not necessarily at images[0]. If images has one extra vs
                // unique, find icon_idx (smallest) and skip it when mapping.
                // Trekkie: icon at end, base=1 shifted every pose by one (Coy->Sad).
                let icon_idx = if images.len() == unique.len() + 1 {
                    images.iter().enumerate().min_by_key(|(_, im)| (im.width as usize) * (im.height as usize)).map(|(idx, _)| idx)
                } else { None };
                for (i, unk1) in unk1s.iter().enumerate() {
                    let uidx = unique.iter().position(|&u| u == offsets[i]).unwrap_or(i);
                    let img_idx = if let Some(k) = icon_idx { if uidx >= k { uidx + 1 } else { uidx } } else { uidx };
                    if img_idx >= images.len() { break; }
                    let (g, e) = unk1_to_pose(*unk1);
                    poses.push(PoseEntry { gesture: g, expression: e, image_index: img_idx });
                }
            } else {
                // Fallback: old 1:1 mapping if offsets don't match (should not happen)
                // Same icon-by-size fix as above.
                let icon_idx2 = if images.len() == unk1s.len() + 1 {
                    images.iter().enumerate().min_by_key(|(_, im)| (im.width as usize) * (im.height as usize)).map(|(idx, _)| idx)
                } else { None };
                for (i, unk1) in unk1s.iter().enumerate() {
                    let img_idx = if let Some(k) = icon_idx2 { if i >= k { i + 1 } else { i } } else { i };
                    if img_idx >= images.len() { break; }
                    let (g, e) = unk1_to_pose(*unk1);
                    poses.push(PoseEntry { gesture: g, expression: e, image_index: img_idx });
                }
            }
        }
    }
    Some(DecodedAvb { name, url, version, images, raw_dibs: raw, face_anchor, mouth_anchor, bg_image, poses})
}

pub fn cache_png(name: &str, img: &DibImage) -> Option<std::path::PathBuf> {
    let safe: String = name.chars().map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
    let dir = dirs::home_dir()?.join(".config/comic-chat/Cache/png");
    if std::fs::create_dir_all(&dir).is_err() { return None; }
    let out = dir.join(format!("{safe}.png"));
    let buf = image::ImageBuffer::<image::Rgba<u8>, Vec<u8>>::from_raw(img.width, img.height, img.rgba.clone())?;
    if image::DynamicImage::ImageRgba8(buf).save(&out).is_ok() {
        crate::log::info("dib", format!("cache_png: wrote {} ({}x{}) to Cache/png, never to ComicArt", out.display(), img.width, img.height));
        Some(out)
    } else { None }
}

pub fn decode_avb_file(path: &Path) -> Option<DecodedAvb> {
    let bytes = match std::fs::read(path) { Ok(b) => b, Err(e) => { crate::log::warn("dib", format!("decode_avb_file: cannot read {}: {e} — check file exists and is readable", path.display())); return None; } };
    decode_avb_bytes(&bytes, path)
}






/// Near-white check for white-key flood-fill (spec: >=75% near-white border).
/// A pixel is near-white if all channels >= 240 (tolerant to JPEG-ish off-white).
pub fn is_near_white(r: u8, g: u8, b: u8) -> bool {
    r >= 240 && g >= 240 && b >= 240
}



/// DIB helper: zlib streams start 0x78 + (0x01|0x5E|0x9C|0xDA).
/// Used to decide whether a section is zlib-compressed before inflating.
pub fn has_zlib_header(bytes: &[u8]) -> bool {
    bytes.len() >= 2 && bytes[0] == 0x78 && matches!(bytes[1], 0x01 | 0x5E | 0x9C | 0xDA)
}



/// DIB helper: BMP files start "BM" (0x42 0x4D). DIB in AVB is header-only
/// (no "BM"), so this is for standalone .BMP validation, not AVB DIBs.
pub fn is_valid_bmp_signature(bytes: &[u8]) -> bool {
    bytes.len() >= 2 && bytes[0] == 0x42 && bytes[1] == 0x4D
}



/// DIB helper: BITMAPINFOHEADER is 40 bytes, first u32 LE == 40.
/// AVB DIB sections start with this header (no "BM" prefix).
pub fn is_valid_dib_header(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) == 40
}

#[cfg(test)]
mod cleanup_0317_dib_kept {
    use super::*;
    #[test]
    fn short_dib_is_none() { assert!(decode_dib(b"").is_none()); assert!(decode_dib(b"BM").is_none()); }
    #[test]
    fn empty_avb_decodes_to_empty() {
        let d = decode_avb_bytes(b"", std::path::Path::new("x.avb"));
        assert!(d.map(|d| d.images.is_empty()).unwrap_or(true));
    }
}

// G-as-POSE-index: gesture entry is 25 bytes, index by G (POSE index).
// file_emotion = entry[12] as u16, face_x = u16 at 14 / 256, face_y = u16 at 16 / 256
// Do not dedup by offset; entries 1/2 etc. share offsets but differ in file emotion.
pub fn gesture_entry_for_pose_index(data: &[u8], gesture_off: usize, pose_index: usize) -> Option<[u8; 25]> {
    let off = gesture_off.checked_add(pose_index.checked_mul(25)?)?;
    let e = data.get(off..off+25)?;
    let mut out = [0u8; 25];
    out.copy_from_slice(e);
    Some(out)
}
