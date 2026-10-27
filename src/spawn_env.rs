//! What a process anc starts inherits from anc's own environment.

use std::process::Command;

/// The variable `anc web` reads the audited MCP endpoint's sign-in token from.
pub const WEB_TOKEN_ENV: &str = "ANC_WEB_TOKEN";

/// Remove from `cmd`'s inherited environment every variable only anc may
/// read, so a secret exported for anc never reaches an audited binary or a
/// tool anc runs.
pub fn withhold_anc_secrets(cmd: &mut Command) -> &mut Command {
    cmd.env_remove(WEB_TOKEN_ENV)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_web_token_is_removed_not_cleared() {
        let mut cmd = Command::new("true");
        withhold_anc_secrets(&mut cmd);
        let envs: Vec<_> = cmd.get_envs().collect();
        assert_eq!(envs, vec![(std::ffi::OsStr::new(WEB_TOKEN_ENV), None)]);
    }
}
