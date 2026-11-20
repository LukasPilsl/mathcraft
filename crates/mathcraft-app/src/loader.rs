//! Runs one load at a time on a background thread, so the window stays responsive while a
//! worksheet is read and evaluated. No egui types here: the caller passes a `wake` callback that
//! asks the window for a repaint when a result is ready.
//!
//! A new [`Loader::start`] makes any earlier load stale. Its result is dropped when it arrives,
//! so opening a second file while the first is still loading shows only the second one. A job
//! gets a cancel flag, which the evaluation checks between regions: the flag is set when the job
//! is superseded, when its loader is dropped, or by [`Loader::cancel`]. A cancelled job still
//! reports, with the regions it did not reach marked as stopped.

use crate::i18n::{tr, trf};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

/// The cancel flag of a job. Set it to stop the evaluation between regions.
pub type CancelFlag = Arc<AtomicBool>;

/// The message from a load thread: the id of the load and its result.
type Done<T> = (u64, Result<T, String>);

/// Runs jobs on background threads and hands back only the latest job's result.
pub struct Loader<T> {
    tx: Sender<Done<T>>,
    rx: Receiver<Done<T>>,
    /// Id of the job whose result is still wanted, or `None` when nothing is pending.
    pending: Option<u64>,
    next_id: u64,
    /// The cancel flag of the latest job.
    cancel: Option<CancelFlag>,
}

impl<T> Drop for Loader<T> {
    /// A dropped loader cancels its latest job: nothing will show its result.
    fn drop(&mut self) {
        if let Some(flag) = &self.cancel {
            flag.store(true, Ordering::Relaxed);
        }
    }
}

impl<T: Send + 'static> Default for Loader<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Send + 'static> Loader<T> {
    pub fn new() -> Self {
        let (tx, rx) = channel();
        Self {
            tx,
            rx,
            pending: None,
            next_id: 0,
            cancel: None,
        }
    }

    /// Asks the latest job to stop at its next limit check. Its result still arrives, so a
    /// cancelled evaluation shows what it reached.
    pub fn cancel(&self) {
        if let Some(flag) = &self.cancel {
            flag.store(true, Ordering::Relaxed);
        }
    }

    /// Whether a result is still wanted. True from `start` until `poll` returns it.
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }

    /// Starts `job` on a new thread. Any earlier job becomes stale and is cancelled. `job` gets the
    /// new job's cancel flag. `wake` runs on the job's thread after its result is sent, so the
    /// caller can request a repaint.
    ///
    /// # Errors
    /// The operating system could not start the thread.
    pub fn start(
        &mut self,
        job: impl FnOnce(CancelFlag) -> Result<T, String> + Send + 'static,
        wake: impl FnOnce() + Send + 'static,
    ) -> Result<(), String> {
        // A failed start must not leave the earlier job pending, and the earlier job stops.
        self.pending = None;
        self.cancel();
        let flag: CancelFlag = Arc::new(AtomicBool::new(false));
        let job_flag = Arc::clone(&flag);
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        let tx = self.tx.clone();
        std::thread::Builder::new()
            .name("mathcraft-load".to_string())
            .spawn(move || {
                let result = catch_unwind(AssertUnwindSafe(|| job(job_flag)))
                    .unwrap_or_else(|_| Err(tr("load.internal").to_string()));
                // The receiver is kept by the loader, so this fails only when the loader is gone.
                let _ = tx.send((id, result));
                wake();
            })
            .map_err(|e| trf("load.thread", &[&e.to_string()]))?;
        self.cancel = Some(flag);
        self.pending = Some(id);
        Ok(())
    }

    /// The result of the latest job, once it has finished. Results of stale jobs are dropped.
    pub fn poll(&mut self) -> Option<Result<T, String>> {
        while let Ok((id, result)) = self.rx.try_recv() {
            if self.pending == Some(id) {
                self.pending = None;
                return Some(result);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::{channel, Receiver};
    use std::time::Duration;

    use super::Loader;

    /// Waits for the next result. A test that never gets one fails rather than hanging.
    fn next<T: Send + 'static>(loader: &mut Loader<T>) -> Result<T, String> {
        for _ in 0..100_000 {
            if let Some(result) = loader.poll() {
                return result;
            }
            std::thread::sleep(Duration::from_micros(50));
        }
        panic!("no result arrived");
    }

    fn wait_for(rx: &Receiver<()>) {
        rx.recv_timeout(Duration::from_secs(10))
            .expect("the job finished");
    }

    #[test]
    fn result_of_a_job_arrives_and_wakes_the_caller() {
        let mut loader: Loader<u32> = Loader::new();
        let (wake_tx, wake_rx) = channel();
        loader
            .start(|_| Ok(7), move || wake_tx.send(()).expect("wake"))
            .expect("thread");
        assert!(loader.busy());
        wait_for(&wake_rx);
        assert_eq!(next(&mut loader), Ok(7));
        assert!(!loader.busy());
        assert!(loader.poll().is_none(), "a result is returned once");
    }

    #[test]
    fn errors_are_returned_as_errors() {
        let mut loader: Loader<u32> = Loader::new();
        loader
            .start(|_| Err("cannot open".to_string()), || {})
            .expect("thread");
        assert_eq!(next(&mut loader), Err("cannot open".to_string()));
    }

    #[test]
    fn a_panicking_job_gives_an_error_not_a_hang() {
        let mut loader: Loader<u32> = Loader::new();
        loader.start(|_| panic!("boom"), || {}).expect("thread");
        let result = next(&mut loader);
        assert!(result.is_err());
        assert!(!loader.busy());
    }

    #[test]
    fn a_result_of_an_earlier_job_is_dropped_when_a_newer_job_starts() {
        let mut loader: Loader<&'static str> = Loader::new();
        // The first job waits until the test releases it, then reports that it has finished.
        let (gate_tx, gate_rx) = channel::<()>();
        let (first_done_tx, first_done_rx) = channel::<()>();
        loader
            .start(
                move |_| {
                    gate_rx.recv().expect("gate");
                    Ok("first")
                },
                move || first_done_tx.send(()).expect("first done"),
            )
            .expect("thread");
        // The second job replaces it and finishes at once.
        loader.start(|_| Ok("second"), || {}).expect("thread");
        assert_eq!(next(&mut loader), Ok("second"));
        // Release the first job and wait until it has sent its (stale) result.
        gate_tx.send(()).expect("release");
        wait_for(&first_done_rx);
        assert!(loader.poll().is_none(), "the stale result is dropped");
        assert!(!loader.busy());
    }

    #[test]
    fn the_latest_of_several_jobs_wins_even_if_it_finishes_first() {
        let mut loader: Loader<u32> = Loader::new();
        let (gate_tx, gate_rx) = channel::<()>();
        loader
            .start(
                move |_| {
                    gate_rx.recv().expect("gate");
                    Ok(1)
                },
                || {},
            )
            .expect("thread");
        loader.start(|_| Ok(2), || {}).expect("thread");
        loader.start(|_| Ok(3), || {}).expect("thread");
        assert_eq!(next(&mut loader), Ok(3));
        gate_tx.send(()).expect("release");
        // Give the stale job time to report, then check that nothing else is returned.
        std::thread::sleep(Duration::from_millis(50));
        assert!(loader.poll().is_none());
    }

    #[test]
    fn cancel_sets_the_flag_of_the_latest_job_and_its_result_still_arrives() {
        let mut loader: Loader<u32> = Loader::new();
        let (flag_tx, flag_rx) = channel();
        loader
            .start(
                move |flag| {
                    flag_tx.send(flag).expect("flag");
                    Ok(5)
                },
                || {},
            )
            .expect("thread");
        let flag = flag_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("job started");
        assert!(!flag.load(std::sync::atomic::Ordering::Relaxed));
        loader.cancel();
        assert!(flag.load(std::sync::atomic::Ordering::Relaxed));
        assert_eq!(next(&mut loader), Ok(5));
    }

    #[test]
    fn a_superseded_job_and_a_dropped_loader_cancel_their_job() {
        let mut loader: Loader<u32> = Loader::new();
        let (flag_tx, flag_rx) = channel();
        let first_tx = flag_tx.clone();
        loader
            .start(
                move |flag| {
                    first_tx.send(flag).expect("flag");
                    Ok(1)
                },
                || {},
            )
            .expect("thread");
        let first = flag_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("first job started");
        loader
            .start(
                move |flag| {
                    flag_tx.send(flag).expect("flag");
                    Ok(2)
                },
                || {},
            )
            .expect("thread");
        let second = flag_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("second job started");
        assert!(first.load(std::sync::atomic::Ordering::Relaxed));
        assert!(!second.load(std::sync::atomic::Ordering::Relaxed));
        drop(loader);
        assert!(second.load(std::sync::atomic::Ordering::Relaxed));
    }
}
