# shellcheck shell=bash
# cargo_build_exe <manifest>: build <manifest>'s cockpit-tui for release and
# print the path of the executable cargo says it wrote.
#
# Read from cargo's own build messages, never put together from a target dir:
# CARGO_TARGET_DIR or a config's build.target-dir moves the dir (#546), and a
# build.target adds <triple>/ under it, where <dir>/release may hold a stale
# native build (#591). The JSON goes to stdout and is read here; the rendered
# diagnostics go to stderr, for the caller to show or capture. serde_json
# escapes " and \ (undone here) and control characters (refused). No path, one
# this cannot read, or two (a build.target list) fails: any pick may be stale.
# No jq: it is optional here.
cargo_build_exe() {
    local m=${1:?cargo_build_exe: a manifest path is required} json exe
    json=$(cargo build --release --locked --message-format=json-render-diagnostics \
        --manifest-path "$m") || return 1
    # The bin target named cockpit-tui: not a dependency's, the lib's or an example's.
    exe=$(printf '%s\n' "$json" | sed -nE '/"reason":"compiler-artifact"/!d
        /"kind":\["bin"\]/!d
        /"name":"cockpit-tui"/!d
        s/.*"executable":"(([^"\\]|\\.)*)".*/\1/p')
    case $exe in
        '') echo "cargo_build_exe: cargo's build output names no cockpit-tui executable" >&2
            return 1 ;;
        *$'\n'*) echo "cargo_build_exe: cargo built more than one cockpit-tui:" \
                "a build.target list? Set one target" >&2
            return 1 ;;
    esac
    # \\ pairs out first: the second half of one is not the start of an escape.
    case ${exe//\\\\/} in
        *\\[bfnrtu]*) echo "cargo_build_exe: cannot read the executable path cargo printed:" \
                "$exe" >&2
            return 1 ;;
    esac
    printf '%s\n' "$exe" | sed -E 's/\\(["\\])/\1/g'
}
