//! DIB / mask decoding now lives in the comic-avatar crate (Step 2a).
//! This module only re-exports it so existing `crate::dib::...` paths
//! in src/ (avb.rs and others) keep working unchanged.
pub use comic_avatar::dib::*;
