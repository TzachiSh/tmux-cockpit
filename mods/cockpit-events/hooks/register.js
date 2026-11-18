// The Claude mod adapter of tmux-cockpit (docs/mod.md). It reports what the settings
// hooks cannot: a main turn that ended interrupted or on an API error. It hands one
// agent-event v1 line to scripts/cockpit-event and never waits for it.
import { toEvent } from './map.js'

// The core, from the mod's folder: mods/cockpit-events -> the repo checkout.
const CORE = '/../../scripts/cockpit-event'
// Agent statuses that count as running for the refused-ask rule.
const BUSY = new Set(['pending', 'running', 'waiting'])

export const register = on => {
  // The refused-ask rule (docs/mod.md): Esc on a permission prompt ends the turn
  // with reason answer, so the mod tracks what tells it from a real answer.
  const openAsks = new Set() // every loop's asked calls whose tool.call has not ended
  const mainAsks = new Map() // this main turn's person asks: tool_use_id -> ended in error
  const agentTurns = new Set() // agent turns heard starting, not yet ending

  const ended = (id, failed) => {
    openAsks.delete(id)
    if (mainAsks.has(id)) mainAsks.set(id, failed)
  }
  // Conditions 1, 2, 4 and 5'. Condition 6 is the agent list, read only when these hold.
  const refusable = e =>
    e.reason === 'answer' &&
    [...mainAsks.values()].includes(true) &&
    openAsks.size === 0 &&
    agentTurns.size === 0

  on('turn.start', ($, e, next) => {
    if (e.agentId !== undefined) agentTurns.add(e.turnId)
    else mainAsks.clear()
    return next(e)
  })

  on('tool.check', async ($, e, next) => {
    const r = await next(e)
    const id = e.tool_use_id
    if (r && r.decision === 'ask' && typeof id === 'string' && id !== '') {
      openAsks.add(id)
      if (e.agentId === undefined && e.approver === 'person') mainAsks.set(id, false)
    }
    return r
  })

  on('tool.call', async ($, e, next) => {
    let failed = false
    try {
      const r = await next(e)
      failed = r !== undefined && r.isError === true
      return r
    } finally {
      ended(e.tool_use_id, failed)
    }
  })

  on('turn.complete', async ($, e, next) => {
    if (e.agentId !== undefined) {
      agentTurns.delete(e.turnId)
      return next(e)
    }
    const maybe = refusable(e)
    mainAsks.clear()
    await push($, e, maybe).catch(() => {})
    return next(e)
  })
}

// Nothing runs outside tmux. The pane is read per event: no session.start follows /clear.
async function push($, e, maybe) {
  const pane = await $.env.get('TMUX_PANE')
  if (!pane) return
  const refusedAsk = maybe && (await noAgentRuns($))
  const session = await $.session.id()
  const at = new Date().toISOString()
  const line = toEvent('turn.complete', e, { pane, session, at, refusedAsk })
  if (line === null) return
  const stdin = JSON.stringify(line) + '\n'
  void $.process.run([$.plugin.root + CORE], { stdin, timeoutMs: 1500 }).catch(() => {})
}

// Condition 6. A list that cannot be read counts as a running agent.
async function noAgentRuns($) {
  try {
    const agents = await $.agent.list()
    return !agents.some(a => BUSY.has(a.status))
  } catch {
    return false
  }
}
