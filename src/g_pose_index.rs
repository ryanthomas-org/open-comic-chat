//! G-as-POSE-index lookup.
//! Wire: #G<pose>10E0<emotion><variant>[R]M1
//! pose = G char - 48, emotion = E 2nd char - 48, variant = E 3rd char - 48
//! AVB gesture entry 25 bytes: 0-3 torso off, 8-11 face off, 12-13 file emotion, 14-15 X/256, 16-17 Y/256
//! Neutral exception: wire (1,0) -> file 9, wire (1,9/6/5) -> file 1

/// Parse wire code into (pose_index, emotion, variant).
pub fn parse_wire(wire: &str) -> Option<(usize, u8, u8)> {
    let g_pos = wire.find('G')?;
    let e_pos = wire.find('E')?;
    if e_pos <= g_pos { return None; }
    let g_char = wire[g_pos + 1..].chars().next()?;
    let pose = (g_char as u32).checked_sub(48)? as usize;
    let e_rest = &wire[e_pos + 1..];
    let mut chars = e_rest.chars();
    let _c0 = chars.next()?;
    let c1 = chars.next()?;
    let c2 = chars.next()?;
    let emotion = (c1 as u32).checked_sub(48)? as u8;
    let variant = (c2 as u32).checked_sub(48)? as u8;
    Some((pose, emotion, variant))
}

/// Expected file emotion for a wire (emotion, variant).
/// Neutral is the exception: (1,0) -> 9, otherwise file == wire emotion.
pub fn expected_file_emotion(emotion: u8, variant: u8) -> u8 {
    if emotion == 1 && variant == 0 { 9 } else { emotion }
}

/// Verify gesture entry file emotion matches wire, with Neutral exception.
/// file_emotion is gesture entry byte 12 (u16 low byte).
pub fn verify_gesture(file_emotion: u16, emotion: u8, variant: u8) -> bool {
    (file_emotion as u8) == expected_file_emotion(emotion, variant)
}
