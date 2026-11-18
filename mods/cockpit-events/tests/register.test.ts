// The mod's contract with the core: what it hands scripts/cockpit-event, and when.
// Each hook a test registers stands for the engine beneath the mod. A test that
// expects no line ends with a control turn (an API error) and waits for its line,
// so "nothing" is a count of 1 and never a guess about timing.
import type { On } from 'claude-code'
import type { Engine } from 'claude-code/testing'
import { expect, test } from 'claude-code/testing'

const SID = '0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0'
const HEX = '0f1e2d3c4b5a69788796a5b4c3d2e1f0'
const SECRET = 'SECRET-ANSWER-7f3'
const KEYS = ['agent', 'at', 'kind', 'outcome', 'pane', 'schema_version', 'session']
const UTC = /^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(\.[0-9]{1,9})?Z$/
const OK = {
  exitCode: 0, stdout: '', stderr: '', isStdoutTruncated: false, isStderrTruncated: false,
}

type Run = { argv: readonly string[]; stdin: string; timeoutMs: number | undefined }
type Call = { approver?: 'person' | 'host'; failed: boolean }
type World = {
  pane?: string | undefined
  run?: 'ok' | 'throw' | 'hang' // how the core's process.run answers
  agents?: { id: string; description: string; type: string; status: string }[]
}

// The world beneath the mod: pane %7, one session, the agent list, the core's runs.
function world($: Engine, on: On, w: World = {}) {
  const runs: Run[] = []
  const waits: [number, () => void][] = []
  const calls: Call[] = []
  const answers: string[] = []
  const pane = { now: 'pane' in w ? w.pane : '%7' }
  const held = { release: () => {}, settled: false } // a hung run, until the test lets it go
  on('env.get', (_, e) => ({ value: e.name === 'TMUX_PANE' ? pane.now : undefined }))
  on('session.id', () => ({ value: SID }))
  on('agent.list', () => ({ value: w.agents ?? [] }))
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
  on('tool.check', (_, e) => ({ decision: e.approver === undefined ? 'allow' : 'ask' }))
  // A tool call that asks: the check runs inside the call, as in a session.
  on('tool.call', async (_, e) => {
    const c = calls.shift() ?? { failed: false }
    if (c.approver !== undefined) {
      const loop = e.agentId === undefined ? {} : { agentId: e.agentId }
      const ask = { tool: e.tool, input: {}, tool_use_id: e.tool_use_id, approver: c.approver }
      await $.tool.check({ ...ask, ...loop })
    }
    return c.failed ? { isError: true, result: undefined, text: 'refused' } : { result: {} }
  })
  const until = (n: number) =>
    new Promise<void>(done => (runs.length >= n ? done() : waits.push([n, done])))
  const lines = () => runs.map(r => JSON.parse(r.stdin))
  const outcomes = () => lines().map(l => l.outcome)
  return { runs, calls, answers, pane, held, until, lines, outcomes }
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
// The control turn: an API error on the main loop, which always gives a line.
async function control($: Engine, w: ReturnType<typeof world>, n: number) {
  await $.turn.complete(end('error'))
  await w.until(n)
}
// One Bash call on the main loop (or agentId's): asked as `approver`, ended errored or not.
async function asked($: Engine, w: ReturnType<typeof world>, c: Call, agentId?: string) {
  w.calls.push(c)
  const loop = agentId === undefined ? {} : { agentId }
  await $.tool.call({ tool: 'Bash', command: 'true', tool_use_id: `toolu_${++turns}`, ...loop })
}
// The case-4 turn: a main turn whose permission ask the person refused with Esc.
async function refusedAskTurn($: Engine, w: ReturnType<typeof world>, c?: Call) {
  await $.turn.start({ text: '', turnId: 't-case4' })
  await asked($, w, c ?? { approver: 'person', failed: true })
  await $.turn.complete(end('answer', { turnId: 't-case4' }))
}

test('turn.complete aborted emits turn.end outcome aborted', async ($, on) => {
  const w = world($, on)
  await $.turn.complete(end('aborted'))
  await w.until(1)
  expect(w.lines()).toEqual([{ schema_version: 1, agent: 'claude', kind: 'turn.end',
    pane: '%7', session: HEX, at: expect.stringMatching(UTC), outcome: 'aborted' }])
  expect(w.runs[0]?.argv).toHaveLength(1)
  expect(w.runs[0]?.argv[0]).toEndWith('/../../scripts/cockpit-event')
  expect(w.runs[0]?.timeoutMs).toBe(1500)
})

test('turn.complete error emits turn.end outcome error', async ($, on) => {
  const w = world($, on)
  await $.turn.complete(end('error'))
  await w.until(1)
  expect(w.lines()).toEqual([{ schema_version: 1, agent: 'claude', kind: 'turn.end',
    pane: '%7', session: HEX, at: expect.stringMatching(UTC), outcome: 'error' }])
})

test('outcome answer emits nothing', async ($, on) => {
  const w = world($, on)
  await $.turn.start({ text: '', turnId: 't-plain' })
  await $.turn.complete(end('answer', { turnId: 't-plain' }))
  await control($, w, 1)
  expect(w.outcomes()).toEqual(['error'])
})

test('subagent turns emit nothing', async ($, on) => {
  const w = world($, on)
  await $.turn.start({ text: '', turnId: 't-a1', agentId: 'a1' })
  await $.turn.complete(end('aborted', { turnId: 't-a1', agentId: 'a1' }))
  await $.turn.start({ text: '', turnId: 't-a2', agentId: 'a2' })
  await $.turn.complete(end('error', { turnId: 't-a2', agentId: 'a2' }))
  await control($, w, 1)
  expect(w.outcomes()).toEqual(['error'])
})

test('no emitted line contains the answer text', async ($, on) => {
  const w = world($, on)
  await $.turn.complete(end('aborted'))
  await refusedAskTurn($, w)
  await control($, w, 3)
  expect(w.answers.filter(a => a === SECRET)).toHaveLength(3)
  expect(w.outcomes()).toEqual(['aborted', 'aborted', 'error'])
  for (const r of w.runs) expect([...r.argv, r.stdin].join(' ')).not.toContain(SECRET)
})

test('every emitted line has exactly the agent-event v1 keys', async ($, on) => {
  const w = world($, on)
  await $.turn.complete(end('aborted'))
  await refusedAskTurn($, w)
  await control($, w, 3)
  for (const r of w.runs) {
    expect(r.stdin.split('\n')).toEqual([expect.any(String), ''])
    expect(Object.keys(JSON.parse(r.stdin)).sort()).toEqual(KEYS)
  }
})

// No claim about the mod's .catch: the kit may skip a hook that rejects (docs/mod.md).
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
    console.log(`probe: awaited=${w.held.settled}`)
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
  await control($, w, 1)
  expect(w.outcomes()).toEqual(['error'])
})

test('a refused main-loop permission ask ends as aborted', async ($, on) => {
  const w = world($, on)
  await refusedAskTurn($, w)
  await control($, w, 2)
  expect(w.outcomes()).toEqual(['aborted', 'error'])
})

test('refused ask, condition 1 false: an ask no person answers emits nothing', async ($, on) => {
  const w = world($, on)
  await refusedAskTurn($, w, { approver: 'host', failed: true })
  await control($, w, 1)
  expect(w.outcomes()).toEqual(['error'])
})

test('refused ask, condition 2 false: the asked call ended without error', async ($, on) => {
  const w = world($, on)
  await refusedAskTurn($, w, { approver: 'person', failed: false })
  await control($, w, 1)
  expect(w.outcomes()).toEqual(['error'])
})

test('refused ask, condition 4 false: another loop still has an open ask', async ($, on) => {
  const w = world($, on)
  const ask = { tool: 'Bash', input: {}, tool_use_id: 'toolu_open', approver: 'person' as const }
  await $.tool.check({ ...ask, agentId: 'a1' })
  await refusedAskTurn($, w)
  await control($, w, 1)
  expect(w.outcomes()).toEqual(['error'])
})

test("refused ask, condition 5' false: an agent turn is still open", async ($, on) => {
  const w = world($, on)
  await $.turn.start({ text: '', turnId: 't-a1', agentId: 'a1' })
  await refusedAskTurn($, w)
  await control($, w, 1)
  expect(w.outcomes()).toEqual(['error'])
})

test('refused ask, condition 6 false: the agent list shows one running', async ($, on) => {
  const a1 = { id: 'a1', description: 'scan', type: 'general-purpose', status: 'running' }
  const w = world($, on, { agents: [a1] })
  await refusedAskTurn($, w)
  await control($, w, 1)
  expect(w.outcomes()).toEqual(['error'])
})
