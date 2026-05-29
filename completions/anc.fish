# Print an optspec for argparse to handle cmd's options that are independent of any subcommand.
function __fish_anc_global_optspecs
	string join \n q/quiet v/verbose examples json color= raw h/help V/version
end

function __fish_anc_needs_command
	# Figure out if the current invocation already has a command.
	set -l cmd (commandline -opc)
	set -e cmd[1]
	argparse -s (__fish_anc_global_optspecs) -- $cmd 2>/dev/null
	or return
	if set -q argv[1]
		# Also print the command, so this can be used to figure out what it is.
		echo $argv[1]
		return 1
	end
	return 0
end

function __fish_anc_using_subcommand
	set -l cmd (__fish_anc_needs_command)
	test -z "$cmd"
	and return 1
	contains -- $cmd[1] $argv
end

complete -c anc -n "__fish_anc_needs_command" -l color -d 'Color control for text output. `auto` (default) emits ANSI styling when stdout is a terminal and `NO_COLOR` is unset. `always` forces styling on; `never` strips it. Honors the `NO_COLOR` environment variable in `auto` mode (https://no-color.org/)' -r -f -a "auto\t''
always\t''
never\t''"
complete -c anc -n "__fish_anc_needs_command" -s q -l quiet -d 'Suppress non-essential output. Default: false (warnings and progress notes are written to stderr)'
complete -c anc -n "__fish_anc_needs_command" -s v -l verbose -d 'Escalate diagnostic detail. `-v` is shorthand for `--verbose`. Mutually exclusive with `--quiet`; the last flag on the command line wins when both appear'
complete -c anc -n "__fish_anc_needs_command" -l examples -d 'Print a curated examples block and exit. Pairs with `--output json` (or `--json`) so structured-output consumers can fetch the examples without parsing the full `--help` body'
complete -c anc -n "__fish_anc_needs_command" -l json -d 'Emit JSON output. Short alias for `--output json` on subcommands that support it. Per the agent-native convention (`p2-should-json-aliases`), the short form works alongside the canonical `--output` enum'
complete -c anc -n "__fish_anc_needs_command" -l raw -d 'Strip section headers, evidence lines, summary line, and badge hint — emit only `id<TAB>status` per audit. Pipe-safe for grep, awk, and downstream tooling that wants the raw verdict stream without prose. Ignored in `--output json` mode'
complete -c anc -n "__fish_anc_needs_command" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c anc -n "__fish_anc_needs_command" -s V -l version -d 'Print version'
complete -c anc -n "__fish_anc_needs_command" -f -a "audit" -d 'Audit a CLI project or binary for agent-readiness'
complete -c anc -n "__fish_anc_needs_command" -f -a "completions" -d 'Generate shell completions'
complete -c anc -n "__fish_anc_needs_command" -f -a "emit" -d 'Render build artifacts (coverage matrix, scorecard schema)'
complete -c anc -n "__fish_anc_needs_command" -f -a "skill" -d 'Install or manage the agentnative skill bundle'
complete -c anc -n "__fish_anc_needs_command" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c anc -n "__fish_anc_using_subcommand audit" -l command -d 'Resolve a command from PATH and run behavioral audits against it' -r -f -a "(__fish_complete_command)"
complete -c anc -n "__fish_anc_using_subcommand audit" -l principle -d 'Filter audits by principle number (1-8)' -r
complete -c anc -n "__fish_anc_using_subcommand audit" -l output -d 'Output format' -r -f -a "text\t''
json\t''"
complete -c anc -n "__fish_anc_using_subcommand audit" -l audit-profile -d 'Exemption category for the target. Suppresses audits that do not apply to this class of tool — e.g., TUI apps legitimately intercept the TTY, so `--audit-profile human-tui` skips the interactive-prompt MUSTs. Suppressed audits emit `Skip` with structured evidence so readers see what was excluded' -r -f -a "human-tui\t'TUI-by-design tools (lazygit, k9s, btop). Suppresses interactive-prompt MUSTs and SIGPIPE — their contract is the TTY'
file-traversal\t'File-traversal utilities (fd, find). Reserved for subcommand-structure relaxations as those audits land'
posix-utility\t'POSIX utilities (cat, sed, awk). P1 interactive-prompt MUSTs satisfied vacuously via stdin-primary input'
diagnostic-only\t'Diagnostic tools (nvidia-smi, vmstat). No write operations, so the P5 mutation-boundary MUSTs do not apply'"
complete -c anc -n "__fish_anc_using_subcommand audit" -l color -d 'Color control for text output. `auto` (default) emits ANSI styling when stdout is a terminal and `NO_COLOR` is unset. `always` forces styling on; `never` strips it. Honors the `NO_COLOR` environment variable in `auto` mode (https://no-color.org/)' -r -f -a "auto\t''
always\t''
never\t''"
complete -c anc -n "__fish_anc_using_subcommand audit" -l binary -d 'Run only behavioral audits (skip source analysis)'
complete -c anc -n "__fish_anc_using_subcommand audit" -l source -d 'Run only source audits (skip behavioral)'
complete -c anc -n "__fish_anc_using_subcommand audit" -l include-tests -d 'Include test code in source analysis'
complete -c anc -n "__fish_anc_using_subcommand audit" -s q -l quiet -d 'Suppress non-essential output. Default: false (warnings and progress notes are written to stderr)'
complete -c anc -n "__fish_anc_using_subcommand audit" -s v -l verbose -d 'Escalate diagnostic detail. `-v` is shorthand for `--verbose`. Mutually exclusive with `--quiet`; the last flag on the command line wins when both appear'
complete -c anc -n "__fish_anc_using_subcommand audit" -l examples -d 'Print a curated examples block and exit. Pairs with `--output json` (or `--json`) so structured-output consumers can fetch the examples without parsing the full `--help` body'
complete -c anc -n "__fish_anc_using_subcommand audit" -l json -d 'Emit JSON output. Short alias for `--output json` on subcommands that support it. Per the agent-native convention (`p2-should-json-aliases`), the short form works alongside the canonical `--output` enum'
complete -c anc -n "__fish_anc_using_subcommand audit" -l raw -d 'Strip section headers, evidence lines, summary line, and badge hint — emit only `id<TAB>status` per audit. Pipe-safe for grep, awk, and downstream tooling that wants the raw verdict stream without prose. Ignored in `--output json` mode'
complete -c anc -n "__fish_anc_using_subcommand audit" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c anc -n "__fish_anc_using_subcommand completions" -l color -d 'Color control for text output. `auto` (default) emits ANSI styling when stdout is a terminal and `NO_COLOR` is unset. `always` forces styling on; `never` strips it. Honors the `NO_COLOR` environment variable in `auto` mode (https://no-color.org/)' -r -f -a "auto\t''
always\t''
never\t''"
complete -c anc -n "__fish_anc_using_subcommand completions" -s q -l quiet -d 'Suppress non-essential output. Default: false (warnings and progress notes are written to stderr)'
complete -c anc -n "__fish_anc_using_subcommand completions" -s v -l verbose -d 'Escalate diagnostic detail. `-v` is shorthand for `--verbose`. Mutually exclusive with `--quiet`; the last flag on the command line wins when both appear'
complete -c anc -n "__fish_anc_using_subcommand completions" -l examples -d 'Print a curated examples block and exit. Pairs with `--output json` (or `--json`) so structured-output consumers can fetch the examples without parsing the full `--help` body'
complete -c anc -n "__fish_anc_using_subcommand completions" -l json -d 'Emit JSON output. Short alias for `--output json` on subcommands that support it. Per the agent-native convention (`p2-should-json-aliases`), the short form works alongside the canonical `--output` enum'
complete -c anc -n "__fish_anc_using_subcommand completions" -l raw -d 'Strip section headers, evidence lines, summary line, and badge hint — emit only `id<TAB>status` per audit. Pipe-safe for grep, awk, and downstream tooling that wants the raw verdict stream without prose. Ignored in `--output json` mode'
complete -c anc -n "__fish_anc_using_subcommand completions" -s h -l help -d 'Print help'
complete -c anc -n "__fish_anc_using_subcommand emit; and not __fish_seen_subcommand_from coverage-matrix schema help" -l color -d 'Color control for text output. `auto` (default) emits ANSI styling when stdout is a terminal and `NO_COLOR` is unset. `always` forces styling on; `never` strips it. Honors the `NO_COLOR` environment variable in `auto` mode (https://no-color.org/)' -r -f -a "auto\t''
always\t''
never\t''"
complete -c anc -n "__fish_anc_using_subcommand emit; and not __fish_seen_subcommand_from coverage-matrix schema help" -s q -l quiet -d 'Suppress non-essential output. Default: false (warnings and progress notes are written to stderr)'
complete -c anc -n "__fish_anc_using_subcommand emit; and not __fish_seen_subcommand_from coverage-matrix schema help" -s v -l verbose -d 'Escalate diagnostic detail. `-v` is shorthand for `--verbose`. Mutually exclusive with `--quiet`; the last flag on the command line wins when both appear'
complete -c anc -n "__fish_anc_using_subcommand emit; and not __fish_seen_subcommand_from coverage-matrix schema help" -l examples -d 'Print a curated examples block and exit. Pairs with `--output json` (or `--json`) so structured-output consumers can fetch the examples without parsing the full `--help` body'
complete -c anc -n "__fish_anc_using_subcommand emit; and not __fish_seen_subcommand_from coverage-matrix schema help" -l json -d 'Emit JSON output. Short alias for `--output json` on subcommands that support it. Per the agent-native convention (`p2-should-json-aliases`), the short form works alongside the canonical `--output` enum'
complete -c anc -n "__fish_anc_using_subcommand emit; and not __fish_seen_subcommand_from coverage-matrix schema help" -l raw -d 'Strip section headers, evidence lines, summary line, and badge hint — emit only `id<TAB>status` per audit. Pipe-safe for grep, awk, and downstream tooling that wants the raw verdict stream without prose. Ignored in `--output json` mode'
complete -c anc -n "__fish_anc_using_subcommand emit; and not __fish_seen_subcommand_from coverage-matrix schema help" -s h -l help -d 'Print help'
complete -c anc -n "__fish_anc_using_subcommand emit; and not __fish_seen_subcommand_from coverage-matrix schema help" -f -a "coverage-matrix" -d 'Render the spec coverage matrix (registry → audits → artifact)'
complete -c anc -n "__fish_anc_using_subcommand emit; and not __fish_seen_subcommand_from coverage-matrix schema help" -f -a "schema" -d 'Print the scorecard JSON Schema (draft 2020-12) to stdout'
complete -c anc -n "__fish_anc_using_subcommand emit; and not __fish_seen_subcommand_from coverage-matrix schema help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c anc -n "__fish_anc_using_subcommand emit; and __fish_seen_subcommand_from coverage-matrix" -l out -d 'Path for the Markdown artifact. Defaults to `docs/coverage-matrix.md`' -r -F
complete -c anc -n "__fish_anc_using_subcommand emit; and __fish_seen_subcommand_from coverage-matrix" -l json-out -d 'Path for the JSON artifact. Defaults to `coverage/matrix.json`' -r -F
complete -c anc -n "__fish_anc_using_subcommand emit; and __fish_seen_subcommand_from coverage-matrix" -l color -d 'Color control for text output. `auto` (default) emits ANSI styling when stdout is a terminal and `NO_COLOR` is unset. `always` forces styling on; `never` strips it. Honors the `NO_COLOR` environment variable in `auto` mode (https://no-color.org/)' -r -f -a "auto\t''
always\t''
never\t''"
complete -c anc -n "__fish_anc_using_subcommand emit; and __fish_seen_subcommand_from coverage-matrix" -l check -d 'Exit non-zero when committed artifacts differ from rendered output. CI drift guard'
complete -c anc -n "__fish_anc_using_subcommand emit; and __fish_seen_subcommand_from coverage-matrix" -s q -l quiet -d 'Suppress non-essential output. Default: false (warnings and progress notes are written to stderr)'
complete -c anc -n "__fish_anc_using_subcommand emit; and __fish_seen_subcommand_from coverage-matrix" -s v -l verbose -d 'Escalate diagnostic detail. `-v` is shorthand for `--verbose`. Mutually exclusive with `--quiet`; the last flag on the command line wins when both appear'
complete -c anc -n "__fish_anc_using_subcommand emit; and __fish_seen_subcommand_from coverage-matrix" -l examples -d 'Print a curated examples block and exit. Pairs with `--output json` (or `--json`) so structured-output consumers can fetch the examples without parsing the full `--help` body'
complete -c anc -n "__fish_anc_using_subcommand emit; and __fish_seen_subcommand_from coverage-matrix" -l json -d 'Emit JSON output. Short alias for `--output json` on subcommands that support it. Per the agent-native convention (`p2-should-json-aliases`), the short form works alongside the canonical `--output` enum'
complete -c anc -n "__fish_anc_using_subcommand emit; and __fish_seen_subcommand_from coverage-matrix" -l raw -d 'Strip section headers, evidence lines, summary line, and badge hint — emit only `id<TAB>status` per audit. Pipe-safe for grep, awk, and downstream tooling that wants the raw verdict stream without prose. Ignored in `--output json` mode'
complete -c anc -n "__fish_anc_using_subcommand emit; and __fish_seen_subcommand_from coverage-matrix" -s h -l help -d 'Print help'
complete -c anc -n "__fish_anc_using_subcommand emit; and __fish_seen_subcommand_from schema" -l color -d 'Color control for text output. `auto` (default) emits ANSI styling when stdout is a terminal and `NO_COLOR` is unset. `always` forces styling on; `never` strips it. Honors the `NO_COLOR` environment variable in `auto` mode (https://no-color.org/)' -r -f -a "auto\t''
always\t''
never\t''"
complete -c anc -n "__fish_anc_using_subcommand emit; and __fish_seen_subcommand_from schema" -s q -l quiet -d 'Suppress non-essential output. Default: false (warnings and progress notes are written to stderr)'
complete -c anc -n "__fish_anc_using_subcommand emit; and __fish_seen_subcommand_from schema" -s v -l verbose -d 'Escalate diagnostic detail. `-v` is shorthand for `--verbose`. Mutually exclusive with `--quiet`; the last flag on the command line wins when both appear'
complete -c anc -n "__fish_anc_using_subcommand emit; and __fish_seen_subcommand_from schema" -l examples -d 'Print a curated examples block and exit. Pairs with `--output json` (or `--json`) so structured-output consumers can fetch the examples without parsing the full `--help` body'
complete -c anc -n "__fish_anc_using_subcommand emit; and __fish_seen_subcommand_from schema" -l json -d 'Emit JSON output. Short alias for `--output json` on subcommands that support it. Per the agent-native convention (`p2-should-json-aliases`), the short form works alongside the canonical `--output` enum'
complete -c anc -n "__fish_anc_using_subcommand emit; and __fish_seen_subcommand_from schema" -l raw -d 'Strip section headers, evidence lines, summary line, and badge hint — emit only `id<TAB>status` per audit. Pipe-safe for grep, awk, and downstream tooling that wants the raw verdict stream without prose. Ignored in `--output json` mode'
complete -c anc -n "__fish_anc_using_subcommand emit; and __fish_seen_subcommand_from schema" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c anc -n "__fish_anc_using_subcommand emit; and __fish_seen_subcommand_from help" -f -a "coverage-matrix" -d 'Render the spec coverage matrix (registry → audits → artifact)'
complete -c anc -n "__fish_anc_using_subcommand emit; and __fish_seen_subcommand_from help" -f -a "schema" -d 'Print the scorecard JSON Schema (draft 2020-12) to stdout'
complete -c anc -n "__fish_anc_using_subcommand emit; and __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c anc -n "__fish_anc_using_subcommand skill; and not __fish_seen_subcommand_from install update help" -l color -d 'Color control for text output. `auto` (default) emits ANSI styling when stdout is a terminal and `NO_COLOR` is unset. `always` forces styling on; `never` strips it. Honors the `NO_COLOR` environment variable in `auto` mode (https://no-color.org/)' -r -f -a "auto\t''
always\t''
never\t''"
complete -c anc -n "__fish_anc_using_subcommand skill; and not __fish_seen_subcommand_from install update help" -s q -l quiet -d 'Suppress non-essential output. Default: false (warnings and progress notes are written to stderr)'
complete -c anc -n "__fish_anc_using_subcommand skill; and not __fish_seen_subcommand_from install update help" -s v -l verbose -d 'Escalate diagnostic detail. `-v` is shorthand for `--verbose`. Mutually exclusive with `--quiet`; the last flag on the command line wins when both appear'
complete -c anc -n "__fish_anc_using_subcommand skill; and not __fish_seen_subcommand_from install update help" -l examples -d 'Print a curated examples block and exit. Pairs with `--output json` (or `--json`) so structured-output consumers can fetch the examples without parsing the full `--help` body'
complete -c anc -n "__fish_anc_using_subcommand skill; and not __fish_seen_subcommand_from install update help" -l json -d 'Emit JSON output. Short alias for `--output json` on subcommands that support it. Per the agent-native convention (`p2-should-json-aliases`), the short form works alongside the canonical `--output` enum'
complete -c anc -n "__fish_anc_using_subcommand skill; and not __fish_seen_subcommand_from install update help" -l raw -d 'Strip section headers, evidence lines, summary line, and badge hint — emit only `id<TAB>status` per audit. Pipe-safe for grep, awk, and downstream tooling that wants the raw verdict stream without prose. Ignored in `--output json` mode'
complete -c anc -n "__fish_anc_using_subcommand skill; and not __fish_seen_subcommand_from install update help" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c anc -n "__fish_anc_using_subcommand skill; and not __fish_seen_subcommand_from install update help" -f -a "install" -d 'Install the skill bundle into a host\'s canonical skills directory'
complete -c anc -n "__fish_anc_using_subcommand skill; and not __fish_seen_subcommand_from install update help" -f -a "update" -d 'Refresh an installed skill bundle to the latest upstream revision'
complete -c anc -n "__fish_anc_using_subcommand skill; and not __fish_seen_subcommand_from install update help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c anc -n "__fish_anc_using_subcommand skill; and __fish_seen_subcommand_from install" -l output -d 'Output format for the result envelope' -r -f -a "text\t''
json\t''"
complete -c anc -n "__fish_anc_using_subcommand skill; and __fish_seen_subcommand_from install" -l color -d 'Color control for text output. `auto` (default) emits ANSI styling when stdout is a terminal and `NO_COLOR` is unset. `always` forces styling on; `never` strips it. Honors the `NO_COLOR` environment variable in `auto` mode (https://no-color.org/)' -r -f -a "auto\t''
always\t''
never\t''"
complete -c anc -n "__fish_anc_using_subcommand skill; and __fish_seen_subcommand_from install" -l all -d 'Install into every known host in one invocation'
complete -c anc -n "__fish_anc_using_subcommand skill; and __fish_seen_subcommand_from install" -l dry-run -d 'Print the resolved git command without spawning. Captures cleanly via `eval $(anc skill install --dry-run <host>)`'
complete -c anc -n "__fish_anc_using_subcommand skill; and __fish_seen_subcommand_from install" -s q -l quiet -d 'Suppress non-essential output. Default: false (warnings and progress notes are written to stderr)'
complete -c anc -n "__fish_anc_using_subcommand skill; and __fish_seen_subcommand_from install" -s v -l verbose -d 'Escalate diagnostic detail. `-v` is shorthand for `--verbose`. Mutually exclusive with `--quiet`; the last flag on the command line wins when both appear'
complete -c anc -n "__fish_anc_using_subcommand skill; and __fish_seen_subcommand_from install" -l examples -d 'Print a curated examples block and exit. Pairs with `--output json` (or `--json`) so structured-output consumers can fetch the examples without parsing the full `--help` body'
complete -c anc -n "__fish_anc_using_subcommand skill; and __fish_seen_subcommand_from install" -l json -d 'Emit JSON output. Short alias for `--output json` on subcommands that support it. Per the agent-native convention (`p2-should-json-aliases`), the short form works alongside the canonical `--output` enum'
complete -c anc -n "__fish_anc_using_subcommand skill; and __fish_seen_subcommand_from install" -l raw -d 'Strip section headers, evidence lines, summary line, and badge hint — emit only `id<TAB>status` per audit. Pipe-safe for grep, awk, and downstream tooling that wants the raw verdict stream without prose. Ignored in `--output json` mode'
complete -c anc -n "__fish_anc_using_subcommand skill; and __fish_seen_subcommand_from install" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c anc -n "__fish_anc_using_subcommand skill; and __fish_seen_subcommand_from update" -l output -d 'Output format for the result envelope' -r -f -a "text\t''
json\t''"
complete -c anc -n "__fish_anc_using_subcommand skill; and __fish_seen_subcommand_from update" -l color -d 'Color control for text output. `auto` (default) emits ANSI styling when stdout is a terminal and `NO_COLOR` is unset. `always` forces styling on; `never` strips it. Honors the `NO_COLOR` environment variable in `auto` mode (https://no-color.org/)' -r -f -a "auto\t''
always\t''
never\t''"
complete -c anc -n "__fish_anc_using_subcommand skill; and __fish_seen_subcommand_from update" -l all -d 'Refresh every known host in one invocation'
complete -c anc -n "__fish_anc_using_subcommand skill; and __fish_seen_subcommand_from update" -l dry-run -d 'Print the resolved commands without spawning'
complete -c anc -n "__fish_anc_using_subcommand skill; and __fish_seen_subcommand_from update" -s q -l quiet -d 'Suppress non-essential output. Default: false (warnings and progress notes are written to stderr)'
complete -c anc -n "__fish_anc_using_subcommand skill; and __fish_seen_subcommand_from update" -s v -l verbose -d 'Escalate diagnostic detail. `-v` is shorthand for `--verbose`. Mutually exclusive with `--quiet`; the last flag on the command line wins when both appear'
complete -c anc -n "__fish_anc_using_subcommand skill; and __fish_seen_subcommand_from update" -l examples -d 'Print a curated examples block and exit. Pairs with `--output json` (or `--json`) so structured-output consumers can fetch the examples without parsing the full `--help` body'
complete -c anc -n "__fish_anc_using_subcommand skill; and __fish_seen_subcommand_from update" -l json -d 'Emit JSON output. Short alias for `--output json` on subcommands that support it. Per the agent-native convention (`p2-should-json-aliases`), the short form works alongside the canonical `--output` enum'
complete -c anc -n "__fish_anc_using_subcommand skill; and __fish_seen_subcommand_from update" -l raw -d 'Strip section headers, evidence lines, summary line, and badge hint — emit only `id<TAB>status` per audit. Pipe-safe for grep, awk, and downstream tooling that wants the raw verdict stream without prose. Ignored in `--output json` mode'
complete -c anc -n "__fish_anc_using_subcommand skill; and __fish_seen_subcommand_from update" -s h -l help -d 'Print help'
complete -c anc -n "__fish_anc_using_subcommand skill; and __fish_seen_subcommand_from help" -f -a "install" -d 'Install the skill bundle into a host\'s canonical skills directory'
complete -c anc -n "__fish_anc_using_subcommand skill; and __fish_seen_subcommand_from help" -f -a "update" -d 'Refresh an installed skill bundle to the latest upstream revision'
complete -c anc -n "__fish_anc_using_subcommand skill; and __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c anc -n "__fish_anc_using_subcommand help; and not __fish_seen_subcommand_from audit completions emit skill help" -f -a "audit" -d 'Audit a CLI project or binary for agent-readiness'
complete -c anc -n "__fish_anc_using_subcommand help; and not __fish_seen_subcommand_from audit completions emit skill help" -f -a "completions" -d 'Generate shell completions'
complete -c anc -n "__fish_anc_using_subcommand help; and not __fish_seen_subcommand_from audit completions emit skill help" -f -a "emit" -d 'Render build artifacts (coverage matrix, scorecard schema)'
complete -c anc -n "__fish_anc_using_subcommand help; and not __fish_seen_subcommand_from audit completions emit skill help" -f -a "skill" -d 'Install or manage the agentnative skill bundle'
complete -c anc -n "__fish_anc_using_subcommand help; and not __fish_seen_subcommand_from audit completions emit skill help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c anc -n "__fish_anc_using_subcommand help; and __fish_seen_subcommand_from emit" -f -a "coverage-matrix" -d 'Render the spec coverage matrix (registry → audits → artifact)'
complete -c anc -n "__fish_anc_using_subcommand help; and __fish_seen_subcommand_from emit" -f -a "schema" -d 'Print the scorecard JSON Schema (draft 2020-12) to stdout'
complete -c anc -n "__fish_anc_using_subcommand help; and __fish_seen_subcommand_from skill" -f -a "install" -d 'Install the skill bundle into a host\'s canonical skills directory'
complete -c anc -n "__fish_anc_using_subcommand help; and __fish_seen_subcommand_from skill" -f -a "update" -d 'Refresh an installed skill bundle to the latest upstream revision'
