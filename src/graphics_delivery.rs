//! Latest-per-view local image delivery. Media/control forwarding never waits
//! for PNG presentation; queues retain at most 16 MiB and 64 view identities.
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::{
    collections::{BTreeMap, VecDeque},
    io::{Read, Write},
    os::unix::net::UnixStream,
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};
struct Pending {
    values: BTreeMap<String, (Value, usize)>,
    order: VecDeque<String>,
    bytes: usize,
    stopped: bool,
    error: Option<String>,
}
impl Pending {
    fn new() -> Self {
        Self {
            values: BTreeMap::new(),
            order: VecDeque::new(),
            bytes: 0,
            stopped: false,
            error: None,
        }
    }
    fn submit(&mut self, value: Value) -> Result<()> {
        ensure!(!self.stopped, "Native image delivery stopped");
        let view = value["view"]
            .as_str()
            .context("Missing native image view")?
            .to_owned();
        ensure!(
            !view.is_empty()
                && view.len() <= 96
                && view
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)),
            "Invalid native image view"
        );
        ensure!(
            value["op"] == "planes" || value["op"] == "clear",
            "Invalid native image operation"
        );
        let charge = serde_json::to_vec(&value)?.len();
        ensure!(
            charge <= 4 * 1024 * 1024,
            "Native image request exceeds limit"
        );
        let previous = self.values.get(&view).map_or(0, |(_, n)| *n);
        ensure!(
            self.bytes - previous + charge <= 16 * 1024 * 1024,
            "Native image delivery queue is busy"
        );
        ensure!(
            self.values.contains_key(&view) || self.values.len() < 64,
            "Too many queued image views"
        );
        if previous == 0 {
            self.order.push_back(view.clone());
        }
        self.bytes = self.bytes - previous + charge;
        self.values.insert(view, (value, charge));
        Ok(())
    }
    fn take(&mut self) -> Option<Value> {
        let view = self.order.pop_front()?;
        let (value, charge) = self.values.remove(&view)?;
        self.bytes -= charge;
        Some(value)
    }
}
pub struct Delivery {
    state: Arc<(Mutex<Pending>, Condvar)>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Delivery {
    pub fn new(upstream: &Arc<Mutex<UnixStream>>) -> Result<Self> {
        let shutdown = upstream.lock().unwrap().try_clone()?;
        let state = Arc::new((Mutex::new(Pending::new()), Condvar::new()));
        let worker = state.clone();
        let thread = std::thread::spawn(move || {
            loop {
                let request = {
                    let (lock, wake) = &*worker;
                    let mut state = lock.lock().unwrap();
                    while state.values.is_empty() && !state.stopped {
                        state = wake.wait(state).unwrap();
                    }
                    if state.stopped {
                        return;
                    }
                    state.take().unwrap()
                };
                let result = (|| -> Result<()> {
                    let path = std::env::var_os("STARFLUX_FONT_OUTPUT")
                        .context("Local image output is unavailable")?;
                    let mut output = UnixStream::connect(path)?;
                    output.set_read_timeout(Some(Duration::from_secs(3)))?;
                    output.set_write_timeout(Some(Duration::from_secs(3)))?;
                    serde_json::to_writer(&mut output, &request)?;
                    output.write_all(b"\n")?;
                    let mut accepted = [0];
                    output.read_exact(&mut accepted)?;
                    ensure!(
                        accepted[0] == 1,
                        "Local native image presenter rejected the frame"
                    );
                    Ok(())
                })();
                if let Err(error) = result {
                    let mut state = worker.0.lock().unwrap();
                    state.error = Some(format!("{error:#}"));
                    state.stopped = true;
                    state.values.clear();
                    state.order.clear();
                    state.bytes = 0;
                    let _ = shutdown.shutdown(std::net::Shutdown::Both);
                    return;
                }
            }
        });
        Ok(Self {
            state,
            thread: Some(thread),
        })
    }
    pub fn submit(&self, request: Value) -> Result<()> {
        self.state.0.lock().unwrap().submit(request)?;
        self.state.1.notify_one();
        Ok(())
    }
    pub fn error(&self) -> Option<String> {
        self.state.0.lock().unwrap().error.clone()
    }
}
impl Drop for Delivery {
    fn drop(&mut self) {
        let mut state = self.state.0.lock().unwrap();
        state.stopped = true;
        state.values.clear();
        state.order.clear();
        state.bytes = 0;
        drop(state);
        self.state.1.notify_one();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replacing_a_view_keeps_latest_clear_and_does_not_starve_other_views() {
        let mut state = Pending::new();
        state
            .submit(serde_json::json!({"op":"planes","view":"first","sequence":1}))
            .unwrap();
        state
            .submit(serde_json::json!({"op":"planes","view":"second","sequence":1}))
            .unwrap();
        for sequence in 2..50 {
            state
                .submit(serde_json::json!({"op":"planes","view":"first","sequence":sequence}))
                .unwrap();
        }
        state
            .submit(serde_json::json!({"op":"clear","view":"first"}))
            .unwrap();
        assert_eq!(state.values.len(), 2);
        assert_eq!(state.take().unwrap()["op"], "clear");
        assert_eq!(state.take().unwrap()["view"], "second");
        assert_eq!(state.bytes, 0);
    }
    #[test]
    fn an_oversized_replacement_keeps_the_last_admitted_request() {
        let mut state = Pending::new();
        state
            .submit(serde_json::json!({"op":"clear","view":"first"}))
            .unwrap();
        let bytes = state.bytes;
        assert!(state.submit(serde_json::json!({"op":"planes","view":"first","payload":"x".repeat(4*1024*1024)})).is_err());
        assert_eq!(state.bytes, bytes);
        assert_eq!(state.take().unwrap()["op"], "clear");
    }
}
