#compdef anc

autoload -U is-at-least

_anc() {
    typeset -A opt_args
    typeset -a _arguments_options
    local ret=1

    if is-at-least 5.2; then
        _arguments_options=(-s -S -C)
    else
        _arguments_options=(-s -C)
    fi

    local context curcontext="$curcontext" state line
    _arguments "${_arguments_options[@]}" : \
'--color=[Color control for text output. \`auto\` (default) emits ANSI styling when stdout is a terminal and \`NO_COLOR\` is unset. \`always\` forces styling on; \`never\` strips it. Honors the \`NO_COLOR\` environment variable in \`auto\` mode (https\://no-color.org/)]:WHEN:(auto always never)' \
'-q[Suppress non-essential output. Default\: false (warnings and progress notes are written to stderr). \`AGENTNATIVE_QUIET\` turns it on for any value but \`0\`, \`false\`, \`no\`, \`off\`, or empty]' \
'--quiet[Suppress non-essential output. Default\: false (warnings and progress notes are written to stderr). \`AGENTNATIVE_QUIET\` turns it on for any value but \`0\`, \`false\`, \`no\`, \`off\`, or empty]' \
'(-q --quiet)-v[Escalate diagnostic detail. \`-v\` is shorthand for \`--verbose\`. Mutually exclusive with \`--quiet\`; the last flag on the command line wins when both appear. \`AGENTNATIVE_VERBOSE\` turns it on for any value but \`0\`, \`false\`, \`no\`, \`off\`, or empty]' \
'(-q --quiet)--verbose[Escalate diagnostic detail. \`-v\` is shorthand for \`--verbose\`. Mutually exclusive with \`--quiet\`; the last flag on the command line wins when both appear. \`AGENTNATIVE_VERBOSE\` turns it on for any value but \`0\`, \`false\`, \`no\`, \`off\`, or empty]' \
'--examples[Print a curated examples block and exit. Pairs with \`--output json\` (or \`--json\`) so structured-output consumers can fetch the examples without parsing the full \`--help\` body]' \
'--json[Emit JSON output. Short alias for \`--output json\` on subcommands that support it. Per the agent-native convention (\`p2-should-json-aliases\`), the short form works alongside the canonical \`--output\` enum]' \
'--raw[Strip section headers, evidence lines, summary line, and badge hint — emit only \`id<TAB>status\` per audit. Pipe-safe for grep, awk, and downstream tooling that wants the raw verdict stream without prose. Ignored in \`--output json\` mode]' \
'-h[Print help (see more with '\''--help'\'')]' \
'--help[Print help (see more with '\''--help'\'')]' \
'-V[Print version]' \
'--version[Print version]' \
":: :_anc_commands" \
"*::: :->anc" \
&& ret=0
    case $state in
    (anc)
        words=($line[1] "${words[@]}")
        (( CURRENT += 1 ))
        curcontext="${curcontext%:*:*}:anc-command-$line[1]:"
        case $line[1] in
            (audit)
_arguments "${_arguments_options[@]}" : \
'(--source)--command=[Resolve a command from PATH and run behavioral audits against it]:NAME:_command_names -e' \
'(--command)--bin=[Grade this binary when the directory builds several\: a bin name, or the path anc prints for it. Unrelated to --binary, which runs only behavioral audits]:NAME:_default' \
'--repo=[Read \`.anc.toml\` from this directory instead of the target'\''s repository. For a repo you fetched for a tool installed elsewhere\: anc reads \`<PATH>/.anc.toml\` over \`~/.anc.toml\` and never fetches]:PATH:_files -/' \
'--principle=[Filter audits by principle number (1-8)]:PRINCIPLE:_default' \
'--output=[Output format]:OUTPUT:(text json)' \
'--audit-profile=[Exemption category for the target. Suppresses audits that do not apply to this class of tool — e.g., TUI apps legitimately intercept the TTY, so \`--audit-profile human-tui\` skips the interactive-prompt MUSTs. Suppressed audits emit \`Skip\` with structured evidence so readers see what was excluded]:CATEGORY:((human-tui\:"TUI-by-design tools (lazygit, k9s, btop). Suppresses interactive-prompt MUSTs and SIGPIPE — their contract is the TTY"
file-traversal\:"File-traversal utilities (fd, find). Reserved for subcommand-structure relaxations as those audits land"
posix-utility\:"POSIX utilities (cat, sed, awk). P1 interactive-prompt MUSTs satisfied vacuously via stdin-primary input"
diagnostic-only\:"Diagnostic tools (nvidia-smi, vmstat). No write operations, so the P5 mutation-boundary MUSTs do not apply"))' \
'--color=[Color control for text output. \`auto\` (default) emits ANSI styling when stdout is a terminal and \`NO_COLOR\` is unset. \`always\` forces styling on; \`never\` strips it. Honors the \`NO_COLOR\` environment variable in \`auto\` mode (https\://no-color.org/)]:WHEN:(auto always never)' \
'--binary[Run only behavioral audits (skip source analysis). To choose which binary a directory grades, use --bin]' \
'--source[Run only source audits (skip behavioral)]' \
'--include-tests[Include test code in source analysis]' \
'-q[Suppress non-essential output. Default\: false (warnings and progress notes are written to stderr). \`AGENTNATIVE_QUIET\` turns it on for any value but \`0\`, \`false\`, \`no\`, \`off\`, or empty]' \
'--quiet[Suppress non-essential output. Default\: false (warnings and progress notes are written to stderr). \`AGENTNATIVE_QUIET\` turns it on for any value but \`0\`, \`false\`, \`no\`, \`off\`, or empty]' \
'(-q --quiet)-v[Escalate diagnostic detail. \`-v\` is shorthand for \`--verbose\`. Mutually exclusive with \`--quiet\`; the last flag on the command line wins when both appear. \`AGENTNATIVE_VERBOSE\` turns it on for any value but \`0\`, \`false\`, \`no\`, \`off\`, or empty]' \
'(-q --quiet)--verbose[Escalate diagnostic detail. \`-v\` is shorthand for \`--verbose\`. Mutually exclusive with \`--quiet\`; the last flag on the command line wins when both appear. \`AGENTNATIVE_VERBOSE\` turns it on for any value but \`0\`, \`false\`, \`no\`, \`off\`, or empty]' \
'--examples[Print a curated examples block and exit. Pairs with \`--output json\` (or \`--json\`) so structured-output consumers can fetch the examples without parsing the full \`--help\` body]' \
'--json[Emit JSON output. Short alias for \`--output json\` on subcommands that support it. Per the agent-native convention (\`p2-should-json-aliases\`), the short form works alongside the canonical \`--output\` enum]' \
'--raw[Strip section headers, evidence lines, summary line, and badge hint — emit only \`id<TAB>status\` per audit. Pipe-safe for grep, awk, and downstream tooling that wants the raw verdict stream without prose. Ignored in \`--output json\` mode]' \
'-h[Print help (see more with '\''--help'\'')]' \
'--help[Print help (see more with '\''--help'\'')]' \
'::path -- Path to project directory or binary:_files' \
&& ret=0
;;
(completions)
_arguments "${_arguments_options[@]}" : \
'--color=[Color control for text output. \`auto\` (default) emits ANSI styling when stdout is a terminal and \`NO_COLOR\` is unset. \`always\` forces styling on; \`never\` strips it. Honors the \`NO_COLOR\` environment variable in \`auto\` mode (https\://no-color.org/)]:WHEN:(auto always never)' \
'-q[Suppress non-essential output. Default\: false (warnings and progress notes are written to stderr). \`AGENTNATIVE_QUIET\` turns it on for any value but \`0\`, \`false\`, \`no\`, \`off\`, or empty]' \
'--quiet[Suppress non-essential output. Default\: false (warnings and progress notes are written to stderr). \`AGENTNATIVE_QUIET\` turns it on for any value but \`0\`, \`false\`, \`no\`, \`off\`, or empty]' \
'(-q --quiet)-v[Escalate diagnostic detail. \`-v\` is shorthand for \`--verbose\`. Mutually exclusive with \`--quiet\`; the last flag on the command line wins when both appear. \`AGENTNATIVE_VERBOSE\` turns it on for any value but \`0\`, \`false\`, \`no\`, \`off\`, or empty]' \
'(-q --quiet)--verbose[Escalate diagnostic detail. \`-v\` is shorthand for \`--verbose\`. Mutually exclusive with \`--quiet\`; the last flag on the command line wins when both appear. \`AGENTNATIVE_VERBOSE\` turns it on for any value but \`0\`, \`false\`, \`no\`, \`off\`, or empty]' \
'--examples[Print a curated examples block and exit. Pairs with \`--output json\` (or \`--json\`) so structured-output consumers can fetch the examples without parsing the full \`--help\` body]' \
'--json[Emit JSON output. Short alias for \`--output json\` on subcommands that support it. Per the agent-native convention (\`p2-should-json-aliases\`), the short form works alongside the canonical \`--output\` enum]' \
'--raw[Strip section headers, evidence lines, summary line, and badge hint — emit only \`id<TAB>status\` per audit. Pipe-safe for grep, awk, and downstream tooling that wants the raw verdict stream without prose. Ignored in \`--output json\` mode]' \
'-h[Print help]' \
'--help[Print help]' \
':shell -- Shell to generate for:(bash elvish fish powershell zsh)' \
&& ret=0
;;
(emit)
_arguments "${_arguments_options[@]}" : \
'--color=[Color control for text output. \`auto\` (default) emits ANSI styling when stdout is a terminal and \`NO_COLOR\` is unset. \`always\` forces styling on; \`never\` strips it. Honors the \`NO_COLOR\` environment variable in \`auto\` mode (https\://no-color.org/)]:WHEN:(auto always never)' \
'-q[Suppress non-essential output. Default\: false (warnings and progress notes are written to stderr). \`AGENTNATIVE_QUIET\` turns it on for any value but \`0\`, \`false\`, \`no\`, \`off\`, or empty]' \
'--quiet[Suppress non-essential output. Default\: false (warnings and progress notes are written to stderr). \`AGENTNATIVE_QUIET\` turns it on for any value but \`0\`, \`false\`, \`no\`, \`off\`, or empty]' \
'(-q --quiet)-v[Escalate diagnostic detail. \`-v\` is shorthand for \`--verbose\`. Mutually exclusive with \`--quiet\`; the last flag on the command line wins when both appear. \`AGENTNATIVE_VERBOSE\` turns it on for any value but \`0\`, \`false\`, \`no\`, \`off\`, or empty]' \
'(-q --quiet)--verbose[Escalate diagnostic detail. \`-v\` is shorthand for \`--verbose\`. Mutually exclusive with \`--quiet\`; the last flag on the command line wins when both appear. \`AGENTNATIVE_VERBOSE\` turns it on for any value but \`0\`, \`false\`, \`no\`, \`off\`, or empty]' \
'--examples[Print a curated examples block and exit. Pairs with \`--output json\` (or \`--json\`) so structured-output consumers can fetch the examples without parsing the full \`--help\` body]' \
'--json[Emit JSON output. Short alias for \`--output json\` on subcommands that support it. Per the agent-native convention (\`p2-should-json-aliases\`), the short form works alongside the canonical \`--output\` enum]' \
'--raw[Strip section headers, evidence lines, summary line, and badge hint — emit only \`id<TAB>status\` per audit. Pipe-safe for grep, awk, and downstream tooling that wants the raw verdict stream without prose. Ignored in \`--output json\` mode]' \
'-h[Print help]' \
'--help[Print help]' \
":: :_anc__subcmd__emit_commands" \
"*::: :->emit" \
&& ret=0

    case $state in
    (emit)
        words=($line[1] "${words[@]}")
        (( CURRENT += 1 ))
        curcontext="${curcontext%:*:*}:anc-emit-command-$line[1]:"
        case $line[1] in
            (coverage-matrix)
_arguments "${_arguments_options[@]}" : \
'--out=[Path for the Markdown artifact. Defaults to \`docs/coverage-matrix.md\`]:PATH:_files' \
'--json-out=[Path for the JSON artifact. Defaults to \`coverage/matrix.json\`]:PATH:_files' \
'--color=[Color control for text output. \`auto\` (default) emits ANSI styling when stdout is a terminal and \`NO_COLOR\` is unset. \`always\` forces styling on; \`never\` strips it. Honors the \`NO_COLOR\` environment variable in \`auto\` mode (https\://no-color.org/)]:WHEN:(auto always never)' \
'--check[Exit non-zero when committed artifacts differ from rendered output. CI drift guard]' \
'-q[Suppress non-essential output. Default\: false (warnings and progress notes are written to stderr). \`AGENTNATIVE_QUIET\` turns it on for any value but \`0\`, \`false\`, \`no\`, \`off\`, or empty]' \
'--quiet[Suppress non-essential output. Default\: false (warnings and progress notes are written to stderr). \`AGENTNATIVE_QUIET\` turns it on for any value but \`0\`, \`false\`, \`no\`, \`off\`, or empty]' \
'(-q --quiet)-v[Escalate diagnostic detail. \`-v\` is shorthand for \`--verbose\`. Mutually exclusive with \`--quiet\`; the last flag on the command line wins when both appear. \`AGENTNATIVE_VERBOSE\` turns it on for any value but \`0\`, \`false\`, \`no\`, \`off\`, or empty]' \
'(-q --quiet)--verbose[Escalate diagnostic detail. \`-v\` is shorthand for \`--verbose\`. Mutually exclusive with \`--quiet\`; the last flag on the command line wins when both appear. \`AGENTNATIVE_VERBOSE\` turns it on for any value but \`0\`, \`false\`, \`no\`, \`off\`, or empty]' \
'--examples[Print a curated examples block and exit. Pairs with \`--output json\` (or \`--json\`) so structured-output consumers can fetch the examples without parsing the full \`--help\` body]' \
'--json[Emit JSON output. Short alias for \`--output json\` on subcommands that support it. Per the agent-native convention (\`p2-should-json-aliases\`), the short form works alongside the canonical \`--output\` enum]' \
'--raw[Strip section headers, evidence lines, summary line, and badge hint — emit only \`id<TAB>status\` per audit. Pipe-safe for grep, awk, and downstream tooling that wants the raw verdict stream without prose. Ignored in \`--output json\` mode]' \
'-h[Print help]' \
'--help[Print help]' \
&& ret=0
;;
(schema)
_arguments "${_arguments_options[@]}" : \
'--color=[Color control for text output. \`auto\` (default) emits ANSI styling when stdout is a terminal and \`NO_COLOR\` is unset. \`always\` forces styling on; \`never\` strips it. Honors the \`NO_COLOR\` environment variable in \`auto\` mode (https\://no-color.org/)]:WHEN:(auto always never)' \
'-q[Suppress non-essential output. Default\: false (warnings and progress notes are written to stderr). \`AGENTNATIVE_QUIET\` turns it on for any value but \`0\`, \`false\`, \`no\`, \`off\`, or empty]' \
'--quiet[Suppress non-essential output. Default\: false (warnings and progress notes are written to stderr). \`AGENTNATIVE_QUIET\` turns it on for any value but \`0\`, \`false\`, \`no\`, \`off\`, or empty]' \
'(-q --quiet)-v[Escalate diagnostic detail. \`-v\` is shorthand for \`--verbose\`. Mutually exclusive with \`--quiet\`; the last flag on the command line wins when both appear. \`AGENTNATIVE_VERBOSE\` turns it on for any value but \`0\`, \`false\`, \`no\`, \`off\`, or empty]' \
'(-q --quiet)--verbose[Escalate diagnostic detail. \`-v\` is shorthand for \`--verbose\`. Mutually exclusive with \`--quiet\`; the last flag on the command line wins when both appear. \`AGENTNATIVE_VERBOSE\` turns it on for any value but \`0\`, \`false\`, \`no\`, \`off\`, or empty]' \
'--examples[Print a curated examples block and exit. Pairs with \`--output json\` (or \`--json\`) so structured-output consumers can fetch the examples without parsing the full \`--help\` body]' \
'--json[Emit JSON output. Short alias for \`--output json\` on subcommands that support it. Per the agent-native convention (\`p2-should-json-aliases\`), the short form works alongside the canonical \`--output\` enum]' \
'--raw[Strip section headers, evidence lines, summary line, and badge hint — emit only \`id<TAB>status\` per audit. Pipe-safe for grep, awk, and downstream tooling that wants the raw verdict stream without prose. Ignored in \`--output json\` mode]' \
'-h[Print help (see more with '\''--help'\'')]' \
'--help[Print help (see more with '\''--help'\'')]' \
&& ret=0
;;
(help)
_arguments "${_arguments_options[@]}" : \
":: :_anc__subcmd__emit__subcmd__help_commands" \
"*::: :->help" \
&& ret=0

    case $state in
    (help)
        words=($line[1] "${words[@]}")
        (( CURRENT += 1 ))
        curcontext="${curcontext%:*:*}:anc-emit-help-command-$line[1]:"
        case $line[1] in
            (coverage-matrix)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(schema)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(help)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
        esac
    ;;
esac
;;
        esac
    ;;
esac
;;
(skill)
_arguments "${_arguments_options[@]}" : \
'--color=[Color control for text output. \`auto\` (default) emits ANSI styling when stdout is a terminal and \`NO_COLOR\` is unset. \`always\` forces styling on; \`never\` strips it. Honors the \`NO_COLOR\` environment variable in \`auto\` mode (https\://no-color.org/)]:WHEN:(auto always never)' \
'-q[Suppress non-essential output. Default\: false (warnings and progress notes are written to stderr). \`AGENTNATIVE_QUIET\` turns it on for any value but \`0\`, \`false\`, \`no\`, \`off\`, or empty]' \
'--quiet[Suppress non-essential output. Default\: false (warnings and progress notes are written to stderr). \`AGENTNATIVE_QUIET\` turns it on for any value but \`0\`, \`false\`, \`no\`, \`off\`, or empty]' \
'(-q --quiet)-v[Escalate diagnostic detail. \`-v\` is shorthand for \`--verbose\`. Mutually exclusive with \`--quiet\`; the last flag on the command line wins when both appear. \`AGENTNATIVE_VERBOSE\` turns it on for any value but \`0\`, \`false\`, \`no\`, \`off\`, or empty]' \
'(-q --quiet)--verbose[Escalate diagnostic detail. \`-v\` is shorthand for \`--verbose\`. Mutually exclusive with \`--quiet\`; the last flag on the command line wins when both appear. \`AGENTNATIVE_VERBOSE\` turns it on for any value but \`0\`, \`false\`, \`no\`, \`off\`, or empty]' \
'--examples[Print a curated examples block and exit. Pairs with \`--output json\` (or \`--json\`) so structured-output consumers can fetch the examples without parsing the full \`--help\` body]' \
'--json[Emit JSON output. Short alias for \`--output json\` on subcommands that support it. Per the agent-native convention (\`p2-should-json-aliases\`), the short form works alongside the canonical \`--output\` enum]' \
'--raw[Strip section headers, evidence lines, summary line, and badge hint — emit only \`id<TAB>status\` per audit. Pipe-safe for grep, awk, and downstream tooling that wants the raw verdict stream without prose. Ignored in \`--output json\` mode]' \
'-h[Print help (see more with '\''--help'\'')]' \
'--help[Print help (see more with '\''--help'\'')]' \
":: :_anc__subcmd__skill_commands" \
"*::: :->skill" \
&& ret=0

    case $state in
    (skill)
        words=($line[1] "${words[@]}")
        (( CURRENT += 1 ))
        curcontext="${curcontext%:*:*}:anc-skill-command-$line[1]:"
        case $line[1] in
            (install)
_arguments "${_arguments_options[@]}" : \
'--output=[Output format for the result envelope]:OUTPUT:(text json)' \
'--color=[Color control for text output. \`auto\` (default) emits ANSI styling when stdout is a terminal and \`NO_COLOR\` is unset. \`always\` forces styling on; \`never\` strips it. Honors the \`NO_COLOR\` environment variable in \`auto\` mode (https\://no-color.org/)]:WHEN:(auto always never)' \
'()--all[Install into every known host in one invocation]' \
'--dry-run[Print the resolved git command without spawning. Captures cleanly via \`eval \$(anc skill install --dry-run <host>)\`]' \
'-q[Suppress non-essential output. Default\: false (warnings and progress notes are written to stderr). \`AGENTNATIVE_QUIET\` turns it on for any value but \`0\`, \`false\`, \`no\`, \`off\`, or empty]' \
'--quiet[Suppress non-essential output. Default\: false (warnings and progress notes are written to stderr). \`AGENTNATIVE_QUIET\` turns it on for any value but \`0\`, \`false\`, \`no\`, \`off\`, or empty]' \
'(-q --quiet)-v[Escalate diagnostic detail. \`-v\` is shorthand for \`--verbose\`. Mutually exclusive with \`--quiet\`; the last flag on the command line wins when both appear. \`AGENTNATIVE_VERBOSE\` turns it on for any value but \`0\`, \`false\`, \`no\`, \`off\`, or empty]' \
'(-q --quiet)--verbose[Escalate diagnostic detail. \`-v\` is shorthand for \`--verbose\`. Mutually exclusive with \`--quiet\`; the last flag on the command line wins when both appear. \`AGENTNATIVE_VERBOSE\` turns it on for any value but \`0\`, \`false\`, \`no\`, \`off\`, or empty]' \
'--examples[Print a curated examples block and exit. Pairs with \`--output json\` (or \`--json\`) so structured-output consumers can fetch the examples without parsing the full \`--help\` body]' \
'--json[Emit JSON output. Short alias for \`--output json\` on subcommands that support it. Per the agent-native convention (\`p2-should-json-aliases\`), the short form works alongside the canonical \`--output\` enum]' \
'--raw[Strip section headers, evidence lines, summary line, and badge hint — emit only \`id<TAB>status\` per audit. Pipe-safe for grep, awk, and downstream tooling that wants the raw verdict stream without prose. Ignored in \`--output json\` mode]' \
'-h[Print help (see more with '\''--help'\'')]' \
'--help[Print help (see more with '\''--help'\'')]' \
'::host -- Target host (claude_code, codex, cursor, opencode). Required unless `--all` is set:(claude_code codex cursor factory kiro opencode)' \
&& ret=0
;;
(update)
_arguments "${_arguments_options[@]}" : \
'--output=[Output format for the result envelope]:OUTPUT:(text json)' \
'--color=[Color control for text output. \`auto\` (default) emits ANSI styling when stdout is a terminal and \`NO_COLOR\` is unset. \`always\` forces styling on; \`never\` strips it. Honors the \`NO_COLOR\` environment variable in \`auto\` mode (https\://no-color.org/)]:WHEN:(auto always never)' \
'()--all[Refresh every known host in one invocation]' \
'--dry-run[Print the resolved commands without spawning]' \
'-q[Suppress non-essential output. Default\: false (warnings and progress notes are written to stderr). \`AGENTNATIVE_QUIET\` turns it on for any value but \`0\`, \`false\`, \`no\`, \`off\`, or empty]' \
'--quiet[Suppress non-essential output. Default\: false (warnings and progress notes are written to stderr). \`AGENTNATIVE_QUIET\` turns it on for any value but \`0\`, \`false\`, \`no\`, \`off\`, or empty]' \
'(-q --quiet)-v[Escalate diagnostic detail. \`-v\` is shorthand for \`--verbose\`. Mutually exclusive with \`--quiet\`; the last flag on the command line wins when both appear. \`AGENTNATIVE_VERBOSE\` turns it on for any value but \`0\`, \`false\`, \`no\`, \`off\`, or empty]' \
'(-q --quiet)--verbose[Escalate diagnostic detail. \`-v\` is shorthand for \`--verbose\`. Mutually exclusive with \`--quiet\`; the last flag on the command line wins when both appear. \`AGENTNATIVE_VERBOSE\` turns it on for any value but \`0\`, \`false\`, \`no\`, \`off\`, or empty]' \
'--examples[Print a curated examples block and exit. Pairs with \`--output json\` (or \`--json\`) so structured-output consumers can fetch the examples without parsing the full \`--help\` body]' \
'--json[Emit JSON output. Short alias for \`--output json\` on subcommands that support it. Per the agent-native convention (\`p2-should-json-aliases\`), the short form works alongside the canonical \`--output\` enum]' \
'--raw[Strip section headers, evidence lines, summary line, and badge hint — emit only \`id<TAB>status\` per audit. Pipe-safe for grep, awk, and downstream tooling that wants the raw verdict stream without prose. Ignored in \`--output json\` mode]' \
'-h[Print help]' \
'--help[Print help]' \
'::host -- Target host. Required unless `--all` is set:(claude_code codex cursor factory kiro opencode)' \
&& ret=0
;;
(help)
_arguments "${_arguments_options[@]}" : \
":: :_anc__subcmd__skill__subcmd__help_commands" \
"*::: :->help" \
&& ret=0

    case $state in
    (help)
        words=($line[1] "${words[@]}")
        (( CURRENT += 1 ))
        curcontext="${curcontext%:*:*}:anc-skill-help-command-$line[1]:"
        case $line[1] in
            (install)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(update)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(help)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
        esac
    ;;
esac
;;
        esac
    ;;
esac
;;
(help)
_arguments "${_arguments_options[@]}" : \
":: :_anc__subcmd__help_commands" \
"*::: :->help" \
&& ret=0

    case $state in
    (help)
        words=($line[1] "${words[@]}")
        (( CURRENT += 1 ))
        curcontext="${curcontext%:*:*}:anc-help-command-$line[1]:"
        case $line[1] in
            (audit)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(completions)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(emit)
_arguments "${_arguments_options[@]}" : \
":: :_anc__subcmd__help__subcmd__emit_commands" \
"*::: :->emit" \
&& ret=0

    case $state in
    (emit)
        words=($line[1] "${words[@]}")
        (( CURRENT += 1 ))
        curcontext="${curcontext%:*:*}:anc-help-emit-command-$line[1]:"
        case $line[1] in
            (coverage-matrix)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(schema)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
        esac
    ;;
esac
;;
(skill)
_arguments "${_arguments_options[@]}" : \
":: :_anc__subcmd__help__subcmd__skill_commands" \
"*::: :->skill" \
&& ret=0

    case $state in
    (skill)
        words=($line[1] "${words[@]}")
        (( CURRENT += 1 ))
        curcontext="${curcontext%:*:*}:anc-help-skill-command-$line[1]:"
        case $line[1] in
            (install)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(update)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
        esac
    ;;
esac
;;
(help)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
        esac
    ;;
esac
;;
        esac
    ;;
esac
}

(( $+functions[_anc_commands] )) ||
_anc_commands() {
    local commands; commands=(
'audit:Audit a CLI project or binary for agent-readiness' \
'completions:Generate shell completions' \
'emit:Render build artifacts (coverage matrix, scorecard schema)' \
'skill:Install or manage the agentnative skill bundle' \
'help:Print this message or the help of the given subcommand(s)' \
    )
    _describe -t commands 'anc commands' commands "$@"
}
(( $+functions[_anc__subcmd__audit_commands] )) ||
_anc__subcmd__audit_commands() {
    local commands; commands=()
    _describe -t commands 'anc audit commands' commands "$@"
}
(( $+functions[_anc__subcmd__completions_commands] )) ||
_anc__subcmd__completions_commands() {
    local commands; commands=()
    _describe -t commands 'anc completions commands' commands "$@"
}
(( $+functions[_anc__subcmd__emit_commands] )) ||
_anc__subcmd__emit_commands() {
    local commands; commands=(
'coverage-matrix:Render the spec coverage matrix (registry → audits → artifact)' \
'schema:Print the scorecard JSON Schema (draft 2020-12) to stdout' \
'help:Print this message or the help of the given subcommand(s)' \
    )
    _describe -t commands 'anc emit commands' commands "$@"
}
(( $+functions[_anc__subcmd__emit__subcmd__coverage-matrix_commands] )) ||
_anc__subcmd__emit__subcmd__coverage-matrix_commands() {
    local commands; commands=()
    _describe -t commands 'anc emit coverage-matrix commands' commands "$@"
}
(( $+functions[_anc__subcmd__emit__subcmd__help_commands] )) ||
_anc__subcmd__emit__subcmd__help_commands() {
    local commands; commands=(
'coverage-matrix:Render the spec coverage matrix (registry → audits → artifact)' \
'schema:Print the scorecard JSON Schema (draft 2020-12) to stdout' \
'help:Print this message or the help of the given subcommand(s)' \
    )
    _describe -t commands 'anc emit help commands' commands "$@"
}
(( $+functions[_anc__subcmd__emit__subcmd__help__subcmd__coverage-matrix_commands] )) ||
_anc__subcmd__emit__subcmd__help__subcmd__coverage-matrix_commands() {
    local commands; commands=()
    _describe -t commands 'anc emit help coverage-matrix commands' commands "$@"
}
(( $+functions[_anc__subcmd__emit__subcmd__help__subcmd__help_commands] )) ||
_anc__subcmd__emit__subcmd__help__subcmd__help_commands() {
    local commands; commands=()
    _describe -t commands 'anc emit help help commands' commands "$@"
}
(( $+functions[_anc__subcmd__emit__subcmd__help__subcmd__schema_commands] )) ||
_anc__subcmd__emit__subcmd__help__subcmd__schema_commands() {
    local commands; commands=()
    _describe -t commands 'anc emit help schema commands' commands "$@"
}
(( $+functions[_anc__subcmd__emit__subcmd__schema_commands] )) ||
_anc__subcmd__emit__subcmd__schema_commands() {
    local commands; commands=()
    _describe -t commands 'anc emit schema commands' commands "$@"
}
(( $+functions[_anc__subcmd__help_commands] )) ||
_anc__subcmd__help_commands() {
    local commands; commands=(
'audit:Audit a CLI project or binary for agent-readiness' \
'completions:Generate shell completions' \
'emit:Render build artifacts (coverage matrix, scorecard schema)' \
'skill:Install or manage the agentnative skill bundle' \
'help:Print this message or the help of the given subcommand(s)' \
    )
    _describe -t commands 'anc help commands' commands "$@"
}
(( $+functions[_anc__subcmd__help__subcmd__audit_commands] )) ||
_anc__subcmd__help__subcmd__audit_commands() {
    local commands; commands=()
    _describe -t commands 'anc help audit commands' commands "$@"
}
(( $+functions[_anc__subcmd__help__subcmd__completions_commands] )) ||
_anc__subcmd__help__subcmd__completions_commands() {
    local commands; commands=()
    _describe -t commands 'anc help completions commands' commands "$@"
}
(( $+functions[_anc__subcmd__help__subcmd__emit_commands] )) ||
_anc__subcmd__help__subcmd__emit_commands() {
    local commands; commands=(
'coverage-matrix:Render the spec coverage matrix (registry → audits → artifact)' \
'schema:Print the scorecard JSON Schema (draft 2020-12) to stdout' \
    )
    _describe -t commands 'anc help emit commands' commands "$@"
}
(( $+functions[_anc__subcmd__help__subcmd__emit__subcmd__coverage-matrix_commands] )) ||
_anc__subcmd__help__subcmd__emit__subcmd__coverage-matrix_commands() {
    local commands; commands=()
    _describe -t commands 'anc help emit coverage-matrix commands' commands "$@"
}
(( $+functions[_anc__subcmd__help__subcmd__emit__subcmd__schema_commands] )) ||
_anc__subcmd__help__subcmd__emit__subcmd__schema_commands() {
    local commands; commands=()
    _describe -t commands 'anc help emit schema commands' commands "$@"
}
(( $+functions[_anc__subcmd__help__subcmd__help_commands] )) ||
_anc__subcmd__help__subcmd__help_commands() {
    local commands; commands=()
    _describe -t commands 'anc help help commands' commands "$@"
}
(( $+functions[_anc__subcmd__help__subcmd__skill_commands] )) ||
_anc__subcmd__help__subcmd__skill_commands() {
    local commands; commands=(
'install:Install the skill bundle into a host'\''s canonical skills directory' \
'update:Refresh an installed skill bundle to the latest upstream revision' \
    )
    _describe -t commands 'anc help skill commands' commands "$@"
}
(( $+functions[_anc__subcmd__help__subcmd__skill__subcmd__install_commands] )) ||
_anc__subcmd__help__subcmd__skill__subcmd__install_commands() {
    local commands; commands=()
    _describe -t commands 'anc help skill install commands' commands "$@"
}
(( $+functions[_anc__subcmd__help__subcmd__skill__subcmd__update_commands] )) ||
_anc__subcmd__help__subcmd__skill__subcmd__update_commands() {
    local commands; commands=()
    _describe -t commands 'anc help skill update commands' commands "$@"
}
(( $+functions[_anc__subcmd__skill_commands] )) ||
_anc__subcmd__skill_commands() {
    local commands; commands=(
'install:Install the skill bundle into a host'\''s canonical skills directory' \
'update:Refresh an installed skill bundle to the latest upstream revision' \
'help:Print this message or the help of the given subcommand(s)' \
    )
    _describe -t commands 'anc skill commands' commands "$@"
}
(( $+functions[_anc__subcmd__skill__subcmd__help_commands] )) ||
_anc__subcmd__skill__subcmd__help_commands() {
    local commands; commands=(
'install:Install the skill bundle into a host'\''s canonical skills directory' \
'update:Refresh an installed skill bundle to the latest upstream revision' \
'help:Print this message or the help of the given subcommand(s)' \
    )
    _describe -t commands 'anc skill help commands' commands "$@"
}
(( $+functions[_anc__subcmd__skill__subcmd__help__subcmd__help_commands] )) ||
_anc__subcmd__skill__subcmd__help__subcmd__help_commands() {
    local commands; commands=()
    _describe -t commands 'anc skill help help commands' commands "$@"
}
(( $+functions[_anc__subcmd__skill__subcmd__help__subcmd__install_commands] )) ||
_anc__subcmd__skill__subcmd__help__subcmd__install_commands() {
    local commands; commands=()
    _describe -t commands 'anc skill help install commands' commands "$@"
}
(( $+functions[_anc__subcmd__skill__subcmd__help__subcmd__update_commands] )) ||
_anc__subcmd__skill__subcmd__help__subcmd__update_commands() {
    local commands; commands=()
    _describe -t commands 'anc skill help update commands' commands "$@"
}
(( $+functions[_anc__subcmd__skill__subcmd__install_commands] )) ||
_anc__subcmd__skill__subcmd__install_commands() {
    local commands; commands=()
    _describe -t commands 'anc skill install commands' commands "$@"
}
(( $+functions[_anc__subcmd__skill__subcmd__update_commands] )) ||
_anc__subcmd__skill__subcmd__update_commands() {
    local commands; commands=()
    _describe -t commands 'anc skill update commands' commands "$@"
}

if [ "$funcstack[1]" = "_anc" ]; then
    _anc "$@"
else
    compdef _anc anc
fi
