use node::{ConfigError, ConfigField, Env, ServiceConfig, Uuid};
use std::str::FromStr;
use std::time::Duration;

/// Configuration for the membership and failure detection service (SWIM-based).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MembershipConfig {
    /// Local address/port for binding the membership QUIC listener (e.g. `"0.0.0.0:7946"`).
    pub bind_addr: String,
    /// Known seed node addresses to contact when joining the cluster.
    pub seeds: Vec<String>,
    /// Cluster ID token. Required for joining nodes (seeds is non-empty), optional for bootstrap nodes.
    pub cluster_id: Option<Uuid>,
    /// Interval between direct failure detector probes (ping).
    pub probe_interval: Duration,
    /// Timeout waiting for direct ping ack before initiating indirect probes.
    pub probe_timeout: Duration,
    /// Timeout while a node is in `Suspect` state before declaring it `Dead`.
    pub suspect_timeout: Duration,
    /// Number of random nodes to request indirect pings through (`PingReq`).
    pub indirect_ping_targets: usize,
    /// Number of random nodes to gossip state updates to on each cycle.
    pub gossip_fanout: usize,
}

impl Default for MembershipConfig {
    fn default() -> Self {
        Self::lan()
    }
}

impl MembershipConfig {
    /// Creates a configuration preset tuned for LAN environments (low latency, fast failure detection).
    pub fn lan() -> Self {
        Self {
            bind_addr: "0.0.0.0:7946".to_string(),
            seeds: Vec::new(),
            cluster_id: None,
            probe_interval: Duration::from_millis(1000),
            probe_timeout: Duration::from_millis(200),
            suspect_timeout: Duration::from_millis(1000),
            indirect_ping_targets: 3,
            gossip_fanout: 3,
        }
    }

    /// Creates a configuration preset tuned for WAN environments (higher latency, jitter tolerance).
    pub fn wan() -> Self {
        Self {
            bind_addr: "0.0.0.0:7946".to_string(),
            seeds: Vec::new(),
            cluster_id: None,
            probe_interval: Duration::from_millis(5000),
            probe_timeout: Duration::from_millis(1000),
            suspect_timeout: Duration::from_millis(10000),
            indirect_ping_targets: 3,
            gossip_fanout: 4,
        }
    }

    /// Creates a new `MembershipConfig` with LAN default settings.
    pub fn new() -> Self {
        Self::lan()
    }

    /// Sets the QUIC bind address.
    pub fn bind_addr(mut self, addr: impl Into<String>) -> Self {
        self.bind_addr = addr.into();
        self
    }

    /// Sets the list of seed node addresses.
    pub fn seeds(mut self, seeds: Vec<String>) -> Self {
        self.seeds = seeds;
        self
    }

    /// Adds a single seed node address.
    pub fn add_seed(mut self, seed: impl Into<String>) -> Self {
        self.seeds.push(seed.into());
        self
    }

    /// Sets the cluster ID authorization token.
    pub fn cluster_id(mut self, cluster_id: Uuid) -> Self {
        self.cluster_id = Some(cluster_id);
        self
    }
}

impl ServiceConfig for MembershipConfig {
    fn schema() -> Vec<ConfigField> {
        vec![
            ConfigField::new("MEMBERSHIP_BIND_ADDR", "String")
                .default("0.0.0.0:7946")
                .description("Local QUIC address and port for the membership service (e.g. 0.0.0.0:7946)"),
            ConfigField::new("MEMBERSHIP_SEEDS", "String")
                .default("")
                .description("Comma-separated list of seed node addresses to contact (e.g. 192.168.1.10:7946,192.168.1.11:7946)"),
            ConfigField::new("MEMBERSHIP_CLUSTER_ID", "String")
                .default("")
                .description("Cluster ID UUID hex token. Strictly required for joining nodes (when seeds is set)"),
        ]
    }

    fn from_env(env: &Env) -> Result<Self, ConfigError> {
        let bind_addr = env
            .get::<String>("MEMBERSHIP_BIND_ADDR")
            .unwrap_or_else(|| "0.0.0.0:7946".to_string());

        let seeds_raw = env.get::<String>("MEMBERSHIP_SEEDS").unwrap_or_default();

        let seeds: Vec<String> = seeds_raw
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let cluster_id = match env.get::<String>("MEMBERSHIP_CLUSTER_ID") {
            Some(ref s) if !s.trim().is_empty() => {
                let parsed = Uuid::from_str(s.trim()).map_err(|_| ConfigError::InvalidValue {
                    service: "membership-service".to_string(),
                    var_name: "MEMBERSHIP_CLUSTER_ID".to_string(),
                    expected_type: "Uuid (32-hex string)".to_string(),
                    raw_value: s.clone(),
                })?;
                Some(parsed)
            }
            _ => None,
        };

        Ok(Self {
            bind_addr,
            seeds,
            cluster_id,
            ..Default::default()
        })
    }
}
