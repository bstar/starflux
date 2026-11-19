//! Programs from the host (tmux, python3 and, through tmux, every shell and
//! agent) must not inherit the private library path `scripts/run-local.sh`
//! sets for STAR/FLUX's own graphics runtime. On a host whose glibc is older
//! than the Nix one, a host program that loads those libraries fails to start
//! (for example tmux loading Nix's libsystemd). `run-local.sh` records the
//! caller's original value in [`ORIGINAL`].
use std::{ffi::OsStr, process::Command};

pub const ORIGINAL: &str = "STARFLUX_HOST_LD_LIBRARY_PATH";

/// A command for a host program, with the caller's original library path.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    if let Some(original) = std::env::var_os(ORIGINAL) {
        if original.is_empty() {
            command.env_remove("LD_LIBRARY_PATH");
        } else {
            command.env("LD_LIBRARY_PATH", original);
        }
    }
    command
}

#[cfg(test)]
mod tests {
    #[test]
    fn host_programs_get_the_original_library_path() {
        // Inspect only the configured environment; nothing is spawned.
        let command = super::command("tmux");
        let configured: Vec<_> = command.get_envs().collect();
        if std::env::var_os(super::ORIGINAL).is_none() {
            assert!(configured.is_empty());
        } else {
            assert!(configured.iter().any(|(key, _)| *key == "LD_LIBRARY_PATH"));
        }
    }
}
