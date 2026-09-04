use aaron_core::{ConfigError, ConfigField, Env, ServiceConfig};
use std::net::SocketAddr;

/// Configuration for the Control Plane (Raft Consensus & Replicated State).
#[derive(Clone, Debug)]
pub struct ControlPlaneConfig {
    /// QUIC network address to bind the Raft consensus endpoint to.
    pub bind_addr: SocketAddr,
    /// Numeric Raft Node ID (defaults to deriving from node UUID if not explicitly specified).
    pub node_id: Option<u64>,
    /// Minimum election timeout in milliseconds.
    pub election_timeout_min_ms: u64,
    /// Maximum election timeout in milliseconds.
    pub election_timeout_max_ms: u64,
    /// Leader heartbeat interval in milliseconds.
    pub heartbeat_interval_ms: u64,
    /// Number of applied logs after which a snapshot is generated.
    pub snapshot_threshold: u64,
}

impl Default for ControlPlaneConfig {
    fn default() -> Self {
        Self {
            bind_addr: "0.0.0.0:18946".parse().unwrap(),
            node_id: None,
            election_timeout_min_ms: 250,
            election_timeout_max_ms: 500,
            heartbeat_interval_ms: 50,
            snapshot_threshold: 1000,
        }
    }
}

impl ServiceConfig for ControlPlaneConfig {
    fn schema() -> Vec<ConfigField> {
        vec![
            ConfigField::new("CONTROL_PLANE_BIND_ADDR", "SocketAddr")
                .default("0.0.0.0:18946")
                .description("QUIC listening address for Raft consensus and control plane RPCs"),
            ConfigField::new("RAFT_NODE_ID", "u64")
                .description("Explicit numeric Raft node identifier (defaults to lower 64 bits of node UUID)"),
            ConfigField::new("RAFT_ELECTION_TIMEOUT_MIN_MS", "u64")
                .default("250")
                .description("Minimum Raft leader election timeout in milliseconds"),
            ConfigField::new("RAFT_ELECTION_TIMEOUT_MAX_MS", "u64")
                .default("500")
                .description("Maximum Raft leader election timeout in milliseconds"),
            ConfigField::new("RAFT_HEARTBEAT_INTERVAL_MS", "u64")
                .default("50")
                .description("Raft leader heartbeat broadcast interval in milliseconds"),
            ConfigField::new("RAFT_SNAPSHOT_THRESHOLD", "u64")
                .default("1000")
                .description("Number of log entries applied before triggering a state snapshot"),
        ]
    }

    fn from_env(env: &Env) -> Result<Self, ConfigError> {
        let bind_addr: SocketAddr = env
            .get("CONTROL_PLANE_BIND_ADDR")
            .unwrap_or_else(|| "0.0.0.0:18946".parse().unwrap());

        let node_id: Option<u64> = env.get("RAFT_NODE_ID");
        let election_timeout_min_ms: u64 = env.get("RAFT_ELECTION_TIMEOUT_MIN_MS").unwrap_or(250);
        let election_timeout_max_ms: u64 = env.get("RAFT_ELECTION_TIMEOUT_MAX_MS").unwrap_or(500);
        let heartbeat_interval_ms: u64 = env.get("RAFT_HEARTBEAT_INTERVAL_MS").unwrap_or(50);
        let snapshot_threshold: u64 = env.get("RAFT_SNAPSHOT_THRESHOLD").unwrap_or(1000);

        Ok(Self {
            bind_addr,
            node_id,
            election_timeout_min_ms,
            election_timeout_max_ms,
            heartbeat_interval_ms,
            snapshot_threshold,
        })
    }
}
