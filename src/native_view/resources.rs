//! Aggregate frontend payload admission. These are payload/working reservations,
//! not a promise about allocator overhead or total process RSS.
use crate::graphics_resources::{Budget, Lease};
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::{
    io::{self, BufRead, Write},
    sync::OnceLock,
};
pub(super) struct Resources {
    pub input: Budget,
    pub presentation: Budget,
    pub incoming: Budget,
    pub transport: Budget,
    pub connections: Budget,
}
pub(super) fn shared() -> &'static Resources {
    static RESOURCES: OnceLock<Resources> = OnceLock::new();
    RESOURCES.get_or_init(|| Resources {
        input: Budget::new(16 * 1024 * 1024),
        presentation: Budget::new(128 * 1024 * 1024),
        incoming: Budget::new(192 * 1024 * 1024),
        transport: Budget::new(32 * 1024 * 1024),
        connections: Budget::new(32),
    })
}
pub(super) struct Incoming {
    pub value: Value,
    _memory: Lease,
}
pub(super) fn read(reader: &mut impl BufRead, budget: &Budget) -> Result<Incoming> {
    let mut memory = budget
        .reserve(4096)
        .context("Native incoming messages are busy")?;
    let mut bytes = Vec::new();
    let mut quoted = false;
    let mut escaped = false;
    let mut nodes = 0usize;
    loop {
        let available = reader.fill_buf()?;
        ensure!(!available.is_empty(), "Native view connection ended");
        let count = available
            .iter()
            .position(|b| *b == b'\n')
            .map_or(available.len(), |n| n + 1);
        let length = bytes.len() + count;
        ensure!(length <= 4 * 1024 * 1024, "Native frame exceeds wire limit");
        // Charge raw growth/string copies separately from JSON structure. Long
        // PNG strings must not reserve forty times their size; tiny arrays and
        // objects still need admission for Value/container allocations.
        for byte in &available[..count] {
            if quoted {
                if escaped {
                    escaped = false;
                } else if *byte == b'\\' {
                    escaped = true;
                } else if *byte == b'"' {
                    quoted = false;
                }
            } else if *byte == b'"' {
                quoted = true;
                nodes += 1;
            } else if !byte.is_ascii_whitespace() {
                nodes += 1;
            }
        }
        ensure!(
            memory.try_resize(length * 4 + nodes * 128 + 4096),
            "Native incoming messages are busy"
        );
        bytes.extend_from_slice(&available[..count]);
        reader.consume(count);
        if bytes.last() == Some(&b'\n') {
            return Ok(Incoming {
                value: serde_json::from_slice(&bytes)?,
                _memory: memory,
            });
        }
    }
}
struct Counter(usize);
impl Write for Counter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let next = self.0.saturating_add(bytes.len());
        if next >= 2 * 1024 * 1024 {
            return Err(io::Error::other("Native request exceeds wire limit"));
        }
        self.0 = next;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub(super) fn encode(value: &Value, budget: &Budget) -> Result<(Vec<u8>, Lease)> {
    let mut counter = Counter(0);
    serde_json::to_writer(&mut counter, value)?;
    let memory = budget
        .reserve(counter.0 + 1)
        .context("Native outgoing messages are busy")?;
    let mut bytes = Vec::with_capacity(counter.0 + 1);
    serde_json::to_writer(&mut bytes, value)?;
    bytes.push(b'\n');
    Ok((bytes, memory))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ingress_is_admitted_before_json_and_releases_on_parse_failure() {
        let budget = Budget::new(4096 + 40 * 64);
        let message = read(&mut &b"{\"message\":\"hello\"}\n"[..], &budget).unwrap();
        assert_eq!(message.value["message"], "hello");
        assert!(budget.used() > 4096);
        drop(message);
        assert_eq!(budget.used(), 0);
        assert!(read(&mut &b"invalid json\n"[..], &budget).is_err());
        assert_eq!(budget.used(), 0);
        assert!(
            read(
                &mut &b"{\"message\":\"too large for this reservation\"}\n"[..],
                &Budget::new(4096)
            )
            .is_err()
        );
    }
    #[test]
    fn encoded_transport_stays_charged_until_packet_is_dropped() {
        let budget = Budget::new(64);
        let (bytes, lease) = encode(&serde_json::json!({"input":"a\nb"}), &budget).unwrap();
        assert_eq!(budget.used(), bytes.len());
        assert!(serde_json::from_slice::<Value>(&bytes).is_ok());
        assert!(encode(&serde_json::json!({"input":"x".repeat(64)}), &budget).is_err());
        drop(bytes);
        drop(lease);
        assert_eq!(budget.used(), 0);
    }
}
