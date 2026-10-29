use super::{AgentType, AuthenticationProbe, ProtocolKind};

pub const AGENT: AgentType = AgentType {
    id: "claude",
    efforts: &["low", "medium", "high", "xhigh", "max"],
    custom_effort: false,
    api_auth: false,
    name: "Claude Code",
    executable: Some("claude"),
    home_directory: Some(".local/bin"),
    local_directory: None,
    supported: true,
    default_profile: true,
    authentication_probe: AuthenticationProbe::Json,
    status_args: &["auth", "status"],
    secret_environment: &[
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "CLAUDE_CODE_OAUTH_TOKEN",
    ],
    protocol: ProtocolKind::Claude,
    launch_args: &[],
    config_environment: Some("CLAUDE_CONFIG_DIR"),
    disable_update_environment: None,
    manual_update: None,
    registry_package: Some("@anthropic-ai/claude-code"),
};
