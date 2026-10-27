//! Serialized model edits and three-way merging for independent presentation views.
use anyhow::Result;
use serde_json::Value;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    fs::{self, File},
    os::unix::{fs::OpenOptionsExt, io::AsRawFd},
    path::PathBuf,
};
thread_local! { static LOCKS: RefCell<BTreeMap<PathBuf,(File,usize)>> = const {RefCell::new(BTreeMap::new())}; }
pub struct Transaction(PathBuf, std::marker::PhantomData<std::rc::Rc<()>>);
impl Transaction {
    pub fn acquire(path: PathBuf) -> Result<Self> {
        LOCKS.with(|locks| -> Result<()> {
            let mut locks = locks.borrow_mut();
            if let Some((_, count)) = locks.get_mut(&path) {
                *count += 1;
                return Ok(());
            }
            let file = fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .mode(0o600)
                .open(&path)?;
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            locks.insert(path.clone(), (file, 1));
            Ok(())
        })?;
        Ok(Self(path, std::marker::PhantomData))
    }
}
impl Drop for Transaction {
    fn drop(&mut self) {
        LOCKS.with(|locks| {
            let mut locks = locks.borrow_mut();
            if let Some((_, count)) = locks.get_mut(&self.0) {
                *count -= 1;
                if *count == 0 {
                    locks.remove(&self.0);
                }
            }
        });
    }
}
pub fn transaction(config: &crate::runtime::Config) -> Result<Transaction> {
    Transaction::acquire(
        crate::project_state::data_root()?.join(format!("{}.model.lock", config.workspace)),
    )
}
pub const SHARED: &[&str] = &[
    "terminal_first",
    "reserved_slots",
    "terminals",
    "terminal_epochs",
    "terminal_labels",
    "agents",
    "agent_labels",
    "app_command",
    "app_targets",
    "app_layout",
];
pub const VIEW: &[&str] = &[
    "space",
    "terminal",
    "split",
    "split_panes",
    "split_ratio",
    "agent",
    "terminal_font_size",
    "terminal_font_explicit",
];
pub fn merge(base: &Value, local: &Value, latest: &Value) -> Value {
    if base.is_null() {
        return local.clone();
    }
    let mut result = latest.clone();
    for key in SHARED {
        let (before, after, current) = (&base[*key], &local[*key], &latest[*key]);
        if before == after {
            continue;
        }
        result[*key] = match (before, after, current) {
            (Value::Object(b), Value::Object(a), Value::Object(c)) => {
                let mut merged = c.clone();
                for key in b.keys().chain(a.keys()) {
                    if b.get(key) != a.get(key) {
                        if let Some(v) = a.get(key) {
                            merged.insert(key.clone(), v.clone());
                        } else {
                            merged.remove(key);
                        }
                    }
                }
                Value::Object(merged)
            }
            (Value::Array(b), Value::Array(a), Value::Array(c)) if *key != "app_layout" => {
                let mut merged = c
                    .iter()
                    .filter(|v| !b.contains(v) || a.contains(v))
                    .cloned()
                    .collect::<Vec<_>>();
                for v in a {
                    if !b.contains(v) && !merged.contains(v) {
                        merged.push(v.clone())
                    }
                }
                Value::Array(merged)
            }
            _ => after.clone(),
        };
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn stale_view_cannot_erase_a_new_terminal_or_another_windows_label() {
        let base =
            json!({"terminals":[0],"terminal_labels":{"0":"Shell"},"terminal":0,"split":false});
        let mut local = base.clone();
        local["terminal_labels"]["0"] = json!("Codex");
        local["split"] = json!(true);
        let mut latest = base.clone();
        latest["terminals"] = json!([0, 1]);
        latest["terminal_labels"]["1"] = json!("Build");
        let result = merge(&base, &local, &latest);
        assert_eq!(result["terminals"], json!([0, 1]));
        assert_eq!(result["terminal_labels"], json!({"0":"Codex","1":"Build"}));
        assert_eq!(result["split"], false);
    }
    #[test]
    fn a_removed_terminal_stays_removed_when_a_stale_view_saves() {
        let base = json!({"terminals":[0,1]});
        assert_eq!(
            merge(&base, &base, &json!({"terminals":[1]}))["terminals"],
            json!([1])
        );
    }
    #[test]
    fn transactions_are_reentrant_and_serialize_threads() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("lock");
        let guard = Transaction::acquire(path.clone()).unwrap();
        drop(Transaction::acquire(path.clone()).unwrap());
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _guard = Transaction::acquire(path).unwrap();
            tx.send(()).unwrap();
        });
        assert!(
            rx.recv_timeout(std::time::Duration::from_millis(30))
                .is_err()
        );
        drop(guard);
        rx.recv_timeout(std::time::Duration::from_secs(1)).unwrap();
        worker.join().unwrap();
    }
}
