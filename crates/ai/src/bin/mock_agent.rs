//! A scripted ACP agent for Quick Ask's integration tests, speaking raw
//! newline-delimited JSON-RPC so the tests also check the wire format.
//!
//! For each prompt it streams "Hello", asks for permission, then streams
//! whether the permission was granted. A prompt containing "SLOW" instead
//! waits for `session/cancel` and ends the turn as cancelled, and one
//! containing "LONG" slowly streams a few paragraphs, for trying the UI.

use std::io::{BufRead, Write};

use serde_json::{Value, json};

fn main() {
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    let mut sessions = 0;

    while let Some(Ok(line)) = lines.next() {
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let id = message["id"].clone();
        match message["method"].as_str() {
            Some("initialize") => reply(
                &id,
                json!({ "protocolVersion": 1, "agentCapabilities": {}, "authMethods": [] }),
            ),
            Some("session/new") => {
                sessions += 1;
                reply(&id, json!({ "sessionId": format!("s{sessions}") }));
            }
            Some("session/prompt") => {
                let session = message["params"]["sessionId"].clone();
                let prompt = message["params"]["prompt"][0]["text"].as_str().unwrap_or("");
                if prompt.contains("LONG") {
                    for word in LONG_ANSWER.split_inclusive(' ') {
                        chunk(&session, word);
                        std::thread::sleep(std::time::Duration::from_millis(15));
                    }
                    reply(&id, json!({ "stopReason": "end_turn" }));
                    continue;
                }
                chunk(&session, "Hello");
                if prompt.contains("SLOW") {
                    wait_for(&mut lines, |m| m["method"] == "session/cancel");
                    reply(&id, json!({ "stopReason": "cancelled" }));
                    continue;
                }
                send(json!({
                    "jsonrpc": "2.0",
                    "id": "permission",
                    "method": "session/request_permission",
                    "params": {
                        "sessionId": session,
                        "toolCall": { "toolCallId": "t1", "title": "Write a file" },
                        "options": [
                            { "optionId": "yes", "name": "Allow", "kind": "allow_once" },
                            { "optionId": "no", "name": "Reject", "kind": "reject_once" }
                        ]
                    }
                }));
                let response = wait_for(&mut lines, |m| m["id"] == "permission");
                let granted = response["result"]["outcome"]["optionId"] == "yes";
                chunk(&session, if granted { " (granted)" } else { " (declined)" });
                reply(&id, json!({ "stopReason": "end_turn" }));
            }
            _ => {}
        }
    }
}

const LONG_ANSWER: &str = "A monad is a way of chaining computations that carry some extra context, \
such as possible failure, multiple results or side effects. It pairs a type constructor with two \
operations: one that wraps a plain value, and one (bind) that feeds a wrapped value into a function \
returning another wrapped value.\n\nIn Rust terms, Option and Result behave monadically: and_then is \
bind, and Some or Ok wraps a value. The ? operator is sugar for the same idea, short-circuiting on \
None or Err.\n\nThe laws (left identity, right identity, associativity) just guarantee that \
chaining behaves the way you'd expect regardless of how you group the steps.";

fn wait_for(
    lines: &mut impl Iterator<Item = std::io::Result<String>>,
    matches: impl Fn(&Value) -> bool,
) -> Value {
    for line in lines.flatten() {
        if let Ok(message) = serde_json::from_str::<Value>(&line)
            && matches(&message)
        {
            return message;
        }
    }
    std::process::exit(0);
}

fn chunk(session: &Value, text: &str) {
    send(json!({
        "jsonrpc": "2.0",
        "method": "session/update",
        "params": {
            "sessionId": session,
            "update": {
                "sessionUpdate": "agent_message_chunk",
                "content": { "type": "text", "text": text }
            }
        }
    }));
}

fn reply(id: &Value, result: Value) {
    send(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
}

fn send(message: Value) {
    let mut stdout = std::io::stdout().lock();
    let _ = writeln!(stdout, "{message}");
    let _ = stdout.flush();
}
