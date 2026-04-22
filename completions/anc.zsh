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
'-q[Suppress non-essential output]' \
'--quiet[Suppress non-essential output]' \
'-h[Print help]' \
'--help[Print help]' \
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
            (check)
_arguments "${_arguments_options[@]}" : \
'(--source)--command=[Resolve a command from PATH and run behavioral checks against it]:NAME:_command_names -e' \
'--principle=[Filter checks by principle number (1-7)]:PRINCIPLE:_default' \
'--output=[Output format]:OUTPUT:(text json)' \
'--audit-profile=[Exemption category for the target. Suppresses checks that do not apply to this class of tool — e.g., TUI apps legitimately intercept the TTY, so \`--audit-profile human-tui\` skips the interactive-prompt MUSTs. Suppressed checks emit \`Skip\` with structured evidence so readers see what was excluded]:CATEGORY:((human-tui\:"TUI-by-design tools (lazygit, k9s, btop). Suppresses interactive-prompt MUSTs and SIGPIPE — their contract is the TTY"
file-traversal\:"File-traversal utilities (fd, find). Reserved for subcommand-structure relaxations as those checks land"
posix-utility\:"POSIX utilities (cat, sed, awk). P1 interactive-prompt MUSTs satisfied vacuously via stdin-primary input"
diagnostic-only\:"Diagnostic tools (nvidia-smi, vmstat). No write operations, so the P5 mutation-boundary MUSTs do not apply"))' \
'--binary[Run only behavioral checks (skip source analysis)]' \
'--source[Run only source checks (skip behavioral)]' \
'--include-tests[Include test code in source analysis]' \
'-q[Suppress non-essential output]' \
'--quiet[Suppress non-essential output]' \
'-h[Print help (see more with '\''--help'\'')]' \
'--help[Print help (see more with '\''--help'\'')]' \
'::path -- Path to project directory or binary:_files' \
&& ret=0
;;
(completions)
_arguments "${_arguments_options[@]}" : \
'-q[Suppress non-essential output]' \
'--quiet[Suppress non-essential output]' \
'-h[Print help]' \
'--help[Print help]' \
':shell -- Shell to generate for:(bash elvish fish powershell zsh)' \
&& ret=0
;;
(generate)
_arguments "${_arguments_options[@]}" : \
'-q[Suppress non-essential output]' \
'--quiet[Suppress non-essential output]' \
'-h[Print help]' \
'--help[Print help]' \
":: :_anc__generate_commands" \
"*::: :->generate" \
&& ret=0

    case $state in
    (generate)
        words=($line[1] "${words[@]}")
        (( CURRENT += 1 ))
        curcontext="${curcontext%:*:*}:anc-generate-command-$line[1]:"
        case $line[1] in
            (coverage-matrix)
_arguments "${_arguments_options[@]}" : \
'--out=[Path for the Markdown artifact. Defaults to \`docs/coverage-matrix.md\`]:PATH:_files' \
'--json-out=[Path for the JSON artifact. Defaults to \`coverage/matrix.json\`]:PATH:_files' \
'--check[Exit non-zero when committed artifacts differ from generated output. CI drift guard]' \
'-q[Suppress non-essential output]' \
'--quiet[Suppress non-essential output]' \
'-h[Print help]' \
'--help[Print help]' \
&& ret=0
;;
(help)
_arguments "${_arguments_options[@]}" : \
":: :_anc__generate__help_commands" \
"*::: :->help" \
&& ret=0

    case $state in
    (help)
        words=($line[1] "${words[@]}")
        (( CURRENT += 1 ))
        curcontext="${curcontext%:*:*}:anc-generate-help-command-$line[1]:"
        case $line[1] in
            (coverage-matrix)
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
":: :_anc__help_commands" \
"*::: :->help" \
&& ret=0

    case $state in
    (help)
        words=($line[1] "${words[@]}")
        (( CURRENT += 1 ))
        curcontext="${curcontext%:*:*}:anc-help-command-$line[1]:"
        case $line[1] in
            (check)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(completions)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(generate)
_arguments "${_arguments_options[@]}" : \
":: :_anc__help__generate_commands" \
"*::: :->generate" \
&& ret=0

    case $state in
    (generate)
        words=($line[1] "${words[@]}")
        (( CURRENT += 1 ))
        curcontext="${curcontext%:*:*}:anc-help-generate-command-$line[1]:"
        case $line[1] in
            (coverage-matrix)
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
'check:Check a CLI project or binary for agent-readiness' \
'completions:Generate shell completions' \
'generate:Generate build artifacts (coverage matrix, etc.)' \
'help:Print this message or the help of the given subcommand(s)' \
    )
    _describe -t commands 'anc commands' commands "$@"
}
(( $+functions[_anc__check_commands] )) ||
_anc__check_commands() {
    local commands; commands=()
    _describe -t commands 'anc check commands' commands "$@"
}
(( $+functions[_anc__completions_commands] )) ||
_anc__completions_commands() {
    local commands; commands=()
    _describe -t commands 'anc completions commands' commands "$@"
}
(( $+functions[_anc__generate_commands] )) ||
_anc__generate_commands() {
    local commands; commands=(
'coverage-matrix:Render the spec coverage matrix (registry → checks → artifact)' \
'help:Print this message or the help of the given subcommand(s)' \
    )
    _describe -t commands 'anc generate commands' commands "$@"
}
(( $+functions[_anc__generate__coverage-matrix_commands] )) ||
_anc__generate__coverage-matrix_commands() {
    local commands; commands=()
    _describe -t commands 'anc generate coverage-matrix commands' commands "$@"
}
(( $+functions[_anc__generate__help_commands] )) ||
_anc__generate__help_commands() {
    local commands; commands=(
'coverage-matrix:Render the spec coverage matrix (registry → checks → artifact)' \
'help:Print this message or the help of the given subcommand(s)' \
    )
    _describe -t commands 'anc generate help commands' commands "$@"
}
(( $+functions[_anc__generate__help__coverage-matrix_commands] )) ||
_anc__generate__help__coverage-matrix_commands() {
    local commands; commands=()
    _describe -t commands 'anc generate help coverage-matrix commands' commands "$@"
}
(( $+functions[_anc__generate__help__help_commands] )) ||
_anc__generate__help__help_commands() {
    local commands; commands=()
    _describe -t commands 'anc generate help help commands' commands "$@"
}
(( $+functions[_anc__help_commands] )) ||
_anc__help_commands() {
    local commands; commands=(
'check:Check a CLI project or binary for agent-readiness' \
'completions:Generate shell completions' \
'generate:Generate build artifacts (coverage matrix, etc.)' \
'help:Print this message or the help of the given subcommand(s)' \
    )
    _describe -t commands 'anc help commands' commands "$@"
}
(( $+functions[_anc__help__check_commands] )) ||
_anc__help__check_commands() {
    local commands; commands=()
    _describe -t commands 'anc help check commands' commands "$@"
}
(( $+functions[_anc__help__completions_commands] )) ||
_anc__help__completions_commands() {
    local commands; commands=()
    _describe -t commands 'anc help completions commands' commands "$@"
}
(( $+functions[_anc__help__generate_commands] )) ||
_anc__help__generate_commands() {
    local commands; commands=(
'coverage-matrix:Render the spec coverage matrix (registry → checks → artifact)' \
    )
    _describe -t commands 'anc help generate commands' commands "$@"
}
(( $+functions[_anc__help__generate__coverage-matrix_commands] )) ||
_anc__help__generate__coverage-matrix_commands() {
    local commands; commands=()
    _describe -t commands 'anc help generate coverage-matrix commands' commands "$@"
}
(( $+functions[_anc__help__help_commands] )) ||
_anc__help__help_commands() {
    local commands; commands=()
    _describe -t commands 'anc help help commands' commands "$@"
}

if [ "$funcstack[1]" = "_anc" ]; then
    _anc "$@"
else
    compdef _anc anc
fi
