//! Quick Ask: one-off questions to an Agent Client Protocol agent.
//!
//! The agent process starts on the first question and stays running, so
//! later questions skip its startup. Every question gets a fresh session in
//! an empty scratch directory, and every permission request is declined:
//! coding agents can edit files and run commands, but here they only answer.

mod path;
mod worker;

use std::sync::Mutex;

use async_channel::{Receiver, Sender};
use greatheights_core::AgentConfig;

use crate::worker::{Job, Worker};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AskEvent {
    /// The next piece of the answer.
    Text(String),
    Done,
    Failed(String),
}

/// A question in flight. Dropping it cancels the question.
pub struct Ask {
    pub events: Receiver<AskEvent>,
    _cancel: Sender<()>,
}

#[derive(Default)]
pub struct Asker {
    worker: Mutex<Option<Worker>>,
}

impl Asker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn ask(&self, agent: &AgentConfig, question: &str) -> Ask {
        let (events_tx, events) = async_channel::unbounded();
        let (cancel, cancelled) = async_channel::bounded(1);

        let mut worker = self.worker.lock().unwrap();
        let reusable = worker
            .as_ref()
            .is_some_and(|w| w.config == *agent && w.is_alive());
        if !reusable {
            // Replacing the old worker shuts its agent down.
            *worker = Some(Worker::start(agent.clone()));
        }
        let job = Job {
            question: question.to_owned(),
            events: events_tx.clone(),
            cancelled,
        };
        if worker.as_ref().unwrap().jobs.try_send(job).is_err() {
            let _ = events_tx.try_send(AskEvent::Failed("The agent isn't running.".into()));
        }

        Ask {
            events,
            _cancel: cancel,
        }
    }
}
