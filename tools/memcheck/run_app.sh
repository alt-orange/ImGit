#!/usr/bin/env bash
# Runs the ImGit app headless through the memory checks' scenarios and fails if any run reports an error or a leak.
#
#   tools/memcheck/run_app.sh asan     <imgit> [repository]   an IMGIT_SANITIZE build (the linux-asan preset)
#   tools/memcheck/run_app.sh valgrind <imgit> [repository]   any build, under Valgrind memcheck (slow: no session)
#
# The scenarios: a screenshot of the history, the File History view with blame, and (asan only) the scripted tour in
# session.txt. Without a DISPLAY each run gets its own Xvfb server. The repository defaults to the current directory.
set -euo pipefail

tool=${1:?usage: run_app.sh asan|valgrind <imgit> [repository]}
app=$(realpath "${2:?usage: run_app.sh asan|valgrind <imgit> [repository]}")
repo=${3:-.}
here=$(cd "$(dirname "$0")" && pwd)
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT

case "$tool" in
    asan)
        export ASAN_OPTIONS="detect_leaks=1:strict_string_checks=1:detect_stack_use_after_return=1:check_initialization_order=1:${ASAN_OPTIONS:-}"
        export LSAN_OPTIONS="suppressions=$here/lsan.supp:print_suppressions=0:${LSAN_OPTIONS:-}"
        export UBSAN_OPTIONS="print_stacktrace=1:${UBSAN_OPTIONS:-}"
        launcher=()
        ;;
    valgrind)
        launcher=(valgrind --error-exitcode=1 --leak-check=full --show-leak-kinds=definite,indirect
                  --errors-for-leak-kinds=definite,indirect --track-origins=yes --num-callers=40
                  --suppressions="$here/valgrind.supp")
        ;;
    *)
        echo "run_app.sh: unknown tool '$tool' (asan or valgrind)" >&2
        exit 2
        ;;
esac

run() {
    local name=$1
    shift
    echo "::group::$tool: $name"
    local status=0
    if [ -n "${DISPLAY:-}" ]; then
        "${launcher[@]}" "$app" "$repo" "$@" || status=$?
    else
        xvfb-run --auto-servernum --server-args="-screen 0 1440x900x24" "${launcher[@]}" "$app" "$repo" "$@" \
            || status=$?
    fi
    echo "::endgroup::"
    if [ "$status" -ne 0 ]; then
        echo "::error::$tool: $name failed (exit $status)"
        failed=1
    fi
}

failed=0
run "history screenshot" --screenshot "$out/history.bmp"
file=README.md
[ -f "$repo/$file" ] || file=$(git -C "$repo" ls-files | head -n 1)
run "file history with blame" --open-file "$file" --file-history --blame --screenshot "$out/file-history.bmp"
if [ "$tool" = asan ]; then
    run "scripted session" --script "$here/session.txt" --shots "$out"
fi
exit "$failed"
