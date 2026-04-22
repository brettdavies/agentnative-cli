# Print an optspec for argparse to handle cmd's options that are independent of any subcommand.
function __fish_anc_global_optspecs
	string join \n q/quiet h/help V/version
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

complete -c anc -n "__fish_anc_needs_command" -s q -l quiet -d 'Suppress non-essential output'
complete -c anc -n "__fish_anc_needs_command" -s h -l help -d 'Print help'
complete -c anc -n "__fish_anc_needs_command" -s V -l version -d 'Print version'
complete -c anc -n "__fish_anc_needs_command" -f -a "check" -d 'Check a CLI project or binary for agent-readiness'
complete -c anc -n "__fish_anc_needs_command" -f -a "completions" -d 'Generate shell completions'
complete -c anc -n "__fish_anc_needs_command" -f -a "generate" -d 'Generate build artifacts (coverage matrix, etc.)'
complete -c anc -n "__fish_anc_needs_command" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c anc -n "__fish_anc_using_subcommand check" -l command -d 'Resolve a command from PATH and run behavioral checks against it' -r -f -a "(__fish_complete_command)"
complete -c anc -n "__fish_anc_using_subcommand check" -l principle -d 'Filter checks by principle number (1-7)' -r
complete -c anc -n "__fish_anc_using_subcommand check" -l output -d 'Output format' -r -f -a "text\t''
json\t''"
complete -c anc -n "__fish_anc_using_subcommand check" -l audit-profile -d 'Exemption category for the target. Suppresses checks that do not apply to this class of tool — e.g., TUI apps legitimately intercept the TTY, so `--audit-profile human-tui` skips the interactive-prompt MUSTs. Suppressed checks emit `Skip` with structured evidence so readers see what was excluded' -r -f -a "human-tui\t'TUI-by-design tools (lazygit, k9s, btop). Suppresses interactive-prompt MUSTs and SIGPIPE — their contract is the TTY'
file-traversal\t'File-traversal utilities (fd, find). Reserved for subcommand-structure relaxations as those checks land'
posix-utility\t'POSIX utilities (cat, sed, awk). P1 interactive-prompt MUSTs satisfied vacuously via stdin-primary input'
diagnostic-only\t'Diagnostic tools (nvidia-smi, vmstat). No write operations, so the P5 mutation-boundary MUSTs do not apply'"
complete -c anc -n "__fish_anc_using_subcommand check" -l binary -d 'Run only behavioral checks (skip source analysis)'
complete -c anc -n "__fish_anc_using_subcommand check" -l source -d 'Run only source checks (skip behavioral)'
complete -c anc -n "__fish_anc_using_subcommand check" -l include-tests -d 'Include test code in source analysis'
complete -c anc -n "__fish_anc_using_subcommand check" -s q -l quiet -d 'Suppress non-essential output'
complete -c anc -n "__fish_anc_using_subcommand check" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c anc -n "__fish_anc_using_subcommand completions" -s q -l quiet -d 'Suppress non-essential output'
complete -c anc -n "__fish_anc_using_subcommand completions" -s h -l help -d 'Print help'
complete -c anc -n "__fish_anc_using_subcommand generate; and not __fish_seen_subcommand_from coverage-matrix help" -s q -l quiet -d 'Suppress non-essential output'
complete -c anc -n "__fish_anc_using_subcommand generate; and not __fish_seen_subcommand_from coverage-matrix help" -s h -l help -d 'Print help'
complete -c anc -n "__fish_anc_using_subcommand generate; and not __fish_seen_subcommand_from coverage-matrix help" -f -a "coverage-matrix" -d 'Render the spec coverage matrix (registry → checks → artifact)'
complete -c anc -n "__fish_anc_using_subcommand generate; and not __fish_seen_subcommand_from coverage-matrix help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c anc -n "__fish_anc_using_subcommand generate; and __fish_seen_subcommand_from coverage-matrix" -l out -d 'Path for the Markdown artifact. Defaults to `docs/coverage-matrix.md`' -r -F
complete -c anc -n "__fish_anc_using_subcommand generate; and __fish_seen_subcommand_from coverage-matrix" -l json-out -d 'Path for the JSON artifact. Defaults to `coverage/matrix.json`' -r -F
complete -c anc -n "__fish_anc_using_subcommand generate; and __fish_seen_subcommand_from coverage-matrix" -l check -d 'Exit non-zero when committed artifacts differ from generated output. CI drift guard'
complete -c anc -n "__fish_anc_using_subcommand generate; and __fish_seen_subcommand_from coverage-matrix" -s q -l quiet -d 'Suppress non-essential output'
complete -c anc -n "__fish_anc_using_subcommand generate; and __fish_seen_subcommand_from coverage-matrix" -s h -l help -d 'Print help'
complete -c anc -n "__fish_anc_using_subcommand generate; and __fish_seen_subcommand_from help" -f -a "coverage-matrix" -d 'Render the spec coverage matrix (registry → checks → artifact)'
complete -c anc -n "__fish_anc_using_subcommand generate; and __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c anc -n "__fish_anc_using_subcommand help; and not __fish_seen_subcommand_from check completions generate help" -f -a "check" -d 'Check a CLI project or binary for agent-readiness'
complete -c anc -n "__fish_anc_using_subcommand help; and not __fish_seen_subcommand_from check completions generate help" -f -a "completions" -d 'Generate shell completions'
complete -c anc -n "__fish_anc_using_subcommand help; and not __fish_seen_subcommand_from check completions generate help" -f -a "generate" -d 'Generate build artifacts (coverage matrix, etc.)'
complete -c anc -n "__fish_anc_using_subcommand help; and not __fish_seen_subcommand_from check completions generate help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c anc -n "__fish_anc_using_subcommand help; and __fish_seen_subcommand_from generate" -f -a "coverage-matrix" -d 'Render the spec coverage matrix (registry → checks → artifact)'
