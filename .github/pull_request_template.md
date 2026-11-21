<!-- Open with your session or lane name. See AGENTS.md, "Keep one way to do each thing". -->

Closes #

## Checks

- [ ] One purpose: every file in the diff is needed for the change in the title.
- [ ] The old way is gone: `git grep` for the old name, option or format finds nothing.
- [ ] No new fallback, alias, shim or deprecation notice.
- [ ] One flow, one place: no second copy of a rule that already has a function.
- [ ] No dead code left behind.
- [ ] A committed test fails without the change, run as CI runs it, and its mutant is named.
- [ ] Small enough to read in one sitting.

## Fail-first

## Tests and gates
