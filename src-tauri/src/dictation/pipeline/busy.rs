//! A transcription's `processing` flag: one transcription at a time,
//! and the flag let go however its task ends.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::Emitter;

use super::{Shared, DICTATION_ERROR};

/// What a transcription task was, for what its crash says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Job {
    /// A dictation: the hotkey's, Undo's, robot mode's.
    Dictation,
    /// An Upload file.
    Upload,
    /// Home's Run again.
    Rerun,
    /// A saved transform (`transforms.rs`).
    Transform,
}

/// Who holds the `processing` flag: the pipeline, or a unit test's stand-in.
pub(crate) trait Holder: Send + Sync + 'static {
    fn processing(&self) -> &AtomicBool;
    /// The task ended without finishing (a panic in it): put things back
    /// and tell the person. Must not panic itself.
    fn crashed(&self, job: Job);
}

/// The `processing` flag (starts refused while it's set: one transcription
/// at a time), held for one transcription task and let go when it ends —
/// **however it ends**. A panic in the task (a bug: `app_kind::word_before`
/// slicing a no-break space in the paste was one) unwound past the line
/// after the `.await` that cleared the flag: every press after it was
/// ignored ("a transcription is still processing"), the bar stayed on
/// "processing", the updater waited for ever — until a restart, and nobody
/// was told. Now the flag goes with the task: dropped without [`Busy::done`]
/// (the task panicked), [`Holder::crashed`] first puts things back and says
/// so (the dictation listed as failed with its audio, "Dictation failed" on
/// the bar).
pub(crate) struct Busy<H: Holder> {
    holder: Arc<H>,
    job: Job,
    finished: bool,
}

impl<H: Holder> Busy<H> {
    /// Take the flag: `None` while another transcription holds it.
    pub(crate) fn take(holder: &Arc<H>, job: Job) -> Option<Self> {
        (!holder.processing().swap(true, Ordering::SeqCst)).then(|| Busy { holder: Arc::clone(holder), job, finished: false })
    }

    /// Set the flag (a stop: starts are refused already while it's set).
    pub(crate) fn hold(holder: &Arc<H>, job: Job) -> Self {
        holder.processing().store(true, Ordering::SeqCst);
        Busy { holder: Arc::clone(holder), job, finished: false }
    }

    /// The task ended as it should: the flag goes, nothing to say.
    pub(crate) fn done(mut self) {
        self.finished = true;
    }
}

impl<H: Holder> Drop for Busy<H> {
    fn drop(&mut self) {
        if !self.finished {
            let (holder, job) = (&self.holder, self.job);
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| holder.crashed(job))).is_err() {
                tracing::error!(?job, "A transcription crashed, and putting things back failed too");
            }
        }
        self.holder.processing().store(false, Ordering::SeqCst);
    }
}

/// What a crashed dictation says on the bar.
const CRASHED: &str = "Dictation failed — Yap hit an error. Try again";

/// Test mode: the next dictation panics in its paste step
/// (`e2e::e2e_crash_next_dictation`).
#[cfg(debug_assertions)]
pub(super) static CRASH_NEXT: AtomicBool = AtomicBool::new(false);

#[cfg(debug_assertions)]
pub(crate) fn crash_next_dictation() {
    CRASH_NEXT.store(true, Ordering::SeqCst);
}

impl Holder for Shared {
    fn processing(&self) -> &AtomicBool {
        &self.processing
    }

    fn crashed(&self, job: Job) {
        tracing::error!(?job, "A transcription crashed; Yap can dictate again");
        self.undoing.store(false, Ordering::SeqCst);
        match job {
            Job::Dictation => {
                // A question for the Voice Assistant that will never come.
                if self.assistant.swap(false, Ordering::SeqCst) {
                    crate::chat::assistant::nothing(&self.app);
                }
                // Listed with its audio, to run again (if it wasn't yet).
                self.list_failed("The dictation crashed");
                let _ = self.app.emit(DICTATION_ERROR, CRASHED);
                let _ = self.app.emit("yap-state", "error");
            }
            Job::Upload => {
                let _ = self.app.emit("yap-upload-error", "The transcription crashed — try again");
            }
            Job::Rerun => {
                let _ = self.app.emit("yap-error", "Run again failed — Yap hit an error. Try again");
            }
            Job::Transform => {
                let _ = self.app.emit(DICTATION_ERROR, "The transform failed — Yap hit an error. Try again");
                let _ = self.app.emit("yap-state", "error");
            }
        }
    }
}

/// `work` on the async runtime, holding `busy` until it ends (see [`Busy`]).
pub(crate) fn spawn_busy<H: Holder>(
    busy: Busy<H>,
    work: impl std::future::Future<Output = ()> + Send + 'static,
) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        work.await;
        busy.done();
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// The pipeline's part in [`Busy`], with the crashes it was told of.
    #[derive(Default)]
    struct Stand {
        processing: AtomicBool,
        crashes: Mutex<Vec<Job>>,
    }

    impl Holder for Stand {
        fn processing(&self) -> &AtomicBool {
            &self.processing
        }
        fn crashed(&self, job: Job) {
            self.crashes.lock().unwrap().push(job);
        }
    }

    /// A panic in a transcription's task (`app_kind::word_before`'s, in the
    /// paste, was one) skipped the line after the `.await` that cleared
    /// `processing`: every press after it was ignored ("a transcription is
    /// still processing") until Yap restarted, and nobody was told.
    #[test]
    fn a_transcription_that_crashes_lets_dictation_go_and_says_so() {
        let stand = Arc::new(Stand::default());
        let busy = Busy::hold(&stand, Job::Dictation);
        let task = spawn_busy(busy, async {
            tokio::task::yield_now().await;
            panic!("a bug after the transcript (this test's own panic)");
        });
        assert!(tauri::async_runtime::block_on(task).is_err(), "the task panicked");
        assert!(!stand.processing.load(Ordering::SeqCst), "dictation is free again");
        assert_eq!(*stand.crashes.lock().unwrap(), vec![Job::Dictation], "and the person is told");

        // One that ends as it should: free again, nothing said.
        let busy = Busy::take(&stand, Job::Upload).expect("free");
        assert!(Busy::take(&stand, Job::Rerun).is_none(), "one transcription at a time");
        tauri::async_runtime::block_on(spawn_busy(busy, async {})).unwrap();
        assert!(!stand.processing.load(Ordering::SeqCst));
        // Taken and given back without a task (Undo with nothing to undo).
        Busy::take(&stand, Job::Dictation).expect("free").done();
        assert!(!stand.processing.load(Ordering::SeqCst));
        assert_eq!(stand.crashes.lock().unwrap().len(), 1, "nothing more said");
    }
}
