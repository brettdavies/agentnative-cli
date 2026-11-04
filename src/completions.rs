//! Shell completion scripts for `anc completions <shell>`.

use std::io::{self, Write};

use clap::{Command, ValueHint};
use clap_complete::{Shell, generate};

/// Writes `bin`'s completion script for `shell` to `out`.
pub fn write(shell: Shell, cmd: &mut Command, bin: &str, out: &mut dyn Write) -> io::Result<()> {
    if shell != Shell::Bash {
        generate(shell, cmd, bin, out);
        return Ok(());
    }
    let mut script = Vec::new();
    generate(shell, cmd, bin, &mut script);
    let script = String::from_utf8_lossy(&script);
    out.write_all(complete_command_names(&script, &command_name_flags(cmd)).as_bytes())
}

// The bash backend ignores `ValueHint::CommandName` and completes every
// value-taking flag with `compgen -f`; clap documents value hints for zsh and
// fish only (https://github.com/clap-rs/clap/issues/3915). Each such flag's
// `case` arm completes names from PATH instead.
fn complete_command_names(script: &str, flags: &[String]) -> String {
    let mut out = String::with_capacity(script.len());
    let mut in_arm = false;
    for line in script.split_inclusive('\n') {
        if !in_arm {
            let pattern = line.trim().strip_suffix(')');
            in_arm = flags.iter().any(|flag| pattern == Some(flag.as_str()));
        }
        if in_arm {
            out.push_str(&line.replace("compgen -f", "compgen -c"));
            in_arm = !line.contains(";;");
        } else {
            out.push_str(line);
        }
    }
    out
}

fn command_name_flags(cmd: &Command) -> Vec<String> {
    let mut flags = Vec::new();
    for arg in cmd.get_arguments() {
        if arg.get_value_hint() != ValueHint::CommandName {
            continue;
        }
        if let Some(longs) = arg.get_long_and_visible_aliases() {
            flags.extend(longs.into_iter().map(|long| format!("--{long}")));
        }
        if let Some(shorts) = arg.get_short_and_visible_aliases() {
            flags.extend(shorts.into_iter().map(|short| format!("-{short}")));
        }
    }
    for sub in cmd.get_subcommands() {
        flags.extend(command_name_flags(sub));
    }
    flags.sort();
    flags.dedup();
    flags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_only_the_command_name_arm() {
        let script = "\
            --command)\n    COMPREPLY=($(compgen -f \"${cur}\"))\n    ;;\n\
            --bin)\n    COMPREPLY=($(compgen -f \"${cur}\"))\n    ;;\n";
        let out = complete_command_names(script, &["--command".to_owned()]);
        assert_eq!(
            out,
            "\
            --command)\n    COMPREPLY=($(compgen -c \"${cur}\"))\n    ;;\n\
            --bin)\n    COMPREPLY=($(compgen -f \"${cur}\"))\n    ;;\n"
        );
    }
}
