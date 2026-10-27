#!/bin/sh
case "$1" in
  --help) echo "Usage: test [OPTIONS]

Examples:
  test --output json
  test --quiet

Options:
  -h, --help       Show help
  -V, --version    Print version
  -q, --quiet      Suppress output
  -v, --verbose    Verbose output
  --output FORMAT  Output format (text, json)
  --no-color       Disable colors [env: NO_COLOR=]"
    exit 0 ;;
  --version) echo "test 0.1.0"; exit 0 ;;
  --output|--format)
    if [ "$2" = "json" ]; then
      echo '{"status":"ok"}'
    else
      echo "text output"
    fi
    exit 0 ;;
  --this-flag-does-not-exist*) echo "error: unknown flag" >&2; exit 2 ;;
  *) echo "test tool"; exit 0 ;;
esac
