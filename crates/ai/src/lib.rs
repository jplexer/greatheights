//! Quick Ask: one-off questions to an Agent Client Protocol agent.
//!
//! The agent process starts on the first question and stays running, so
//! later questions skip its startup. Every question gets a fresh session in
//! an empty scratch directory, unless it follows up on an earlier answer's
//! [`Chat`], and every permission request is declined: coding agents can
//! edit files and run commands, but here they only answer.

mod path;
mod worker;

use std::sync::{Arc, Mutex, OnceLock};

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
    /// The conversation this question belongs to, for follow-ups.
    pub chat: Chat,
    _cancel: Sender<()>,
}

/// An agent session that follow-up questions can continue.
#[derive(Clone, Debug)]
pub struct Chat {
    /// Which agent process holds the session; it dies with the process.
    worker: u64,
    /// Set once the agent has started the session.
    session: Arc<OnceLock<String>>,
}

#[derive(Default)]
pub struct Asker {
    worker: Mutex<Option<Worker>>,
}

impl Asker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Asks a question in a new session.
    pub fn ask(&self, agent: &AgentConfig, question: &str) -> Ask {
        self.start_ask(agent, question, None)
    }

    /// Asks a question in `chat`'s session, so the agent sees the
    /// conversation so far.
    pub fn follow_up(&self, agent: &AgentConfig, chat: &Chat, question: &str) -> Ask {
        self.start_ask(agent, question, Some(chat))
    }

    fn start_ask(&self, agent: &AgentConfig, question: &str, chat: Option<&Chat>) -> Ask {
        let (events_tx, events) = async_channel::unbounded();
        let (cancel, cancelled) = async_channel::bounded(1);
        let fail = |reason: &str| {
            let _ = events_tx.try_send(AskEvent::Failed(reason.into()));
        };
        let mut worker = self.worker.lock().unwrap();
        let worker = Self::worker_for(&mut worker, agent);
        let ask_chat = match chat {
            Some(chat) => chat.clone(),
            None => Chat {
                worker: worker.id,
                session: Arc::default(),
            },
        };
        let session = match chat {
            None => None,
            Some(chat) => match chat.session.get() {
                Some(session) if chat.worker == worker.id => Some(session.clone()),
                _ => {
                    fail("The chat ended when the agent restarted. Ask again to start a new one.");
                    return Ask {
                        events,
                        chat: ask_chat,
                        _cancel: cancel,
                    };
                }
            },
        };
        let job = Job::Ask(AskJob {
            question: question.to_owned(),
            session,
            model: agent.model.clone(),
            chat: ask_chat.session.clone(),
            events: events_tx.clone(),
            cancelled,
        });
        if worker.jobs.try_send(job).is_err() {
            fail("The agent isn't running.");
        }
        Ask {
            events,
            chat: ask_chat,
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
        let worker = Self::worker_for(&mut worker, agent);
        worker.jobs.try_send(job).map_err(|_| ())
    }

    /// The running worker for `agent`, starting one if needed.
    fn worker_for<'a>(worker: &'a mut Option<Worker>, agent: &AgentConfig) -> &'a Worker {
        // The model is chosen per session, so a model change keeps the agent.
        let reusable = worker
            .as_ref()
            .is_some_and(|w| w.config.same_launch(agent) && w.is_alive());
        if !reusable {
            // Replacing the old worker shuts its agent down.
            *worker = Some(Worker::start(agent.clone()));
        }
        worker.as_ref().unwrap()
    }
}
