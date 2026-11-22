//! AVB / BGB parsing now lives in the comic-avatar crate (Step 2b).
//! This module only re-exports it so existing `crate::avb::...` paths
//! in src/ keep working unchanged.
pub use comic_avatar::avb::*;
