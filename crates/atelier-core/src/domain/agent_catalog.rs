#[derive(Clone, Copy)]
pub struct ManualUpdate {
    pub package: &'static str,
    pub command: &'static str,
    pub root: &'static str,
    pub root_environment: Option<&'static str>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AuthenticationProbe {
    ExitStatus,
    Json,
    None,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProtocolKind {
    Codex,
    Claude,
    Command,
    Acp,
}

#[derive(Clone, Copy)]
pub struct AgentType {
    pub id: &'static str,
    pub efforts: &'static [&'static str],
    pub custom_effort: bool,
    pub api_auth: bool,
    pub name: &'static str,
    pub executable: Option<&'static str>,
    pub home_directory: Option<&'static str>,
    pub local_directory: Option<&'static str>,
    pub supported: bool,
    pub default_profile: bool,
    pub authentication_probe: AuthenticationProbe,
    pub status_args: &'static [&'static str],
    pub secret_environment: &'static [&'static str],
    pub protocol: ProtocolKind,
    pub launch_args: &'static [&'static str],
    pub config_environment: Option<&'static str>,
    pub disable_update_environment: Option<(&'static str, &'static str)>,
    pub manual_update: Option<ManualUpdate>,
    pub registry_package: Option<&'static str>,
}

const ACP_DEFAULTS: AgentType = AgentType {
    id: "",
    name: "",
    executable: None,
    home_directory: None,
    local_directory: None,
    supported: true,
    default_profile: false,
    authentication_probe: AuthenticationProbe::None,
    status_args: &[],
    secret_environment: &[],
    protocol: ProtocolKind::Acp,
    launch_args: &[],
    config_environment: None,
    disable_update_environment: None,
    manual_update: None,
    registry_package: None,
    efforts: &[],
    custom_effort: true,
    api_auth: false,
};

pub fn update_package_supported(package: &str, manual: bool) -> bool {
    AGENTS.iter().any(|agent| {
        if manual {
            agent
                .manual_update
                .is_some_and(|update| update.package == package)
        } else {
            agent.registry_package == Some(package)
        }
    })
}

mod acp;
mod claude;
mod codex;
mod command;
mod copilot;
mod gemini;
mod grok;
mod opencode;

pub const AGENTS: &[AgentType] = &[
    codex::AGENT,
    claude::AGENT,
    command::AGENT,
    acp::AGENT,
    grok::AGENT,
    opencode::AGENT,
    gemini::AGENT,
    copilot::AGENT,
];

pub fn find(id: &str) -> Option<&'static AgentType> {
    AGENTS
        .iter()
        .find(|agent| agent.id == id && agent.supported)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::agent_options::agent_options;
    use serde_json::json;

    #[test]
    fn catalog_controls_effort_validation() {
        let ids = AGENTS
            .iter()
            .map(|agent| agent.id)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(ids.len(), AGENTS.len());
        assert!(
            AGENTS
                .iter()
                .all(|agent| !agent.id.is_empty() && !agent.name.is_empty())
        );
        assert!(
            AGENTS
                .iter()
                .filter(|agent| agent.protocol == ProtocolKind::Acp && agent.executable.is_some())
                .all(|agent| !agent.launch_args.is_empty())
        );
        for agent in AGENTS.iter().filter(|agent| agent.supported) {
            for effort in agent.efforts {
                assert!(agent_options(agent.id, &json!({"effort": effort}), None).is_ok());
            }
        }
        assert!(agent_options("codex", &json!({"effort": "max"}), None).is_ok());
        assert!(
            agent_options(
                "opencode",
                &json!({"model": "{\"provider\":\"synthetic\"}"}),
                None
            )
            .is_ok()
        );
        assert!(agent_options("opencode", &json!({"model": "bad\nmodel"}), None).is_err());
        assert!(agent_options("command", &json!({"effort": "custom"}), None).is_ok());
        assert!(agent_options("unknown", &json!({}), None).is_err());
        assert!(update_package_supported("grok-native", true));
        assert!(!update_package_supported("grok-native", false));
        assert!(update_package_supported("@openai/codex", false));
    }

    #[test]
    fn custom_acp_requires_an_executable_and_valid_arguments() {
        use crate::domain::agent_options::provider_settings;
        let profile = json!({"id":"custom","name":"Custom","type":"acp","command":"agent","protocolArgs":["serve"]});
        let saved = provider_settings(&serde_json::value::to_raw_value(&profile).unwrap()).unwrap();
        assert_eq!(
            saved.get::<Vec<String>>("protocolArgs").unwrap().unwrap(),
            ["serve"]
        );
        let mut invalid = profile.clone();
        invalid["command"] = json!("");
        assert!(provider_settings(&serde_json::value::to_raw_value(&invalid).unwrap()).is_err());
        invalid["command"] = json!("agent");
        invalid["protocolArgs"] = json!([{"bad":true}]);
        assert!(provider_settings(&serde_json::value::to_raw_value(&invalid).unwrap()).is_err());
    }
}
