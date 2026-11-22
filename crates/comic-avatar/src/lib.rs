//! Avatar / ComicArt loading: AVB + BGB + DIB.
//! Codifies avatar.cpp/.h, bodycam.cpp/.h, the DIB loader, and the
//! AVB/BGB handling in comic-chat-rs 0.3.17 src/avb.rs + src/dib.rs.
//!
//! File formats (authoritative, from the Microsoft source + observed files):
//!
//! AVB (avatar, v2.5): TYPED records, NOT `[tag][len][payload]` chunks.
//!   Verified against Trekkie2018.AVB + AVBuster + MS avatar.h:
//!   File = sequence of `u16 type` records: 0x8181 header, 1=name,
//!   2/8=unk, 0x0A/0x0B/0x0C=gesture list (count + 25-byte entries),
//!   0x100/0x103/0x104/0x105/0x106/0x107=strings + zlibOffset, 6=end.
//!   Gesture entry (25 bytes): i32 gestureOffset, i32 unknownOffset,
//!   i32 poseOffset, i16 unk1=POSE CODE, 4 bytes face (bytes 15,17 = x,y),
//!   i16 unk2==256, i16 unk3==257, i16 unk4==770, u8 unk5==3.
//!   BMP at gestureOffset+zlibOffset: gesture hdr (3x u16) + palette
//!   (num_colors*3 RGB) + BITMAPINFOHEADER + len_decomp/len_comp + zlib data.
//!   unk1: 1=Happy, 2=Coy, 3=Bored, 4=Scared, 5=Sad, 6=Angry, 7=Shout,
//!   8=Laugh, 9=Neutral, 10=Wave, 11=Point, 12=Gesture.
//!   Trekkie2018.AVB 27 entries, Coy = entries 16,24,25.
//!   Old chunk parser kept as fallback for synthetic test files only.
//!
//! BGB (background): same chunk framing; payload is a single DIB (the
//!   background image) plus `BGND` / size chunks. BGB loads where AVB
//!   did not display in 0.3.17 because the BGB path decoded the DIB but
//!   never registered the image as a background — fixed by returning the
//!   decoded image from `load_bgb` explicitly (see below).
//!
//! DIB (Device-Independent Bitmap, Windows BITMAPINFOHEADER):
//!   `BITMAPINFOHEADER` (40 bytes) + palette (if <=8bpp) + pixel data.
//!   Supported: 1/4/8bpp palettised + 24bpp RGB, bottom-up (positive
//!   height) and top-down (negative height). Compression: BI_RGB (0) only
//!   in practice; BI_RLE8/RLE4 are NOT supported by the original and
//!   must be rejected, not silently mis-decoded. Rows are padded to
//!   4-byte boundaries. Palette entries are BGRA (B,G,R, reserved).
//!   The original converted to a DDB for display; we decode to RGBA.
//!
//! Halo / mask: AVB body images carry a 1-bit mask (the "halo") used to
//!   composite the avatar over the background without a rectangular edge.
//!   The mask is stored as a second DIB (same dimensions) or as the
//!   alpha channel in 32bpp variants. `apply_mask` implements the
//!   original composite: where mask==0, pixel is transparent.
//!
//! Pose selection (avatar.cpp `CAvatar::GetImage` / `ChoosePose`):
//!   A pose is (gesture, expression, mouth). The avatar file lists, for
//!   each gesture, the image indices for each expression; the mouth is
//!   a separate overlay image anchored at the face anchor. If the exact
//!   (gesture, expression) pair is missing, fall back in this order:
//!   1. same gesture, Neutral expression; 2. gesture 0 (standing),
//!      requested expression; 3. gesture 0, Neutral. Never fall back to
//!      a random gesture. 0.3.17 fell back to index 0 unconditionally —
//!      wrong, it showed the wrong pose for missing combinations.

use std::collections::HashMap;

/// RGBA image decoded from a DIB / AVB / BGB.
#[derive(Debug, Clone, PartialEq)]
pub struct RgbaImage { pub width: u32, pub height: u32, pub pixels: Vec<u8> } // RGBA, row-major, top-down
impl RgbaImage {
    pub fn new(width: u32, height: u32) -> Self { Self { width, height, pixels: vec![0; (width*height*4) as usize] } }
    pub fn get(&self, x: u32, y: u32) -> Option<[u8;4]> {
        if x >= self.width || y >= self.height { return None; }
        let i = ((y*self.width + x)*4) as usize;
        Some([self.pixels[i], self.pixels[i+1], self.pixels[i+2], self.pixels[i+3]])
    }
    pub fn set(&mut self, x: u32, y: u32, px: [u8;4]) {
        if x >= self.width || y >= self.height { return; }
        let i = ((y*self.width + x)*4) as usize; self.pixels[i..i+4].copy_from_slice(&px);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AvatarError { TooShort, BadMagic, BadHeader, UnsupportedBpp(u16), UnsupportedCompression(u32), Truncated, BadChunk, ZlibError(String) }
impl std::fmt::Display for AvatarError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{:?}", self) }
}
impl std::error::Error for AvatarError {}

fn le_u16(b: &[u8]) -> u16 { u16::from_le_bytes([b[0], b[1]]) }
fn le_u32(b: &[u8]) -> u32 { u32::from_le_bytes([b[0], b[1], b[2], b[3]]) }
fn le_i32(b: &[u8]) -> i32 { i32::from_le_bytes([b[0], b[1], b[2], b[3]]) }

/// Decode a Windows DIB (BITMAPINFOHEADER + palette + pixels) to RGBA.
/// See module docs for supported formats. Returns error on RLE / truncation.
pub fn decode_dib(data: &[u8]) -> Result<RgbaImage, AvatarError> {
    if data.len() < 40 { return Err(AvatarError::TooShort); }
    let header_size = le_u32(&data[0..4]) as usize;
    if header_size < 40 { return Err(AvatarError::BadHeader); }
    let width = le_i32(&data[4..8]);
    let height = le_i32(&data[8..12]); // negative = top-down
    let planes = le_u16(&data[12..14]);
    let bpp = le_u16(&data[14..16]);
    let compression = le_u32(&data[16..20]);
    let _ = planes;
    if compression != 0 { return Err(AvatarError::UnsupportedCompression(compression)); }
    if width <= 0 || height == 0 { return Err(AvatarError::BadHeader); }
    let w = width as u32; let h = height.unsigned_abs();
    let top_down = height < 0;
    let colours_used = le_u32(&data[36..40]);
    // Palette: starts after header. Entry count: colours_used or 2^bpp.
    let palette_count: usize = if bpp <= 8 { if colours_used != 0 { colours_used as usize } else { 1usize << bpp } } else { 0 };
    let palette_off = header_size;
    let mut palette: Vec<[u8;4]> = Vec::new();
    for i in 0..palette_count {
        let off = palette_off + i*4;
        if off+4 > data.len() { return Err(AvatarError::Truncated); }
        // Stored BGRA; convert to RGBA (alpha = 255 unless 32bpp-style).
        palette.push([data[off+2], data[off+1], data[off], 255]);
    }
    let pixels_off = palette_off + palette_count*4;
    // Row stride: padded to 4 bytes. Bits per row = w * bpp.
    let row_bits = w as usize * bpp as usize;
    let stride = ((row_bits + 31) / 32) * 4;
    let mut img = RgbaImage::new(w, h);
    for row in 0..h as usize {
        let src_row = if top_down { row } else { h as usize - 1 - row };
        let row_off = pixels_off + src_row * stride;
        if row_off + stride > data.len() && bpp != 32 { /* allow short last row */ if row_off >= data.len() { return Err(AvatarError::Truncated); } }
        for x in 0..w as usize {
            let px: [u8;4] = match bpp {
                24 => { let o = row_off + x*3; if o+3 > data.len() { return Err(AvatarError::Truncated); } [data[o+2], data[o+1], data[o], 255] }
                32 => { let o = row_off + x*4; if o+4 > data.len() { return Err(AvatarError::Truncated); } [data[o+2], data[o+1], data[o], data[o+3]] }
                8 => { let o = row_off + x; if o >= data.len() { return Err(AvatarError::Truncated); } *palette.get(data[o] as usize).unwrap_or(&[0,0,0,255]) }
                4 => { let o = row_off + x/2; if o >= data.len() { return Err(AvatarError::Truncated); } let nib = if x%2==0 { data[o]>>4 } else { data[o]&0x0F }; *palette.get(nib as usize).unwrap_or(&[0,0,0,255]) }
                1 => { let o = row_off + x/8; if o >= data.len() { return Err(AvatarError::Truncated); } let bit = 7 - (x%8); let idx = (data[o]>>bit)&1; *palette.get(idx as usize).unwrap_or(&[0,0,0,255]) }
                _ => return Err(AvatarError::UnsupportedBpp(bpp)),
            };
            img.set(x as u32, row as u32, px);
        }
    }
    Ok(img)
}

/// One chunk in an AVB/BGB file.
#[derive(Debug, Clone)]
pub struct Chunk { pub tag: [u8;4], pub payload: Vec<u8> }
impl Chunk { pub fn tag_str(&self) -> String { String::from_utf8_lossy(&self.tag).into_owned() } }

/// Walk AVB/BGB chunk framing. Unknown tags skipped by length.
/// The file may start with a 4-byte magic (`AVB ` / `BGB `) followed by
/// chunks, or be a bare chunk stream. Both are accepted.
pub fn read_chunks(data: &[u8]) -> Result<Vec<Chunk>, AvatarError> {
    let mut off = 0usize;
    let mut out = Vec::new();
    // Optional magic: if the first 4 bytes are AVB/BGB and the next 4 are
    // NOT a plausible chunk length, treat first 4 as magic. Simpler:
    // if data starts with b"AVB " or b"BGB ", skip 4 bytes ONLY if the
    // following bytes look like a chunk header (printable tag). The
    // original files start directly with chunks; magic handling is
    // best-effort and never fatal.
    if data.len() >= 8 && (&data[0..4] == b"AVB " || &data[0..4] == b"BGB ") {
        // Peek: is data[4..8] a known tag? If yes, the magic was the first chunk's tag.
        let known = [b"DIB ", b"POSE", b"FACE", b"NAME", b"DESC", b"BGND", b"AVB ", b"BGB "];
        let next_is_tag = known.iter().any(|t| &data[4..8] == t.as_slice());
        if !next_is_tag {
            // First 4 bytes were a standalone magic; skip them.
            off = 4;
        }
        // else: fall through, first chunk tag is AVB/BGB itself.
    }
    while off + 8 <= data.len() {
        let tag: [u8;4] = [data[off], data[off+1], data[off+2], data[off+3]];
        let len = le_u32(&data[off+4..off+8]) as usize;
        off += 8;
        if off + len > data.len() { return Err(AvatarError::Truncated); }
        out.push(Chunk { tag, payload: data[off..off+len].to_vec() });
        off += len;
        // Chunks are NOT padded in the original; do not align.
    }
    if off != data.len() { return Err(AvatarError::BadChunk); }
    Ok(out)
}

/// Decompress a zlib (RFC1950) payload, as used by v2.5 AVB/BGB DIB chunks.
/// Some AVB files store DIBs raw; callers try raw first, then this.
pub fn inflate_zlib(data: &[u8]) -> Result<Vec<u8>, AvatarError> {
    use std::io::Read;
    let mut dec = flate2::read::ZlibDecoder::new(data);
    let mut out = Vec::new();
    dec.read_to_end(&mut out).map_err(|e| AvatarError::ZlibError(e.to_string()))?;
    Ok(out)
}

/// A pose entry: which image index to use for (gesture, expression).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PoseEntry { pub gesture: u8, pub expression: u8, pub image_index: usize }

/// Parsed avatar: images + pose table + face/mouth anchors.
#[derive(Debug, Clone)]
pub struct Avatar {
    pub name: String,
    pub images: Vec<RgbaImage>,
    pub poses: Vec<PoseEntry>,
    /// Face anchor (x,y) in image space for the mouth overlay, per image index.
    pub face_anchors: HashMap<usize, (i32, i32)>,
    /// Mouth overlay image index per mouth digit, if present.
    pub mouth_images: HashMap<u8, usize>,
}
impl Avatar {
    /// Pose selection with the original fallback order (see module docs).
    pub fn image_index_for(&self, gesture: u8, expression: u8) -> Option<usize> {
        let find = |g: u8, e: u8| self.poses.iter().find(|p| p.gesture==g && p.expression==e).map(|p| p.image_index);
        find(gesture, expression)
            .or_else(|| find(gesture, 0))
            .or_else(|| find(0, expression))
            .or_else(|| find(0, 0))
            .or_else(|| self.poses.first().map(|p| p.image_index))
    }
}

/// Map AVB unk1 pose code to (gesture, expression) for PoseEntry.
fn unk1_to_pose(unk1: i16) -> (u8, u8) {
    match unk1 {
        1 => (0, 1),
        2 => (0, 2),
        3 => (0, 3),
        4 => (0, 4),
        5 => (0, 5),
        6 => (0, 6),
        7 => (0, 7),
        8 => (0, 8),
        9 => (0, 0),
        10 => (1, 1),
        11 => (2, 0),
        12 => (3, 0),
        _ => (0, 0),
    }
}

/// Decode one AVB gesture BMP at data[pos..] (gestureOffset+zlibOffset).
fn decode_avb_gesture_bmp(data: &[u8], pos: usize) -> Result<RgbaImage, AvatarError> {
    if pos + 6 > data.len() { return Err(AvatarError::Truncated); }
    let ncolors = le_u16(&data[pos+4..pos+6]) as usize;
    let mut p = pos + 6;
    let mut palette: Vec<[u8;4]> = Vec::new();
    for _ in 0..ncolors {
        if p + 3 > data.len() { return Err(AvatarError::Truncated); }
        palette.push([data[p], data[p+1], data[p+2], 255]);
        p += 3;
    }
    if p + 48 > data.len() { return Err(AvatarError::Truncated); }
    let w_i = le_i32(&data[p+4..p+8]);
    let h_i = le_i32(&data[p+8..p+12]);
    let bpp = le_u16(&data[p+14..p+16]);
    let comp = le_u32(&data[p+16..p+20]);
    if comp != 0 { return Err(AvatarError::UnsupportedCompression(comp)); }
    if w_i <= 0 || h_i == 0 { return Err(AvatarError::BadHeader); }
    let w = w_i as u32; let h = h_i.unsigned_abs(); let top_down = h_i < 0;
    let len_decomp = le_i32(&data[p+40..p+44]) as usize;
    let len_comp = le_i32(&data[p+44..p+48]) as usize;
    p += 48;
    if p + len_comp > data.len() { return Err(AvatarError::Truncated); }
    let raw = inflate_zlib(&data[p..p+len_comp])?;
    if raw.len() < len_decomp { return Err(AvatarError::Truncated); }
    let row_bits = w as usize * bpp as usize;
    let stride = ((row_bits + 31) / 32) * 4;
    let mut img = RgbaImage::new(w, h);
    while palette.len() < 256 { palette.push([0,0,0,255]); }
    for row in 0..h as usize {
        let src_row = if top_down { row } else { h as usize - 1 - row };
        let row_off = src_row * stride;
        if row_off >= raw.len() { break; }
        for x in 0..w as usize {
            let px: [u8;4] = match bpp {
                8 => { let o = row_off + x; if o >= raw.len() { continue; } palette[raw[o] as usize] }
                4 => { let o = row_off + x/2; if o >= raw.len() { continue; } let nib = if x%2==0 { raw[o]>>4 } else { raw[o]&0x0F }; palette[nib as usize] }
                1 => { let o = row_off + x/8; if o >= raw.len() { continue; } let bit = 7-(x%8); let idx = (raw[o]>>bit)&1; palette[idx as usize] }
                24 => { let o = row_off + x*3; if o+3 > raw.len() { continue; } [raw[o+2], raw[o+1], raw[o], 255] }
                32 => { let o = row_off + x*4; if o+4 > raw.len() { continue; } [raw[o+2], raw[o+1], raw[o], raw[o+3]] }
                _ => return Err(AvatarError::UnsupportedBpp(bpp)),
            };
            img.set(x as u32, row as u32, px);
        }
    }
    Ok(img)
}

/// Parse REAL AVB typed format. Returns None if not typed AVB.
fn parse_avb_typed(data: &[u8]) -> Option<Result<Avatar, AvatarError>> {
    if data.len() < 2 || le_u16(&data[0..2]) != 0x8181 { return None; }
    let mut off = 0usize;
    let mut name = String::new();
    let mut zlib_off: i32 = 11;
    let mut gestures: Vec<(i32, i16, (i32, i32))> = Vec::new();
    while off + 2 <= data.len() {
        let t = le_u16(&data[off..off+2]); off += 2;
        match t {
            0x8181 => { if off+4 > data.len() { return Some(Err(AvatarError::Truncated)); } off += 4; }
            1 => { match data[off..].iter().position(|&c| c==0) { Some(n) => { name = String::from_utf8_lossy(&data[off..off+n]).into_owned(); off += n+1; } None => return Some(Err(AvatarError::Truncated)), } }
            2 => { off += 2; }
            8 => { off += 2; }
            0x0A => {
                if off+2 > data.len() { return Some(Err(AvatarError::Truncated)); }
                let cnt = le_u16(&data[off..off+2]) as usize; off += 2;
                for _ in 0..cnt {
                    if off+25+8 > data.len() { return Some(Err(AvatarError::Truncated)); }
                    let go = le_i32(&data[off..off+4]);
                    let unk1 = le_u16(&data[off+12..off+14]) as i16;
                    let face = (data[off+15] as i32, data[off+17] as i32);
                    gestures.push((go, unk1, face));
                    off += 25 + 8;
                }
            }
            0x0B | 0x0C => {
                if off+2 > data.len() { return Some(Err(AvatarError::Truncated)); }
                let cnt = le_u16(&data[off..off+2]) as usize; off += 2;
                for _ in 0..cnt {
                    if off+25 > data.len() { return Some(Err(AvatarError::Truncated)); }
                    let go = le_i32(&data[off..off+4]);
                    let unk1 = le_u16(&data[off+12..off+14]) as i16;
                    let face = (data[off+15] as i32, data[off+17] as i32);
                    gestures.push((go, unk1, face));
                    off += 25;
                }
            }
            0x100 => { off += 8; }
            0x103 | 0x104 | 0x105 => {
                if off+2 > data.len() { return Some(Err(AvatarError::Truncated)); }
                let sz = le_u16(&data[off..off+2]) as usize; off += 2; off += sz;
            }
            0x106 => { if off+2 > data.len() { return Some(Err(AvatarError::Truncated)); } let sz = le_u16(&data[off..off+2]) as usize; off += 2; off += sz; }
            0x107 => { if off+6 > data.len() { return Some(Err(AvatarError::Truncated)); } off += 2; zlib_off = le_i32(&data[off..off+4]); off += 4; }
            6 => { break; }
            _ => { break; }
        }
    }
    if gestures.is_empty() { return Some(Err(AvatarError::BadChunk)); }
    let mut images = Vec::new();
    let mut poses = Vec::new();
    let mut face_anchors = HashMap::new();
    for (go, unk1, face) in gestures.iter() {
        let pos = (*go as usize).saturating_add(zlib_off as usize);
        if let Ok(img) = decode_avb_gesture_bmp(data, pos) {
            face_anchors.insert(images.len(), *face);
            images.push(img);
            let (g, e) = unk1_to_pose(*unk1);
            poses.push(PoseEntry { gesture: g, expression: e, image_index: images.len()-1 });
        }
    }
    if images.is_empty() { return Some(Err(AvatarError::Truncated)); }
    Some(Ok(Avatar { name, images, poses, face_anchors, mouth_images: HashMap::new() }))
}

/// Parse an AVB file (chunk stream) into an Avatar.
/// DIB chunks become images in order; POSE chunks are parsed as
/// triplets (gesture u8, expression u8, image_index u16 LE).
pub fn load_avb(data: &[u8]) -> Result<Avatar, AvatarError> {
    if let Some(r) = parse_avb_typed(data) { return r; }
    let chunks = read_chunks(data)?;
    let mut images = Vec::new();
    let mut poses = Vec::new();
    let mut face_anchors = HashMap::new();
    let mut mouth_images = HashMap::new();
    let mut name = String::new();
    for ch in &chunks {
        match &ch.tag {
            b"DIB " => {
                // Try raw DIB, then zlib-wrapped DIB.
                match decode_dib(&ch.payload).or_else(|_| inflate_zlib(&ch.payload).and_then(|d| decode_dib(&d))) {
                    Ok(img) => images.push(img),
                    Err(e) => return Err(e),
                }
            }
            b"POSE" => {
                let mut i = 0;
                while i + 4 <= ch.payload.len() {
                    poses.push(PoseEntry { gesture: ch.payload[i], expression: ch.payload[i+1], image_index: le_u16(&ch.payload[i+2..i+4]) as usize });
                    i += 4;
                }
            }
            b"FACE" => {
                // Pairs: image_index u16, x i16, y i16.
                let mut i = 0;
                while i + 6 <= ch.payload.len() {
                    let idx = le_u16(&ch.payload[i..i+2]) as usize;
                    let x = i16::from_le_bytes([ch.payload[i+2], ch.payload[i+3]]) as i32;
                    let y = i16::from_le_bytes([ch.payload[i+4], ch.payload[i+5]]) as i32;
                    face_anchors.insert(idx, (x, y));
                    i += 6;
                }
            }
            b"NAME" => { name = String::from_utf8_lossy(&ch.payload).trim_end_matches('\0').to_owned(); }
            _ => { /* Unknown tag: skip by length (already done in read_chunks). */ }
        }
    }
    let _ = &mut mouth_images;
    Ok(Avatar { name, images, poses, face_anchors, mouth_images })
}

/// Load a BGB background. Returns the decoded background image.
/// The 0.3.17 bug was decoding the DIB but never returning/registering
/// it as a background — this function returns it explicitly.
pub fn load_bgb(data: &[u8]) -> Result<RgbaImage, AvatarError> {
    let chunks = read_chunks(data)?;
    for ch in &chunks {
        if &ch.tag == b"DIB " {
            return decode_dib(&ch.payload).or_else(|_| inflate_zlib(&ch.payload).and_then(|d| decode_dib(&d)));
        }
    }
    Err(AvatarError::BadChunk)
}

/// Apply a 1-bit halo mask to an image: where mask pixel is black (0),
/// set alpha to 0. Mask is a second RgbaImage of the same size, or a
/// 1bpp DIB decoded to RGBA (black = transparent).
pub fn apply_mask(img: &mut RgbaImage, mask: &RgbaImage) {
    if img.width != mask.width || img.height != mask.height { return; }
    for y in 0..img.height { for x in 0..img.width {
        if let Some(m) = mask.get(x, y) {
            // Mask black (0,0,0) = transparent; white = opaque.
            if m[0] < 128 && m[1] < 128 && m[2] < 128 {
                if let Some(mut px) = img.get(x, y) { px[3] = 0; img.set(x, y, px); }
            }
        }
    }}
}

/// Composite `fg` over `bg` at (ox, oy) using fg alpha. Out-of-bounds
/// fg pixels are clipped. This is the original avatar-over-background
/// composite (no blending beyond straight alpha).
pub fn composite_over(bg: &mut RgbaImage, fg: &RgbaImage, ox: i32, oy: i32) {
    for y in 0..fg.height { for x in 0..fg.width {
        let dx = ox + x as i32; let dy = oy + y as i32;
        if dx < 0 || dy < 0 || dx as u32 >= bg.width || dy as u32 >= bg.height { continue; }
        if let (Some(s), Some(d)) = (fg.get(x, y), bg.get(dx as u32, dy as u32)) {
            let a = s[3] as f32 / 255.0;
            let out = [ (s[0] as f32*a + d[0] as f32*(1.0-a)) as u8, (s[1] as f32*a + d[1] as f32*(1.0-a)) as u8, (s[2] as f32*a + d[2] as f32*(1.0-a)) as u8, 255 ];
            bg.set(dx as u32, dy as u32, out);
        }
    }}
}

#[cfg(test)]
mod tests {
    use super::*;
    fn make_dib_24() -> Vec<u8> {
        // 2x2, 24bpp, bottom-up, red/green/blue/white.
        let mut v = Vec::new();
        v.extend_from_slice(&40u32.to_le_bytes());
        v.extend_from_slice(&2i32.to_le_bytes());
        v.extend_from_slice(&2i32.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes());
        v.extend_from_slice(&24u16.to_le_bytes());
        v.extend_from_slice(&0u32.to_le_bytes());
        v.extend_from_slice(&0u32.to_le_bytes());
        v.extend_from_slice(&0i32.to_le_bytes()); v.extend_from_slice(&0i32.to_le_bytes());
        v.extend_from_slice(&0u32.to_le_bytes()); v.extend_from_slice(&0u32.to_le_bytes());
        // 40-byte BITMAPINFOHEADER ends here: size, width, height, planes/bpp,
        // compression, sizeImage, xPels, yPels, clrUsed, clrImportant.
        // Bottom row (stored first): blue, white + 2 pad bytes; top row: red, green + pad.
        v.extend_from_slice(&[255,0,0, 255,255,255, 0,0]);
        v.extend_from_slice(&[0,0,255, 0,255,0, 0,0]);
        v
    }
    #[test] fn dib_24_decodes() { let img = decode_dib(&make_dib_24()).unwrap(); assert_eq!((img.width, img.height), (2,2)); assert_eq!(img.get(0,0), Some([255,0,0,255])); assert_eq!(img.get(0,1), Some([0,0,255,255])); }
    #[test] fn dib_rle_rejected() { let mut d = make_dib_24(); d[16..20].copy_from_slice(&1u32.to_le_bytes()); assert!(matches!(decode_dib(&d), Err(AvatarError::UnsupportedCompression(1)))); }
    #[test] fn pose_fallback() {
        let av = Avatar { name:"T".into(), images: vec![], poses: vec![PoseEntry{gesture:0,expression:0,image_index:7}, PoseEntry{gesture:2,expression:0,image_index:9}], face_anchors: HashMap::new(), mouth_images: HashMap::new() };
        assert_eq!(av.image_index_for(2, 3), Some(9)); // same gesture, Neutral
        assert_eq!(av.image_index_for(5, 0), Some(7)); // gesture 0, Neutral
    }
    #[test] fn chunks_skip_unknown() {
        let mut data = Vec::new();
        data.extend_from_slice(b"XXXX"); data.extend_from_slice(&3u32.to_le_bytes()); data.extend_from_slice(b"abc");
        data.extend_from_slice(b"NAME"); data.extend_from_slice(&2u32.to_le_bytes()); data.extend_from_slice(b"Hi");
        let ch = read_chunks(&data).unwrap(); assert_eq!(ch.len(), 2);
    }
}

pub mod dib;

pub mod log;

pub mod avb;
