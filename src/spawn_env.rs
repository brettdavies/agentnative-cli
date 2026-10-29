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

/// What a probe gets in place of the operator's display and desktop session
/// bus, so a GUI app run as `tool -V` cannot open a window or have the bus
/// start a service. Either would outlive the probe out of reach of its
/// process-group kill: an app that detaches with `setsid` leaves the group,
/// and the bus starts a service as its own child.
///
/// `DISPLAY`, `XAUTHORITY`, and `WAYLAND_SOCKET` (a compositor's connection
/// handed down as a file descriptor) are removed. The other two are pointed
/// at an address nothing listens on, because a client left without them
/// connects to the default socket in `$XDG_RUNTIME_DIR` instead
/// (`wayland-0`, `bus`).
const DISPLAY_ENV: &[(&str, Option<&str>)] = &[
    ("DISPLAY", None),
    ("XAUTHORITY", None),
    ("WAYLAND_SOCKET", None),
    ("WAYLAND_DISPLAY", Some("/dev/null")),
    ("DBUS_SESSION_BUS_ADDRESS", Some("unix:path=/dev/null")),
];

/// Withhold the operator's display and desktop session bus from `cmd`.
pub fn withhold_display(cmd: &mut Command) -> &mut Command {
    for (name, unreachable) in DISPLAY_ENV {
        match unreachable {
            Some(address) => cmd.env(name, address),
            None => cmd.env_remove(name),
        };
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
    fn the_display_is_withheld_not_cleared() {
        let mut cmd = Command::new("true");
        withhold_display(&mut cmd);
        let mut envs: Vec<_> = cmd
            .get_envs()
            .map(|(name, value)| {
                (
                    name.to_str().expect("utf-8 name"),
                    value.map(|value| value.to_str().expect("utf-8 value")),
                )
            })
            .collect();
        envs.sort_unstable();
        assert_eq!(
            envs,
            [
                ("DBUS_SESSION_BUS_ADDRESS", Some("unix:path=/dev/null")),
                ("DISPLAY", None),
                ("WAYLAND_DISPLAY", Some("/dev/null")),
                ("WAYLAND_SOCKET", None),
                ("XAUTHORITY", None),
            ]
        );
    }
}
