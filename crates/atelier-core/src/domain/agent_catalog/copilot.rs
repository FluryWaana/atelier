use super::{ACP_DEFAULTS, AgentType};

pub const AGENT: AgentType = AgentType {
    id: "copilot",
    name: "GitHub Copilot CLI",
    executable: Some("copilot"),
    launch_args: &["--acp", "--stdio"],
    config_environment: Some("COPILOT_HOME"),
    disable_update_environment: Some(("COPILOT_AUTO_UPDATE", "false")),
    ..ACP_DEFAULTS
};
