# shellcheck shell=bash
# cargo_target_dir <manifest>: print the directory cargo builds <manifest> into.
#
# Asked of cargo, not assumed to be tui/target: CARGO_TARGET_DIR or a config's
# build.target-dir moves it, a relative one from the cwd (#546). cargo's own
# error stays on stderr. The path comes JSON-escaped, and serde_json escapes "
# and \ (undone here) and control characters (not): an answer this cannot read
# falls back to ${CARGO_TARGET_DIR:-<manifest dir>/target}, the old guess, so
# no checkout is worse off than before. No jq: it is optional in this repo.
cargo_target_dir() {
    local m=${1:?cargo_target_dir: a manifest path is required} raw
    raw=$(cargo metadata --format-version 1 --no-deps --offline --manifest-path "$m" |
        sed -nE 's/.*"target_directory":"(([^"\\]|\\.)*)".*/\1/p')
    case $raw in
        *\\[bfnrtu]*) raw= ;;
    esac
    raw=$(printf '%s' "$raw" | sed -E 's/\\(["\\])/\1/g')
    printf '%s\n' "${raw:-${CARGO_TARGET_DIR:-$(dirname "$m")/target}}"
}
