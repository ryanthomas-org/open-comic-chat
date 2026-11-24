// Per-AVB lookup: body comes from current AVB table, not Trekkie hard-code.
use crate::pose_tables::{GEORDI_BODIES, TREKKIE2018_BODIES};

fn table_for(avb_name: &str) -> &'static [(u8, u16, u8, u8, &'static str)] {
    if avb_name.to_lowercase().contains("geordi") || avb_name.to_lowercase().contains("trekie") {
        GEORDI_BODIES
    } else {
        TREKKIE2018_BODIES
    }
}

pub fn body_for_pose(face_emo: u8, face_int_10: u8, avb_name: &str) -> u8 {
    body_for_pose_cycled(face_emo, face_int_10, avb_name, 0)
}

pub fn body_for_pose_cycled(face_emo: u8, face_int_10: u8, avb_name: &str, cycle: usize) -> u8 {
    let table = table_for(avb_name);
    // Collect all bodies with this face emotion - match on avb_emo (2nd field), not w_face_emo (4th).
    // Geordi Neutral has avb_emo 9 but w 1 (wire G010 has no E), Trekkie Neutral has avb_emo 9 w 9.
    // Matching on w collapsed Neutral (1,0) and Happy (1,9) to same body.
    let mut matches: Vec<(u8, &str)> = Vec::new();
    for (body, avb_emo, _avb_int, _w_face_emo, wire) in table {
        if *avb_emo == face_emo as u16 {
            matches.push((*body, wire));
        }
    }
    if matches.is_empty() {
        // Fallback: first body is neutral for both Trekkie (G010) and Geordi (G010).
        // Old code searched w==1, which is Happy for Trekkie, not neutral.
        return table[0].0;
    }
    if matches.len() == 1 {
        return matches[0].0;
    }
    // Multiple bodies, same emotion: pick by wire intensity closest to face_int_10.
    // Wire form is G..E..X.. where X is int char, e.g. G@10E029R -> int 9, GH10E028R -> 8.
    // Neutral bodies all have int 0 (G010E090, G410E090, G810E090, GC10E090), so intensity cannot distinguish them.
    // For Neutral, cycle through them: 0,4,8,19 for Trekkie.
    if face_emo == 9 {
        // Trekkie neutral bodies are 0,4,8,19; Geordi neutral is just body 0 (others are w==1).
        // Filter to those with int 0, cycle in table order.
        let neutrals: Vec<u8> = matches.iter().map(|(b, _)| *b).collect();
        return neutrals[cycle % neutrals.len()];
    }
    // For Coy/Happy/Laugh/etc, pick closest wire int to face_int_10
    let mut best = matches[0].0;
    let mut best_dist = 100i32;
    for (body, wire) in &matches {
        // Parse int: after 'E', second char after face_emo? Wire is GxxExx...: E + face_idx(0) + face_emo + face_int
        // Example: G@10E029R -> after E: '0','2','9' -> face_int = '9' - '0' = 9
        let wire_int = wire.chars().collect::<Vec<char>>();
        // Find 'E', then int is at E+3 (E, face_idx, face_emo, face_int)
        let mut wi = 10i32;
        if let Some(epos) = wire.find('E') {
            let chars: Vec<char> = wire.chars().collect();
            if chars.len() > epos + 3 {
                let c = chars[epos + 3];
                if c.is_ascii_digit() {
                    wi = (c as i32) - ('0' as i32);
                } else if c == ':' {
                    wi = 10;
                } else if c == ';' {
                    wi = 11;
                } else if c == '<' {
                    wi = 12;
                }
            }
        }
        let d = (wi - face_int_10 as i32).abs();
        if d < best_dist {
            best_dist = d;
            best = *body;
        }
    }
    best
}

pub fn wire_for_body(body: u8, face_emo: u8, face_int_10: u8, avb_name: &str) -> String {
    // Use the table wire verbatim, it already has correct G/E/R for this AVB.
    // Old code rebuilt it with char math and dropped R, so Coy G@10E029R became G@10E029M1.
    let table = table_for(avb_name);
    for (b, _avb_emo, _avb_int, _w, wire) in table {
        if *b == body {
            let mut w = wire.to_string();
            // Table wires are like G@10E029R or G010, without (#...) and sometimes without M1.
            // Ensure M1 suffix, preserve R.
            if !w.ends_with("M1") {
                w.push_str("M1");
            }
            // Caller in app.rs wraps with (#...), so return bare G...M1 here to keep call sites unchanged.
            // If caller already expects (#...), it wraps; do not double-wrap here.
            let _ = (face_emo, face_int_10);
            return w;
        }
    }
    // Fallback: table wire not found, use old construction but preserve R lookup from body_for_pose table
    format!("G{}{}{}E0{}{}M1", (b'0' + body) as char, '1', '0', (b'0' + face_emo) as char, (b'0' + face_int_10) as char)
}
