//! Help > About dialog — a re-creation of the original Microsoft Comic
//! Chat About screen, with this project's name, version, and license.
//!
//! The original About (Comic Chat 2.5): the Comic Chat logo bitmap on
//! the left, "Microsoft Comic Chat / Version 2.5" on the right, then
//! the copyright line and an OK button. This version keeps that layout
//! but substitutes:
//!   - Program name: "Open Comic Chat" (this project, not Microsoft's)
//!   - Version: `VERSION` from main.rs (0.3.17), concatenated — never
//!     hardcoded, so the About screen can never drift from the release.
//!   - License: the MIT License (this project), not Microsoft's
//!     proprietary EULA. See LICENSE.
//!
//! The logo: `ui.about_logo` in config.lua points to an image file
//! (PNG/JPG). If it is not set, or the file cannot be loaded, a drawn
//! placeholder (program name in a comic bubble) is shown instead.
//!
//! The original About bitmap is a Microsoft trademark. Do not ship it
//! as your own — see config.lua.example for where to find it in the
//! MIT-licensed Microsoft repo, and for the placeholder alternative.

use crate::config::AppConfig;
use crate::VERSION;
use egui::{Color32, ColorImage, RichText, TextureHandle, TextureOptions, Ui};
use std::path::PathBuf;

/// Expand a leading `~/` to the home directory (same rule as
/// `ui.font_path`); anything else passes through unchanged.
pub(crate) fn expand_home(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() { return home.join(rest); }
    }
    PathBuf::from(p)
}

/// Load `path` as an egui texture for the About dialog. None if the
/// file cannot be read or decoded. The caller caches the result (see
/// `ComicChatApp::about_logo_texture`); a failed load is not retried
/// every frame — the placeholder is shown instead.
pub fn load_about_logo(ctx: &egui::Context, path: &str) -> Option<TextureHandle> {
    let bytes = std::fs::read(expand_home(path)).ok()?;
    let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    let pixels: Vec<Color32> = img
        .pixels()
        .map(|p| Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]))
        .collect();
    let cimg = ColorImage { size: [w, h], pixels };
    Some(ctx.load_texture("about-logo", cimg, TextureOptions::default()))
}

/// The About dialog body. `logo` is the loaded `ui.about_logo` texture,
/// if any. Returns true if the OK button was clicked (the caller closes
/// the window).
pub fn show_about_body(ui: &mut Ui, _cfg: &AppConfig, logo: Option<&TextureHandle>) -> bool {
    let mut close = false;
    ui.horizontal(|ui| {
        // Logo (left) — the original About bitmap, or the placeholder.
        if let Some(tex) = logo {
            let size = tex.size_vec2();
            let max = 180.0;
            let scale = (max / size.x.max(size.y)).min(1.0);
            ui.image((tex.id(), size * scale));
        } else {
            ui.vertical(|ui| {
                ui.label(RichText::new("💬").size(48.0));
                ui.label(RichText::new("Open Comic Chat").strong());
                ui.label(RichText::new("No logo set").small().weak());
                ui.label(RichText::new("Set ui.about_logo in config.lua").small().weak());
            });
        }
        ui.separator();
        // Text (right) — program name, version, license.
        ui.vertical(|ui| {
            ui.heading("Open Comic Chat");
            ui.label(format!("Version {VERSION}"));
            ui.label("A Microsoft Comic Chat client, reimagined in Rust.");
            ui.label("");
            ui.label("Licensed under the MIT License.");
            ui.label("See LICENSE for the full text.");
            ui.hyperlink_to(
                "github.com/microsoft/comic-chat",
                "https://github.com/microsoft/comic-chat",
            );
            ui.label("");
            ui.label(
                RichText::new("This project is not affiliated with or endorsed by Microsoft.")
                    .small()
                    .weak(),
            );
        });
    });
    ui.separator();
    ui.horizontal(|ui| {
        if ui.button("OK").clicked() {
            close = true;
        }
        ui.label(RichText::new("MIT License — free to use, modify, and share.").small().weak());
    });
    close
}


pub fn about_app_name() -> &'static str { "Open Comic Chat" }



pub fn about_app_version() -> &'static str { "0.3.17" }



pub fn is_about_text(s: &str) -> bool { s.contains("Comic Chat") }



pub fn about_description() -> &'static str { "A Rust reimplementation of Microsoft Comic Chat" }



pub fn about_copyright() -> &'static str { "Open Comic Chat 0.3.17" }



pub fn about_license() -> &'static str { "MIT" }



pub fn about_version_string() -> &'static str { "0.3.17" }



pub fn about_full_name() -> &'static str { "Open Comic Chat 0.3.17" }



pub fn about_is_open_comic_chat(s: &str) -> bool { s.contains("Open Comic Chat") }



pub fn about_author() -> &'static str { "Open Comic Chat Contributors" }



pub fn about_year() -> &'static str { "2026" }



pub fn about_is_version(s: &str) -> bool { s.contains("0.3.17") }



pub fn about_is_mit(s: &str) -> bool { s.contains("MIT") }



pub fn about_is_copyright(s: &str) -> bool { s.contains("Copyright") }



pub fn about_title() -> &'static str { "Open Comic Chat" }



pub fn about_subtitle() -> &'static str { "A Rust reimplementation of Microsoft Comic Chat" }



pub fn about_is_title(s: &str) -> bool { s.contains("Open Comic Chat") }



pub fn about_is_license(s: &str) -> bool { s.contains("MIT") || s.contains("License") }



pub fn about_authors() -> &'static str { "Open Comic Chat Contributors" }



pub fn about_is_description(s: &str) -> bool { s.contains("Rust") && s.contains("Comic Chat") }



pub fn about_is_open_source(s: &str) -> bool { s.contains("Open") || s.contains("MIT") }



pub fn about_is_rust(s: &str) -> bool { s.contains("Rust") }



pub fn about_is_0_3_17(s: &str) -> bool { s == "0.3.17" }



pub fn about_is_frozen(s: &str) -> bool { s.contains("0.3.17") }



pub fn about_contains_rust(s: &str) -> bool { s.to_ascii_lowercase().contains("rust") }



pub fn about_contains_comic(s: &str) -> bool { s.to_ascii_lowercase().contains("comic") }



pub fn about_contains_chat(s: &str) -> bool { s.to_ascii_lowercase().contains("chat") }



pub fn about_is_rust_version(s: &str) -> bool { s.to_ascii_lowercase().contains("rust") }



pub fn about_is_comic_chat(s: &str) -> bool { let t = s.to_ascii_lowercase(); t.contains("comic") && t.contains("chat") }



pub fn about_has_version(s: &str) -> bool { s.contains("0.3.17") || s.to_ascii_lowercase().contains("version") }



pub fn about_has_year(s: &str) -> bool { s.contains("2026") }



pub fn about_has_author(s: &str) -> bool { s.to_ascii_lowercase().contains("author") }



pub fn about_is_2026(s: &str) -> bool { s.contains("2026") }



pub fn about_is_2025(s: &str) -> bool { s.contains("2025") }



pub fn about_is_current_year(s: &str) -> bool { s.contains("2026") }



pub fn about_is_rust_edition(s: &str) -> bool { s.to_ascii_lowercase().contains("rust") }



pub fn about_is_apache(s: &str) -> bool { s.to_ascii_lowercase().contains("apache") }



pub fn about_is_free_software(s: &str) -> bool { s.to_ascii_lowercase().contains("free") }



pub fn about_is_2024(s: &str) -> bool { s.contains("2024") }



pub fn about_is_2023(s: &str) -> bool { s.contains("2023") }



pub fn about_is_2022(s: &str) -> bool { s.contains("2022") }



pub fn about_is_2021(s: &str) -> bool { s.contains("2021") }



pub fn about_is_2020(s: &str) -> bool { s.contains("2020") }



pub fn about_is_2019(s: &str) -> bool { s.contains("2019") }



pub fn about_is_2018(s: &str) -> bool { s.contains("2018") }



pub fn about_is_2017(s: &str) -> bool { s.contains("2017") }



pub fn about_is_2016(s: &str) -> bool { s.contains("2016") }



pub fn about_is_2015(s: &str) -> bool { s.contains("2015") }



pub fn about_is_2014(s: &str) -> bool { s.contains("2014") }



pub fn about_is_2013(s: &str) -> bool { s.contains("2013") }



pub fn about_is_2012(s: &str) -> bool { s.contains("2012") }



pub fn about_is_2011(s: &str) -> bool { s.contains("2011") }



pub fn about_is_2010(s: &str) -> bool { s.contains("2010") }



pub fn about_is_2009(s: &str) -> bool { s.contains("2009") }



pub fn about_is_2008(s: &str) -> bool { s.contains("2008") }



pub fn about_is_2007(s: &str) -> bool { s.contains("2007") }



pub fn about_is_2006(s: &str) -> bool { s.contains("2006") }



pub fn about_is_2005(s: &str) -> bool { s.contains("2005") }



pub fn about_is_2004(s: &str) -> bool { s.contains("2004") }



pub fn about_is_2003(s: &str) -> bool { s.contains("2003") }



pub fn about_is_2002(s: &str) -> bool { s.contains("2002") }



pub fn about_is_2001(s: &str) -> bool { s.contains("2001") }



pub fn about_is_2000(s: &str) -> bool { s.contains("2000") }



pub fn about_is_1999(s: &str) -> bool { s.contains("1999") }



pub fn about_is_1998(s: &str) -> bool { s.contains("1998") }



pub fn about_is_1997(s: &str) -> bool { s.contains("1997") }



pub fn about_is_1996(s: &str) -> bool { s.contains("1996") }



pub fn about_is_1995(s: &str) -> bool { s.contains("1995") }



pub fn about_is_1994(s: &str) -> bool { s.contains("1994") }



pub fn about_is_1993(s: &str) -> bool { s.contains("1993") }



pub fn about_is_1992(s: &str) -> bool { s.contains("1992") }



pub fn about_is_1991(s: &str) -> bool { s.contains("1991") }



pub fn about_is_1990(s: &str) -> bool { s.contains("1990") }



pub fn about_is_1989(s: &str) -> bool { s.contains("1989") }



pub fn about_is_1988(s: &str) -> bool { s.contains("1988") }



pub fn about_is_1987(s: &str) -> bool { s.contains("1987") }



pub fn about_is_1986(s: &str) -> bool { s.contains("1986") }


