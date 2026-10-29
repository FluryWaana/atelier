use super::{ACP_DEFAULTS, AgentType};

pub const AGENT: AgentType = AgentType {
    id: "gemini",
    name: "Gemini CLI",
    executable: Some("gemini"),
    launch_args: &["--acp"],
    config_environment: Some("GEMINI_CLI_HOME"),
    ..ACP_DEFAULTS
};
