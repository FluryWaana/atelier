use super::{ACP_DEFAULTS, AgentType, ManualUpdate};

pub const AGENT: AgentType = AgentType {
    id: "grok",
    name: "Grok",
    executable: Some("grok"),
    home_directory: Some(".grok/bin"),
    launch_args: &["agent", "stdio"],
    config_environment: Some("GROK_HOME"),
    disable_update_environment: Some(("GROK_DISABLE_AUTOUPDATER", "1")),
    manual_update: Some(ManualUpdate {
        package: "grok-native",
        command: "update",
        root: ".grok",
        root_environment: Some("GROK_HOME"),
    }),
    ..ACP_DEFAULTS
};
