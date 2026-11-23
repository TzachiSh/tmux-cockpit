// The mod's contract with the core: what it hands scripts/cockpit-event, and when.
// Each hook a test registers stands for the engine beneath the mod. A test that
// expects no line ends with a control turn and waits for its line, so "nothing" is
// a count and never a guess about timing.
import type { On } from 'claude-code'
import type { Engine } from 'claude-code/testing'
import { expect, test } from 'claude-code/testing'

const SID = '0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0'
const HEX = '0f1e2d3c4b5a69788796a5b4c3d2e1f0'
const SECRET = 'SECRET-ANSWER-7f3'
const KEYS_END = ['agent', 'at', 'kind', 'outcome', 'pane', 'schema_version', 'session']
const KEYS_ENDED = ['agent', 'at', 'kind', 'pane', 'schema_version', 'session']
const UTC = /^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(\.[0-9]{1,9})?Z$/
const OK = {
  exitCode: 0, stdout: '', stderr: '', isStdoutTruncated: false, isStderrTruncated: false,
}
// What core answers for each: the approved call ran and failed, the refused one never ran.
const FAILED = { isError: true, result: undefined, text: 'Exit code 1' }
const REFUSED = { isError: true, result: undefined, text: 'The user refused this call.' }

type Run = { argv: readonly string[]; stdin: string; timeoutMs: number | undefined }
type World = {
  pane?: string | undefined
  run?: 'ok' | 'throw' | 'hang' // how the core's process.run answers
}

// The world beneath the mod: pane %7, one session, the core's runs, and tool calls
// that ask the person and end in error, approved (the tool ran) or refused (it did not).
function world($: Engine, on: On, w: World = {}) {
  const runs: Run[] = []
  const waits: [number, () => void][] = []
  const answers: string[] = []
  const approvals: boolean[] = []
  const pane = { now: 'pane' in w ? w.pane : '%7' }
  const held = { release: () => {}, settled: false } // a hung run, until the test lets it go
  on('env.get', (_, e) => ({ value: e.name === 'TMUX_PANE' ? pane.now : undefined }))
  on('session.id', () => ({ value: SID }))
  on('process.run', (_, e) => {
    runs.push({ argv: e.argv, stdin: e.init?.stdin ?? '', timeoutMs: e.init?.timeoutMs })
    for (const [n, done] of waits) if (runs.length >= n) done()
    if (w.run === 'throw') throw new Error('no core here')
    if (w.run !== 'hang') return { value: OK }
    return new Promise<{ value: typeof OK }>(done => {
      held.release = () => {
        held.settled = true
        done({ value: OK })
      }
    })
  })
  on('turn.start', (_, e) => ({ turnId: e.turnId }))
  on('turn.complete', (_, e) => {
    answers.push(e.answer)
    return { text: 'beneath' }
  })
  // The kit raises its own check for the call, with no approver: let that pass, so the
  // call reaches the hook below, which raises the person's ask as a session does.
  on('tool.check', (_, e) => ({ decision: e.approver === 'person' ? 'ask' : 'allow' }))
  on('tool.call', async (_, e) => {
    const approved = approvals.shift() ?? false
    await $.tool.check({ tool: e.tool, input: {}, tool_use_id: e.tool_use_id, approver: 'person' })
    return approved ? FAILED : REFUSED
  })
  const until = (n: number) =>
    new Promise<void>(done => (runs.length >= n ? done() : waits.push([n, done])))
  const lines = () => runs.map(r => JSON.parse(r.stdin))
  const outcomes = () => lines().map(l => l.outcome)
  const kinds = () => lines().map(l => l.kind)
  return { runs, answers, approvals, pane, held, until, lines, outcomes, kinds }
}

let turns = 0
const end = (reason: 'answer' | 'aborted' | 'error', more: object = {}) => ({
  reason,
  answer: SECRET,
  durationMs: 5,
  isAborted: reason === 'aborted',
  turnId: `t-main-${++turns}`,
  ...more,
})
// A main turn with one Bash call the person was asked about, ending as an answer.
async function askedTurn($: Engine, w: ReturnType<typeof world>, approved: boolean) {
  const turnId = `t-asked-${++turns}`
  await $.turn.start({ text: '', turnId })
  w.approvals.push(approved)
  const r = await $.tool.call({ tool: 'Bash', command: 'false', tool_use_id: `toolu_${turns}` })
  expect(r).toMatchObject({ isError: true }) // the call ended in error either way
  expect(r).toMatchObject({ text: approved ? FAILED.text : REFUSED.text })
  await $.turn.complete(end('answer', { turnId }))
}

test('turn.complete aborted emits exactly one turn.end outcome aborted', async ($, on) => {
  const w = world($, on)
  await $.turn.complete(end('aborted'))
  await $.turn.complete(end('error')) // the control, so a second aborted line would show
  await w.until(2)
  expect(w.outcomes()).toEqual(['aborted', 'error'])
  expect(w.lines()[0]).toEqual({ schema_version: 2, agent: 'claude', kind: 'turn.end',
    pane: '%7', session: HEX, at: expect.stringMatching(UTC), outcome: 'aborted' })
  expect(w.runs[0]?.argv).toHaveLength(1)
  expect(w.runs[0]?.argv[0]).toEndWith('/../../scripts/cockpit-event')
  expect(w.runs[0]?.timeoutMs).toBe(1500)
})

test('turn.complete error emits exactly one turn.end outcome error', async ($, on) => {
  const w = world($, on)
  await $.turn.complete(end('error'))
  await $.turn.complete(end('aborted')) // the control, so a second error line would show
  await w.until(2)
  expect(w.outcomes()).toEqual(['error', 'aborted'])
  expect(w.lines()[0]).toEqual({ schema_version: 2, agent: 'claude', kind: 'turn.end',
    pane: '%7', session: HEX, at: expect.stringMatching(UTC), outcome: 'error' })
})

test('turn.complete answer emits exactly one input.ended, with no outcome', async ($, on) => {
  const w = world($, on)
  await $.turn.complete(end('answer'))
  await $.turn.complete(end('error')) // the control, so a second input.ended would show
  await w.until(2)
  expect(w.kinds()).toEqual(['input.ended', 'turn.end'])
  expect(w.lines()[0]).toEqual({ schema_version: 2, agent: 'claude', kind: 'input.ended',
    pane: '%7', session: HEX, at: expect.stringMatching(UTC) })
})

// The mod cannot tell a refused ask from an approved one whose call failed, and does not try:
// the core ends only an ask still waiting, and the approved call's own hook has answered it.
test('an answer after an approved ask that failed and one after a refused ask emit one line',
  async ($, on) => {
    const w = world($, on)
    await askedTurn($, w, true)
    await askedTurn($, w, false)
    await $.turn.complete(end('error')) // the control, so a third line would show
    await w.until(3)
    expect(w.kinds()).toEqual(['input.ended', 'input.ended', 'turn.end'])
    expect({ ...w.lines()[0], at: '' }).toEqual({ ...w.lines()[1], at: '' })
  })

test('subagent turns emit nothing', async ($, on) => {
  const w = world($, on)
  await $.turn.start({ text: '', turnId: 't-a1', agentId: 'a1' })
  await $.turn.complete(end('aborted', { turnId: 't-a1', agentId: 'a1' }))
  await $.turn.start({ text: '', turnId: 't-a2', agentId: 'a2' })
  await $.turn.complete(end('error', { turnId: 't-a2', agentId: 'a2' }))
  await $.turn.start({ text: '', turnId: 't-a3', agentId: 'a3' })
  await $.turn.complete(end('answer', { turnId: 't-a3', agentId: 'a3' }))
  await $.turn.complete(end('error'))
  await w.until(1)
  expect(w.outcomes()).toEqual(['error'])
})

test('no emitted line contains the answer text', async ($, on) => {
  const w = world($, on)
  await $.turn.complete(end('aborted'))
  await $.turn.complete(end('error'))
  await $.turn.complete(end('answer'))
  await w.until(3)
  expect(w.answers.filter(a => a === SECRET)).toHaveLength(3)
  for (const r of w.runs) expect([...r.argv, r.stdin].join(' ')).not.toContain(SECRET)
})

test('every emitted line has exactly the agent-event keys of its kind', async ($, on) => {
  const w = world($, on)
  await $.turn.complete(end('aborted'))
  await $.turn.complete(end('error'))
  await $.turn.complete(end('answer'))
  await w.until(3)
  expect(w.kinds()).toEqual(['turn.end', 'turn.end', 'input.ended'])
  for (const r of w.runs) {
    const l = JSON.parse(r.stdin)
    expect(r.stdin.split('\n')).toEqual([expect.any(String), ''])
    expect(Object.keys(l).sort()).toEqual(l.kind === 'input.ended' ? KEYS_ENDED : KEYS_END)
  }
})

// No claim about the mod's .catch: the kit skips a hook that rejects (docs/mod.md).
test('a refused process.run still ends the turn', async ($, on) => {
  const w = world($, on, { run: 'throw' })
  const r = await $.turn.complete(end('aborted'))
  await w.until(1)
  expect(r).toMatchObject({ text: 'beneath' })
  expect(w.outcomes()).toEqual(['aborted'])
})

// A hook that waited for the run would never return here: the test's 2000 ms bound
// fails it instead of hanging the file. The run is let go at the end either way.
test('the hook does not wait for the push', { timeoutMs: 2000 }, async ($, on) => {
  const w = world($, on, { run: 'hang' })
  try {
    const r = await $.turn.complete(end('aborted'))
    await w.until(1)
    expect(r).toMatchObject({ text: 'beneath' })
    expect(w.runs).toHaveLength(1)
    expect(w.held.settled).toBe(false)
  } finally {
    w.held.release()
  }
})

test('outside tmux nothing is run', async ($, on) => {
  const w = world($, on, { pane: undefined })
  await $.turn.complete(end('aborted'))
  w.pane.now = '%7' // the pane is read per event: the control turn finds one
  await $.turn.complete(end('error'))
  await w.until(1)
  expect(w.outcomes()).toEqual(['error'])
})
