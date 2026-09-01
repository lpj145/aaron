use snafu::Snafu;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum ShardError {
    #[snafu(display("Control Plane Raft quorum is not available or local node is not the elected leader"))]
    ControlPlaneUnavailable,

    #[snafu(display("Manual shard assignment requires at least 3 distinct nodes (1 Primary + >=2 Replicas), but received {count} nodes"))]
    InsufficientNodes { count: usize },

    #[snafu(display("The node {node} cannot be both Primary and Replica for shard {shard_id}"))]
    DuplicateNodeAssignment { shard_id: u32, node: String },

    #[snafu(display("Shard ID {shard_id} is out of bounds (total configured: {total_shards})"))]
    InvalidShardId { shard_id: u32, total_shards: u32 },

    #[snafu(display("Serialization error: {source}"))]
    Serialization { source: serde_json::Error },

    #[snafu(display("Raft consensus error: {message}"))]
    Raft { message: String },
}
