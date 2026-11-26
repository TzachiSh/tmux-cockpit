Written by decision-ui-research for tmux-cockpit-2a [fa458f] on 2026-11-26, while master was a11b1776. It cites no file in the repo.

# Research: decision UIs and auto-answer for tmux-cockpit

Written by the decision-ui-research agent, 2026-11-26. Sources are web searches and page fetches made that day.

## 1. Decision-support UI

- **Recommend without nudging.** Don't pre-select an option, because users then miss the question or submit the wrong answer. Ordering options by likelihood "can reinforce bias" ([GOV.UK radios](https://design-system.service.gov.uk/components/radios/)). Confirmations should have no default "Yes". They should name the consequence, and the buttons should say what happens ("Delete file" / "Keep file") ([NN/g](https://www.nngroup.com/articles/confirmation-dialog/)).
- **"Why" cuts both ways.** Explanations raised acceptance of AI advice "regardless of its correctness" ([Bansal, CHI'21](https://dl.acm.org/doi/fullHtml/10.1145/3411764.3445717)). Showing the suggestion only on demand, or only after the person has decided, reduced overreliance ([Buçinca, CSCW'21](https://arxiv.org/abs/2102.09692)). "Make clear why" is among the most violated human-AI guidelines ([Amershi, CHI'19](https://dl.acm.org/doi/fullHtml/10.1145/3290605.3300233)).
- **Risk per option.** Confirm only serious or irreversible actions, otherwise people click through, and offer undo instead ([NN/g](https://www.nngroup.com/articles/confirmation-dialog/)). OpenHands tags each action LOW/MEDIUM/HIGH/UNKNOWN and asks about UNKNOWN by default ([docs](https://docs.openhands.dev/sdk/arch/security)).
- **Progressive disclosure.** Show the few key options first. More than two levels and users get lost ([NN/g](https://www.nngroup.com/articles/progressive-disclosure/)).
- **Flow of several questions.** Ask one question per page. Add a "Question 3 of 9" indicator only if research shows people need it, and only when the total is reliable ([GOV.UK](https://design-system.service.gov.uk/patterns/question-pages/)). End with a "Check answers" summary that has a Change action on each row ([GOV.UK](https://design-system.service.gov.uk/patterns/check-answers/)). Show the list of steps with the current one highlighted ([NN/g wizards](https://www.nngroup.com/articles/wizards/)).
- **Terminal examples.**
  - huh splits a form into groups (pages) and has an accessible mode ([repo](https://github.com/charmbracelet/huh)).
  - gh replaced its redrawing arrow-key menus with numbered lists, and its braille spinner with static text, because screen readers can't follow redraws ([blog](https://github.blog/engineering/user-experience/building-a-more-accessible-github-cli/), [ACCESSIBILITY.md](https://github.com/cli/cli/blob/trunk/.github/ACCESSIBILITY.md)).
  - lazygit undoes with `z` through the reflog, and the confirm names the reset target ([keys](https://github.com/jesseduffield/lazygit/blob/master/docs/keybindings/Keybindings_en.md)).
  - k9s has `--readonly`, and its `ctrl-d` delete needs a confirm ([k9s](https://k9scli.io/topics/commands/)).
  - Textual has Collapsible and Sparkline widgets ([docs](https://textual.textualize.io/widgets/collapsible/)).

## 2. Visual aids

- **When they help.** Comparing amounts: position on a shared scale (aligned bars) is read most accurately, while area and colour saturation are read worst ([Cleveland–McGill](https://www.textbookofusability.com/references/clevelandmcgill1984.html)). Trends: a word-sized sparkline adds history and reduces recency bias ([Tufte](https://www.edwardtufte.com/notebook/sparkline-theory-and-practice-edward-tufte/)).
- **When they add noise.** Exact values belong in tables, and graphs suit spatial tasks only ([Vessey](https://onlinelibrary.wiley.com/doi/10.1111/j.1540-5915.1991.tb00344.x)).
- **Terminal limits.** Braille plots show replacement characters in fonts without them ([ratatui](https://docs.rs/ratatui/latest/ratatui/symbols/marker/enum.Marker.html)). Colour must never be the only cue ([WCAG 1.4.1](https://www.w3.org/WAI/WCAG22/Understanding/use-of-color)). Honour `NO_COLOR` ([no-color.org](https://no-color.org/)). Ratatui ships Sparkline, BarChart, and Chart with braille markers ([Sparkline](https://docs.rs/ratatui/latest/ratatui/widgets/struct.Sparkline.html), [Chart](https://docs.rs/ratatui/latest/ratatui/widgets/struct.Chart.html)).

## 3. Auto-approval practice

### Claude Code

- **Rule order.** Rules run deny, then ask, then allow, and the first match wins. Deny and ask rules still apply when a hook returns allow ([permissions](https://code.claude.com/docs/en/permissions)). When several PreToolUse hooks disagree, deny beats defer, which beats ask, which beats allow ([hooks](https://code.claude.com/docs/en/hooks)).
- **A hook can answer AskUserQuestion.** A PreToolUse hook returns `permissionDecision: "allow"` plus `updatedInput`: the original `questions` and an `answers` map from question text to option label. Multi-select labels are joined with commas. "allow" alone is not enough. PermissionRequest's `decision.updatedInput` works the same way. The reason given with an allow goes only to the debug log ([hooks](https://code.claude.com/docs/en/hooks)).
- **Auto mode excludes by default:** merging a PR no human approved, force push, production deploys, and posting `/merge`-style bot commands. It pauses after 3 blocks in a row or 20 in total. Blocked actions are listed under "Recently blocked" ([modes](https://code.claude.com/docs/en/permission-modes)). Admins can disable auto and bypass modes ([permissions](https://code.claude.com/docs/en/permissions)).
- **Measured results.** The classifier blocks 0.4% of benign actions but lets 17% of real overeager ones through. Most misses accepted "approval-shaped evidence" that didn't cover the action's blast radius ([Anthropic](https://www.anthropic.com/research/claude-code-auto-mode)).

### Other agent tools

- **Cursor** offers Auto-review, Allowlist and Run Everything ([run modes](https://cursor.com/docs/agent/security/run-modes)). It matches allowlist entries by prefix, so `git` covers any git command, and calls them "not a security boundary" ([ref](https://cursor.com/docs/reference/permissions)).
- **Codex** rules decide allow, prompt or forbidden, and the strictest match wins. Each rule carries a justification and `match`/`not_match` examples that are checked at load. `codex execpolicy check` is a dry run. Chained commands are split, so allowing `git add` does not allow `git add . && rm -rf /` ([rules](https://learn.chatgpt.com/docs/agent-configuration/rules), [README](https://github.com/openai/codex/blob/main/codex-rs/execpolicy/README.md)).
- **Aider** `--yes-always` approves everything ([options](https://aider.chat/docs/config/options.html)). The safety net is a commit per edit plus `/undo` ([git](https://aider.chat/docs/git.html)).
- **Copilot agent** can't approve or merge its own PR, and the person who asked for the PR can't approve it either. Workflows wait for a human to approve them. Its commits are signed and link to the session log ([risks](https://docs.github.com/en/copilot/concepts/security-governance-and-network-settings/risks-and-mitigations)).
- **OpenHands** `ConfirmRisky` asks at or above a set risk level ([SDK guide](https://docs.openhands.dev/sdk/guides/security)).

### CI and ops

- **GitHub auto-merge** runs only after required reviews and checks pass. It turns itself off if a non-writer pushes or the base branch changes ([docs](https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/incorporating-changes-from-a-pull-request/automatically-merging-a-pull-request)). An approval can be dismissed when the diff or merge base changes ([protected branches](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches)).
- **Renovate** automerges per rule (for example minor and patch updates only, never 0.x), only when green and up to date, one PR per run. It waits out a release cooldown, has a `dryRun` mode, and states "Automerge: Enabled" in the PR body ([automerge](https://docs.renovatebot.com/key-concepts/automerge/), [age](https://docs.renovatebot.com/key-concepts/minimum-release-age/), [dryRun](https://docs.renovatebot.com/self-hosted-configuration/)).
- **Dependabot** auto-merge is a workflow that checks the update type, then runs `gh pr merge --auto` ([docs](https://docs.github.com/en/code-security/tutorials/secure-your-dependencies/automate-dependabot-with-actions)).
- **Kubernetes** policies take Deny, Warn or Audit actions ([VAP](https://kubernetes.io/docs/reference/access-authn-authz/validating-admission-policy/)). Gatekeeper adds a `dryrun` action ([violations](https://open-policy-agent.github.io/gatekeeper/website/docs/v3.21.x/violations/)) and periodic audits of existing objects ([audit](https://open-policy-agent.github.io/gatekeeper/website/docs/audit/)).

## 4. Failure modes

- **Rubber-stamping.** Users accept 93–97% of prompts ([research](https://www.anthropic.com/research/claude-code-auto-mode), [article](https://claude.com/resources/articles/auto-mode-default-in-claude-code)). Training doesn't fix automation bias, and complacency grows when people watch several tasks at once ([Parasuraman–Manzey](https://pubmed.ncbi.nlm.nih.gov/21077562/)). A board of several agent panes is exactly that setting.
- **Rules growing in scope.** Quoting, subshells, base64 and scripts all slipped past Cursor's denylist, and Cursor dropped the feature ([Backslash](https://www.backslash.security/blog/cursor-ai-security-flaw-autorun-denylist)). Claude Code drops broad interpreter allow rules in auto mode and calls Bash patterns that constrain arguments fragile ([modes](https://code.claude.com/docs/en/permission-modes), [permissions](https://code.claude.com/docs/en/permissions)). Codex says to review the rule prefixes it suggests before accepting them ([rules](https://learn.chatgpt.com/docs/agent-configuration/rules)).
- **Stale context.** A limit stated in chat can be lost to compaction; Claude Code recommends a deny rule instead ([modes](https://code.claude.com/docs/en/permission-modes)). GitHub dismisses approvals when the diff or merge base changes ([protected branches](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches)). A changed Kubernetes policy does not recheck objects that already exist ([tutorial](https://kubernetes.io/docs/tutorials/cluster-management/admission-policies/)).
- **Prompt injection.** Cut one of the three legs: private data, untrusted content, outbound channels ([Willison](https://simonwillison.net/2025/Aug/9/bay-area-ai/)). Claude's classifier never sees tool output ([Anthropic](https://www.anthropic.com/research/claude-code-auto-mode)). Copilot strips hidden HTML comments from issue text before the agent reads it ([risks](https://docs.github.com/en/copilot/concepts/security-governance-and-network-settings/risks-and-mitigations)). OWASP adds human approval for high-impact actions, logging and rate limits ([LLM06](https://github.com/OWASP/www-project-top-10-for-large-language-model-applications/blob/main/2_0_vulns/LLM06_ExcessiveAgency.md)).

**Not covered by the docs:** whether an answer supplied by a hook shows up in Claude Code's own UI or transcript.

## Recommendations

1. **Answer through the documented route.** Use a PreToolUse hook that returns allow plus `answers`, never keystrokes into the pane. Keep a cockpit log of every auto answer: rule, question, answer, evidence.
2. **Test facts, not question text.** A rule checks verified facts: the reviewer's verdict at head sha X, and CI green at X on a fresh merge with master. If the head or merge base moves, the answer is void.
3. **Exclude outward and irreversible options by default.** Claude's own classifier blocks a merge no human approved, so the "merge?" rule should name the owner's standing ruling.
4. **Shadow mode before going live.** A new rule first shows "would answer B" while the human still answers. Each rule carries match and not-match examples checked at load.
5. **Panel layout:**
   - nothing pre-selected
   - the recommendation shown as a text tag with a one-line reason
   - a reversibility tag on each option
   - details folded away
   - for irreversible questions, the human picks before seeing the recommendation
6. **Flow.** Show "Q2/4" (AskUserQuestion asks 1–4 questions) and a check-answers summary with change keys before submitting.
7. **Visual aid only for quantities.** Use aligned bars or a sparkline, with a text fallback. Never use colour alone, honour `NO_COLOR`, and leave braille off by default.
8. **Kill switch and limits.** One key pauses every auto answer. Auto-answering also stops after N answers in a row that no human looked at, and on any rule error. Add a per-agent rate limit and a "recent auto answers" list.
