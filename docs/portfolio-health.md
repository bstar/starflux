# Portfolio health

STAR/FLUX watches each portfolio for runaway storage and wasteful agent
contexts. It measures only: it never deletes files, compacts conversations or
restarts agents. The portfolio coordinator turns findings into scoped tasks for
the owning project's agent, which run only after you approve them.

## What is measured

While any STAR/FLUX frontend is open, a background sampler runs for every
portfolio (including Unassigned). A private lock prevents duplicate samplers.

- **Free space**, about once a minute, for each drive holding a local member
  project, with a 90-sample history used to compute the loss per hour.
- **Project disk use**, about every 15 minutes, for each local member project.
  Each scan stays on the project's filesystem, does not follow links, and stops
  after 20 seconds or two million entries (marked partial). Usage is broken down
  into build output (`target`, `build`, `dist`, `node_modules`, `Intermediate`,
  `.gradle`, …), logs, artifacts, disk images, `.git` and other, with the
  largest files.
- **Agent storage:** the total size of Codex session logs (`$CODEX_HOME/sessions`)
  and each agent's own session log.
- **Agent context:** for each local Codex agent, the tokens in its context after
  its last turn against the model's window, the share of input served from the
  prompt cache, and the size of the fixed instructions it pays for every turn:
  the global `AGENTS.md`, the project's `AGENTS.md` chain from the repository
  root (capped like Codex at 32 KiB) and `PORTFOLIO.md`. Values come from the
  `token_count` records in the tail of the agent's Codex rollout file.

Sizes are allocated bytes, so sparse disk images count only written blocks;
btrfs compression is not reflected. Remote (SSH) projects and agents are not
measured yet and are listed as such.

## Alerts

| Alert | Condition |
|---|---|
| Low space (critical) | under 20 GB or 5% free |
| Low space (warning) | under 10% free |
| Filling fast (critical) | losing over 1 GB/hour and full within 24 hours |
| Runaway file (critical) | one file grew by 1 GB or more between scans (within 2 hours) |
| Project growth | a project grew by 20 GB or more between scans |
| Large log | a file under a logs directory, or a `.log` file, of 10 GB or more |
| Agent storage | Codex session logs total 20 GB or more |
| Context near limit | an agent uses 70% or more of its window |
| Idle with large context | an idle agent holds 50% or more of its window |
| Low cache reuse | under 50% of a large (20k+ token) input came from cache |
| Large fixed instructions | over 24 KiB (about 6k tokens) of instruction files |
| Large session log | an agent's session log exceeds 200 MB |

## Where it is visible

- Portfolio headers show `⚠ Health: N critical` or `Health: N warning(s)`.
- Project rows show markers such as `⚠ runaway file`, `⚠ growing fast`,
  `large log` or `agent context large`.
- The portfolio menu has **Portfolio health (disk & agent context)**, and every
  Portfolio Coordinator page lists it second, after the portfolio context.

The health page shows the alerts, each drive's free space and trend, each
project's usage by category and largest file, and each agent's context use,
cache reuse, instruction size and session log size. **Check now** rescans
immediately.

## The coordinator

Reviews include the latest health sample as evidence. The coordinator proposes
cleanup tasks for the owning project's agent that prune regenerable output and
bound the cause (log caps, timeouts, retention) while keeping source, user data
and deliverables. For context efficiency it proposes trimming or splitting
oversized `AGENTS.md` or `PORTFOLIO.md` content and recommends compacting or
restarting conversations that are near their limit or idle with large contexts.

Implementation: `src/portfolio_health.rs` and `src/agent_context.rs`. Samples
are stored privately as `health.json` beside the portfolio's coordinator state.
