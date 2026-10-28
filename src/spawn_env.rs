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

/// The variables that connect a process to a display or to the desktop
/// session bus.
///
/// A probe runs without them, so a GUI app run as `tool -V` cannot open a
/// window or have the session bus start a service. Either would outlive the
/// probe out of reach of its process-group kill: an app that detaches with
/// `setsid` leaves the group, and the bus starts a service as its own child.
const DISPLAY_ENV: &[&str] = &[
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XAUTHORITY",
    "DBUS_SESSION_BUS_ADDRESS",
];

/// Remove from `cmd`'s inherited environment every variable that connects
/// a process to a display or to the desktop session bus.
pub fn withhold_display(cmd: &mut Command) -> &mut Command {
    for name in DISPLAY_ENV {
        cmd.env_remove(name);
    }
    cmd
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

    #[test]
    fn the_display_is_removed_not_cleared() {
        let mut cmd = Command::new("true");
        withhold_display(&mut cmd);
        let mut removed: Vec<_> = cmd
            .get_envs()
            .map(|(name, value)| {
                assert_eq!(value, None, "{name:?} is set, not removed");
                name.to_str().expect("utf-8 name")
            })
            .collect();
        removed.sort_unstable();
        assert_eq!(
            removed,
            [
                "DBUS_SESSION_BUS_ADDRESS",
                "DISPLAY",
                "WAYLAND_DISPLAY",
                "XAUTHORITY"
            ]
        );
    }
}
