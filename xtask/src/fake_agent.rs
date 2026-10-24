use serde_json::{Value, json};
use std::io::{self, BufRead, Write};

fn receive(reader: &mut impl BufRead) -> Result<Value, Box<dyn std::error::Error>> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 || line.len() > 512 * 1024 {
        return Err("Synthetic agent input missing or excessive".into());
    }
    Ok(serde_json::from_str(&line)?)
}
fn send(value: Value) -> Result<(), Box<dyn std::error::Error>> {
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, &value)?;
    stdout.write_all(b"\n")?;
    stdout.flush()?;
    Ok(())
}

pub fn run(codex: bool) -> Result<(), Box<dyn std::error::Error>> {
    let mode = std::fs::read_to_string(".agent-run-fixture")?;
    if ![
        "atelier-r03-synthetic-only",
        "atelier-r03-synthetic-malformed",
        "atelier-r03-synthetic-failure",
    ]
    .contains(&mode.as_str())
    {
        return Err("Not an agent run fixture".into());
    }
    let mut input = io::stdin().lock();
    let first = receive(&mut input)?;
    if codex {
        if first["method"] != "initialize" {
            return Err("Missing synthetic Codex handshake".into());
        }
        send(json!({"id":"initialize","result":{}}))?;
        if receive(&mut input)?["method"] != "initialized" {
            return Err("Missing initialized notification".into());
        }
        let mut thread = receive(&mut input)?;
        if thread["method"] == "config/read" {
            send(
                json!({"id":"credential-storage","result":{"config":{"cli_auth_credentials_store":"ephemeral"}}}),
            )?;
            thread = receive(&mut input)?;
        }
        if thread["method"] == "account/login/start" {
            if thread["params"]["apiKey"] != "synthetic-run-key" {
                return Err("Only synthetic credentials accepted".into());
            }
            send(json!({"id":"login","result":{"type":"apiKey"}}))?;
            thread = receive(&mut input)?;
        }
        if thread["method"] != "thread/start" || thread["params"]["approvalPolicy"] != "on-request"
        {
            return Err("Invalid synthetic thread policy".into());
        }
        send(json!({"id":"thread","result":{"thread":{"id":"synthetic-thread"}}}))?;
        if receive(&mut input)?["method"] != "turn/start" {
            return Err("Missing synthetic turn".into());
        }
        send(json!({"id":"turn","result":{}}))?;
        if mode == "atelier-r03-synthetic-malformed" {
            io::stdout().write_all(b"not-json\n")?;
            io::stdout().flush()?;
            return Ok(());
        }
        send(
            json!({"method":"item/completed","params":{"threadId":"synthetic-thread","item":{"type":"reasoning","text":"private-synthetic-reasoning"}}}),
        )?;
        send(
            json!({"id":23,"method":"item/commandExecution/requestApproval","params":{"threadId":"synthetic-thread","command":"echo synthetic","reason":"Synthetic approval"}}),
        )?;
        let approval = receive(&mut input)?;
        if approval["id"] != 23 {
            return Err("Synthetic approval routed incorrectly".into());
        }
        let accepted = approval["result"]["decision"] == "accept";
        send(
            json!({"id":"question-wire","method":"item/tool/requestUserInput","params":{"threadId":"synthetic-thread","questions":[{"id":"original-question","question":"Synthetic private answer?","isSecret":true}]}}),
        )?;
        let answer = receive(&mut input)?;
        if answer["id"] != "question-wire" {
            return Err("Synthetic question routed incorrectly".into());
        }
        let text = answer["result"]["answers"]["original-question"]["answers"][0]
            .as_str()
            .unwrap_or("");
        send(
            json!({"method":"item/agentMessage/delta","params":{"threadId":"synthetic-thread","delta":"synthetic-run-"}}),
        )?;
        send(
            json!({"method":"item/agentMessage/delta","params":{"threadId":"synthetic-thread","delta":"key public preview é😀"}}),
        )?;
        send(
            json!({"method":"item/completed","params":{"threadId":"synthetic-thread","item":{"type":"agentMessage","text":format!("Public synthetic response é😀 synthetic-run-key {text}")}}}),
        )?;
        send(
            json!({"method":"thread/tokenUsage/updated","params":{"threadId":"synthetic-thread","tokenUsage":{"total":{"inputTokens":5,"cachedInputTokens":2,"outputTokens":3}}}}),
        )?;
        send(
            json!({"method":"turn/completed","params":{"threadId":"synthetic-thread","turn":{"status":if accepted && mode!="atelier-r03-synthetic-failure"{"completed"}else{"failed"}}}}),
        )?;
    } else {
        if first["type"] != "control_request" || first["request_id"] != "initialize" {
            return Err("Missing synthetic Claude handshake".into());
        }
        send(
            json!({"type":"control_response","response":{"request_id":"initialize","subtype":"success"}}),
        )?;
        if receive(&mut input)?["type"] != "user" {
            return Err("Missing synthetic Claude prompt".into());
        }
        if mode == "atelier-r03-synthetic-malformed" {
            io::stdout().write_all(b"not-json\n")?;
            io::stdout().flush()?;
            return Ok(());
        }
        send(
            json!({"type":"assistant","message":{"content":[{"type":"thinking","thinking":"private-synthetic-reasoning"}]}}),
        )?;
        send(
            json!({"type":"control_request","request_id":"approval-wire","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"echo synthetic"}}}),
        )?;
        let approval = receive(&mut input)?;
        if approval["response"]["request_id"] != "approval-wire" {
            return Err("Synthetic approval routed incorrectly".into());
        }
        let accepted = approval["response"]["response"]["behavior"] == "allow";
        send(
            json!({"type":"control_request","request_id":"question-wire","request":{"subtype":"can_use_tool","tool_name":"AskUserQuestion","input":{"questions":[{"question":"Synthetic private answer?","isSecret":true}]}}}),
        )?;
        let answer = receive(&mut input)?;
        let text =
            answer["response"]["response"]["updatedInput"]["answers"]["Synthetic private answer?"]
                [0]
            .as_str()
            .unwrap_or("");
        send(
            json!({"type":"assistant","message":{"content":[{"type":"text","text":format!("Public synthetic response é😀 {text}")}]}}),
        )?;
        let success = accepted && mode != "atelier-r03-synthetic-failure";
        send(
            json!({"type":"result","subtype":if success{"success"}else{"error_during_execution"},"is_error":!success,"result":format!("Public synthetic result é😀 {text}"),"usage":{"input_tokens":5,"cache_read_input_tokens":2,"output_tokens":3}}),
        )?;
    }
    // EOF confirms that the owner closed stdin after the final protocol result.
    let mut tail = String::new();
    if input.read_line(&mut tail)? != 0 {
        return Err("Unexpected synthetic message after completion".into());
    }
    Ok(())
}
