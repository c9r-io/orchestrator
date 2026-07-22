use std::collections::HashMap;

pub(super) trait AgentLookup {
    fn get_agent(&self, name: &str) -> Option<&crate::config::AgentConfig>;
    fn has_capability(&self, capability: &str) -> bool;
    fn agents_with_capability<'a>(
        &'a self,
        capability: &'a str,
    ) -> Vec<(&'a str, &'a crate::config::AgentConfig)>;
}

impl AgentLookup for HashMap<String, crate::config::AgentConfig> {
    fn get_agent(&self, name: &str) -> Option<&crate::config::AgentConfig> {
        self.get(name)
    }

    fn has_capability(&self, capability: &str) -> bool {
        self.values().any(|a| a.supports_capability(capability))
    }

    fn agents_with_capability<'a>(
        &'a self,
        capability: &'a str,
    ) -> Vec<(&'a str, &'a crate::config::AgentConfig)> {
        self.iter()
            .filter(|(_, agent)| agent.enabled && agent.supports_capability(capability))
            .map(|(name, agent)| (name.as_str(), agent))
            .collect()
    }
}

impl AgentLookup for HashMap<String, &crate::config::AgentConfig> {
    fn get_agent(&self, name: &str) -> Option<&crate::config::AgentConfig> {
        self.get(name).copied()
    }

    fn has_capability(&self, capability: &str) -> bool {
        self.values().any(|a| a.supports_capability(capability))
    }

    fn agents_with_capability<'a>(
        &'a self,
        capability: &'a str,
    ) -> Vec<(&'a str, &'a crate::config::AgentConfig)> {
        self.iter()
            .filter(|(_, agent)| agent.enabled && agent.supports_capability(capability))
            .map(|(name, agent)| (name.as_str(), *agent))
            .collect()
    }
}
