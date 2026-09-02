---
name: cockpit-concede
description: End the turn when an active /goal cannot or must not be met — a permission you will not route around, a goal that turned out wrong, a decision only the human can make. Use instead of ending the turn again (the goal hook only sends you back) or asking a question the human is not there to answer.
allowed-tools: Bash(*review-gate*--concede*)
---

Say in one line what blocks the goal and what you tried, then run:

    "${TMUX_COCKPIT_DIR:-$HOME/.tmux/plugins/tmux-cockpit}/scripts/review-gate" --concede "<that line>"

Run it in your own pane, not through `ssh` or a nested `tmux -L`: the record is
keyed to the pane and to that pane's tmux server, and one written under another
server never fires here.

It prints one line confirming (or fails, outside a cockpit pane: then say
so and end your turn anyway). End your turn. The cockpit ends it at the next Stop, ahead of
the goal hook's block, and shows your line to the human as the stop reason.
The goal itself stays set: the human's next prompt resumes it, `/goal clear`
ends it. Concede once; do not retry the blocked action to "prove" it.
