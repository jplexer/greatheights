//! A long-lived connection to one agent process, on its own thread.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    CancelNotification, ContentBlock, InitializeRequest, NewSessionRequest, PermissionOptionKind,
    PromptRequest, RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionId, SessionNotification, SessionUpdate, TextContent,
};
use agent_client_protocol::{AcpAgent, AcpAgentConfig, Agent, Client, ConnectionTo};
use async_channel::{Receiver, Sender};
use futures::future::{Either, select};
use greatheights_core::AgentConfig;

use crate::{AskEvent, path};

pub struct Job {
    pub question: String,
    pub events: Sender<AskEvent>,
    /// Fires, or closes, when the asker gives up on the answer.
    pub cancelled: Receiver<()>,
}

pub struct Worker {
    pub config: AgentConfig,
    pub jobs: Sender<Job>,
}

/// The question being answered, so streamed updates reach the right asker.
#[derive(Default)]
struct Current {
    session: Option<SessionId>,
    events: Option<Sender<AskEvent>>,
}

type Shared = Arc<Mutex<Current>>;

impl Worker {
    pub fn start(config: AgentConfig) -> Self {
        let (jobs, queue) = async_channel::unbounded::<Job>();
        let agent = config.clone();
        let spawned = std::thread::Builder::new()
            .name("quick-ask".into())
            .spawn(move || {
                let current = Shared::default();
                let result = async_io::block_on(run(&agent, queue.clone(), current.clone()));
                let reason = match result {
                    Ok(()) => "The agent stopped.".to_owned(),
                    Err(err) => {
                        log::warn!("agent {} failed: {err}", agent.display_name());
                        format!("{} failed: {err}", agent.display_name())
                    }
                };
                // Whoever is still waiting deserves to know why.
                if let Some(events) = current.lock().unwrap().events.take() {
                    let _ = events.try_send(AskEvent::Failed(reason.clone()));
                }
                while let Ok(job) = queue.try_recv() {
                    let _ = job.events.try_send(AskEvent::Failed(reason.clone()));
                }
            });
        if let Err(err) = spawned {
            log::error!("couldn't start the Quick Ask thread: {err}");
        }
        Self { config, jobs }
    }

    /// False once the agent has exited and its thread has finished.
    pub fn is_alive(&self) -> bool {
        !self.jobs.is_closed()
    }
}

async fn run(config: &AgentConfig, queue: Receiver<Job>, current: Shared) -> Result<(), String> {
    let updates = current.clone();
    Client
        .builder()
        .name("greatheights")
        .on_receive_notification(
            async move |notification: SessionNotification, _cx| {
                forward_text(&updates, notification);
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |request: RequestPermissionRequest, responder, _cx| {
                responder.respond(decline(&request))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(
            AcpAgent::new(launch_config(config)),
            async move |connection: ConnectionTo<Agent>| {
                connection
                    .send_request(InitializeRequest::new(ProtocolVersion::V1))
                    .block_task()
                    .await?;
                let scratch = scratch_dir();
                while let Ok(job) = queue.recv().await {
                    answer(&connection, &scratch, job, &current).await;
                }
                Ok(())
            },
        )
        .await
        .map_err(|err| err.to_string())
}

async fn answer(connection: &ConnectionTo<Agent>, scratch: &PathBuf, job: Job, current: &Shared) {
    let session = match connection
        .send_request(NewSessionRequest::new(scratch.clone()))
        .block_task()
        .await
    {
        Ok(response) => response.session_id,
        Err(err) => {
            let _ = job
                .events
                .try_send(AskEvent::Failed(format!("Couldn't start a session: {err}")));
            return;
        }
    };
    *current.lock().unwrap() = Current {
        session: Some(session.clone()),
        events: Some(job.events.clone()),
    };

    let prompt = connection
        .send_request(PromptRequest::new(
            session.clone(),
            vec![ContentBlock::Text(TextContent::new(frame(&job.question)))],
        ))
        .block_task();
    let cancelled = job.cancelled.recv();
    let result = match select(Box::pin(prompt), Box::pin(cancelled)).await {
        Either::Left((result, _)) => result,
        Either::Right((_, prompt)) => {
            // Let the agent wind down cleanly; it ends the turn as cancelled.
            let _ = connection.send_notification(CancelNotification::new(session));
            prompt.await
        }
    };

    let event = match result {
        Ok(_) => AskEvent::Done,
        Err(err) => AskEvent::Failed(err.to_string()),
    };
    let mut current = current.lock().unwrap();
    current.session = None;
    if let Some(events) = current.events.take() {
        let _ = events.try_send(event);
    }
}

fn forward_text(current: &Shared, notification: SessionNotification) {
    let current = current.lock().unwrap();
    if current.session.as_ref() != Some(&notification.session_id) {
        return;
    }
    if let SessionUpdate::AgentMessageChunk(chunk) = notification.update
        && let ContentBlock::Text(text) = chunk.content
        && let Some(events) = &current.events
    {
        let _ = events.try_send(AskEvent::Text(text.text));
    }
}

/// Quick Ask only answers questions, so anything needing permission (edits,
/// commands) is declined, preferring the agent's own "reject" option.
fn decline(request: &RequestPermissionRequest) -> RequestPermissionResponse {
    let reject = request.options.iter().find(|option| {
        matches!(
            option.kind,
            PermissionOptionKind::RejectOnce | PermissionOptionKind::RejectAlways
        )
    });
    RequestPermissionResponse::new(match reject {
        Some(option) => RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
            option.option_id.clone(),
        )),
        None => RequestPermissionOutcome::Cancelled,
    })
}

fn frame(question: &str) -> String {
    format!(
        "You're answering a quick question typed into a desktop launcher. \
         Answer directly and concisely in plain text, without Markdown. \
         Don't use tools, read or write files, or run commands.\n\n{question}"
    )
}

fn launch_config(config: &AgentConfig) -> AcpAgentConfig {
    let path = config.env.get("PATH").cloned().or_else(path::login_path);
    let command = path::resolve(&config.command, path.as_deref());
    let mut launch = AcpAgentConfig::new(command)
        .args(config.args.clone())
        .envs(config.env.clone());
    if let Some(path) = path {
        launch = launch.env("PATH", path);
    }
    launch
}

/// An empty directory for sessions, so the agent has no project to look at.
fn scratch_dir() -> PathBuf {
    let dir = std::env::temp_dir().join("greatheights-quick-ask");
    if let Err(err) = std::fs::create_dir_all(&dir) {
        log::warn!("couldn't create {}: {err}", dir.display());
    }
    dir
}
