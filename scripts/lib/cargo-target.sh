# shellcheck shell=bash
# cargo_target_dir <manifest>: print the directory cargo builds <manifest> into.
#
# Asked of cargo, not assumed to be tui/target: CARGO_TARGET_DIR or a config's
# build.target-dir moves it, a relative one from the cwd (#546). cargo's own
# error stays on stderr. The path comes JSON-escaped: serde_json escapes " and
# \ (undone here) and control characters (not). An answer this cannot read
# falls back to CARGO_TARGET_DIR, which overrides any config. Unset, it fails:
# a guess such as tui/target may hold a stale build. No jq: it is optional here.
cargo_target_dir() {
    local m=${1:?cargo_target_dir: a manifest path is required} raw
    raw=$(cargo metadata --format-version 1 --no-deps --offline --manifest-path "$m" |
        sed -nE 's/.*"target_directory":"(([^"\\]|\\.)*)".*/\1/p')
    # \\ pairs out first: the second half of one is not the start of an escape.
    case ${raw//\\\\/} in
        *\\[bfnrtu]*) raw= ;;
    esac
    raw=$(printf '%s' "$raw" | sed -E 's/\\(["\\])/\1/g')
    raw=${raw:-${CARGO_TARGET_DIR:-}}
    [ -n "$raw" ] || {
        echo "cargo_target_dir: cannot read cargo's target_directory; set CARGO_TARGET_DIR" >&2
        return 1
    }
    printf '%s\n' "$raw"
}
