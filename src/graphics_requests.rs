//! Source-owned graphics request lifecycle. Reserve response order at the first
//! chunk, retain reply identity across errors, and never leave a canceled upload
//! blocking subsequent terminal queries. No PTY or presenter is created here.
use crate::{
    graphics_reply::{self, Queue, QueueError, Ticket},
    graphics_resources::{Lease, Resources, UPLOAD_RESERVATION},
    graphics_store::Generation,
    graphics_transfer::{Assembler, Controls, Error, Transmission},
};
use std::time::{Duration, Instant};
const UPLOAD_IDLE: Duration = Duration::from_secs(30);
const UPLOAD_TOTAL: Duration = Duration::from_secs(300);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Failure {
    Queue(QueueError),
    Protocol(Error),
}
impl From<QueueError> for Failure {
    fn from(error: QueueError) -> Self {
        Self::Queue(error)
    }
}
impl From<Error> for Failure {
    fn from(error: Error) -> Self {
        Self::Protocol(error)
    }
}
pub struct Job {
    pub transmission: Transmission,
    ticket: Ticket,
    _upload: Lease,
}
struct Partial {
    ticket: Ticket,
    policy: Controls,
    upload: Lease,
    started: Instant,
    progress: Instant,
}
pub struct Requests {
    queue: Queue,
    assembler: Assembler,
    partial: Option<Partial>,
    resources: Resources,
}
impl Requests {
    pub fn new(generation: Generation) -> Self {
        Self::with_resources(generation, Resources::shared())
    }
    pub fn with_resources(generation: Generation, resources: Resources) -> Self {
        Self {
            queue: Queue::new(generation),
            assembler: Default::default(),
            partial: None,
            resources,
        }
    }
    pub fn reset(&mut self, generation: Generation) {
        self.assembler.cancel();
        self.partial = None;
        self.queue.reset(generation);
    }
    fn finish(
        &mut self,
        ticket: &Ticket,
        policy: &Controls,
        result: Result<(), graphics_reply::Failure>,
    ) -> Result<(), Failure> {
        match graphics_reply::graphics(policy, result) {
            Ok(bytes) => self.queue.complete(ticket, bytes).map_err(Into::into),
            Err(error) => {
                // A malformed/unimplemented reply identity must not wedge DA
                // forever. No guessed ID, path or arbitrary error text escapes.
                self.queue.complete(ticket, Vec::new())?;
                Err(error.into())
            }
        }
    }
    pub fn cancel(&mut self) -> Result<(), Failure> {
        self.abort(Error::Invalid)
    }
    fn abort(&mut self, error: Error) -> Result<(), Failure> {
        self.assembler.cancel();
        if let Some(partial) = self.partial.take() {
            self.finish(
                &partial.ticket,
                &partial.policy,
                Err(graphics_reply::Failure::Protocol(error)),
            )?;
        }
        Ok(())
    }
    /// The source owner ticks this even when no output arrives. Empty chunks
    /// cannot renew an upload, and trickling bytes cannot evade the total limit.
    pub fn expire(&mut self, now: Instant) -> Result<bool, Failure> {
        let expired = self.partial.as_ref().is_some_and(|partial| {
            now.saturating_duration_since(partial.progress) >= UPLOAD_IDLE
                || now.saturating_duration_since(partial.started) >= UPLOAD_TOTAL
        });
        if expired {
            self.abort(Error::Timeout)?;
        }
        Ok(expired)
    }
    pub fn push(&mut self, data: &[u8], cursor: (u16, u16)) -> Result<Option<Job>, Failure> {
        self.push_at(data, cursor, Instant::now())
    }
    fn push_at(
        &mut self,
        data: &[u8],
        cursor: (u16, u16),
        now: Instant,
    ) -> Result<Option<Job>, Failure> {
        if data.first() != Some(&b'G') {
            return Ok(None);
        }
        // An expired malformed reply identity may produce no usable response,
        // but its state has already been discarded; accept a fresh command.
        if let Err(error @ Failure::Queue(_)) = self.expire(now) {
            return Err(error);
        }
        let header = match Controls::from_apc(data) {
            Ok(header) => header,
            Err(error) => {
                self.cancel()?;
                return Err(error.into());
            }
        };
        if self.partial.is_none() && header.is_continuation() {
            return Ok(None);
        }
        if self.partial.is_some() && header.character(b'a', b't') == Ok(b'd') {
            self.cancel()?;
        }
        if self.partial.is_none() {
            let ticket = self.queue.reserve()?;
            let Some(upload) = self.resources.uploads.reserve(
                if matches!(header.character(b'a', b't'), Ok(b'd' | b'p')) {
                    0
                } else {
                    UPLOAD_RESERVATION
                },
            ) else {
                self.finish(
                    &ticket,
                    &header,
                    Err(graphics_reply::Failure::Protocol(Error::Busy)),
                )?;
                return Err(Error::Busy.into());
            };
            self.partial = Some(Partial {
                ticket,
                policy: header.clone(),
                upload,
                started: now,
                progress: now,
            });
        } else if header.is_continuation()
            && let Err(error) = self.partial.as_mut().unwrap().policy.inherit_quiet(&header)
        {
            self.cancel()?;
            return Err(error.into());
        }
        match self.assembler.push(data, cursor) {
            Ok(None) => {
                if data
                    .iter()
                    .position(|byte| *byte == b';')
                    .is_some_and(|index| index + 1 < data.len())
                {
                    self.partial.as_mut().unwrap().progress = now;
                }
                Ok(None)
            }
            Ok(Some(transmission)) => {
                let mut partial = self.partial.take().ok_or(Error::Invalid)?;
                partial.upload.shrink_to(transmission.allocated_bytes());
                Ok(Some(Job {
                    transmission,
                    ticket: partial.ticket,
                    _upload: partial.upload,
                }))
            }
            Err(error) => {
                self.assembler.cancel();
                if let Some(partial) = self.partial.take() {
                    self.finish(
                        &partial.ticket,
                        &partial.policy,
                        Err(graphics_reply::Failure::Protocol(error)),
                    )?;
                }
                Err(error.into())
            }
        }
    }
    pub fn complete(
        &mut self,
        job: &Job,
        result: Result<(), graphics_reply::Failure>,
    ) -> Result<(), Failure> {
        self.finish(&job.ticket, &job.transmission.controls, result)
    }
    pub fn ready(&mut self, bytes: Vec<u8>) -> Result<(), Failure> {
        let ticket = self.queue.reserve()?;
        if let Err(error) = self.queue.complete(&ticket, bytes) {
            self.queue.complete(&ticket, Vec::new())?;
            return Err(error.into());
        }
        Ok(())
    }
    pub fn drain(&mut self, limit: usize) -> Vec<u8> {
        self.queue.drain(limit)
    }
    pub fn remaining(&self) -> usize {
        self.queue.remaining()
    }
    pub fn pending(&self, job: &Job) -> bool {
        self.queue.pending(&job.ticket)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn identity() -> Generation {
        Generation {
            host: "private".into(),
            boot: uuid::Uuid::new_v4(),
            pid: 123,
            start_ticks: 1,
            epoch: uuid::Uuid::new_v4(),
        }
    }
    fn isolated() -> Requests {
        Requests::with_resources(
            identity(),
            Resources::new(128 * 1024 * 1024, 512 * 1024 * 1024),
        )
    }
    #[test]
    fn idle_expiry_releases_upload_memory_and_orders_timeout_before_da() {
        let resources = Resources::new(UPLOAD_RESERVATION, 512 * 1024 * 1024);
        let mut r = Requests::with_resources(identity(), resources.clone());
        let start = Instant::now();
        r.push_at(b"Gi=1,a=q,m=1;AAAA", (0, 0), start).unwrap();
        r.ready(DA.to_vec()).unwrap();
        assert!(!r.expire(start + Duration::from_secs(29)).unwrap());
        assert!(r.drain(8192).is_empty());
        assert!(r.expire(start + Duration::from_secs(30)).unwrap());
        assert_eq!(resources.uploads.used(), 0);
        assert_eq!(
            r.drain(8192),
            b"\x1b_Gi=1;ETIMEDOUT:Graphics upload timed out\x1b\\\x1b[?1;2c"
        );
        assert!(!r.expire(start + Duration::from_secs(60)).unwrap());
        assert!(
            r.push_at(b"Gm=0;AAAA", (0, 0), start + Duration::from_secs(61))
                .unwrap()
                .is_none()
        );
        assert_eq!(resources.uploads.used(), 0);
        assert!(r.drain(8192).is_empty());
        let fresh = r
            .push_at(b"Gi=2,a=q;AAAA", (0, 0), start + Duration::from_secs(61))
            .unwrap()
            .unwrap();
        r.complete(&fresh, Ok(())).unwrap();
        assert_eq!(r.drain(8192), b"\x1b_Gi=2;OK\x1b\\");
    }
    #[test]
    fn empty_chunks_do_not_renew_idle_deadline_and_preserve_quiet_policy() {
        let mut r = isolated();
        let start = Instant::now();
        r.push_at(b"Gi=1,a=q,m=1;AAAA", (0, 0), start).unwrap();
        r.push_at(b"Gm=1,q=2;", (0, 0), start + Duration::from_secs(29))
            .unwrap();
        r.ready(DA.to_vec()).unwrap();
        assert!(r.expire(start + Duration::from_secs(30)).unwrap());
        assert_eq!(r.drain(8192), DA);
    }
    #[test]
    fn active_trickle_still_expires_at_total_deadline() {
        let mut r = isolated();
        let start = Instant::now();
        r.push_at(b"Gi=1,a=q,m=1;AAAA", (0, 0), start).unwrap();
        for seconds in (29..300).step_by(29) {
            r.push_at(b"Gm=1;AAAA", (0, 0), start + Duration::from_secs(seconds))
                .unwrap();
        }
        assert!(!r.expire(start + Duration::from_secs(299)).unwrap());
        assert!(r.expire(start + Duration::from_secs(300)).unwrap());
        assert!(r.partial.is_none());
        assert_eq!(
            r.drain(8192),
            b"\x1b_Gi=1;ETIMEDOUT:Graphics upload timed out\x1b\\"
        );
    }
    #[test]
    fn old_ticks_and_source_reset_cannot_expire_a_fresh_upload() {
        let mut r = isolated();
        let start = Instant::now();
        r.push_at(b"Gi=1,a=q,m=1;AAAA", (0, 0), start).unwrap();
        r.reset(identity());
        r.push_at(
            b"Gi=2,a=q,m=1;AAAA",
            (0, 0),
            start + Duration::from_secs(40),
        )
        .unwrap();
        assert!(!r.expire(start + Duration::from_secs(30)).unwrap());
        assert!(!r.expire(start + Duration::from_secs(41)).unwrap());
        r.cancel().unwrap();
        assert_eq!(
            r.drain(8192),
            b"\x1b_Gi=2;EINVAL:Invalid graphics data\x1b\\"
        );
    }
    const DA: &[u8] = b"\x1b[?1;2c";
    #[test]
    fn first_chunk_reserves_order_even_when_da_precedes_the_final_chunk() {
        let mut r = isolated();
        assert!(
            r.push(b"Gi=31,a=q,f=24,s=1,v=1,m=1;", (0, 0))
                .unwrap()
                .is_none()
        );
        r.ready(DA.to_vec()).unwrap();
        assert!(r.drain(8192).is_empty());
        let job = r.push(b"Gm=0;AAAA", (2, 3)).unwrap().unwrap();
        assert_eq!(job.transmission.cursor, (2, 3));
        r.complete(&job, Ok(())).unwrap();
        assert_eq!(r.drain(8192), b"\x1b_Gi=31;OK\x1b\\\x1b[?1;2c");
    }
    #[test]
    fn malformed_continuation_reports_the_initial_identity_and_unblocks_da() {
        let mut r = isolated();
        r.push(b"Gi=31,a=q,m=1;AAAA", (0, 0)).unwrap();
        r.ready(DA.to_vec()).unwrap();
        assert!(matches!(
            r.push(b"Gm=0;====", (0, 0)),
            Err(Failure::Protocol(Error::Invalid))
        ));
        assert_eq!(
            r.drain(8192),
            b"\x1b_Gi=31;EINVAL:Invalid graphics data\x1b\\\x1b[?1;2c"
        );
        assert!(r.partial.is_none());
        let job = r.push(b"Gi=32,a=q;AAAA", (0, 0)).unwrap().unwrap();
        r.complete(&job, Ok(())).unwrap();
        assert_eq!(r.drain(8192), b"\x1b_Gi=32;OK\x1b\\");
    }
    #[test]
    fn cancellation_and_quiet_overrides_release_pending_query_barriers() {
        let mut r = isolated();
        r.push(b"Gi=1,a=q,m=1;AAAA", (0, 0)).unwrap();
        r.push(b"Gm=1,q=2;", (0, 0)).unwrap();
        r.ready(DA.to_vec()).unwrap();
        r.cancel().unwrap();
        assert_eq!(r.drain(8192), DA);
        assert!(r.partial.is_none());
        r.push(b"Gi=2,a=q,m=1,q=2;AAAA", (0, 0)).unwrap();
        r.push(b"Gm=1,q=0;", (0, 0)).unwrap();
        r.ready(DA.to_vec()).unwrap();
        r.cancel().unwrap();
        assert_eq!(
            r.drain(8192),
            b"\x1b_Gi=2;EINVAL:Invalid graphics data\x1b\\\x1b[?1;2c"
        );
    }
    #[test]
    fn delete_cancels_old_upload_and_gets_its_own_response_identity() {
        let mut r = isolated();
        r.push(b"Gi=1,a=q,m=1;AAAA", (0, 0)).unwrap();
        r.ready(DA.to_vec()).unwrap();
        let deletion = r.push(b"Ga=d,d=I,i=2", (0, 0)).unwrap().unwrap();
        r.complete(&deletion, Ok(())).unwrap();
        assert_eq!(
            r.drain(8192),
            b"\x1b_Gi=1;EINVAL:Invalid graphics data\x1b\\\x1b[?1;2c\x1b_Gi=2;OK\x1b\\"
        );
    }
    #[test]
    fn reset_rejects_old_jobs_and_drops_partial_source_state() {
        let mut r = isolated();
        let old = r.push(b"Gi=1,a=q;AAAA", (0, 0)).unwrap().unwrap();
        r.push(b"Gi=2,a=q,m=1;AAAA", (0, 0)).unwrap();
        r.reset(identity());
        assert_eq!(
            r.complete(&old, Ok(())),
            Err(Failure::Queue(QueueError::Stale))
        );
        r.ready(DA.to_vec()).unwrap();
        assert_eq!(r.drain(8192), DA);
        assert!(r.partial.is_none());
    }
    #[test]
    fn saturated_reply_queue_rejects_new_upload_before_buffering_it() {
        let mut r = isolated();
        for _ in 0..64 {
            r.ready(DA.to_vec()).unwrap();
        }
        assert!(matches!(
            r.push(b"Gi=1,a=q,m=1;AAAA", (0, 0)),
            Err(Failure::Queue(QueueError::Full))
        ));
        assert!(r.partial.is_none());
        assert_eq!(r.drain(8192).len(), 64 * DA.len());
        let job = r.push(b"Gi=2,a=q;AAAA", (0, 0)).unwrap().unwrap();
        r.complete(&job, Ok(())).unwrap();
        assert_eq!(r.drain(8192), b"\x1b_Gi=2;OK\x1b\\");
    }
    #[test]
    fn unrepresentable_reply_or_oversized_ready_message_does_not_wedge_later_queries() {
        let mut r = isolated();
        let job = r.push(b"GI=1,a=q;AAAA", (0, 0)).unwrap().unwrap();
        assert_eq!(
            r.complete(&job, Ok(())),
            Err(Failure::Protocol(Error::Unsupported))
        );
        r.ready(DA.to_vec()).unwrap();
        assert_eq!(r.drain(8192), DA);
        assert_eq!(
            r.ready(vec![0; 257]),
            Err(Failure::Queue(QueueError::TooLarge))
        );
        r.ready(DA.to_vec()).unwrap();
        assert_eq!(r.drain(8192), DA);
    }
    #[test]
    fn shared_upload_budget_releases_on_cancel_error_reset_and_job_drop() {
        let resources = Resources::new(UPLOAD_RESERVATION, 512 * 1024 * 1024);
        let mut a = Requests::with_resources(identity(), resources.clone());
        let mut b = Requests::with_resources(identity(), resources.clone());
        a.push(b"Gi=1,a=q,m=1;AAAA", (0, 0)).unwrap();
        assert_eq!(resources.uploads.used(), UPLOAD_RESERVATION);
        assert!(matches!(
            b.push(b"Gi=2,a=q,m=1;AAAA", (0, 0)),
            Err(Failure::Protocol(Error::Busy))
        ));
        b.ready(DA.to_vec()).unwrap();
        assert_eq!(
            b.drain(8192),
            b"\x1b_Gi=2;EBUSY:Graphics decoder busy\x1b\\\x1b[?1;2c"
        );
        a.cancel().unwrap();
        assert_eq!(resources.uploads.used(), 0);
        b.push(b"Gi=3,a=q,m=1;AAAA", (0, 0)).unwrap();
        b.reset(identity());
        assert_eq!(resources.uploads.used(), 0);
        assert!(b.push(b"Gi=4,a=q;====", (0, 0)).is_err());
        assert_eq!(resources.uploads.used(), 0);
        let job = b.push(b"Gi=5,a=q;AAAA", (0, 0)).unwrap().unwrap();
        assert_eq!(resources.uploads.used(), job.transmission.allocated_bytes());
        assert!(resources.uploads.used() > 0);
        drop(job);
        assert_eq!(resources.uploads.used(), 0);
    }
    #[test]
    fn no_upload_budget_is_needed_to_delete_or_place_existing_images() {
        let resources = Resources::new(0, 0);
        let mut r = Requests::with_resources(identity(), resources.clone());
        for body in [b"Ga=d,i=1".as_slice(), b"Ga=p,i=2"] {
            let job = r.push(body, (0, 0)).unwrap().unwrap();
            r.complete(&job, Ok(())).unwrap();
        }
        assert_eq!(resources.uploads.used(), 0);
        assert!(matches!(
            r.push(b"Ga=p,i=3;AAAA", (0, 0)),
            Err(Failure::Protocol(Error::Invalid))
        ));
        r.ready(DA.to_vec()).unwrap();
        assert!(!r.drain(8192).is_empty());
    }
}
