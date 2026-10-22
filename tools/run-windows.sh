#!/bin/sh
# Builds the generic shell for Windows and runs it from WSL, where the real
# GPU is (CLAUDE.md, "GPU work runs on Windows"). Documents go to the
# Windows user's Documents folder; arguments go to xworks, such as a
# document to open.
#
#   tools/run-windows.sh [--release] [FILE.xw]
set -eu
cd "$(dirname "$0")/.."
profile=debug
if [ "${1:-}" = "--release" ]; then
    profile=release
    shift
fi
# cargo-xwin cross-compiles; bundled SQLite needs LLVM's llvm-lib.
llvm=$(ls -d /usr/lib/llvm-*/bin 2>/dev/null | sort -V | tail -1)
if [ -n "$llvm" ]; then
    PATH="$llvm:$PATH"
fi
if [ "$profile" = release ]; then
    cargo xwin build --target x86_64-pc-windows-msvc -p xworks --release
else
    cargo xwin build --target x86_64-pc-windows-msvc -p xworks
fi
XWORKS_ROOT="$(wslpath -w "$PWD")"
export XWORKS_ROOT
WSLENV="XWORKS_ROOT${WSLENV:+:$WSLENV}"
export WSLENV
exec "target/x86_64-pc-windows-msvc/$profile/xworks.exe" "$@"
