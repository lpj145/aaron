use crate::Uuid;

/// Command published on [`crate::EventHub`] to bind and persist the cluster identity for the local node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindClusterIdCommand {
    pub cluster_id: Uuid,
}

impl BindClusterIdCommand {
    /// Creates a new `BindClusterIdCommand` for the given cluster UUID.
    pub fn new(cluster_id: Uuid) -> Self {
        Self { cluster_id }
    }
}

/// Command published on [`crate::EventHub`] to dynamically spawn a new supervised instance of a registered service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartServiceCommand {
    pub service_name: String,
}

impl StartServiceCommand {
    /// Creates a new `StartServiceCommand` for the specified service name.
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
        }
    }
}
