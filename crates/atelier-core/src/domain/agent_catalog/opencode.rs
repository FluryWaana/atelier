use super::{ACP_DEFAULTS, AgentType};

pub const AGENT: AgentType = AgentType {
    id: "opencode",
    name: "OpenCode",
    executable: Some("opencode"),
    home_directory: Some(".opencode/bin"),
    launch_args: &["acp"],
    config_environment: Some("OPENCODE_CONFIG_DIR"),
    disable_update_environment: Some(("OPENCODE_DISABLE_AUTOUPDATE", "1")),
    ..ACP_DEFAULTS
};
