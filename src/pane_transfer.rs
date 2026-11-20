//! Move a live native terminal to another project on the same host.
//!
//! The process, its supervisor, its terminal epoch and its conversation records
//! stay the same; only which project owns the terminal changes. The durable
//! handoff is ordered so at most one project holds a usable binding: the
//! destination model records the moved binding first, the host then moves the
//! authoritative broker slot, and only then does the source model drop the pane.
//! An interruption leaves the project whose slot is missing reporting an error
//! instead of two projects driving one terminal.
//!
//! tmux terminals live on their project's own tmux server, and imported,
//! adopted and plugin terminals keep their owners, so only native terminals move.
use crate::{project_state::ProjectState, runtime::Config};
use anyhow::{Context, Result, ensure};

pub fn move_to_project(from: &Config, pane: usize, to: &Config) -> Result<usize> {
    ensure!(
        from.workspace != to.workspace,
        "The terminal is already in this project"
    );
    ensure!(
        from.host == to.host && from.ssh_config == to.ssh_config,
        "Terminals can only move between projects on the same host"
    );
    ensure!(
        !from.externally_owned(pane)
            && from.adopted_terminal(pane).is_none()
            && crate::plugin_panes::owner(from, pane)?.is_none(),
        "Imported, transferred and plugin terminals keep their owner and cannot move"
    );
    let reference = crate::native_backend::reference(from, pane)?.context(
        "Only native terminals can move between projects; tmux terminals stay on their project's server",
    )?;
    // Both models are locked for the whole handoff, always in name order.
    let (_first, _second) = if from.workspace < to.workspace {
        let first = crate::project_sync::transaction(from)?;
        (first, crate::project_sync::transaction(to)?)
    } else {
        let first = crate::project_sync::transaction(to)?;
        (first, crate::project_sync::transaction(from)?)
    };
    let mut source = ProjectState::load(from)?;
    let epoch = source
        .terminal_epochs
        .get(&pane)
        .cloned()
        .context("Terminal has no generation")?;
    ensure!(
        source.native_sources.get(&pane) == Some(&reference)
            && reference.binding.terminal_epoch == epoch,
        "Terminal changed before the move"
    );
    let mut destination = ProjectState::load(to)?;
    let target = destination.add_terminal()?;
    ensure!(
        from.remote(pane) == to.remote(target),
        "Terminals can only move between projects on the same host"
    );
    destination.terminal_epochs.insert(target, epoch.clone());
    if let Some(label) = source.terminal_labels.get(&pane) {
        destination.terminal_labels.insert(target, label.clone());
    }
    let mut moved = reference.binding.clone();
    moved.project = to.workspace.clone();
    moved.pane = target;
    destination.native_sources.insert(
        target,
        crate::native_backend::Pane {
            namespace: reference.namespace.clone(),
            binding: moved.clone(),
            previous: None,
        },
    );
    destination.validate()?;
    destination.save(to)?;
    let destination_pane = crate::native_backend::Pane {
        namespace: reference.namespace.clone(),
        binding: moved.clone(),
        previous: None,
    };
    let transferred = crate::native_backend::rpc(
        from,
        pane,
        serde_json::json!({
            "op": "transfer",
            "target": reference,
            "project": to.workspace,
            "pane": target,
        }),
    )
    .and_then(|reply| {
        Ok(serde_json::from_value::<crate::native_backend::Pane>(
            reply,
        )?)
    });
    match transferred {
        Ok(transferred) => ensure!(
            transferred.binding == moved,
            "Host moved the terminal to an unexpected slot"
        ),
        Err(error) => {
            // The reply is uncertain. Only the host's slot record decides who
            // owns the terminal now, so ask it before choosing either project.
            let landed = crate::native_backend::rpc(
                to,
                target,
                serde_json::json!({"op": "reattach", "target": destination_pane}),
            )
            .is_ok();
            if !landed {
                destination.remove_terminal(target);
                destination.save(to)?;
                return Err(error.context("The terminal was not moved"));
            }
        }
    }
    source.remove_terminal(pane);
    source.save(from)?;
    crate::agent_state::transfer(from, pane, to, target, &epoch)?;
    // Written before returning: an API process exits as soon as the move does,
    // which would lose an event queued on a background thread.
    let _ = crate::automation::journal::occurred(
        &format!("panes/{}/{}", to.workspace, target + 1),
        "moved",
        serde_json::json!({
            "project": to.workspace, "pane": target + 1, "epoch": epoch,
            "from": {"project": from.workspace, "pane": pane + 1},
        }),
    );
    Ok(target)
}
