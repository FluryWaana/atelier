use super::{AgentType, AuthenticationProbe, ProtocolKind};

pub const AGENT: AgentType = AgentType {
    id: "codex",
    efforts: &["none", "minimal", "low", "medium", "high", "xhigh"],
    custom_effort: true,
    api_auth: true,
    name: "Codex",
    executable: Some("codex"),
    home_directory: None,
    local_directory: Some("Programs/OpenAI/Codex/bin"),
    supported: true,
    default_profile: true,
    authentication_probe: AuthenticationProbe::ExitStatus,
    status_args: &["login", "status"],
    secret_environment: &["CODEX_API_KEY", "OPENAI_API_KEY", "CODEX_ACCESS_TOKEN"],
    protocol: ProtocolKind::Codex,
    launch_args: &[],
    config_environment: Some("CODEX_HOME"),
    disable_update_environment: None,
    manual_update: None,
    registry_package: Some("@openai/codex"),
};
