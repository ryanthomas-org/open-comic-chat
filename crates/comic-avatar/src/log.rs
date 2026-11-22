//! Minimal log shim for the comic-avatar crate.
//!
//! The binary's real log module (src/log.rs, writes to the debug log
//! file) cannot be seen from a library crate. dib.rs calls
//! `crate::log::{debug,info,warn}` at 8 sites; in the crate those
//! resolve here and are no-ops. Decoding behavior is unchanged.
//! If dib logging is wanted back, pass a logger callback in — do not
//! silently rewire this to eprintln (that would spam stderr per frame).
pub fn debug(_tag: &str, _msg: String) {}
pub fn info(_tag: &str, _msg: String) {}
pub fn warn(_tag: &str, _msg: String) {}
