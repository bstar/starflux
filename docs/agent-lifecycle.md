# Agent identity, health and recovery

Right-click a terminal tab and choose **Agent status / conversation**. From the keyboard, press Ctrl+B, then x to open the pane menu. The status view separates terminal availability, the foreground agent process, its backend connection, and task lifecycle. An alive shell or agent process does not establish that its server connection is healthy. Backend health stays `unknown` unless a failure is explicitly observed or reported.

The known “Automatic reconnect could not restore this session” and “Server connection could not be restored” footers produce `backend_unavailable`, rather than idle. Affected projects get an error marker. Output detection is a heuristic; the inspector labels it accordingly and does not export transcript text.

The inspector explains why a registered report lost authority. The JSON health field `report_status` distinguishes an archived record, process identity mismatch, clock skew, heartbeat expiry, and a terminal-observed backend failure overriding a current report. It also identifies current and unregistered reports.

A confirmed terminal transport failure takes precedence over the last reported lifecycle. A disconnected presentation bridge can also appear alongside a running agent and its current lifecycle when the terminal owner remains reachable.

Integrations can register an exact native conversation reference and a native resume argument vector. **Resume saved conversation in new tab** uses that registered vector and retains the original pane and process. It never guesses a conversation by directory, title, modification time or “last session.” When resume is unavailable, the menu explains whether the conversation reference or its command is missing. **Recover to shell…** remains separate and requires the existing confirmation before stopping the foreground program.

## Structured reports

Run these commands on the workspace controller machine, including through SSH when that controller is remote:

```sh
starflux --workspace PROJECT --agent-status
starflux --workspace PROJECT --pane 1 --agent-report '{...}'
```

`--agent-status` lists all project terminals, layered health and archived run records. `--pane` and the status list use one-based pane numbers. Saved records and UI snapshots retain zero-based internal pane IDs. Inspecting status does not start, stop or replace agents.

Add `--agent-status-pane N` to inspect only the named terminal and its saved records. The provider startup adapter uses this scoped form so unrelated panes do not delay registration.

A report contains:

```json
{
  "run_id": "af428fa9-8ac2-4f08-b208-f3fc57efcb48",
  "sequence": 1,
  "epoch": "COPY_FROM_AGENT_STATUS",
  "agent_pid": 12345,
  "agent_start_ticks": 987654,
  "boot_id": "COPY_HOST_BOOT_UUID_FROM_AGENT_STATUS",
  "harness": "codex",
  "source": "codex:my-integration",
  "state": "working",
  "conversation_id": "NATIVE_CONVERSATION_UUID",
  "resume_argv": ["codex", "resume", "NATIVE_CONVERSATION_UUID"]
}
```

The integration supplies its actual conversation reference and resume arguments. The Codex example uses the installed CLI's explicit session-ID resume form; STAR/FLUX does not derive it from transcript text. Use the epoch, agent PID, process start ticks and `boot_id` from the matching pane's status. The boot UUID belongs to the terminal host, including for SSH projects. Legacy panes may have a null epoch. Use a fresh UUID for each agent run and increase sequence numbers monotonically. Preserve the registered identity and resume vector in subsequent reports. Optional conversation and resume fields may be omitted if unavailable.

Supported lifecycle values are `unknown`, `idle`, `working`, `needs_input`, `waiting_for_approval`, `completed`, `failed`, `stopped`, and `backend_unavailable`. Working and approval states require heartbeats at least once per minute to remain authoritative. After 60 seconds, the inspector marks the report stale and falls back to labeled process/screen observations; the saved conversation reference remains available. A live backend-failure footer overrides an older integration heartbeat.

Reports are rejected when the terminal generation, foreground process identity (including process start ticks and host boot UUID), harness or sequence does not match. Updating an older superseded run is rejected. To register a different conversation in the same agent process, start a new run at sequence 1 and set `replaces_run` to the current run UUID; this prevents delayed registrations from silently replacing a newer run. Whenever a new run supplies `replaces_run`, that reference must still match the active run for the pane, including after an agent process change. Omit it when registering a fresh run without a predecessor.

Records are private metadata in `$XDG_DATA_HOME/starflux/projects/PROJECT.agents.json` (normally `~/.local/share/starflux/projects/`). Previous runs remain archived when a new run starts. Saved resume commands are shell-quoted argument vectors, scoped to the project's host and directory, and run only after an explicit resume action. They are retained with their new terminal across frontend reattachment.

This interface is implemented and tested with disposable agent fixtures. Claude Code launches register automatically through per-launch hooks, and Codex registers through a reviewed, trusted hook. Agents that were already running are not modified to install hooks, and their conversations are not discovered after the fact. STAR/FLUX can detect their process and screen health without a structured report, but cannot promise exact conversation recovery until their identity is registered.

A saved report from a previous boot cannot establish that a current agent is the same process, even if its PID and start ticks coincide. Legacy records without a boot identity remain readable and retain their explicit conversation/resume references, but are never authoritative live lifecycle reports. New reports must include the current boot identity; missing or stale identities are rejected.

When explicit shell recovery replaces an abandoned PTY bridge or an exited plugin terminal, the saved conversation stays available in the recovered pane. Its original report remains stale and cannot establish the health of the replacement process.

Explicit resume uses a durable one-shot launch receipt. A later missing backend opens a shell rather than executing the saved resume hint again. See [cold-start recovery and saved screens](cold-start.md).

## AI-session recovery acceptance

AI-session recovery must establish the provider's exact conversation identity
after reconnect/resume. Restoring a shell is a separate operation and must not
be reported as successful AI recovery. Missing or ambiguous provider references
must be visible; directory/title matching, newest-file selection and `--last`
are not identity evidence. Recovery must preserve model/provider/authentication
choices and avoid resubmitting the previous prompt or pending approvals.

The staged native backend preserves registered references across deliberate
recovery and has private local/SSH tests for the exact saved resume vector. Those
fixtures use disposable fake agents: they do not yet prove native Codex/Claude
conversation resumption or automatic identity capture for arbitrary existing
agents. These remain required before claiming provider-complete recovery.

Codex 0.169.0's exported experimental schema includes session-start lifecycle
events. The official [hook guide](https://learn.chatgpt.com/docs/hooks) documents
the event's current session ID and startup/resume/fork sources. It also states
that switching away does not immediately end a session. Thus a hook's thread
reference alone is insufficient to infer which STAR/FLUX pane currently displays
that thread. The adapter must bind registration to the pane/launch incarnation
and handle thread switches and delayed callbacks. Existing hooks and provider
trust settings must be preserved. No hooks have been installed in live agents.

The Linux fixture `scripts/test-codex-resume-offline.py /path/to/codex` verifies
the installed Codex app-server's exact-ID contract. It uses Bubblewrap to mount
a disposable profile and disable networking. With Codex 0.169.0, a fresh server
resumed the persisted thread by ID and rejected an unknown ID.

The same fixture opens `codex resume --no-daemon --no-alt-screen ID` in a
private PTY and checks that the interactive `/status` output contains the
original thread ID. A disposable provider configuration keeps the fixture
independent of sign-in. The test checks conversation identity and does not
invoke a model or assert that the synthetic history item renders in the TUI.

Pass `--starflux /absolute/path/to/private-launcher` to exercise the STAR/FLUX
Agent Status resume action with the real Codex executable. The fixture registers
the provider-confirmed ID and the exact launch arguments, opens a new tab, and
checks the ID reported by Codex. It also checks process retention on frontend
refresh and explicit recovery to a shell. Local graphical, local cells and
loopback SSH cells passed using Codex 0.169.0 with the disposable profile.
On 2026-11-19 the same fixture passed with Codex 0.170.0 in all four local/SSH
and graphical/cells combinations.

`scripts/test-claude-resume-offline.py /path/to/claude --starflux LAUNCHER`
verifies Claude Code the same way. With Claude Code 2.1.328 in a disposable,
network-disabled profile, `SessionStart` reported the exact startup and resume
IDs, and the STAR/FLUX Agent Status resume action relaunched that exact Claude
session locally and over loopback SSH in cells, keeping the original process.
Claude Code launches on local panes are registered automatically. STAR/FLUX
adds a `SessionStart` hook to the launch with Claude's per-launch `--settings`,
so the user's own settings are untouched; the hook's resume template keeps the
launch's `--model`, `--permission-mode`, `--agent` and `--fallback-model`
choices. A launch that already passes `--settings` is left unchanged, and
`STARFLUX_AGENT_HOOKS=0` disables the addition. The fixture's automatic run
starts a sandboxed Claude without any settings and passed with Claude Code
2.1.328.

The same hook reports the session's lifecycle after registration:
`UserPromptSubmit` reports working, `Notification` reports waiting for approval
for permission prompts and needs input otherwise, `Stop` reports completed and
`SessionEnd` reports stopped. Each advances the registered run's sequence; an
event from another process or conversation, or one that repeats the current
state, changes nothing. These reports drive agent status and the attention
inbox. Claude launches get all five events automatically, and `--print-config`
includes them for a reviewed Codex hook. `scripts/test-agent-session-hook.py`
checks every state and the refusals; the automatic Claude fixture observed the
real `UserPromptSubmit` report through STAR/FLUX. Remote panes and Codex keep the explicit hook: Codex requires persisted
hook trust, and STAR/FLUX never bypasses it. The daemon-backed Codex path remains
unverified.

An opt-in local adapter is available at `scripts/agent-session-hook.py`. Add it
as a **synchronous** Codex `SessionStart` command for the bound launch and
review it through Codex's hook trust UI. New owned terminal launches pass
`STARFLUX_WORKSPACE`, `STARFLUX_PANE`, and `STARFLUX_PANE_EPOCH` to their
processes. New sessions, projects, and worktree workspaces also assign a
generation to their first owned pane. The adapter uses this binding and an
explicit resume argument
template that preserves the launch's provider and model choices.

```sh
starflux --agent-session-hook -- /path/to/codex resume --no-daemon '{session_id}'
```

The installed STAR/FLUX executable includes the hook and runs it with `python3`.
Add `--print-config` before `--` to print a JSON fragment for the provider's
hook configuration. Review and merge the fragment with existing hooks.
The generated command uses `starflux` from the provider's PATH by default.
Use `--starflux /path/to/starflux` to choose a stable launcher path explicitly.
The resume template starts with an executable and contains exactly one
`{session_id}` argument. It accepts up to 64 nonempty arguments of up to
4096 UTF-8 bytes each. Control characters are rejected.
See the [Codex hook configuration reference](https://developers.openai.com/de-DE/docs/hooks)
and the [Claude Code hook reference](https://code.claude.com/docs/en/hooks).
The hook accepts the provider's session ID from stdin and checks the foreground
process's ancestry and start time before registering it. A detached callback,
recycled pane, or hook running from a shared daemon outside the bound process
tree cannot register. The adapter skips unbound launches, including legacy
panes without a generation.
The `--workspace`, `--pane`, and `--epoch` flags also allow an explicit binding
for a launch that supplies its own integration. Background hooks can finish
out of order,
so the adapter supports synchronous starts and does not infer a thread switch
from an unrelated or delayed event. A Codex startup callback cannot replace
a conversation already registered by the same process. The fixture captures the event from the
real Codex app-server using a managed
hook confined to the disposable profile's mount namespace. Codex 0.169.0
emitted the event when the first turn began after resume. The fixture replays
only the event name, source and session ID from its bound process and verifies
that the resulting exact command resumes the real Codex thread. On the resumed
pane's first private turn, Codex also invokes the adapter directly and
registers the same ID against the new pane's generation. This validates the
per-pane, daemon-free path. Automatic hook setup and the shared daemon path
remain separate work.

The same adapter accepts Claude Code `SessionStart` events. Configure it for a
bound launch with a resume template matching that launch's model and settings.

```sh
starflux --agent-session-hook --provider claude -- \
  /path/to/claude --resume '{session_id}'
```

Claude Code can deliver SessionStart in the background. Once a bound Claude
process has registered a conversation, the adapter rejects a different ID
from that process. Use a new bound process to register another conversation.
See the [Claude Code hook reference](https://code.claude.com/docs/en/hooks).

Run `python3 scripts/test-claude-resume-offline.py /path/to/claude` to verify
provider startup and exact resume in a disposable profile. Claude Code 2.1.326
reported the same UUID on startup and when invoked with `--resume` in a new
process. This probe runs print mode with networking disabled. The interactive
Claude path through STAR/FLUX still requires end-to-end validation.

Pass `--starflux /absolute/path/to/private-launcher` to the Claude probe to
verify the STAR/FLUX cells resume action. A fixture supplies the source
registration, then STAR/FLUX launches real Claude Code by UUID in a new pane.
Claude's own callback registers the UUID against the new pane. The source
process remains alive. This test uses print mode. The interactive client
stopped at its connectivity check in the offline profile.

For an imported agent-multiplexer terminal, the tab menu's **Agent status / conversation**
view checks the live source and displays its pane ID and shell PID. The
diagnostic is read-only and reports when the source identity is unavailable.

The release gate can also run the offline provider probes. Set
`STARFLUX_TEST_CODEX_BIN` or `STARFLUX_TEST_CLAUDE_BIN` to the absolute path
of the provider executable. These opt-in checks use disposable profiles
and isolated networking. The Codex gate uses cells over loopback SSH.
The Claude Code gate uses print mode in cells.
