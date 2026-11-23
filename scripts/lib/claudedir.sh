#!/usr/bin/env bash
# Sourced, never run. Claude Code's config dir, where it keeps settings.json, skills/, plugins/
# and projects/ (#1425). Builtins only and bash 3.2 safe: install.sh's rows and the hooks use it.

# CLAUDE_CONFIG_DIR when set and not empty, else ~/.claude: Claude Code's own documented
# default (https://code.claude.com/docs/en/env-vars). Sets claude_cfg; 1 when neither names one.
claude_cfg_dir() {
    claude_cfg=${CLAUDE_CONFIG_DIR:-${HOME:+$HOME/.claude}}
    [ -n "$claude_cfg" ]
}
