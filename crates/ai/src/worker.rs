//! A long-lived connection to one agent process, on its own thread.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    CancelNotification, ContentBlock, InitializeRequest, NewSessionRequest, NewSessionResponse,
    PermissionOptionKind, PromptRequest, RequestPermissionOutcome, RequestPermissionRequest,
    RequestPermissionResponse, SelectedPermissionOutcome, SessionConfigKind, SessionConfigOption,
    SessionConfigOptionCategory, SessionConfigOptionValue, SessionConfigSelectOptions, SessionId,
    SessionNotification, SessionUpdate, SetSessionConfigOptionRequest, TextContent,
};
use agent_client_protocol::{AcpAgent, AcpAgentConfig, Agent, Client, ConnectionTo};
use async_channel::{Receiver, Sender};
use futures::future::{Either, select};
use greatheights_core::AgentConfig;

use crate::{AskEvent, ModelChoice, Models, path};

pub enum Job {
    Ask(AskJob),
    Models(Sender<Result<Models, String>>),
}

pub struct AskJob {
    pub question: String,
    /// Model id to switch the session to, if not the agent's default.
    pub model: Option<String>,
    pub events: Sender<AskEvent>,
    /// Fires, or closes, when the asker gives up on the answer.
    pub cancelled: Receiver<()>,
}

impl Job {
    fn fail(self, reason: String) {
        match self {
            Job::Ask(job) => {
                let _ = job.events.try_send(AskEvent::Failed(reason));
            }
            Job::Models(reply) => {
                let _ = reply.try_send(Err(reason));
            }
        }
    }
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
                    job.fail(reason.clone());
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
                    match job {
                        Job::Ask(job) => answer(&connection, &scratch, job, &current).await,
                        Job::Models(reply) => {
                            let models = list_models(&connection, &scratch).await;
                            let _ = reply.try_send(models);
                        }
                    }
                }
                Ok(())
            },
        )
        .await
        .map_err(|err| err.to_string())
}

async fn answer(connection: &ConnectionTo<Agent>, scratch: &PathBuf, job: AskJob, current: &Shared) {
    let response = match new_session(connection, scratch).await {
        Ok(response) => response,
        Err(err) => {
            let _ = job.events.try_send(AskEvent::Failed(err));
            return;
        }
    };
    let session = response.session_id.clone();
    if let Some(model) = &job.model {
        select_model(connection, &response, model).await;
    }
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

async fn new_session(
    connection: &ConnectionTo<Agent>,
    scratch: &PathBuf,
) -> Result<NewSessionResponse, String> {
    connection
        .send_request(NewSessionRequest::new(scratch.clone()))
        .block_task()
        .await
        .map_err(|err| format!("Couldn't start a session: {err}"))
}

async fn list_models(connection: &ConnectionTo<Agent>, scratch: &PathBuf) -> Result<Models, String> {
    let response = new_session(connection, scratch).await?;
    let Some((default, choices)) = model_option(&response).and_then(select_values) else {
        return Ok(Models::default());
    };
    Ok(Models {
        choices,
        default: Some(default),
    })
}

/// Switches a new session to `model`, if the agent offers it. Otherwise the
/// question still goes ahead with the agent's default.
async fn select_model(connection: &ConnectionTo<Agent>, session: &NewSessionResponse, model: &str) {
    let Some(option) = model_option(session) else {
        log::warn!("agent offers no model choice; using its default");
        return;
    };
    let Some((current, choices)) = select_values(option) else {
        return;
    };
    if current == model {
        return;
    }
    if !choices.iter().any(|choice| choice.id == model) {
        log::warn!("agent no longer offers model {model}; using its default");
        return;
    }
    let request = SetSessionConfigOptionRequest::new(
        session.session_id.clone(),
        option.id.clone(),
        SessionConfigOptionValue::value_id(model.to_owned()),
    );
    if let Err(err) = connection.send_request(request).block_task().await {
        log::warn!("couldn't switch to model {model}: {err}");
    }
}

fn model_option(session: &NewSessionResponse) -> Option<&SessionConfigOption> {
    session
        .config_options
        .as_ref()?
        .iter()
        .find(|option| option.category == Some(SessionConfigOptionCategory::Model))
}

/// The current value and every choice of a select option, groups flattened.
fn select_values(option: &SessionConfigOption) -> Option<(String, Vec<ModelChoice>)> {
    let SessionConfigKind::Select(select) = &option.kind else {
        return None;
    };
    let options = match &select.options {
        SessionConfigSelectOptions::Ungrouped(options) => options.iter().collect::<Vec<_>>(),
        SessionConfigSelectOptions::Grouped(groups) => {
            groups.iter().flat_map(|group| &group.options).collect()
        }
        _ => return None,
    };
    let choices = options
        .into_iter()
        .map(|option| ModelChoice {
            id: option.value.0.to_string(),
            name: option.name.clone(),
            description: option.description.clone(),
        })
        .collect();
    Some((select.current_value.0.to_string(), choices))
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
         Answer directly and concisely; Markdown is fine. \
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
