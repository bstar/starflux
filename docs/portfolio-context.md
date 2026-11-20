# Portfolio context

Each portfolio can have a `PORTFOLIO.md`: shared, user-owned context for work
that spans its projects. The portfolio coordinator reads it, and every Codex
agent STAR/FLUX starts in a member project receives it.

## Location

By default the file lives at the shared root directory of the portfolio's local
member projects. For projects in `~/projects/star/starfold` and
`~/projects/star/starflux`, that is `~/projects/star/PORTFOLIO.md`, where it can
be edited and versioned like any other file. Choose **Choose a different
location…** for another absolute local path. A portfolio with no local projects
has no default root and needs an explicit location. The choice is stored in the
portfolio's private `context.json`; it is never written into a repository.

The file must be a regular UTF-8 file of at most 32 KiB. Links are rejected.
Every agent pays for this text on every turn, so keep it short.

## Where it is visible

- Every portfolio header leads with its context state: `Context: none`,
  `Context ✓`, or `Context ✓ · 2 agents need restart`.
- A project row shows `context outdated · restart agent` when one of its Codex
  agents runs an older revision or started without the context.
- The portfolio menu's first item is **Portfolio context (PORTFOLIO.md)**. The
  first item on every Portfolio Coordinator page shows the same status.

The context page shows the status, revision, size, location, which agents are
current or need a restart, and a preview. Its actions create the file from a
template, open it in `$VISUAL`/`$EDITOR` in a new terminal tab of the active
local project, change or reset the location, and copy the coordinator charter
into the file as an Objectives section.

## Delivery to agents

When STAR/FLUX starts a Codex agent in a member project (locally or over SSH,
fresh or resumed), it adds `-c developer_instructions=…` after the `codex`
executable. The instructions name the portfolio and project, ask the agent to
change only its own project and report sibling-project work as a follow-up, and
carry the file text with its revision. A `developer_instructions` override the
user already passes is left in place and wins. Other harnesses and shells are
unchanged. No repository `AGENTS.md` is modified.

Running agents keep the revision they started with. Nothing restarts
automatically; restart an agent to apply edits. Local agents' revisions are read
from their command lines; agents on SSH hosts are reported as unknown.

## The coordinator

The coordinator receives the file text in a dedicated `PORTFOLIO CONTEXT`
section in addition to its charter. It never performs work: every change,
including edits to `PORTFOLIO.md`, cleanup and configuration, becomes an exact
scoped proposal for the owning project's agent, approved by the user. Cross-project
changes are split into per-project proposals linked by dependencies. When a
handoff is delivered to an agent that did not start with the current revision,
the task text carries the current context; otherwise it names the revision.

Implementation: `src/portfolio_context.rs`, with delivery hooks in
`src/runtime.rs`, `src/native_backend.rs` and `src/category_agent_runtime.rs`,
and the prompt section in `scripts/category-provider.py`.
