//! Ordered responses for a single source owner. A presenter must not construct
//! its own competing response queue. This module never writes to a PTY.
use crate::{
    graphics_store::Generation,
    graphics_transfer::{Controls, Error},
};
use std::collections::VecDeque;
const MAX_SLOTS: usize = 64;
const MAX_REPLY: usize = 256;
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Ticket {
    generation: Generation,
    serial: u64,
}
struct Slot {
    serial: u64,
    bytes: Option<Vec<u8>>,
}
pub struct Queue {
    generation: Generation,
    serial: u64,
    slots: VecDeque<Slot>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueError {
    Full,
    TooLarge,
    Stale,
    AlreadyCompleted,
}
impl Queue {
    pub fn new(generation: Generation) -> Self {
        Self {
            generation,
            serial: 0,
            slots: VecDeque::new(),
        }
    }
    pub fn reset(&mut self, generation: Generation) {
        self.generation = generation;
        self.slots.clear();
    }
    pub fn reserve(&mut self) -> Result<Ticket, QueueError> {
        if self.slots.len() >= MAX_SLOTS {
            return Err(QueueError::Full);
        }
        self.serial = self.serial.checked_add(1).ok_or(QueueError::Full)?;
        self.slots.push_back(Slot {
            serial: self.serial,
            bytes: None,
        });
        Ok(Ticket {
            generation: self.generation.clone(),
            serial: self.serial,
        })
    }
    pub fn remaining(&self) -> usize {
        MAX_SLOTS - self.slots.len()
    }
    pub fn pending(&self, ticket: &Ticket) -> bool {
        ticket.generation == self.generation
            && self
                .slots
                .iter()
                .any(|s| s.serial == ticket.serial && s.bytes.is_none())
    }
    pub fn complete(&mut self, ticket: &Ticket, bytes: Vec<u8>) -> Result<(), QueueError> {
        if ticket.generation != self.generation {
            return Err(QueueError::Stale);
        }
        if bytes.len() > MAX_REPLY {
            return Err(QueueError::TooLarge);
        }
        let slot = self
            .slots
            .iter_mut()
            .find(|s| s.serial == ticket.serial)
            .ok_or(QueueError::Stale)?;
        if slot.bytes.is_some() {
            return Err(QueueError::AlreadyCompleted);
        }
        slot.bytes = Some(bytes);
        Ok(())
    }
    /// Consumes only a completed prefix. An immediate DA response waits behind
    /// an unresolved preceding graphics query even if its decoder finishes late.
    pub fn drain(&mut self, limit: usize) -> Vec<u8> {
        let mut bytes = Vec::new();
        while let Some(slot) = self.slots.front() {
            let Some(reply) = slot.bytes.as_ref() else {
                break;
            };
            if bytes.len() + reply.len() > limit {
                break;
            }
            bytes.extend_from_slice(reply);
            self.slots.pop_front();
        }
        bytes
    }
}
#[derive(Clone, Copy, Debug)]
pub enum Failure {
    Protocol(Error),
    Missing,
}
impl Failure {
    fn message(self) -> &'static str {
        match self {
            Self::Protocol(error) => error.message(),
            Self::Missing => "ENOENT:Image not found",
        }
    }
}
pub fn graphics(controls: &Controls, result: Result<(), Failure>) -> Result<Vec<u8>, Error> {
    let quiet = controls.number(b'q', 0)?;
    if quiet > 2 {
        return Err(Error::Invalid);
    }
    if quiet == 2 || (quiet == 1 && result.is_ok()) {
        return Ok(Vec::new());
    }
    if controls.has(b'I') {
        return Err(Error::Unsupported);
    }
    let id = controls.number(b'i', 0)?;
    if id == 0 {
        return Ok(Vec::new());
    }
    let placement = controls.number(b'p', 0)?;
    let mut response = format!("\x1b_Gi={id}");
    if placement != 0 {
        response.push_str(&format!(",p={placement}"));
    }
    response.push(';');
    response.push_str(result.map_or_else(Failure::message, |_| "OK"));
    response.push_str("\x1b\\");
    Ok(response.into_bytes())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics_transfer::Assembler;
    fn generation() -> Generation {
        Generation {
            host: "proof".into(),
            boot: uuid::Uuid::new_v4(),
            pid: 123,
            start_ticks: 1,
            epoch: uuid::Uuid::new_v4(),
        }
    }
    fn controls(header: &str) -> Controls {
        Assembler::default()
            .push(format!("G{header};").as_bytes(), (0, 0))
            .unwrap()
            .unwrap()
            .controls
    }
    #[test]
    fn query_reply_precedes_following_da_despite_reversed_job_completion() {
        let mut queue = Queue::new(generation());
        let query = queue.reserve().unwrap();
        let da = queue.reserve().unwrap();
        queue.complete(&da, b"\x1b[?1;2c".to_vec()).unwrap();
        assert!(queue.drain(8192).is_empty());
        let reply = graphics(&controls("a=q,i=31"), Ok(())).unwrap();
        queue.complete(&query, reply).unwrap();
        assert_eq!(queue.drain(8192), b"\x1b_Gi=31;OK\x1b\\\x1b[?1;2c");
        assert!(queue.drain(8192).is_empty());
        assert_eq!(queue.complete(&query, Vec::new()), Err(QueueError::Stale));
    }
    #[test]
    fn old_workers_cannot_complete_new_source_or_reused_source_tickets() {
        let old = generation();
        let mut queue = Queue::new(old.clone());
        let ticket = queue.reserve().unwrap();
        queue.reset(old);
        let fresh = queue.reserve().unwrap();
        assert_ne!(ticket.serial, fresh.serial);
        assert_eq!(
            queue.complete(&ticket, b"OLD".to_vec()),
            Err(QueueError::Stale)
        );
        queue.reset(generation());
        assert_eq!(
            queue.complete(&fresh, b"OLD".to_vec()),
            Err(QueueError::Stale)
        );
        assert!(queue.drain(8192).is_empty());
    }
    #[test]
    fn reply_bounds_duplicate_completion_and_partial_drain_preserve_order() {
        let mut queue = Queue::new(generation());
        let a = queue.reserve().unwrap();
        let b = queue.reserve().unwrap();
        assert_eq!(
            queue.complete(&a, vec![0; MAX_REPLY + 1]),
            Err(QueueError::TooLarge)
        );
        queue.complete(&a, b"ONE".to_vec()).unwrap();
        assert_eq!(
            queue.complete(&a, b"BAD".to_vec()),
            Err(QueueError::AlreadyCompleted)
        );
        queue.complete(&b, b"TWO".to_vec()).unwrap();
        assert!(queue.drain(2).is_empty());
        assert_eq!(queue.drain(3), b"ONE");
        assert_eq!(queue.drain(4), b"TWO");
        for _ in 0..MAX_SLOTS {
            queue.reserve().unwrap();
        }
        assert_eq!(queue.reserve(), Err(QueueError::Full));
        assert!(queue.drain(8192).is_empty());
    }
    #[test]
    fn quiet_unnamed_missing_and_error_responses_are_protocol_bytes_only() {
        assert_eq!(
            graphics(&controls("i=4294967295,p=4294967295"), Ok(())).unwrap(),
            b"\x1b_Gi=4294967295,p=4294967295;OK\x1b\\"
        );
        assert!(graphics(&controls("i=1,q=1"), Ok(())).unwrap().is_empty());
        assert!(
            graphics(&controls("i=1,q=2"), Err(Failure::Missing))
                .unwrap()
                .is_empty()
        );
        assert!(graphics(&controls("i=1,q=2"), Ok(())).unwrap().is_empty());
        assert_eq!(
            graphics(&controls("i=1,q=1"), Err(Failure::Missing)).unwrap(),
            b"\x1b_Gi=1;ENOENT:Image not found\x1b\\"
        );
        assert_eq!(
            graphics(&controls("i=1"), Err(Failure::Protocol(Error::Unsupported))).unwrap(),
            b"\x1b_Gi=1;ENOTSUP:Unsupported graphics operation\x1b\\"
        );
        assert!(graphics(&controls("a=t"), Ok(())).unwrap().is_empty());
    }
}
