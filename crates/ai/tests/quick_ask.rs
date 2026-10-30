//! Drives `Asker` against the scripted agent in `src/bin/mock_agent.rs`.

use std::time::Duration;

use greatheights_ai::{Ask, AskEvent, Asker, ModelChoice, Models};
use greatheights_core::AgentConfig;

fn mock_agent() -> AgentConfig {
    AgentConfig {
        name: Some("Mock".into()),
        command: env!("CARGO_BIN_EXE_mock_agent").into(),
        args: Vec::new(),
        env: Default::default(),
        model: None,
    }
}

fn next(ask: &Ask) -> AskEvent {
    async_io::block_on(async {
        let timeout = async {
            async_io::Timer::after(Duration::from_secs(10)).await;
            panic!("timed out waiting for the agent");
        };
        futures::future::select(Box::pin(ask.events.recv()), Box::pin(timeout))
            .await
            .factor_first()
            .0
            .expect("events closed")
    })
}

fn answer(ask: &Ask) -> String {
    let mut text = String::new();
    loop {
        match next(ask) {
            AskEvent::Text(chunk) => text.push_str(&chunk),
            AskEvent::Done => return text,
            AskEvent::Failed(err) => panic!("ask failed: {err}"),
        }
    }
}

#[test]
fn streams_the_answer_and_declines_permissions() {
    let asker = Asker::new();
    assert_eq!(answer(&asker.ask(&mock_agent(), "hi")), "Hello (declined)");
}

#[test]
fn reuses_the_agent_and_survives_cancellation() {
    let asker = Asker::new();
    let slow = asker.ask(&mock_agent(), "SLOW");
    assert_eq!(next(&slow), AskEvent::Text("Hello".into()));
    drop(slow);

    // Same agent process (sessions keep counting), still answering.
    assert_eq!(answer(&asker.ask(&mock_agent(), "again")), "Hello (declined)");
}

#[test]
fn lists_the_agents_models() {
    let models = async_io::block_on(Asker::new().models(&mock_agent()).recv()).unwrap();
    assert_eq!(
        models,
        Ok(Models {
            choices: vec![
                ModelChoice {
                    id: "fast".into(),
                    name: "Fast".into(),
                    description: None
                },
                ModelChoice {
                    id: "smart".into(),
                    name: "Smart".into(),
                    description: Some("Slower, better".into())
                },
            ],
            default: Some("fast".into()),
        })
    );
}

#[test]
fn asks_with_the_chosen_model() {
    let asker = Asker::new();
    assert_eq!(answer(&asker.ask(&mock_agent(), "MODEL")), "model=fast");
    let smart = AgentConfig {
        model: Some("smart".into()),
        ..mock_agent()
    };
    assert_eq!(answer(&asker.ask(&smart, "MODEL")), "model=smart");
    // Unknown models fall back to the default instead of failing.
    let gone = AgentConfig {
        model: Some("retired".into()),
        ..mock_agent()
    };
    assert_eq!(answer(&asker.ask(&gone, "MODEL")), "model=fast");
}

#[test]
fn reports_agents_that_cannot_start() {
    let broken = AgentConfig {
        command: "/nonexistent/agent".into(),
        ..mock_agent()
    };
    match next(&Asker::new().ask(&broken, "hi")) {
        AskEvent::Failed(err) => assert!(err.contains("Mock"), "unexpected error: {err}"),
        other => panic!("expected a failure, got {other:?}"),
    }
}
