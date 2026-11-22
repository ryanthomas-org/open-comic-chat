/// Frozen version — DO NOT BUMP without explicit user consent.
///
/// Freeze plan: stay on 0.3.17 for a long time, until user is floored by
/// improvements AND consents to an explicit recommendation to iterate versions.
/// Single source of truth for the whole stack. Cargo.toml must match.
/// `cargo test` fails if they diverge — intentional.
pub const VERSION: &str = "0.3.17";
/// Marketing / compat line, also frozen.
pub const VERSION_LINE: &str = "3.3";
/// True while frozen. A future unfreeze patch must set this false AND
/// update VERSION AND Cargo.toml together, with user consent.
pub const VERSION_FROZEN: bool = true;



pub fn is_frozen_version() -> bool { VERSION == "0.3.17" }

#[cfg(test)]
mod cleanup_0317_version_frozen {
    use super::{VERSION, VERSION_FROZEN, VERSION_LINE};
    #[test]
    fn version_is_frozen_0_3_17() {
        assert_eq!(VERSION, "0.3.17");
        assert_eq!(VERSION_LINE, "3.3");
        assert!(VERSION_FROZEN);
    }
    #[test]
    fn cargo_version_matches_code() { assert_eq!(env!("CARGO_PKG_VERSION"), VERSION); }
}
