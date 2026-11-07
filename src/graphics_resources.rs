//! Shared graphics payload accounting for one backend process: uploads,
//! decode/composition work, and retained pixels. These are not total RSS limits.
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicUsize, Ordering},
};
pub const UPLOAD_RESERVATION: usize = 32 * 1024 * 1024;
pub const DECODE_RESERVATION: usize = 160 * 1024 * 1024;
struct Counter {
    limit: usize,
    used: AtomicUsize,
}
#[derive(Clone)]
pub struct Budget(Arc<Counter>);
pub struct Lease {
    counter: Arc<Counter>,
    bytes: usize,
}
impl Budget {
    pub fn new(limit: usize) -> Self {
        Self(Arc::new(Counter {
            limit,
            used: AtomicUsize::new(0),
        }))
    }
    pub fn used(&self) -> usize {
        self.0.used.load(Ordering::Acquire)
    }
    pub fn reserve(&self, bytes: usize) -> Option<Lease> {
        self.0
            .used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes)
                    .filter(|total| *total <= self.0.limit)
            })
            .ok()?;
        Some(Lease {
            counter: self.0.clone(),
            bytes,
        })
    }
}
impl Lease {
    /// Atomic replacement admission. Existing stored bytes remain charged if
    /// growth fails; shrinking or dropping always releases exactly our charge.
    pub fn try_resize(&mut self, bytes: usize) -> bool {
        if bytes <= self.bytes {
            self.shrink_to(bytes);
            return true;
        }
        let additional = bytes - self.bytes;
        if self
            .counter
            .used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(additional)
                    .filter(|used| *used <= self.counter.limit)
            })
            .is_err()
        {
            return false;
        }
        self.bytes = bytes;
        true
    }
    /// Uploads reserve for worst-case vector growth before allocation. Completed
    /// uploads/results reduce their charge while retaining ownership of bytes.
    pub fn shrink_to(&mut self, bytes: usize) {
        assert!(
            bytes <= self.bytes,
            "graphics reservation cannot grow without admission"
        );
        self.counter
            .used
            .fetch_sub(self.bytes - bytes, Ordering::AcqRel);
        self.bytes = bytes;
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.counter.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}
#[derive(Clone)]
pub struct Resources {
    pub uploads: Budget,
    pub working: Budget,
    pub retained: Budget,
    pub presentation: Budget,
    active: Arc<AtomicUsize>,
}
pub struct WorkerPermit(Arc<AtomicUsize>);
impl Drop for WorkerPermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
impl Resources {
    pub fn new(upload_limit: usize, working_limit: usize) -> Self {
        Self {
            uploads: Budget::new(upload_limit),
            working: Budget::new(working_limit),
            retained: Budget::new(512 * 1024 * 1024),
            presentation: Budget::new(128 * 1024 * 1024),
            active: Arc::new(AtomicUsize::new(0)),
        }
    }
    pub fn shared() -> Self {
        static SHARED: OnceLock<Resources> = OnceLock::new();
        SHARED
            .get_or_init(|| Self::new(128 * 1024 * 1024, 512 * 1024 * 1024))
            .clone()
    }
    pub fn worker(&self) -> Option<WorkerPermit> {
        self.active
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
                (active < 2).then_some(active + 1)
            })
            .ok()?;
        Some(WorkerPermit(self.active.clone()))
    }
    pub fn active_decoders(&self) -> usize {
        self.active.load(Ordering::Acquire)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resizing_existing_charge_is_atomic_and_failed_growth_preserves_capacity() {
        let budget = Budget::new(12);
        let mut first = budget.reserve(4).unwrap();
        let second = budget.reserve(8).unwrap();
        assert!(!first.try_resize(5));
        assert_eq!(budget.used(), 12);
        assert!(first.try_resize(4));
        assert_eq!(budget.used(), 12);
        drop(second);
        assert!(first.try_resize(12));
        assert_eq!(budget.used(), 12);
        assert!(!first.try_resize(usize::MAX));
        assert_eq!(budget.used(), 12);
        assert!(first.try_resize(3));
        assert_eq!(budget.used(), 3);
        drop(first);
        assert_eq!(budget.used(), 0);
    }
    #[test]
    fn all_resource_clones_share_two_decoder_slots() {
        let resources = Resources::new(0, 0);
        let other = resources.clone();
        let first = resources.worker().unwrap();
        let second = other.worker().unwrap();
        assert_eq!(resources.active_decoders(), 2);
        assert!(other.worker().is_none());
        drop(first);
        assert_eq!(other.active_decoders(), 1);
        let third = resources.worker().unwrap();
        drop(second);
        drop(third);
        assert_eq!(resources.active_decoders(), 0);
    }
    #[test]
    fn lease_drop_and_shrink_release_exact_bytes_across_clones() {
        let budget = Budget::new(12);
        let other = budget.clone();
        let mut first = budget.reserve(8).unwrap();
        assert!(other.reserve(5).is_none());
        first.shrink_to(3);
        let second = other.reserve(9).unwrap();
        assert_eq!(budget.used(), 12);
        drop(first);
        assert_eq!(other.used(), 9);
        drop(second);
        assert_eq!(budget.used(), 0);
        let _zero = budget.reserve(0).unwrap();
        assert!(budget.reserve(usize::MAX).is_none());
    }
    #[test]
    fn concurrent_reservations_never_oversubscribe_or_leak() {
        let budget = Budget::new(32);
        let peak = Arc::new(AtomicUsize::new(0));
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let budget = budget.clone();
                let peak = peak.clone();
                scope.spawn(move || {
                    for _ in 0..200 {
                        if let Some(lease) = budget.reserve(8) {
                            peak.fetch_max(budget.used(), Ordering::AcqRel);
                            std::thread::yield_now();
                            drop(lease);
                        }
                    }
                });
            }
        });
        assert!(peak.load(Ordering::Acquire) <= 32);
        assert_eq!(budget.used(), 0);
    }
}
