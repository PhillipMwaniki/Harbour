//! Round-trip prompts from the core to the user.
//!
//! Some things the core cannot decide alone: whether to trust a host key,
//! what the passphrase for a key file is. The core emits an event carrying a
//! prompt id, then awaits an answer that arrives as a separate `invoke`.
//!
//! The registry is deliberately ignorant of what a prompt *means*. It hands
//! out ids, parks futures, and matches answers to them; the SSH code owns the
//! payload shapes. That keeps SFTP overwrite prompts and vault unlocks - both
//! later milestones - from needing anything new here.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::sync::oneshot;

use crate::error::{AppError, AppResult};

/// How long an unanswered prompt is kept alive. A user who has walked away
/// should not leave a half-open connection and a parked task behind forever.
pub const PROMPT_TIMEOUT: Duration = Duration::from_secs(300);

pub type PromptId = String;

#[derive(Default)]
pub struct Prompts {
    pending: Mutex<HashMap<PromptId, oneshot::Sender<Value>>>,
}

impl Prompts {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Registers a prompt. The caller emits an event carrying
    /// [`Pending::id`], then awaits [`Pending::answer`].
    pub fn begin(self: &Arc<Self>) -> Pending {
        let id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(id.clone(), tx);
        Pending {
            id,
            rx: Some(rx),
            registry: Arc::clone(self),
        }
    }

    /// Delivers an answer. Unknown ids are an error rather than a silent
    /// no-op: it means the prompt timed out or the connection went away, and
    /// the UI should stop waiting on it.
    pub fn answer(&self, id: &str, value: Value) -> AppResult<()> {
        let tx = self
            .pending
            .lock()
            .remove(id)
            .ok_or_else(|| AppError::PromptNotFound(id.to_string()))?;
        // A receiver dropped between `remove` and `send` means the asker gave
        // up first; the answer is simply stale.
        let _ = tx.send(value);
        Ok(())
    }

    fn forget(&self, id: &str) {
        self.pending.lock().remove(id);
    }

    /// Number of prompts still waiting. Exposed for tests and diagnostics.
    pub fn outstanding(&self) -> usize {
        self.pending.lock().len()
    }
}

/// A registered, not-yet-answered prompt. Dropping it deregisters the prompt,
/// so an abandoned connection cannot leak an entry.
pub struct Pending {
    id: PromptId,
    /// `Option` only so that `answer` can take it out of a value that has a
    /// `Drop` impl; it is always `Some` until then.
    rx: Option<oneshot::Receiver<Value>>,
    registry: Arc<Prompts>,
}

impl Pending {
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Waits for the answer and deserialises it into `T`.
    ///
    /// Times out after [`PROMPT_TIMEOUT`]; a malformed answer is treated as no
    /// answer at all, because the alternative is a connection stuck open on a
    /// UI bug.
    pub async fn answer<T: DeserializeOwned>(mut self) -> AppResult<T> {
        let id = self.id.clone();
        let rx = self
            .rx
            .take()
            .expect("a prompt can only be awaited once, which the type system enforces");

        let value = match tokio::time::timeout(PROMPT_TIMEOUT, rx).await {
            Ok(Ok(value)) => value,
            // The sender was dropped - the registry was cleared out from under
            // us - or nothing arrived in time. Both mean "no answer".
            // Dropping `self` deregisters the prompt either way.
            Ok(Err(_)) | Err(_) => return Err(AppError::PromptTimedOut),
        };

        serde_json::from_value(value)
            .map_err(|err| AppError::internal(format!("malformed answer to prompt {id}: {err}")))
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        self.registry.forget(&self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq, Eq)]
    struct Answer {
        accept: bool,
    }

    #[tokio::test]
    async fn an_answer_reaches_the_waiter() {
        let prompts = Prompts::new();
        let pending = prompts.begin();
        let id = pending.id().to_string();

        let waiter = tokio::spawn(async move { pending.answer::<Answer>().await });
        // The registry holds the prompt until it is answered.
        assert_eq!(prompts.outstanding(), 1);

        prompts
            .answer(&id, serde_json::json!({ "accept": true }))
            .unwrap();

        assert_eq!(waiter.await.unwrap().unwrap(), Answer { accept: true });
        assert_eq!(prompts.outstanding(), 0);
    }

    #[tokio::test]
    async fn answering_an_unknown_prompt_is_an_error() {
        let prompts = Prompts::new();
        let err = prompts
            .answer("nope", serde_json::json!({}))
            .expect_err("an unknown prompt id must not be accepted");
        assert_eq!(err.code(), "PROMPT_NOT_FOUND");
    }

    #[tokio::test]
    async fn a_prompt_cannot_be_answered_twice() {
        let prompts = Prompts::new();
        let pending = prompts.begin();
        let id = pending.id().to_string();

        prompts
            .answer(&id, serde_json::json!({ "accept": true }))
            .unwrap();
        let err = prompts
            .answer(&id, serde_json::json!({ "accept": false }))
            .expect_err("the second answer has nothing to answer");
        assert_eq!(err.code(), "PROMPT_NOT_FOUND");
        drop(pending);
    }

    /// An asker that goes away - a cancelled connection, say - must not leave
    /// its prompt sitting in the map.
    #[tokio::test]
    async fn dropping_the_waiter_deregisters_the_prompt() {
        let prompts = Prompts::new();
        let pending = prompts.begin();
        let id = pending.id().to_string();
        assert_eq!(prompts.outstanding(), 1);

        drop(pending);

        assert_eq!(prompts.outstanding(), 0);
        assert!(prompts.answer(&id, serde_json::json!({})).is_err());
    }

    #[tokio::test]
    async fn a_malformed_answer_is_rejected() {
        let prompts = Prompts::new();
        let pending = prompts.begin();
        let id = pending.id().to_string();

        let waiter = tokio::spawn(async move { pending.answer::<Answer>().await });
        prompts
            .answer(&id, serde_json::json!({ "accept": "yes please" }))
            .unwrap();

        let err = waiter.await.unwrap().expect_err("a bad answer is an error");
        assert_eq!(err.code(), "INTERNAL");
    }
}
