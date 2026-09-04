use aaron_core::{ConfigError, ConfigField, Env, ServiceConfig};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardConfig {
    /// Quantidade total de partições virtuais (configurável via SHARD_TOTAL_COUNT).
    pub total_shards: u32,
    /// Fator de replicação padrão (Primary + Réplicas, mínimo 3).
    pub replication_factor: usize,
    /// Se este nó atua como coordenador no Control Plane.
    pub is_coordinator: bool,
}

impl Default for ShardConfig {
    fn default() -> Self {
        Self {
            total_shards: 1024,
            replication_factor: 3,
            is_coordinator: false,
        }
    }
}

impl ServiceConfig for ShardConfig {
    fn schema() -> Vec<ConfigField> {
        vec![
            ConfigField::new("SHARD_TOTAL_COUNT", "u32")
                .default("1024")
                .description("Total virtual partition count"),
            ConfigField::new("SHARD_REPLICATION_FACTOR", "usize")
                .default("3")
                .description("Default replication factor (Primary + Replicas)"),
            ConfigField::new("SHARD_IS_COORDINATOR", "bool")
                .default("false")
                .description("Whether this node acts as Control Plane Shard Coordinator"),
        ]
    }

    fn from_env(env: &Env) -> Result<Self, ConfigError> {
        let total_shards = env.get::<u32>("SHARD_TOTAL_COUNT").unwrap_or(1024);
        let replication_factor = env.get::<usize>("SHARD_REPLICATION_FACTOR").unwrap_or(3);
        let is_coordinator = env.get::<bool>("SHARD_IS_COORDINATOR").unwrap_or(false);

        Ok(Self {
            total_shards,
            replication_factor,
            is_coordinator,
        })
    }
}
