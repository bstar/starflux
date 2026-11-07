//! Two decoder workers and a bounded input/completion queue. Decoding happens
//! outside the presenter. No worker writes responses or commits source state.
use crate::{
    graphics_requests::Job,
    graphics_resources::{DECODE_RESERVATION, Lease, Resources},
    graphics_transfer::{Error, Pixels},
};
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded};
use std::time::Duration;
const WORKERS: usize = 2;
const QUEUED: usize = 8;
struct Work {
    tag: u64,
    job: Job,
}
pub struct Decoded {
    pub pixels: Pixels,
    _working: Lease,
}
pub struct Completion {
    pub tag: u64,
    pub job: Job,
    pub result: Result<Decoded, Error>,
}
pub struct Pool {
    input: Option<Sender<Work>>,
    output: Option<Receiver<Completion>>,
}
impl Pool {
    pub fn new(resources: Resources) -> Self {
        let (input, work) = bounded::<Work>(QUEUED);
        let (results, output) = bounded(QUEUED);
        for _ in 0..WORKERS {
            let work = work.clone();
            let results = results.clone();
            let resources = resources.clone();
            std::thread::spawn(move || {
                while let Ok(task) = work.recv() {
                    let worker = resources.worker();
                    let result = if worker.is_some()
                        && let Some(mut working) = resources.working.reserve(DECODE_RESERVATION)
                    {
                        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            task.job.transmission.decode_image()
                        })) {
                            Ok(Ok(pixels)) => {
                                working.shrink_to(pixels.rgba.capacity());
                                Ok(Decoded {
                                    pixels,
                                    _working: working,
                                })
                            }
                            Ok(Err(error)) => Err(error),
                            Err(_) => Err(Error::Invalid),
                        }
                    } else {
                        Err(Error::Busy)
                    };
                    drop(worker);
                    if results
                        .send(Completion {
                            tag: task.tag,
                            job: task.job,
                            result,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            });
        }
        Self {
            input: Some(input),
            output: Some(output),
        }
    }
    pub fn try_submit(&self, tag: u64, job: Job) -> Result<(), Box<Job>> {
        self.input
            .as_ref()
            .unwrap()
            .try_send(Work { tag, job })
            .map_err(|error| Box::new(error.into_inner().job))
    }
    pub fn receive(&self, timeout: Duration) -> Result<Completion, RecvTimeoutError> {
        self.output.as_ref().unwrap().recv_timeout(timeout)
    }
}
impl Drop for Pool {
    fn drop(&mut self) {
        // Closing both ends releases queued jobs and wakes blocked result
        // senders. A presenter is never asked to join a decoding thread.
        self.input.take();
        self.output.take();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{graphics_reply, graphics_requests::Requests, graphics_store::Generation};
    fn requests(resources: Resources) -> Requests {
        Requests::with_resources(
            Generation {
                host: "decode-test".into(),
                boot: uuid::Uuid::new_v4(),
                pid: 123,
                start_ticks: 1,
                epoch: uuid::Uuid::new_v4(),
            },
            resources,
        )
    }
    fn job(requests: &mut Requests, id: u32) -> Job {
        requests
            .push(format!("Gi={id},f=24,s=1,v=1;AQID").as_bytes(), (0, 0))
            .unwrap()
            .unwrap()
    }
    #[test]
    fn asynchronous_results_keep_input_and_output_accounted_until_consumed() {
        let resources = Resources::new(UPLOAD_RESERVATION_FOR_TEST, 512 * 1024 * 1024);
        let pool = Pool::new(resources.clone());
        let mut requests = requests(resources.clone());
        for id in 1..=4 {
            assert!(
                pool.try_submit(u64::from(id), job(&mut requests, id))
                    .is_ok()
            );
        }
        let mut found = Vec::new();
        for _ in 0..4 {
            let completion = pool.receive(Duration::from_secs(5)).unwrap();
            assert_eq!(
                completion.result.as_ref().unwrap().pixels.rgba,
                [1, 2, 3, 255]
            );
            assert!(resources.uploads.used() > 0);
            assert!(resources.working.used() >= 4);
            requests.complete(&completion.job, Ok(())).unwrap();
            found.push(completion.tag);
            drop(completion);
        }
        found.sort();
        assert_eq!(found, [1, 2, 3, 4]);
        assert_eq!(resources.uploads.used(), 0);
        assert_eq!(resources.working.used(), 0);
        assert_eq!(
            requests.drain(8192),
            b"\x1b_Gi=1;OK\x1b\\\x1b_Gi=2;OK\x1b\\\x1b_Gi=3;OK\x1b\\\x1b_Gi=4;OK\x1b\\"
        );
    }
    const UPLOAD_RESERVATION_FOR_TEST: usize = 128 * 1024 * 1024;
    #[test]
    fn busy_decoder_failure_can_be_replied_without_wedging_later_queries() {
        let resources = Resources::new(UPLOAD_RESERVATION_FOR_TEST, DECODE_RESERVATION - 1);
        let pool = Pool::new(resources.clone());
        let mut requests = requests(resources.clone());
        assert!(pool.try_submit(1, job(&mut requests, 1)).is_ok());
        requests.ready(b"DA".to_vec()).unwrap();
        let completion = pool.receive(Duration::from_secs(5)).unwrap();
        assert!(matches!(completion.result, Err(Error::Busy)));
        requests
            .complete(
                &completion.job,
                Err(graphics_reply::Failure::Protocol(Error::Busy)),
            )
            .unwrap();
        drop(completion);
        assert_eq!(
            requests.drain(8192),
            b"\x1b_Gi=1;EBUSY:Graphics decoder busy\x1b\\DA"
        );
        assert_eq!(resources.uploads.used(), 0);
        assert_eq!(resources.working.used(), 0);
    }
    #[test]
    fn dropping_a_saturated_pool_releases_queued_completed_and_running_jobs() {
        let resources = Resources::new(UPLOAD_RESERVATION_FOR_TEST, 512 * 1024 * 1024);
        let pool = Pool::new(resources.clone());
        let mut requests = requests(resources.clone());
        let mut rejected = false;
        for id in 1..=64 {
            let item = job(&mut requests, id);
            if let Err(item) = pool.try_submit(u64::from(id), item) {
                requests
                    .complete(&item, Err(graphics_reply::Failure::Protocol(Error::Busy)))
                    .unwrap();
                drop(item);
                rejected = true;
                break;
            }
        }
        assert!(rejected);
        drop(pool);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while (resources.uploads.used() != 0 || resources.working.used() != 0)
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(resources.uploads.used(), 0);
        assert_eq!(resources.working.used(), 0);
    }
}
