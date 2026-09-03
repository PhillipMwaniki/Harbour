//! The queue: which transfers run, how many at once, and what each is doing.
//!
//! Every transfer is a tokio task. It waits for one of a session's slots,
//! plans, then copies file by file, checking a [`Control`] between chunks so a
//! pause or a cancel lands within a fraction of a second. Every change to a
//! transfer's snapshot goes out through one emitter - progress at most a few
//! times a second, state changes always - so the UI is a projection of the
//! engine and never has to guess.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use russh_sftp::client::SftpSession;
use tokio::sync::{oneshot, Notify, Semaphore};

use crate::error::{AppError, AppResult};
use crate::session::SessionId;
use crate::transfer::copy::{self, Gate};
use crate::transfer::{
    ConflictInfo, ConflictPolicy, Request, Resolution, Transfer, TransferId, TransferState,
};

/// Transfers running at once against one connection. Two keeps a large file
/// from starving a small one without saturating the channel's window.
pub const PER_SESSION_CONCURRENCY: usize = 2;
/// How often progress alone may be emitted. State changes ignore this.
const EMIT_INTERVAL: Duration = Duration::from_millis(120);

pub type Emitter = Arc<dyn Fn(&Transfer) + Send + Sync>;

/// The knobs the outside world has on a running transfer.
struct Control {
    paused: AtomicBool,
    cancelled: AtomicBool,
    /// Woken on resume and on cancel, so a paused task re-reads the flags.
    wake: Notify,
    /// Present while the task is stopped on a conflict, waiting to be told.
    resolution: Mutex<Option<oneshot::Sender<(Resolution, bool)>>>,
    policy: Mutex<ConflictPolicy>,
}

struct Job {
    snapshot: Transfer,
    control: Arc<Control>,
    last_emit: Instant,
}

pub struct Engine {
    jobs: Mutex<Vec<Job>>,
    slots: Mutex<HashMap<SessionId, Arc<Semaphore>>>,
    emit: Emitter,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or(0)
}

impl Engine {
    pub fn new(emit: Emitter) -> Arc<Self> {
        Arc::new(Self {
            jobs: Mutex::new(Vec::new()),
            slots: Mutex::new(HashMap::new()),
            emit,
        })
    }

    /// Queues a transfer and starts it as soon as its session has a slot.
    pub fn enqueue(
        self: &Arc<Self>,
        session_id: SessionId,
        sftp: Arc<SftpSession>,
        request: Request,
        policy: ConflictPolicy,
    ) -> Transfer {
        let id = uuid::Uuid::new_v4().to_string();
        let snapshot = Transfer {
            id: id.clone(),
            session_id: session_id.clone(),
            direction: request.direction,
            source: request.source.clone(),
            destination: request.destination.clone(),
            state: TransferState::Queued,
            conflict: None,
            bytes_done: 0,
            bytes_total: 0,
            files_done: 0,
            files_total: 0,
            current_file: None,
            error: None,
            queued_at: now(),
        };
        let control = Arc::new(Control {
            paused: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            wake: Notify::new(),
            resolution: Mutex::new(None),
            policy: Mutex::new(policy),
        });

        self.jobs.lock().push(Job {
            snapshot: snapshot.clone(),
            control: Arc::clone(&control),
            last_emit: Instant::now(),
        });
        (self.emit)(&snapshot);

        let slot = Arc::clone(
            self.slots
                .lock()
                .entry(session_id)
                .or_insert_with(|| Arc::new(Semaphore::new(PER_SESSION_CONCURRENCY))),
        );
        let engine = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            engine.run(id, sftp, request, control, slot).await;
        });
        snapshot
    }

    pub fn list(&self) -> Vec<Transfer> {
        self.jobs
            .lock()
            .iter()
            .map(|job| job.snapshot.clone())
            .collect()
    }

    pub fn get(&self, id: &str) -> AppResult<Transfer> {
        self.jobs
            .lock()
            .iter()
            .find(|job| job.snapshot.id == id)
            .map(|job| job.snapshot.clone())
            .ok_or_else(|| AppError::TransferNotFound(id.to_string()))
    }

    fn control(&self, id: &str) -> AppResult<Arc<Control>> {
        self.jobs
            .lock()
            .iter()
            .find(|job| job.snapshot.id == id)
            .map(|job| Arc::clone(&job.control))
            .ok_or_else(|| AppError::TransferNotFound(id.to_string()))
    }

    pub fn pause(&self, id: &str) -> AppResult<()> {
        let control = self.control(id)?;
        control.paused.store(true, Ordering::Release);
        // The task notices at its next chunk; the state changes there, so the
        // UI shows Paused once the copy has actually stopped.
        Ok(())
    }

    pub fn resume(&self, id: &str) -> AppResult<()> {
        let control = self.control(id)?;
        control.paused.store(false, Ordering::Release);
        control.wake.notify_one();
        Ok(())
    }

    /// Stops a transfer wherever it is: queued, mid-file, paused, or waiting
    /// on a conflict.
    pub fn cancel(&self, id: &str) -> AppResult<()> {
        let control = self.control(id)?;
        cancel_control(&control);
        // A transfer that has not yet been scheduled has no task awake to
        // notice, so its state is set here; a running one reports itself.
        self.update(id, true, |transfer| {
            if transfer.state == TransferState::Queued {
                transfer.state = TransferState::Cancelled;
            }
        });
        Ok(())
    }

    /// Answers the conflict a transfer is stopped on. `apply_to_all` makes the
    /// same answer automatic for the rest of that transfer.
    pub fn resolve(&self, id: &str, resolution: Resolution, apply_to_all: bool) -> AppResult<()> {
        let control = self.control(id)?;
        let sender = control.resolution.lock().take().ok_or_else(|| {
            AppError::Transfer(format!("transfer {id} is not waiting on a conflict"))
        })?;
        sender
            .send((resolution, apply_to_all))
            .map_err(|_| AppError::Transfer(format!("transfer {id} stopped waiting")))
    }

    /// Forgets a finished transfer. A live one has to be cancelled first.
    pub fn remove(&self, id: &str) -> AppResult<()> {
        let mut jobs = self.jobs.lock();
        let index = jobs
            .iter()
            .position(|job| job.snapshot.id == id)
            .ok_or_else(|| AppError::TransferNotFound(id.to_string()))?;
        if !jobs[index].snapshot.state.is_finished() {
            return Err(AppError::Transfer(format!(
                "transfer {id} is still running"
            )));
        }
        jobs.remove(index);
        Ok(())
    }

    pub fn clear_finished(&self) -> usize {
        let mut jobs = self.jobs.lock();
        let before = jobs.len();
        jobs.retain(|job| !job.snapshot.state.is_finished());
        before - jobs.len()
    }

    /// The session is gone, and with it every transfer riding on it.
    pub fn cancel_session(&self, session_id: &str) {
        let controls: Vec<(TransferId, Arc<Control>)> = self
            .jobs
            .lock()
            .iter()
            .filter(|job| {
                job.snapshot.session_id == session_id && !job.snapshot.state.is_finished()
            })
            .map(|job| (job.snapshot.id.clone(), Arc::clone(&job.control)))
            .collect();
        for (id, control) in controls {
            cancel_control(&control);
            self.update(&id, true, |transfer| {
                if transfer.state == TransferState::Queued {
                    transfer.state = TransferState::Cancelled;
                }
            });
        }
        self.slots.lock().remove(session_id);
    }

    /// Changes a snapshot and tells the world. `force` bypasses the progress
    /// throttle; every state change passes `true`.
    fn update(&self, id: &str, force: bool, change: impl FnOnce(&mut Transfer)) {
        let emitted = {
            let mut jobs = self.jobs.lock();
            let Some(job) = jobs.iter_mut().find(|job| job.snapshot.id == id) else {
                return;
            };
            change(&mut job.snapshot);
            if force || job.last_emit.elapsed() >= EMIT_INTERVAL {
                job.last_emit = Instant::now();
                Some(job.snapshot.clone())
            } else {
                None
            }
        };
        if let Some(snapshot) = emitted {
            (self.emit)(&snapshot);
        }
    }

    fn set_state(&self, id: &str, state: TransferState) {
        self.update(id, true, |transfer| {
            transfer.state = state;
            if state != TransferState::Conflict {
                transfer.conflict = None;
            }
        });
    }

    async fn run(
        self: Arc<Self>,
        id: TransferId,
        sftp: Arc<SftpSession>,
        request: Request,
        control: Arc<Control>,
        slot: Arc<Semaphore>,
    ) {
        let Ok(_permit) = slot.acquire().await else {
            return;
        };
        if control.cancelled.load(Ordering::Acquire) {
            self.set_state(&id, TransferState::Cancelled);
            return;
        }
        self.set_state(&id, TransferState::Running);

        let gate = JobGate {
            engine: Arc::clone(&self),
            id: id.clone(),
            control: Arc::clone(&control),
        };

        match self.copy(&id, &sftp, &request, &control, &gate).await {
            Ok(wrote_anything) => self.set_state(
                &id,
                if wrote_anything {
                    TransferState::Done
                } else {
                    TransferState::Skipped
                },
            ),
            Err(AppError::Transfer(reason)) if reason == CANCELLED => {
                self.set_state(&id, TransferState::Cancelled);
            }
            Err(err) => self.update(&id, true, |transfer| {
                transfer.state = TransferState::Failed;
                transfer.conflict = None;
                transfer.error = Some(err.to_string());
            }),
        }
    }

    /// The body of a transfer. Returns whether any file was written.
    async fn copy(
        &self,
        id: &str,
        sftp: &SftpSession,
        request: &Request,
        control: &Arc<Control>,
        gate: &JobGate,
    ) -> AppResult<bool> {
        let direction = request.direction;
        let plan = copy::plan(sftp, direction, &request.source, &request.destination).await?;
        self.update(id, true, |transfer| {
            transfer.bytes_total = plan.total_bytes;
            transfer.files_total = plan.files.len() as u32;
        });

        let mut wrote = false;
        for directory in &plan.directories {
            gate.wait().await?;
            wrote |= copy::ensure_directory(sftp, direction, directory).await?;
        }

        for mut item in plan.files {
            gate.wait().await?;
            self.update(id, true, |transfer| {
                transfer.current_file = Some(item.source.clone());
            });

            let mut offset = 0;
            if let Some((size, modified)) =
                copy::existing(sftp, direction, &item.destination).await?
            {
                let policy = *control.policy.lock();
                let resolution = match policy {
                    ConflictPolicy::Ask => {
                        let info = ConflictInfo {
                            path: item.destination.clone(),
                            source_size: item.size,
                            source_modified: item.modified,
                            destination_size: size,
                            destination_modified: modified,
                            resumable: size < item.size,
                        };
                        self.ask(id, control, info).await?
                    }
                    ConflictPolicy::Overwrite => Resolution::Overwrite,
                    ConflictPolicy::Skip => Resolution::Skip,
                    ConflictPolicy::Resume => Resolution::Resume,
                    ConflictPolicy::Rename => Resolution::Rename,
                };

                match resolution {
                    Resolution::Cancel => return Err(AppError::Transfer(CANCELLED.into())),
                    Resolution::Skip => {
                        self.update(id, true, |transfer| {
                            transfer.bytes_done += item.size;
                            transfer.files_done += 1;
                        });
                        continue;
                    }
                    Resolution::Overwrite => {}
                    Resolution::Resume => {
                        if size == item.size {
                            // Already complete: nothing to copy, nothing to lose.
                            self.update(id, true, |transfer| {
                                transfer.bytes_done += item.size;
                                transfer.files_done += 1;
                            });
                            continue;
                        }
                        if size < item.size {
                            offset = size;
                            self.update(id, true, |transfer| transfer.bytes_done += size);
                        }
                        // Larger than the source cannot be a partial copy of
                        // it, so it is overwritten from the start.
                    }
                    Resolution::Rename => {
                        item.destination =
                            copy::available_name(sftp, direction, &item.destination).await?;
                    }
                }
            }

            copy::copy_file(sftp, direction, &item, offset, gate).await?;
            wrote = true;
            self.update(id, true, |transfer| transfer.files_done += 1);
        }

        Ok(wrote)
    }

    /// Stops the transfer on a conflict until someone answers, or cancels.
    async fn ask(
        &self,
        id: &str,
        control: &Arc<Control>,
        info: ConflictInfo,
    ) -> AppResult<Resolution> {
        let (sender, receiver) = oneshot::channel();
        *control.resolution.lock() = Some(sender);
        self.update(id, true, |transfer| {
            transfer.state = TransferState::Conflict;
            transfer.conflict = Some(info);
        });

        let (resolution, apply_to_all) = receiver.await.unwrap_or((Resolution::Cancel, false));
        if apply_to_all {
            if let Some(policy) = resolution.as_policy() {
                *control.policy.lock() = policy;
            }
        }
        if resolution != Resolution::Cancel {
            self.set_state(id, TransferState::Running);
        }
        Ok(resolution)
    }
}

const CANCELLED: &str = "cancelled";

fn cancel_control(control: &Control) {
    control.cancelled.store(true, Ordering::Release);
    control.wake.notify_one();
    // A task waiting on a conflict is told the answer is "cancel".
    if let Some(sender) = control.resolution.lock().take() {
        let _ = sender.send((Resolution::Cancel, false));
    }
}

/// What a running copy checks between chunks.
struct JobGate {
    engine: Arc<Engine>,
    id: TransferId,
    control: Arc<Control>,
}

impl Gate for JobGate {
    async fn wait(&self) -> AppResult<()> {
        loop {
            if self.control.cancelled.load(Ordering::Acquire) {
                return Err(AppError::Transfer(CANCELLED.into()));
            }
            if !self.control.paused.load(Ordering::Acquire) {
                return Ok(());
            }
            self.engine.set_state(&self.id, TransferState::Paused);
            self.control.wake.notified().await;
            if !self.control.paused.load(Ordering::Acquire)
                && !self.control.cancelled.load(Ordering::Acquire)
            {
                self.engine.set_state(&self.id, TransferState::Running);
            }
        }
    }

    fn progress(&self, bytes: u64) {
        self.engine
            .update(&self.id, false, |transfer| transfer.bytes_done += bytes);
    }
}
