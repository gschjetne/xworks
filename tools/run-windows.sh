#!/bin/sh
# Builds the generic shell for Windows and runs it from WSL, where the real
# GPU is (CLAUDE.md, "GPU work runs on Windows"). Documents go to the
# Windows user's Documents folder; arguments go to xworks, such as a
# document to open. With -p, it builds that tool too and runs it instead,
# such as budgets, which runs the shell beside it.
#
# The programs, spec/, fonts/ and icons/ are copied to a folder on Windows'
# own disk first, as an installed xWorks would have them: loaded across
# WSL's file share, the program alone takes seconds to start.
#
#   tools/run-windows.sh [--release] [-p TOOL] [ARGS...]
set -eu
cd "$(dirname "$0")/.."
profile=debug
run=xworks
while [ $# -gt 0 ]; do
    case "$1" in
    --release) profile=release; shift ;;
    -p) run=$2; shift 2 ;;
    *) break ;;
    esac
done
# cargo-xwin cross-compiles; bundled SQLite needs LLVM's llvm-lib.
llvm=$(ls -d /usr/lib/llvm-*/bin 2>/dev/null | sort -V | tail -1)
if [ -n "$llvm" ]; then
    PATH="$llvm:$PATH"
fi
set -- -p xworks -p "$run" "$@"
if [ "$profile" = release ]; then
    cargo xwin build --target x86_64-pc-windows-msvc --release "$1" "$2" "$3" "$4"
else
    cargo xwin build --target x86_64-pc-windows-msvc "$1" "$2" "$3" "$4"
fi
shift 4
local=$(wslpath "$(cmd.exe /c "echo %LOCALAPPDATA%" 2>/dev/null | tr -d '\r')")/xworks-dev
mkdir -p "$local"
rsync -a --delete spec fonts icons "$local/"
cp "target/x86_64-pc-windows-msvc/$profile/xworks.exe" "target/x86_64-pc-windows-msvc/$profile/$run.exe" "$local/"
XWORKS_ROOT="$(wslpath -w "$local")"
export XWORKS_ROOT
WSLENV="XWORKS_ROOT${WSLENV:+:$WSLENV}"
export WSLENV
exec "$local/$run.exe" "$@"
