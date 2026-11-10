# shellcheck shell=bash
# Sourced, never run: unsets what an inherited environment would point git at
# instead of the repo it runs in, the replacement settings that move git off its
# default view of that repo, and the modes that change what a pathspec matches.
# The list is cockpit-tui's gitsafe::REDIRECT and gitsafe::PATHSPEC, which the
# board strips from every git call, so tmux-review, tmux-branch and the review
# diff read the repo the board counts (#730). tui/tests/gitsafe_env.rs pins the
# copy. GIT_CEILING_DIRECTORIES stays: it can only stop the search lower.

GIT_ENV_STRIP=(GIT_DIR GIT_WORK_TREE GIT_COMMON_DIR GIT_INDEX_FILE GIT_OBJECT_DIRECTORY
    GIT_ATTR_SOURCE GIT_REPLACE_REF_BASE GIT_NO_REPLACE_OBJECTS
    GIT_LITERAL_PATHSPECS GIT_GLOB_PATHSPECS GIT_NOGLOB_PATHSPECS GIT_ICASE_PATHSPECS)
unset "${GIT_ENV_STRIP[@]}"
