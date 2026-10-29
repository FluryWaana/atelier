use atelier_core::{
    application::agent_protocol::{Kind, Protocol, ProtocolConfig},
    contracts::AgentAnswer,
};

#[test]
fn launch_configuration_preserves_provider_modes() {
    assert!(Kind::from_id("unknown").is_none());
    assert_eq!(
        Kind::Codex.launch_arguments("codex", false, "", "", true),
        [
            "app-server",
            "-c",
            "cli_auth_credentials_store=\"ephemeral\""
        ]
    );
    let args = Kind::Claude.launch_arguments("claude", false, "model", "high", false);
    assert!(
        args.windows(2)
            .any(|pair| pair == ["--permission-mode", "plan"])
    );
    assert!(args.windows(2).any(|pair| pair == ["--model", "model"]));
    assert!(args.windows(2).any(|pair| pair == ["--effort", "high"]));
    assert_eq!(
        Kind::from_id("grok")
            .unwrap()
            .launch_arguments("grok", true, "", "", false),
        ["agent", "stdio"]
    );
    assert_eq!(
        Kind::from_id("opencode")
            .unwrap()
            .launch_arguments("opencode", true, "", "", false),
        ["acp"]
    );
    assert_eq!(
        Kind::from_id("gemini")
            .unwrap()
            .launch_arguments("gemini", true, "", "", false),
        ["--acp"]
    );
    assert_eq!(
        Kind::from_id("copilot")
            .unwrap()
            .launch_arguments("copilot", true, "", "", false),
        ["--acp", "--stdio"]
    );
}
use serde_json::{Value, json};

fn priced_codex(model: &str) -> Protocol {
    let mut protocol = Protocol::new(ProtocolConfig {
        kind: Kind::Codex,
        edit: false,
        repo: "synthetic/worktree".into(),
        prompt: "Synthetic pricing only".into(),
        model: model.into(),
        effort: String::new(),
        api_key: String::new(),
        secrets: vec![],
    });
    protocol.start().unwrap();
    protocol
        .receive(json!({"id":"initialize","result":{}}))
        .unwrap();
    protocol
        .receive(json!({"id":"thread","result":{"thread":{"id":"priced-thread"}}}))
        .unwrap();
    protocol
}

#[test]
fn codex_estimates_are_dated_requested_model_equivalents_and_snapshots_are_not_added() {
    let mut protocol = priced_codex("gpt-6.1-sol");
    let event = json!({"method":"thread/tokenUsage/updated","params":{"threadId":"priced-thread","tokenUsage":{"total":{"inputTokens":1000,"cachedInputTokens":200,"outputTokens":500}}}});
    for _ in 0..2 {
        let usage = protocol.receive(event.clone()).unwrap().usage.unwrap();
        assert!((usage["estimatedCostUsd"].as_f64().unwrap() - 0.00662).abs() < 1e-12);
        assert_eq!(usage["inputTokens"], 1000);
        assert_eq!(usage["pricingModel"], "gpt-6.1-sol");
        assert_eq!(usage["pricingDate"], "2026-10-26");
        assert_eq!(usage["costScope"], "requested-model-thread");
    }
    let mut foreign = event.clone();
    foreign["params"]["threadId"] = json!("another-thread");
    assert!(protocol.receive(foreign).unwrap().usage.is_none());
    assert!(protocol.receive(json!({"method":"model/rerouted","params":{"threadId":"another-thread","toModel":"gpt-6-astra"}})).unwrap().usage.is_none());
    let invalidated = protocol.receive(json!({"method":"model/rerouted","params":{"threadId":"priced-thread","toModel":"gpt-6-astra"}})).unwrap().usage.unwrap();
    assert!(invalidated.get("estimatedCostUsd").is_none());
    assert_eq!(invalidated["inputTokens"], 1000);
    assert!(
        protocol
            .receive(event)
            .unwrap()
            .usage
            .unwrap()
            .get("estimatedCostUsd")
            .is_none()
    );
}

#[test]
fn unknown_models_and_missing_or_inconsistent_cache_counters_never_become_zero_cost() {
    for (model, cached) in [
        ("synthetic-unknown", json!(0)),
        ("gpt-6.1-sol", Value::Null),
        ("gpt-6.1-sol", json!(11)),
    ] {
        let mut protocol = priced_codex(model);
        let usage = protocol.receive(json!({"method":"thread/tokenUsage/updated","params":{"threadId":"priced-thread","tokenUsage":{"total":{"inputTokens":10,"cachedInputTokens":cached,"outputTokens":2}}}})).unwrap().usage.unwrap();
        assert!(usage.get("estimatedCostUsd").is_none());
        assert_eq!(usage["inputTokens"], 10);
    }
    let mut protocol = priced_codex("gpt-6.1-sol");
    let usage = protocol.receive(json!({"method":"thread/tokenUsage/updated","params":{"threadId":"priced-thread","tokenUsage":{"total":{"inputTokens":1_000_000,"cachedInputTokens":1_000_000,"outputTokens":0}}}})).unwrap().usage.unwrap();
    assert_eq!(usage["estimatedCostUsd"], json!(0.1));
}
use std::collections::BTreeMap;

#[test]
fn claude_cost_estimates_keep_their_session_scope_and_invalid_or_crash_zeroes_stay_unknown() {
    for (amount, subtype, expected) in [
        (json!(0.012345), "success", Some(0.012345)),
        (json!(0), "success", Some(0.0)),
        (json!(0), "error_during_execution", None),
        (json!(-1), "success", None),
        (json!("0.1"), "success", None),
        (json!(1e100), "success", None),
    ] {
        let mut protocol = ready(Kind::Claude, false);
        let effects=protocol.receive(json!({"type":"result","subtype":subtype,"is_error":subtype!="success","total_cost_usd":amount,"usage":{"input_tokens":3,"output_tokens":2}})).unwrap();
        let usage = effects.usage.unwrap();
        assert_eq!(usage["estimatedCostUsd"].as_f64(), expected);
        assert_eq!(usage["inputTokens"], 3);
        if expected.is_some() {
            assert_eq!(usage["costSource"], "claude-cli");
            assert_eq!(usage["costScope"], "cli-session");
        } else {
            assert!(usage.get("costScope").is_none());
        }
    }
    let mut protocol = ready(Kind::Claude, false);
    let usage = protocol
        .receive(json!({"type":"result","subtype":"success","is_error":false,"total_cost_usd":0.5}))
        .unwrap()
        .usage
        .unwrap();
    assert!(usage.get("inputTokens").is_none());
    assert_eq!(usage["estimatedCostUsd"], 0.5);
}

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
fn acp_session_requires_user_approval_and_rejects_wrong_session() {
    let mut protocol = Protocol::new(ProtocolConfig {
        kind: Kind::Acp,
        edit: true,
        repo: "/synthetic".into(),
        prompt: "task".into(),
        model: String::new(),
        effort: String::new(),
        api_key: String::new(),
        secrets: vec![],
    });
    assert_eq!(protocol.start().unwrap().send[0]["method"], "initialize");
    assert_eq!(
        protocol
            .receive(json!({"jsonrpc":"2.0","id":"initialize","result":{"protocolVersion":1}}))
            .unwrap()
            .send[0]["method"],
        "session/new"
    );
    assert_eq!(
        protocol
            .receive(json!({"jsonrpc":"2.0","id":"session","result":{"sessionId":"synthetic"}}))
            .unwrap()
            .send[0]["method"],
        "session/prompt"
    );
    let request = json!({"jsonrpc":"2.0","id":7,"method":"session/request_permission","params":{"sessionId":"synthetic","toolCall":{"title":"Edit"},"options":[{"optionId":"allow","kind":"allow_once"},{"optionId":"deny","kind":"reject_once"}]}});
    protocol.receive(request).unwrap();
    let interaction = protocol.interactions().pop().unwrap();
    assert_eq!(interaction.title, "Edit");
    assert!(interaction.title_key.is_none());
    assert!(interaction.can_allow);
    assert!(
        protocol
            .receive(json!({"jsonrpc":"2.0","id":"prompt","result":{"stopReason":"end_turn"}}))
            .is_err()
    );
    let response = protocol.reply(answer(&interaction.id, "allow")).unwrap();
    assert_eq!(response.send[0]["result"]["outcome"]["optionId"], "allow");
    assert!(protocol.receive(json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"wrong","update":{}}})).is_err());
    let usage = protocol.receive(json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"synthetic","update":{"sessionUpdate":"usage_update","used":25,"size":100}}})).unwrap();
    assert_eq!(usage.usage.unwrap()["contextUsedTokens"], 25);
    assert!(protocol.receive(json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"synthetic","update":{"sessionUpdate":"usage_update","used":101,"size":100}}})).is_err());
    protocol.receive(json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"synthetic","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"done"}}}})).unwrap();
    let result = protocol
        .receive(json!({"jsonrpc":"2.0","id":"prompt","result":{"stopReason":"end_turn"}}))
        .unwrap();
    assert_eq!(result.complete, Some(true));
    assert_eq!(result.summary.as_deref(), Some("done"));
}

#[test]
fn acp_analysis_requires_a_known_plan_mode() {
    let new = || {
        Protocol::new(ProtocolConfig {
            kind: Kind::Acp,
            edit: false,
            repo: "/synthetic".into(),
            prompt: "task".into(),
            model: String::new(),
            effort: String::new(),
            api_key: String::new(),
            secrets: vec![],
        })
    };
    let mut absent = new();
    absent.start().unwrap();
    absent
        .receive(json!({"jsonrpc":"2.0","id":"initialize","result":{"protocolVersion":1}}))
        .unwrap();
    assert!(
        absent
            .receive(json!({"jsonrpc":"2.0","id":"session","result":{"sessionId":"synthetic"}}))
            .is_err()
    );
    let mut protocol = new();
    protocol.start().unwrap();
    protocol
        .receive(json!({"jsonrpc":"2.0","id":"initialize","result":{"protocolVersion":1}}))
        .unwrap();
    let mode = protocol.receive(json!({"jsonrpc":"2.0","id":"session","result":{"sessionId":"synthetic","modes":{"availableModes":[{"id":"plan"}]}}})).unwrap();
    assert_eq!(mode.send[0]["method"], "session/set_mode");
    let prompt = protocol
        .receive(json!({"jsonrpc":"2.0","id":"mode","result":{}}))
        .unwrap();
    assert_eq!(prompt.send[0]["method"], "session/prompt");
    protocol.receive(json!({"jsonrpc":"2.0","id":5,"method":"session/request_permission","params":{"sessionId":"synthetic","toolCall":{"title":"Edit"},"options":[{"optionId":"allow","kind":"allow_once"},{"optionId":"deny","kind":"reject_once"}]}})).unwrap();
    let interaction = protocol.interactions().pop().unwrap();
    assert!(!interaction.can_allow);
    assert!(protocol.reply(answer(&interaction.id, "allow")).is_err());
    assert_eq!(
        protocol
            .reply(answer(&interaction.id, "deny"))
            .unwrap()
            .send[0]["result"]["outcome"]["optionId"],
        "deny"
    );

    let mut configured = new();
    configured.start().unwrap();
    configured
        .receive(json!({"jsonrpc":"2.0","id":"initialize","result":{"protocolVersion":1}}))
        .unwrap();
    let setup = configured.receive(json!({"jsonrpc":"2.0","id":"session","result":{"sessionId":"synthetic","configOptions":[{"id":"mode","category":"mode","options":[{"value":"plan"}]}]}})).unwrap();
    assert_eq!(setup.send[0]["method"], "session/set_config_option");
    assert!(configured.receive(json!({"jsonrpc":"2.0","id":"config-mode","result":{"configOptions":[{"id":"mode","currentValue":"edit"}]}})).is_err());
}

#[test]
fn acp_model_and_effort_are_confirmed_before_the_prompt() {
    let mut protocol = Protocol::new(ProtocolConfig {
        kind: Kind::Acp,
        edit: true,
        repo: "/synthetic".into(),
        prompt: "task".into(),
        model: "model-2".into(),
        effort: "high".into(),
        api_key: String::new(),
        secrets: vec![],
    });
    protocol.start().unwrap();
    protocol
        .receive(json!({"jsonrpc":"2.0","id":"initialize","result":{"protocolVersion":1}}))
        .unwrap();
    let options = json!([
        {"id":"model","category":"model","currentValue":"model-1","options":[{"group":"models","options":[{"value":"model-1"},{"value":"model-2"}]}]},
        {"id":"effort","category":"thought_level","currentValue":"low","options":[{"value":"low"},{"value":"high"}]}
    ]);
    let first = protocol.receive(json!({"jsonrpc":"2.0","id":"session","result":{"sessionId":"synthetic","configOptions":options}})).unwrap();
    assert_eq!(first.send[0]["params"]["value"], "model-2");
    let updated = json!([
        {"id":"model","category":"model","currentValue":"model-2","options":[{"value":"model-2"}]},
        {"id":"effort","category":"thought_level","currentValue":"low","options":[{"value":"low"},{"value":"high"}]}
    ]);
    let second = protocol
        .receive(json!({"jsonrpc":"2.0","id":"config-option","result":{"configOptions":updated}}))
        .unwrap();
    assert_eq!(second.send[0]["params"]["value"], "high");
    let final_options = json!([
        {"id":"model","category":"model","currentValue":"model-2","options":[{"value":"model-2"}]},
        {"id":"effort","category":"thought_level","currentValue":"high","options":[{"value":"high"}]}
    ]);
    let prompt = protocol
        .receive(
            json!({"jsonrpc":"2.0","id":"config-option","result":{"configOptions":final_options}}),
        )
        .unwrap();
    assert_eq!(prompt.send[0]["method"], "session/prompt");
}

#[test]
fn acp_form_elicitation_validates_answers_and_rejects_unsupported_modes() {
    let mut protocol = Protocol::new(ProtocolConfig {
        kind: Kind::Acp,
        edit: true,
        repo: "/synthetic".into(),
        prompt: "task".into(),
        model: String::new(),
        effort: String::new(),
        api_key: String::new(),
        secrets: vec![],
    });
    assert!(
        protocol.start().unwrap().send[0]["params"]["clientCapabilities"]["elicitation"]["form"]
            .is_object()
    );
    protocol
        .receive(json!({"jsonrpc":"2.0","id":"initialize","result":{"protocolVersion":1}}))
        .unwrap();
    protocol
        .receive(json!({"jsonrpc":"2.0","id":"session","result":{"sessionId":"synthetic"}}))
        .unwrap();
    let effects = protocol.receive(json!({"jsonrpc":"2.0","id":45,"method":"elicitation/create","params":{"sessionId":"synthetic","mode":"url"}})).unwrap();
    assert_eq!(effects.send[0]["error"]["code"], -32602);
    assert!(protocol.interactions().is_empty());
    assert!(!protocol.waiting());
    let form = json!({"jsonrpc":"2.0","id":46,"method":"elicitation/create","params":{"sessionId":"synthetic","mode":"form","message":"Configure","requestedSchema":{"type":"object","properties":{"count":{"type":"integer","minimum":2},"name":{"type":"string","enum":["one","two"],"minLength":3},"optional":{"type":"boolean"}},"required":["count","name"]}}});
    protocol.receive(form).unwrap();
    let view = protocol.interactions().pop().unwrap();
    assert_eq!(
        view.title_key.as_deref(),
        Some("message.the_agent_needs_your_reply")
    );
    assert!(view.title.is_empty());
    assert_eq!(view.detail, "Configure");
    assert!(view.secret_warning);
    let mut reply = answer(&view.id, "answer");
    reply.answers.insert("0".into(), vec!["bad".into()]);
    reply.answers.insert("1".into(), vec!["one".into()]);
    reply.answers.insert("2".into(), vec![String::new()]);
    assert!(protocol.reply(reply.clone()).is_err());
    assert!(protocol.waiting());
    reply.answers.insert("0".into(), vec!["1".into()]);
    assert!(protocol.reply(reply.clone()).is_err());
    assert!(protocol.waiting());
    reply.answers.insert("0".into(), vec!["3".into()]);
    let result = protocol.reply(reply).unwrap();
    assert_eq!(
        result.send[0]["result"],
        json!({"action":"accept","content":{"count":3,"name":"one"}})
    );
    assert!(!protocol.waiting());
    let unsupported = json!({"jsonrpc":"2.0","id":47,"method":"elicitation/create","params":{"sessionId":"synthetic","mode":"form","message":"Constrained","requestedSchema":{"type":"object","properties":{"name":{"type":"string","format":"email"}}}}});
    assert_eq!(
        protocol.receive(unsupported).unwrap().send[0]["result"]["action"],
        "decline"
    );
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
    let usage = effects.usage.unwrap();
    assert_eq!(usage["inputTokens"], 3);
    assert!(usage["cachedInputTokens"].is_null());
    assert!(usage["cacheCreationInputTokens"].is_null());
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
