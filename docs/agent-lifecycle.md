# Agent identity, health and recovery

Right-click a terminal tab and choose **Agent status / conversation**. From the keyboard, press Ctrl+B, then x to open the pane menu. The status view separates terminal availability, the foreground agent process, its backend connection, and task lifecycle. An alive shell or agent process does not establish that its server connection is healthy. Backend health stays `unknown` unless a failure is explicitly observed or reported.

The known “Automatic reconnect could not restore this session” and “Server connection could not be restored” footers produce `backend_unavailable`, rather than idle. Affected projects get an error marker. Output detection is a heuristic; the inspector labels it accordingly and does not export transcript text.

Integrations can register an exact native conversation reference and a native resume argument vector. **Resume saved conversation in new tab** uses that registered vector and retains the original pane and process. It never guesses a conversation by directory, title, modification time or “last session.” Without a registered resume vector, this action is absent. **Recover to shell…** remains separate and requires the existing confirmation before stopping the foreground program.

## Structured reports

Run these commands on the workspace controller machine, including through SSH when that controller is remote:

```sh
starflux --workspace PROJECT --agent-status
starflux --workspace PROJECT --pane 1 --agent-report '{...}'
```

`--agent-status` lists all project terminals, layered health and archived run records. `--pane` and the status list use one-based pane numbers. Saved records and UI snapshots retain zero-based internal pane IDs. Inspecting status does not start, stop or replace agents.

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

Reports are rejected when the terminal generation, foreground process identity (including process start ticks and host boot UUID), harness or sequence does not match. Updating an older superseded run is rejected. To register a different conversation in the same agent process, start a new run at sequence 1 and set `replaces_run` to the current run UUID; this prevents delayed registrations from silently replacing a newer run.

Records are private metadata in `$XDG_DATA_HOME/starflux/projects/PROJECT.agents.json` (normally `~/.local/share/starflux/projects/`). Previous runs remain archived when a new run starts. Saved resume commands are shell-quoted argument vectors, scoped to the project's host and directory, and run only after an explicit resume action. They are retained with their new terminal across frontend reattachment.

This interface is implemented and tested with disposable agent fixtures. Automatic provider-specific hook installation and conversation discovery for existing Codex/Claude/etc. processes remain part of the parity work; existing live sessions are not modified to install hooks. STAR/FLUX can detect their process and screen health without a structured report, but cannot promise exact conversation recovery until their identity is registered.

A saved report from a previous boot cannot establish that a current agent is the same process, even if its PID and start ticks coincide. Legacy records without a boot identity remain readable and retain their explicit conversation/resume references, but are never authoritative live lifecycle reports. New reports must include the current boot identity; missing or stale identities are rejected.

Explicit resume uses a durable one-shot launch receipt. A later missing backend opens a shell rather than executing the saved resume hint again. See [cold-start recovery and saved screens](cold-start.md).
