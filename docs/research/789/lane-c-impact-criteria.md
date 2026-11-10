# Lane C: impact-based criteria for "plan or ask first"

Question: which criteria do engineering orgs and agent vendors use to decide a change needs a plan or design first, and which can a panel judge from request text, terminal screen and a short repo summary (branch, status, recent commits, files touched)?

Read-only lane. All sources accessed 2026-11-10.

## Findings

| # | Finding | Source | Grade |
|---|---|---|---|
| 1 | One-way vs two-way doors. Quote: "consequential and irreversible or nearly irreversible – one-way doors"; "they're two-way doors." Reversible decisions "can and should be made quickly by high judgment individuals or small groups"; irreversible ones "must be made methodically, carefully, slowly, with great deliberation and consultation" | https://www.sec.gov/Archives/edgar/data/1018724/000119312516530910/d168744dex991.htm (2015 shareholder letter) | CONFIRMED |
| 2 | Blast radius and reversibility. Quote: "Make frequent, small, reversible changes"; smaller changes "reduces the blast radius and allows for faster reversal". A principle, no threshold | https://docs.aws.amazon.com/wellarchitected/latest/framework/oe-design-principles.html | CONFIRMED |
| 3 | Design-doc triggers at one company: new sensitive data ("personal data, money information, or secrets"), "Considerable changes on authentication or authorization", "a new architectural change (new database, workers, etc.)", "a new third-party tool will see merchant or customer data". Listed under "Do not write a new design document for": bug fixes, UI changes, and new API endpoints that expose the same data under the same auth rules ("A pull request is enough" is said only of that last item). Quotes came through a fetch tool. Closest match to the owner's foundations list | https://handbook.polar.sh/engineering/design-documents | CONFIRMED, one company |
| 4 | Architectural significance: "a requirement that has a measurable effect on the architecture". No concrete threshold | https://adr.github.io/ | CONFIRMED |
| 5 | Risk-fitted process: "fit the deployment process to the risk profile of a given service." Does not classify changes by risk | https://sre.google/sre-book/release-engineering/ | CONFIRMED, principle only |
| 6 | Mage (opened): a design doc is a good idea "anytime you're trying to solve a technical problem that takes longer than a day"; not needed for "very small features or technical problems with very obvious solutions". Infisical (search snippet only, the page returned 404): feature not well defined, high-security, or more than one engineer-week | https://dev.to/mage_ai/how-to-write-technical-design-docs-c02 | Mage CONFIRMED; Infisical PLAUSIBLE. The first pass's "more than one reasonable solution, or more than one team or service" is in neither source and was dropped |
| 7 | Claude Code guidance: "For changes you want to review before they touch disk, switch to plan mode." No size or risk threshold, so the user decides. The best-practices page has no plan-or-skip criteria by keyword search | https://code.claude.com/docs/en/common-workflows | CONFIRMED |

## Candidate checklist (inference, the lane's own synthesis, not a vendor rubric)

| Criterion | Source | Can the panel see it? |
|---|---|---|
| a. Touches a foundation: auth, money, secrets, personal data, data model or migration | 3 | Yes from request text; partly from file paths |
| b. Adds a new architectural component (database, worker, service, dependency) | 3 | Partly |
| c. One-way door: cannot be undone by reverting the commit | 1 | Partly; reversibility often needs reading code |
| d. Blast radius: crosses more than one module, team or service | 2, 6 | Partly; files touched give a count, nothing before work starts |
| e. More than one reasonable solution | 6 | Yes from request text |
| f. Stated purpose present | none found, the owner's own | Yes |
| g. Architectural significance | 4 | Partly; the judge must infer from wording |
| h. Skip case: small UI or logic change reusing existing data and auth | 3 | Yes |

Not judgeable from the inputs: effort in weeks, number of coordinating people.

Refute pass, sourcing per criterion: a and b rest on Polar alone (one company). c rests on the 2015 letter; "cannot be undone by reverting the commit" is the lane's own wording. d has the AWS "reduces the blast radius" line only; "crosses more than one module, team or service" has no source. e (more than one reasonable solution) and f (stated purpose) have no source. g has the ADR definition but no operational test. h rests on Polar's endpoints exclusion. The ordering suggestion has no source.

Ordering suggestion (speculation): c, a and b force plan or question; h plus f allows a direct write; e and d decide the middle.

## Not found

- An OpenAI, Google or Anthropic statement giving criteria for when a coding agent must plan first.
- Opened pages for the Mage, Infisical and other design-doc thresholds.
- A formal risk-based change classification (not searched).
- Evidence on how reliably an LLM judge applies these criteria.
