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
complete -c anc -n "__fish_anc_needs_command" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c anc -n "__fish_anc_using_subcommand check" -l command -d 'Resolve a command from PATH and run behavioral checks against it' -r -f -a "(__fish_complete_command)"
complete -c anc -n "__fish_anc_using_subcommand check" -l principle -d 'Filter checks by principle number (1-7)' -r
complete -c anc -n "__fish_anc_using_subcommand check" -l output -d 'Output format' -r -f -a "text\t''
json\t''"
complete -c anc -n "__fish_anc_using_subcommand check" -l binary -d 'Run only behavioral checks (skip source analysis)'
complete -c anc -n "__fish_anc_using_subcommand check" -l source -d 'Run only source checks (skip behavioral)'
complete -c anc -n "__fish_anc_using_subcommand check" -l include-tests -d 'Include test code in source analysis'
complete -c anc -n "__fish_anc_using_subcommand check" -s q -l quiet -d 'Suppress non-essential output'
complete -c anc -n "__fish_anc_using_subcommand check" -s h -l help -d 'Print help'
complete -c anc -n "__fish_anc_using_subcommand completions" -s q -l quiet -d 'Suppress non-essential output'
complete -c anc -n "__fish_anc_using_subcommand completions" -s h -l help -d 'Print help'
complete -c anc -n "__fish_anc_using_subcommand help; and not __fish_seen_subcommand_from check completions help" -f -a "check" -d 'Check a CLI project or binary for agent-readiness'
complete -c anc -n "__fish_anc_using_subcommand help; and not __fish_seen_subcommand_from check completions help" -f -a "completions" -d 'Generate shell completions'
complete -c anc -n "__fish_anc_using_subcommand help; and not __fish_seen_subcommand_from check completions help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
