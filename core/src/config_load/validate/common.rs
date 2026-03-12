use std::collections::HashMap;

pub(super) trait AgentLookup {
    fn get_agent(&self, name: &str) -> Option<&crate::config::AgentConfig>;
    fn has_capability(&self, capability: &str) -> bool;
}

impl AgentLookup for HashMap<String, crate::config::AgentConfig> {
    fn get_agent(&self, name: &str) -> Option<&crate::config::AgentConfig> {
        self.get(name)
    }

    fn has_capability(&self, capability: &str) -> bool {
        self.values().any(|a| a.supports_capability(capability))
    }
}

impl AgentLookup for HashMap<String, &crate::config::AgentConfig> {
    fn get_agent(&self, name: &str) -> Option<&crate::config::AgentConfig> {
        self.get(name).copied()
    }

    fn has_capability(&self, capability: &str) -> bool {
        self.values().any(|a| a.supports_capability(capability))
    }
}
