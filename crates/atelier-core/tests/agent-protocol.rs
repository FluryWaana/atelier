use atelier_core::{
    application::agent_protocol::{Kind, Protocol, ProtocolConfig},
    contracts::AgentAnswer,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn protocol(kind: Kind, edit: bool) -> Protocol {
    Protocol::new(ProtocolConfig {
        kind,
        edit,
        repo: "synthetic/worktree".into(),
        prompt: "Synthetic task".into(),
        model: "synthetic-model".into(),
        effort: "low".into(),
        api_key: if kind == Kind::Codex {
            "synthetic-key".into()
        } else {
            String::new()
        },
        secrets: vec!["synthetic-token".into()],
    })
}
fn ready(kind: Kind, edit: bool) -> Protocol {
    let mut protocol = protocol(kind, edit);
    assert_eq!(protocol.start().unwrap().send.len(), 1);
    assert!(protocol.start().is_err());
    if kind == Kind::Codex {
        let effects = protocol
            .receive(json!({"id":"initialize","result":{}}))
            .unwrap();
        assert_eq!(effects.send[0]["method"], "initialized");
        assert_eq!(effects.send[1]["method"], "config/read");
        let login=protocol.receive(json!({"id":"credential-storage","result":{"config":{"cli_auth_credentials_store":"ephemeral"}}})).unwrap();
        assert_eq!(login.send[0]["params"]["apiKey"], "synthetic-key");
        let thread = protocol.receive(json!({"id":"login","result":{}})).unwrap();
        assert_eq!(
            thread.send[0]["params"]["sandbox"],
            if edit { "workspace-write" } else { "read-only" }
        );
        let turn = protocol
            .receive(json!({"id":"thread","result":{"thread":{"id":"thread-1"}}}))
            .unwrap();
        assert_eq!(turn.send[0]["params"]["input"][0]["text"], "Synthetic task");
        protocol.receive(json!({"id":"turn","result":{}})).unwrap();
    } else {
        let effects=protocol.receive(json!({"type":"control_response","response":{"request_id":"initialize","subtype":"success"}})).unwrap();
        assert_eq!(effects.send[0]["message"]["content"], "Synthetic task");
    }
    protocol
}
fn answer(id: &str, decision: &str) -> AgentAnswer {
    AgentAnswer {
        request_id: id.into(),
        decision: decision.into(),
        answers: BTreeMap::new(),
    }
}

#[test]
fn codex_approvals_require_reviewable_changes_and_cannot_be_replayed_or_replace_pending_requests() {
    let mut protocol = ready(Kind::Codex, true);
    let request = json!({"id":17,"method":"item/fileChange/requestApproval","params":{"threadId":"thread-1","itemId":"edit-1","reason":"synthetic-token"}});
    protocol.receive(request.clone()).unwrap();
    assert!(protocol.receive(request.clone()).is_err());
    let view = protocol.interactions().pop().unwrap();
    assert!(!view.can_allow);
    assert!(!view.detail.contains("synthetic-token"));
    assert!(protocol.reply(answer(&view.id, "allow")).is_err());
    assert!(protocol.waiting());
    let result = protocol.reply(answer(&view.id, "deny")).unwrap();
    assert_eq!(
        result.send[0],
        json!({"id":17,"result":{"decision":"decline"}})
    );
    assert!(protocol.reply(answer(&view.id, "deny")).is_err());
    protocol.receive(json!({"method":"item/started","params":{"threadId":"thread-1","item":{"type":"fileChange","id":"edit-1","changes":[{"path":"synthetic.txt","diff":"+hello"}]}}})).unwrap();
    protocol.receive(request).unwrap();
    let view = protocol.interactions().pop().unwrap();
    assert!(view.can_allow);
    assert!(view.detail.contains("+hello"));
    assert_eq!(
        protocol.reply(answer(&view.id, "allow")).unwrap().send[0]["result"]["decision"],
        "accept"
    );
    protocol
        .receive(json!({"id":18,"method":"mcpServer/elicitation/request","params":{}}))
        .map(|effects| assert_eq!(effects.send[0]["error"]["code"], -32601))
        .unwrap();
    assert!(protocol.interactions().is_empty());
}

#[test]
fn codex_secret_questions_keep_routing_ids_and_mask_complete_public_messages() {
    let mut protocol = ready(Kind::Codex, false);
    protocol.receive(json!({"id":"wire-7","method":"item/tool/requestUserInput","params":{"threadId":"thread-1","questions":[{"id":"original-id","question":"Synthetic password","isSecret":true}]}})).unwrap();
    let view = protocol.interactions().pop().unwrap();
    let mut reply = answer(&view.id, "answer");
    assert!(protocol.reply(reply.clone()).is_err());
    reply
        .answers
        .insert("0".into(), vec!["private-synthetic-answer".into()]);
    let effects = protocol.reply(reply).unwrap();
    assert_eq!(
        effects.send[0]["result"]["answers"]["original-id"]["answers"][0],
        "private-synthetic-answer"
    );
    let effects=protocol.receive(json!({"method":"item/completed","params":{"threadId":"thread-1","item":{"type":"agentMessage","text":"synthetic-key synthetic-token private-synthetic-answer"}}})).unwrap();
    assert_eq!(
        effects.summary.as_deref(),
        Some("[secret] [secret] [secret]")
    );
    assert!(
        effects
            .text
            .iter()
            .all(|text| !text.contains("private-synthetic-answer"))
    );
    assert!(protocol.receive(json!({"method":"item/completed","params":{"threadId":"unrelated","item":{"type":"agentMessage","text":"unrelated"}}})).unwrap().text.is_empty());
    assert!(protocol.receive(json!({"method":"item/completed","params":{"threadId":"thread-1","item":{"type":"reasoning","text":"private reasoning"}}})).unwrap().text.is_empty());
    protocol
        .receive(json!({"method":"error","params":{"willRetry":false}}))
        .unwrap();
    assert_eq!(protocol.receive(json!({"method":"turn/completed","params":{"threadId":"thread-1","turn":{"status":"completed"}}})).unwrap().complete,Some(false));
}

#[test]
fn claude_questions_retain_original_input_and_deny_cleared_or_invalid_requests() {
    let mut protocol = ready(Kind::Claude, false);
    protocol.receive(json!({"type":"control_request","request_id":"request-1","request":{"subtype":"can_use_tool","tool_name":"AskUserQuestion","input":{"retained":"original","questions":[{"question":"Synthetic choice?","options":[{"label":"Option A","description":"A"}],"multiSelect":true}]}}})).unwrap();
    let view = protocol.interactions().pop().unwrap();
    assert_eq!(view.questions[0].options, vec!["Option A"]);
    let mut reply = answer(&view.id, "answer");
    reply
        .answers
        .insert("0".into(), vec!["Option A".into(), "Other".into()]);
    let effects = protocol.reply(reply).unwrap();
    let input = &effects.send[0]["response"]["response"]["updatedInput"];
    assert_eq!(input["retained"], "original");
    assert_eq!(
        input["answers"]["Synthetic choice?"],
        json!(["Option A", "Other"])
    );
    protocol.receive(json!({"type":"control_request","request_id":"request-2","request":{"subtype":"can_use_tool","tool_name":"Write","input":{"file_path":"synthetic.txt","content":"synthetic-token"}}})).unwrap();
    let view = protocol.interactions().pop().unwrap();
    assert!(!view.can_allow);
    assert!(protocol.reply(answer(&view.id, "allow")).is_err());
    protocol
        .receive(json!({"type":"control_cancel_request","request_id":"request-2"}))
        .unwrap();
    assert!(protocol.reply(answer(&view.id, "deny")).is_err());
    assert!(protocol.receive(json!({"type":"assistant","message":{"content":[{"type":"thinking","thinking":"never publish"}]}})).unwrap().text.is_empty());
    let effects=protocol.receive(json!({"type":"result","subtype":"success","is_error":false,"result":"synthetic-token","usage":{"input_tokens":3,"output_tokens":2}})).unwrap();
    assert_eq!(effects.complete, Some(true));
    assert_eq!(effects.summary.as_deref(), Some("[secret]"));
    assert_eq!(effects.usage.unwrap()["inputTokens"], 3);
}

#[test]
fn malformed_sequences_questions_and_excessive_pending_work_fail_closed() {
    let mut protocol = protocol(Kind::Codex, false);
    assert!(protocol.receive(Value::Null).is_err());
    assert!(
        protocol
            .receive(json!({"method":"turn/completed","params":{"turn":{"status":"completed"}}}))
            .is_err()
    );
    let mut protocol = ready(Kind::Codex, false);
    assert!(protocol.receive(json!({"id":1,"method":"item/tool/requestUserInput","params":{"questions":[{"id":"same","question":"A"},{"id":"same","question":"B"}]}})).is_err());
    assert!(protocol.receive(json!({"id":1,"method":"item/tool/requestUserInput","params":{"questions":[{"id":"a","question":"A"},{"id":"b","question":"A"}]}})).is_err());
    for id in 0..32 {
        protocol.receive(json!({"id":id,"method":"item/commandExecution/requestApproval","params":{"command":"echo synthetic"}})).unwrap();
    }
    assert!(protocol.receive(json!({"id":33,"method":"item/commandExecution/requestApproval","params":{"command":"echo synthetic"}})).is_err());
    protocol
        .receive(json!({"method":"turn/completed","params":{"turn":{"status":"completed"}}}))
        .unwrap();
    assert!(protocol.interactions().is_empty());
}

#[test]
fn streaming_preview_withholds_incomplete_secrets_and_masks_before_truncation() {
    let mut protocol = ready(Kind::Codex, false);
    for character in "é😀 synthetic-key ........................................".chars() {
        let effects=protocol.receive(json!({"method":"item/agentMessage/delta","params":{"threadId":"thread-1","delta":character.to_string()}})).unwrap();
        let preview = effects.preview.unwrap();
        assert!(!preview.contains("synthetic"));
        assert!(!preview.contains("synthe"));
    }
    let effects=protocol.receive(json!({"method":"item/agentMessage/delta","params":{"threadId":"thread-1","delta":".".repeat(6000)}})).unwrap();
    assert!(effects.preview.unwrap().len() <= 4000);
    assert!(protocol.receive(json!({"method":"item/agentMessage/delta","params":{"threadId":"thread-1","delta":"x".repeat(1024*1024)}})).is_err());
}

#[test]
fn api_login_is_never_sent_when_the_cli_cannot_confirm_memory_only_credentials() {
    for storage in [json!("file"), json!("keyring"), Value::Null] {
        let mut protocol = protocol(Kind::Codex, false);
        protocol.start().unwrap();
        let effects = protocol
            .receive(json!({"id":"initialize","result":{}}))
            .unwrap();
        assert!(
            effects
                .send
                .iter()
                .all(|message| message["method"] != "account/login/start")
        );
        assert!(protocol.receive(json!({"id":"credential-storage","result":{"config":{"cli_auth_credentials_store":storage}}})).is_err());
    }
}
