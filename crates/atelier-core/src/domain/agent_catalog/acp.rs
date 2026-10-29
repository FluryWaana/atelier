use super::{ACP_DEFAULTS, AgentType};

pub const AGENT: AgentType = AgentType {
    id: "acp",
    name: "Agent ACP personnalisé",
    ..ACP_DEFAULTS
};
