#compdef agentnative

autoload -U is-at-least

_agentnative() {
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
":: :_agentnative_commands" \
"*::: :->agentnative" \
&& ret=0
    case $state in
    (agentnative)
        words=($line[1] "${words[@]}")
        (( CURRENT += 1 ))
        curcontext="${curcontext%:*:*}:agentnative-command-$line[1]:"
        case $line[1] in
            (check)
_arguments "${_arguments_options[@]}" : \
'--principle=[Filter checks by principle number (1-7)]:PRINCIPLE:_default' \
'--output=[Output format]:OUTPUT:(text json)' \
'--binary[Run only behavioral checks (skip source analysis)]' \
'--source[Run only source checks (skip behavioral)]' \
'--include-tests[Include test code in source analysis]' \
'-q[Suppress non-essential output]' \
'--quiet[Suppress non-essential output]' \
'-h[Print help]' \
'--help[Print help]' \
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
(help)
_arguments "${_arguments_options[@]}" : \
":: :_agentnative__help_commands" \
"*::: :->help" \
&& ret=0

    case $state in
    (help)
        words=($line[1] "${words[@]}")
        (( CURRENT += 1 ))
        curcontext="${curcontext%:*:*}:agentnative-help-command-$line[1]:"
        case $line[1] in
            (check)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(completions)
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
}

(( $+functions[_agentnative_commands] )) ||
_agentnative_commands() {
    local commands; commands=(
'check:Check a CLI project or binary for agent-readiness' \
'completions:Generate shell completions' \
'help:Print this message or the help of the given subcommand(s)' \
    )
    _describe -t commands 'agentnative commands' commands "$@"
}
(( $+functions[_agentnative__check_commands] )) ||
_agentnative__check_commands() {
    local commands; commands=()
    _describe -t commands 'agentnative check commands' commands "$@"
}
(( $+functions[_agentnative__completions_commands] )) ||
_agentnative__completions_commands() {
    local commands; commands=()
    _describe -t commands 'agentnative completions commands' commands "$@"
}
(( $+functions[_agentnative__help_commands] )) ||
_agentnative__help_commands() {
    local commands; commands=(
'check:Check a CLI project or binary for agent-readiness' \
'completions:Generate shell completions' \
'help:Print this message or the help of the given subcommand(s)' \
    )
    _describe -t commands 'agentnative help commands' commands "$@"
}
(( $+functions[_agentnative__help__check_commands] )) ||
_agentnative__help__check_commands() {
    local commands; commands=()
    _describe -t commands 'agentnative help check commands' commands "$@"
}
(( $+functions[_agentnative__help__completions_commands] )) ||
_agentnative__help__completions_commands() {
    local commands; commands=()
    _describe -t commands 'agentnative help completions commands' commands "$@"
}
(( $+functions[_agentnative__help__help_commands] )) ||
_agentnative__help__help_commands() {
    local commands; commands=()
    _describe -t commands 'agentnative help help commands' commands "$@"
}

if [ "$funcstack[1]" = "_agentnative" ]; then
    _agentnative "$@"
else
    compdef _agentnative agentnative
fi
