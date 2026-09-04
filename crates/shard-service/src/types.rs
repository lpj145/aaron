use node::Uuid;
pub use node::{MemberRole, ShardEvent, ShardGroup, ShardRole};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub type ShardId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShardStatus {
    Healthy,
    Degraded,
    Unassigned,
}

fn default_service_name() -> String {
    "default".to_string()
}

/// Registro de designação de uma partição por serviço (Estágio 1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShardPlacement {
    #[serde(default = "default_service_name")]
    pub service_name: String,
    pub shard_id: ShardId,
    pub primary: Uuid,
    pub replicas: Vec<Uuid>,
    pub status: ShardStatus,
    pub epoch: u64,
}

impl ShardPlacement {
    pub fn new(shard_id: ShardId, primary: Uuid, replicas: Vec<Uuid>, epoch: u64) -> Self {
        Self::with_service("default", shard_id, primary, replicas, epoch)
    }

    pub fn with_service(
        service_name: impl Into<String>,
        shard_id: ShardId,
        primary: Uuid,
        replicas: Vec<Uuid>,
        epoch: u64,
    ) -> Self {
        Self {
            service_name: service_name.into(),
            shard_id,
            primary,
            replicas,
            status: ShardStatus::Healthy,
            epoch,
        }
    }

    /// Retorna o conjunto de todos os nós participantes (Primary + Réplicas).
    pub fn all_nodes(&self) -> BTreeSet<Uuid> {
        let mut set = BTreeSet::new();
        set.insert(self.primary);
        for r in &self.replicas {
            set.insert(*r);
        }
        set
    }

    /// Retorna a contagem total de nós atribuídos a esta partição.
    pub fn node_count(&self) -> usize {
        self.all_nodes().len()
    }

    /// Serializa o registro de designação da partição com FlatBuffers zero-copy.
    pub fn to_bytes(&self) -> Vec<u8> {
        use crate::proto::aaron::node as proto_node;
        use crate::proto::aaron::shard as proto_shard;
        use planus::WriteAsOffset;

        let mut builder = planus::Builder::new();
        let primary_proto = proto_node::Uuid {
            high: self.primary.high,
            low: self.primary.low,
        };
        let replicas_proto: Vec<proto_node::Uuid> = self
            .replicas
            .iter()
            .map(|u| proto_node::Uuid {
                high: u.high,
                low: u.low,
            })
            .collect();

        let status_proto = match self.status {
            ShardStatus::Healthy => proto_shard::ShardStatus::Healthy,
            ShardStatus::Degraded => proto_shard::ShardStatus::Degraded,
            ShardStatus::Unassigned => proto_shard::ShardStatus::Unassigned,
        };

        let stored = proto_shard::StoredShardPlacement {
            shard_id: self.shard_id,
            primary: Some(primary_proto),
            replicas: Some(replicas_proto),
            status: status_proto,
            epoch: self.epoch,
            service_name: Some(self.service_name.clone()),
        };

        let offset = stored.prepare(&mut builder);
        builder.finish(offset, None).to_vec()
    }

    /// Desserializa o registro de designação a partir de buffer FlatBuffers binário
    /// (com fallback retrocompatível para JSON legado).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, node::BoxError> {
        use crate::proto::aaron::node as proto_node;
        use crate::proto::aaron::shard as proto_shard;
        use planus::ReadAsRoot;

        if let Ok(root_ref) = proto_shard::StoredShardPlacementRef::read_as_root(bytes)
            && let Ok(stored) = proto_shard::StoredShardPlacement::try_from(root_ref) {
                let primary_proto = stored.primary.unwrap_or(proto_node::Uuid { high: 0, low: 0 });
                let primary = Uuid::new(primary_proto.high, primary_proto.low);
                let replicas = stored
                    .replicas
                    .unwrap_or_default()
                    .into_iter()
                    .map(|u| Uuid::new(u.high, u.low))
                    .collect();
                let status = match stored.status {
                    proto_shard::ShardStatus::Healthy => ShardStatus::Healthy,
                    proto_shard::ShardStatus::Degraded => ShardStatus::Degraded,
                    proto_shard::ShardStatus::Unassigned => ShardStatus::Unassigned,
                };
                let service_name = stored.service_name.unwrap_or_else(|| "default".to_string());
                return Ok(Self {
                    service_name,
                    shard_id: stored.shard_id,
                    primary,
                    replicas,
                    status,
                    epoch: stored.epoch,
                });
            }

        // Fallback de retrocompatibilidade para JSON legado
        if let Ok(s) = std::str::from_utf8(bytes)
            && let Ok(p) = serde_json::from_str::<ShardPlacement>(s) {
                return Ok(p);
            }

        Err("Failed to deserialize StoredShardPlacement FlatBuffers binary".into())
    }
}

/// Razões explícitas de recusa de um comando Raft pelo Worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RejectReason {
    None,
    StaleEpoch,
    StaleTerm,
    NotAMember,
    Busy,
}

/// Comandos disparados pelo Control Plane para nós do grupo do Shard (Raft).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShardCommand {
    /// Bootstrap atômico do Worker com todas as suas partições de uma só vez.
    Bootstrap {
        shards: Vec<ShardGroup>,
    },
    /// Adição dinâmica de uma partição em runtime.
    Join {
        shard_id: ShardId,
        members: Vec<Uuid>,
        role: MemberRole,
    },
    /// Altera o papel do nó no quórum (unifica promoção e despromoção).
    SetRole {
        shard_id: ShardId,
        role: MemberRole,
    },
    /// Remove o nó do quórum do shard (Raft leave/shutdown).
    Leave {
        shard_id: ShardId,
    },
    /// Atribuição de partição legada enviada via canal de controle de frame.
    Assign {
        service_name: String,
        shard_id: ShardId,
        role: ShardRole,
        primary: Uuid,
        replicas: Vec<Uuid>,
        epoch: u64,
    },
    /// Notificação de rebalanceamento legado para os membros do shard.
    Rebalance {
        service_name: String,
        shard_id: ShardId,
        target_nodes: Vec<Uuid>,
        epoch: u64,
    },
}

/// Resposta formal do nó após tentar aplicar um ShardCommand no seu Raft local.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShardResponse {
    /// O comando foi aplicado com sucesso no Raft local.
    Applied {
        shard_id: ShardId,
        current_role: MemberRole,
        term: u64,
    },
    /// O nó rejeitou a transição.
    Rejected {
        shard_id: ShardId,
        reason: RejectReason,
    },
}

/// Avisos informativos e confirmações enviados pelo Worker de volta ao Control Plane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShardNotification {
    /// O nó anuncia que foi eleito líder no seu Raft local.
    LeaderElected {
        shard_id: ShardId,
        leader: Uuid,
        term: u64,
    },
    /// O nó anuncia que assumiu a liderança do shard no seu Raft local (legado).
    LeaderAnnounced {
        service_name: String,
        shard_id: ShardId,
        leader: Uuid,
        term: u64,
    },
    /// O worker confirma que concluiu a preparação/pausa do shard para transição.
    PreparedAck {
        service_name: String,
        shard_id: ShardId,
        epoch: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shard_placement_flatbuffers_roundtrip() {
        let primary = Uuid::random();
        let rep1 = Uuid::random();
        let rep2 = Uuid::random();

        let placement = ShardPlacement {
            service_name: "treasurer".to_string(),
            shard_id: 42,
            primary,
            replicas: vec![rep1, rep2],
            status: ShardStatus::Healthy,
            epoch: 1725280000,
        };

        // 1. Serialize to FlatBuffers
        let bytes = placement.to_bytes();
        assert!(!bytes.is_empty());

        // 2. Deserialize from FlatBuffers
        let recovered = ShardPlacement::from_bytes(&bytes).expect("Failed to deserialize FlatBuffers");
        assert_eq!(placement, recovered);

        // 3. Compare size against legacy JSON
        let json_bytes = serde_json::to_vec(&placement).unwrap();
        println!(
            "FlatBuffers size: {} bytes | JSON size: {} bytes",
            bytes.len(),
            json_bytes.len()
        );

        // 4. Test backward compatibility fallback for legacy JSON
        let recovered_from_json =
            ShardPlacement::from_bytes(&json_bytes).expect("Failed to parse legacy JSON");
        assert_eq!(placement, recovered_from_json);
    }
}
