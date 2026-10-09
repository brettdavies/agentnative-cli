//! The `.anc.toml [p2] json_probe` call: a read-only invocation that the
//! tool's repository or the operator declares, run exactly as written.

use std::ffi::OsString;

use crate::anc_toml::{JSON_PROBE_KEY, Sourced};
use crate::runner::{BinaryRunner, RunStatus};
use crate::types::{AuditStatus, Mitigation, Verdict};

/// Run the declared probe: Pass when it exits 0 with JSON on stdout, naming
/// the probe and the file that declared it; Fail otherwise, saying what it
/// did instead.
pub(super) fn audit_declared_probe(runner: &BinaryRunner, probe: &Sourced<Vec<String>>) -> Verdict {
    let shown = probe_invocation(runner, &probe.value);
    let cited = probe.cite(JSON_PROBE_KEY);
    match run_declared_probe(runner, &probe.value) {
        Ok(()) => Verdict {
            status: AuditStatus::Pass,
            mitigation: Some(Mitigation::Config(format!(
                "`{shown}` printed JSON; probe declared via {cited}"
            ))),
        },
        Err(why) => AuditStatus::Fail(format!(
            "`{shown}`, the probe declared via {cited}, {why}. The declared probe must exit 0 \
             and print JSON on stdout."
        ))
        .into(),
    }
}

/// Run `args` exactly as declared: no shell, with the runner's timeout,
/// closed stdin, and `NO_COLOR=1`. `Ok` when the call exits 0 and its stdout
/// parses as JSON; otherwise what it did instead.
pub(super) fn run_declared_probe(runner: &BinaryRunner, args: &[String]) -> Result<(), String> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = runner.run(&args, &[]);
    match result.status {
        RunStatus::Ok => {}
        RunStatus::Timeout => return Err("timed out".into()),
        RunStatus::Crash { signal } => return Err(format!("was killed by signal {signal}")),
        RunStatus::NotFound | RunStatus::PermissionDenied | RunStatus::Error(_) => {
            return Err("could not be run".into());
        }
    }
    match result.exit_code {
        Some(0) => {}
        Some(code) => return Err(format!("exited {code}")),
        None => return Err("exited without an exit code".into()),
    }
    let stdout = result.stdout.trim();
    if stdout.is_empty() || serde_json::from_str::<serde_json::Value>(stdout).is_err() {
        return Err("printed no JSON on stdout".into());
    }
    Ok(())
}

/// The declared call as evidence shows it: the binary's name, then the
/// arguments, quoted where a shell would need it.
pub(super) fn probe_invocation(runner: &BinaryRunner, args: &[String]) -> String {
    let argv: Vec<OsString> = runner
        .binary_stem()
        .into_iter()
        .map(OsString::from)
        .chain(args.iter().map(OsString::from))
        .collect();
    crate::argv::format_invocation(&argv)
}
