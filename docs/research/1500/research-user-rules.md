Written by decision-ui-research for tmux-cockpit-2a [fa458f] on 2026-11-26, while master was a11b1776. The one repo file it cites is AGENTS.md.

# Research: should tmux-cockpit's auto-answer set be a user-defined rule list?

Written by the decision-ui-research agent, 2026-11-26. Every claim links its source. Lines marked **Uncertain** are inferences or gaps, not findings.

## Verdict

**Good with limits.** A rule list the owner writes is normal practice: Claude Code, Codex, Renovate and GitHub all work that way. It is safe only in a narrow shape:

- Rules match facts the cockpit checks itself, never the question text.
- Rules can only switch on, narrow or tune a fixed set of built-in actions.
- A built-in never-list that no rule can override.
- The rule file is owner-only and pinned by hash.
- The cockpit refuses to run on any error.
- Every new rule starts in shadow mode.

Free-form matching on question text, rules that run commands, or rules supplied by a repository or an agent would be a bad idea.

## 1. How comparable tools do it

| Tool | Who writes rules | Format and validation | Default | Never auto-approved |
| --- | --- | --- | --- | --- |
| Claude Code permissions | User, project, local and managed settings files. Managed settings beat every other level, and `allowManagedPermissionRulesOnly` makes them the only source ([permissions](https://code.claude.com/docs/en/permissions)) | `Tool(pattern)` allow/ask/deny rules, checked deny first, then ask, then allow; a deny at any level can't be overridden ([permissions](https://code.claude.com/docs/en/permissions)). A project's allow rules apply only after the user accepts a trust dialog that lists them ([permissions](https://code.claude.com/docs/en/permissions)) | Manual mode prompts for edits, shell and network ([permissions](https://code.claude.com/docs/en/permissions)) | Allow rules don't approve writes to protected paths such as `.claude`, `.git`, `.mcp.json` ([modes](https://code.claude.com/docs/en/permission-modes)). Interactive tools and `requiresUserInteraction` MCP tools still prompt after a hook "allow" ([permissions](https://code.claude.com/docs/en/permissions)). Auto mode drops broad interpreter rules such as `Bash(python*)` ([modes](https://code.claude.com/docs/en/permission-modes)) |
| Claude Code hooks | Same settings scopes. Hooks from a repository run even before the folder is trusted ([permissions](https://code.claude.com/docs/en/permissions)) | Program logic, so nothing is validated beyond the JSON output. When hooks disagree, deny > defer > ask > allow ([hooks](https://code.claude.com/docs/en/hooks)) | None | A hook "allow" can't beat a deny or ask rule ([permissions](https://code.claude.com/docs/en/permissions)) |
| GitHub auto-merge and rulesets | Writers enable auto-merge per PR once the repo allows it ([settings](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-auto-merge-for-pull-requests-in-your-repository)). Admins write rulesets, with an `evaluate` (dry-run) status and bypass actors ([REST](https://docs.github.com/en/rest/repos/rules)) | Declarative rules. `evaluate` is GitHub Enterprise only ([REST](https://docs.github.com/en/rest/repos/rules)). GitHub's guidance: start in Evaluate, then pilot, then enforce ([GitHub Learn](https://learn.github.com/well-architected/governance/recommendations/managing-repositories-at-scale/rulesets-best-practices)) | Auto-merge waits for every required review and check ([docs](https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/incorporating-changes-from-a-pull-request/automatically-merging-a-pull-request)) | Turns off when a non-writer pushes or the base changes (same). Approvals can be dismissed as stale ([protected branches](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches)) |
| Renovate | Repository maintainers, in `renovate.json` `packageRules` ([automerge](https://docs.renovatebot.com/key-concepts/automerge/)) | Schema-checked JSON, plus `renovate-config-validator`. An invalid config opens an issue and "Renovate will stop PRs until it is resolved" ([source](https://github.com/renovatebot/renovate/blob/main/lib/workers/repository/error-config.ts)). The validator has missed real errors ([#15999](https://github.com/renovatebot/renovate/issues/15999), [#34542](https://github.com/renovatebot/renovate/discussions/34542)) | `automerge: false` ([options](https://docs.renovatebot.com/configuration-options/)) | Branches that are failing or behind the base; one automerge per run ([automerge](https://docs.renovatebot.com/key-concepts/automerge/)). Releases younger than `minimumReleaseAge` ([age](https://docs.renovatebot.com/key-concepts/minimum-release-age/)) |
| Dependabot | Maintainers, in a workflow file ([docs](https://docs.github.com/en/code-security/tutorials/secure-your-dependencies/automate-dependabot-with-actions)) | Workflow conditions on `fetch-metadata` outputs such as update type ([fetch-metadata](https://github.com/dependabot/fetch-metadata)) | No auto-merge unless a workflow adds it | Whatever branch protection requires; the docs tell you to require status checks (same) |
| Codex execpolicy | User `~/.codex/rules/`. Project rules load only when the project layer is trusted. Admins push restrictive rules through `requirements.toml` ([rules](https://learn.chatgpt.com/docs/agent-configuration/rules)) | Starlark `prefix_rule` with `match`/`not_match` examples checked at load, plus `codex execpolicy check` ([README](https://github.com/openai/codex/blob/main/codex-rs/execpolicy/README.md)) | No match means the normal approval flow (same) | `forbidden` wins over everything (`forbidden` > `prompt` > `allow`). Compound shell is split, or else judged as one string ([rules](https://learn.chatgpt.com/docs/agent-configuration/rules)) |
| claude-squad | User, one boolean `auto_yes` or `-y` flag ([config.go](https://github.com/smtg-ai/claude-squad/blob/a4ab6988/config/config.go), [README](https://github.com/smtg-ai/claude-squad)) | No rules. A daemon presses Enter on every detected prompt in every running session ([daemon.go](https://github.com/smtg-ai/claude-squad/blob/main/daemon/daemon.go)) | Off; marked "[experimental]" | Nothing excluded. Users asked for per-session control ([#151](https://github.com/smtg-ai/claude-squad/issues/151)) and for `-y` to mean bypass mode ([#222](https://github.com/smtg-ai/claude-squad/issues/222)) |
| vibe-kanban | User, per-agent "variants" in `profiles.json` or a form ([docs](https://vibekanban.com/docs/configuration-customisation/agent-configurations)) | Booleans and enums only, no rules | Every built-in DEFAULT variant is the most permissive: `dangerously_skip_permissions: true`, `yolo: true`, `danger-full-access` ([default_profiles.json](https://github.com/BloopAI/vibe-kanban/blob/main/crates/executors/default_profiles.json)). An APPROVALS variant is opt-in ([discussion #2307](https://github.com/BloopAI/vibe-kanban/discussions/2307)) | Nothing excluded in the DEFAULT variant |

**Pattern.** The mature tools (Claude Code, Codex, GitHub, Renovate) all have four things:

- The user writes rules, but a built-in deny or never-list beats them.
- Rules match a structured fact, not prose.
- There is a validation or dry-run step.
- Rules from the repository or the project need a trust step before they apply.

The agent managers (claude-squad, vibe-kanban) offer only all-or-nothing switches with no exclusions. That is the model to avoid.

## 2. Risks and what reduces them

### Rules too broad

- **Evidence.** Prefix allowlists cover more than intended: Cursor's `git` entry matches any git command ([Cursor](https://cursor.com/docs/reference/permissions)). String denylists were bypassed four ways, and Cursor dropped the feature ([Backslash](https://www.backslash.security/blog/cursor-ai-security-flaw-autorun-denylist)). Rules a tool proposes widen scope unless reviewed ([Codex](https://learn.chatgpt.com/docs/agent-configuration/rules)). Claude Code's "don't ask again" saves rules to local settings ([hooks, `permission_suggestions`](https://code.claude.com/docs/en/hooks)).
- **What helps.** Shadow mode shows over-matching before it acts: GitHub Evaluate ([REST](https://docs.github.com/en/rest/repos/rules)), Gatekeeper `dryrun` ([docs](https://open-policy-agent.github.io/gatekeeper/website/docs/v3.21.x/violations/)). `match`/`not_match` examples catch obvious mistakes at load ([README](https://github.com/openai/codex/blob/main/codex-rs/execpolicy/README.md)). The log only shows the harm afterwards.

### Rules going stale when the head or base moves

- **Evidence.** GitHub dismisses stale approvals and can require branches to be up to date ([protected branches](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches)). Auto-merge turns off on a non-writer push or a base switch ([docs](https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/incorporating-changes-from-a-pull-request/automatically-merging-a-pull-request)). The merge API and `gh pr merge --match-head-commit` refuse with 409 if the head moved ([REST](https://docs.github.com/en/rest/pulls/pulls), [gh](https://cli.github.com/manual/gh_pr_merge)). A changed Kubernetes policy does not recheck existing objects ([tutorial](https://kubernetes.io/docs/tutorials/cluster-management/admission-policies/)).
- **What helps.** Re-check the facts at action time, and pin the sha in the action itself. Shadow mode and the log don't fix staleness.

### Silent merges or deletions

- **Evidence.** In this repo a merge can't be undone: "A merged change is never reverted" (AGENTS.md). A remote branch can be restored from its PR ([docs](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-branches-in-your-repository/deleting-and-restoring-branches-in-a-pull-request)). Local unreachable objects survive about 2 weeks before `git gc` prunes them ([git-gc](https://git-scm.com/docs/git-gc)).
- **What helps.** Notification plus log makes the merge visible but not reversible, so the merge rule rests entirely on its preconditions. Logging the branch tip sha makes cleanup recoverable.
- **Uncertain:** whether every object survives a worktree removal plus branch deletion depends on gc timing in that repo.

### Prompt injection steering which rule matches

- **Evidence.** The agent writes the AskUserQuestion question and options; a hook supplies only `answers` ([hooks](https://code.claude.com/docs/en/hooks)). A hijacked agent can therefore word a question to look like "merge?". GitHub warns that anyone with write access can set any status check, and lets you require a specific app as the source ([protected branches](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches)). The same problem applies to a "VERDICT: MERGE" comment posted from a gh login that every lane shares. Claude's classifier ignores tool output so content can't steer it ([Anthropic](https://www.anthropic.com/research/claude-code-auto-mode)). Willison: cut one leg of the trifecta ([post](https://simonwillison.net/2025/Aug/9/bay-area-ai/)).
- **What helps.** Match on facts the cockpit derives itself: the PR from the lane's own branch, the sha and CI from the GitHub API, and the verdict's author proven by something the implementing lane can't fake. Shadow mode and the log only catch this afterwards.

### A config file an agent itself can edit

- **Evidence.** Claude Code protects its own config paths ([modes](https://code.claude.com/docs/en/permission-modes)), but a cockpit rule file is not on that list. Every agent runs as the tmux server's owner, and the owner "always" has access to the server ([tmux(1)](https://man7.org/linux/man-pages/man1/tmux.1.html)). So rules kept in tmux options can be rewritten by any pane. Claude Code holds a repository's allow rules until a trust dialog lists them ([permissions](https://code.claude.com/docs/en/permissions)); that is the model for "changed rules need re-acceptance".
- **What helps.** Store the file outside every repo and every tmux option. Pin its hash. On any change, disable all rules until the owner re-accepts the diff. A deny rule and PreToolUse hook on the path are a second layer, but Bash patterns are fragile ([permissions](https://code.claude.com/docs/en/permissions)).
- **Uncertain:** within one Unix user there is no hard boundary. An agent that can send keys could also accept a prompt, so this is detection, not prevention.

### What the owner gives up by declining a kill switch

- **The circuit breaker.** Claude's auto mode has a fixed one: it pauses after 3 blocks in a row or 20 in total ([modes](https://code.claude.com/docs/en/permission-modes)). OWASP lists rate limits so damage is caught "before significant damage can occur" ([LLM06](https://github.com/OWASP/www-project-top-10-for-large-language-model-applications/blob/main/2_0_vulns/LLM06_ExcessiveAgency.md)). GitHub keeps break-glass bypass actors ([GitHub Learn](https://learn.github.com/well-architected/governance/recommendations/managing-repositories-at-scale/rulesets-best-practices)).
- **The cost here.** Without a switch, a misfiring rule keeps acting across every lane until someone edits the file that may itself be the problem. Review by notification only happens after the merge, which can't be reverted.
- **A cheaper middle ground the owner might accept, since nobody presses it:**
  - Fail closed on any rule or evidence error, as Renovate stops PRs on a config error ([source](https://github.com/renovatebot/renovate/blob/main/lib/workers/repository/error-config.ts)).
  - A per-rule rate cap.
  - Owner re-acceptance after a file change.

## 3. Suggested shape

- **Where and who.** One owner file under `~/.config/tmux-cockpit/` (path to be decided), never in a repo or a tmux option, written by the owner only. The cockpit loads it at start and on change. A changed hash shows a diff and keeps every rule off until the owner accepts.
- **One way.** The three picked rules ship as default entries in this same format, not as a separate built-in path, to keep one way per AGENTS.md "Keep one way to do each thing".
- **Rule form.**
  - `action`: one of a fixed set of cockpit handlers (`merge`, `cleanup-lane`, `continue-queue`). No free command.
  - `when`: a conjunction over a closed list of fact predicates the cockpit computes. Examples: `verdict.merge_at == pr.head_sha`, `verdict.author != pr.lane`, `ci.conclusion == success on merge(pr.head, master@now)`, `lane.unpushed == 0`, `queue.owner_set == true`. No regex over question text.
  - `mode`: `shadow` or `live`.
  - `examples`: fact snapshots that must match, and ones that must not, checked at load (the Codex `match`/`not_match` pattern).
- **At action time.**
  - Re-read every fact.
  - Pin the sha in the action (`--match-head-commit`).
  - Log the rule id, the facts with their sources, and the answer.
  - Send the notification.
- **Safe defaults.**
  - A new rule starts in `shadow`.
  - An unknown predicate, a missing fact or a failed example means the rule is off and the question goes to the owner.
  - A per-rule cap per hour.
- **Never auto-answered, hard-coded and outside the file:**
  - questions whose options leave the fixed action set ("new direction or unknown")
  - force push, history rewrite, deleting a branch or worktree with unpushed or unmerged commits
  - anything touching secrets, permissions or the cockpit's own config
  - a merge whose verdict author can't be told apart from the PR's own lane

**Uncertain, worth one probe on a private server before building:** in Claude Code's auto mode, the agent's own `gh pr merge` after a hook-answered "yes" may still be blocked. Auto mode blocks "merging a pull request no human has approved" ([modes](https://code.claude.com/docs/en/permission-modes)), and the classifier does not see tool results ([modes](https://code.claude.com/docs/en/permission-modes)). Having the cockpit run the merge itself would avoid this and keep the sha pin under its control.

## What would change the verdict

- **Towards "bad idea":**
  - The owner wants rules over question text or free commands.
  - The cockpit can't detect edits to the rule file.
  - Verdict authorship can't be proven, so the merge rule is unsafe in any format.
- **Towards "built-ins only":** a count of real AskUserQuestion prompts shows the three rules cover nearly all auto-answerable cases. A rule engine would then add surface for little gain.
- **Towards fewer limits:** merges become revertible, which they are not in this repo.
