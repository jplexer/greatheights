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

use crate::worker::{AskJob, Job, Worker};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AskEvent {
    /// The next piece of the answer.
    Text(String),
    Done,
    Failed(String),
}

/// A model the agent offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelChoice {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Models {
    /// Empty if the agent doesn't let clients pick a model.
    pub choices: Vec<ModelChoice>,
    /// The agent's default for new sessions.
    pub default: Option<String>,
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
        let job = Job::Ask(AskJob {
            question: question.to_owned(),
            model: agent.model.clone(),
            events: events_tx.clone(),
            cancelled,
        });
        if self.send(agent, job).is_err() {
            let _ = events_tx.try_send(AskEvent::Failed("The agent isn't running.".into()));
        }
        Ask {
            events,
            _cancel: cancel,
        }
    }

    /// Lists the models the agent offers. Starts the agent if needed.
    pub fn models(&self, agent: &AgentConfig) -> Receiver<Result<Models, String>> {
        let (reply, models) = async_channel::bounded(1);
        if self.send(agent, Job::Models(reply.clone())).is_err() {
            let _ = reply.try_send(Err("The agent isn't running.".into()));
        }
        models
    }

    fn send(&self, agent: &AgentConfig, job: Job) -> Result<(), ()> {
        let mut worker = self.worker.lock().unwrap();
        // The model is chosen per session, so a model change keeps the agent.
        let reusable = worker
            .as_ref()
            .is_some_and(|w| w.config.same_launch(agent) && w.is_alive());
        if !reusable {
            // Replacing the old worker shuts its agent down.
            *worker = Some(Worker::start(agent.clone()));
        }
        worker.as_ref().unwrap().jobs.try_send(job).map_err(|_| ())
    }
}
