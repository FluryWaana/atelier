use super::{AgentType, AuthenticationProbe, ProtocolKind};

pub const AGENT: AgentType = AgentType {
    id: "command",
    efforts: &[],
    custom_effort: true,
    api_auth: false,
    name: "Commande personnalisée",
    executable: None,
    home_directory: None,
    local_directory: None,
    supported: true,
    default_profile: false,
    authentication_probe: AuthenticationProbe::Json,
    status_args: &[],
    secret_environment: &[],
    protocol: ProtocolKind::Command,
    launch_args: &[],
    config_environment: None,
    disable_update_environment: None,
    manual_update: None,
    registry_package: None,
};
