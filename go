#!/bin/bash
set -e
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" >/dev/null && pwd )"
cd "${SCRIPT_DIR}"

function error {
    echo -e >&2 "\033[31m${1}\033[0m";
    exit 1;
}

function notice {
    echo -e >&2 "\033[33m${1}\033[0m";
}

function ensure_env {
    command -v git >/dev/null 2>&1 || error "Please install git"
}

function cmd_check {
  cargo fix
  cmd_fmt
  cargo clippy
  RUSTDOCFLAGS="-D warnings" cargo doc
}

function cmd_fmt {
  cargo +nightly fmt
}

function cmd_usage {
  cat <<-eof
Usage: ./go [cmd]

where [cmd] is one eof

    check   Run all things you would want to check before pushing
    fmt     Run auto-formatting
eof
}

ensure_env

command=""
if (( $# > 0 )); then
    command="${1}"
    shift
fi

case "${command}" in
    check) cmd_check "$@" ;;
    fmt) cmd_fmt "$@" ;;
    *) cmd_usage
esac
