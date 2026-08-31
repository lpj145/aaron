use node::{NodeId, Uuid};
use planus::{ReadAsRoot, WriteAsOffset};
use std::fmt;
use std::net::SocketAddr;
use std::str::FromStr;

use crate::member::{Member, MemberStatus};
use crate::proto;

/// Errors that can occur during membership message serialization and deserialization.
#[derive(Debug)]
pub enum MessageError {
    Planus(planus::Error),
    InvalidSocketAddr {
        raw: String,
        err: std::net::AddrParseError,
    },
    MissingField(&'static str),
    EmptyPayload,
}

impl fmt::Display for MessageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Planus(err) => write!(f, "FlatBuffers decoding error: {err}"),
            Self::InvalidSocketAddr { raw, err } => {
                write!(f, "Invalid socket address string '{raw}': {err}")
            }
            Self::MissingField(field) => {
                write!(
                    f,
                    "Missing expected required field in FlatBuffers message: {field}"
                )
            }
            Self::EmptyPayload => write!(f, "Unrecognized or empty message payload"),
        }
    }
}

impl std::error::Error for MessageError {}

impl From<planus::Error> for MessageError {
    fn from(err: planus::Error) -> Self {
        Self::Planus(err)
    }
}

/// Strongly-typed network message exchanged between cluster nodes over QUIC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    /// Direct probe sent to a target node.
    Ping {
        seq: u64,
        sender: Member,
        gossip: Vec<Member>,
    },
    /// Direct acknowledgement sent in response to a Ping or PingReq.
    Ack {
        seq: u64,
        sender: Member,
        gossip: Vec<Member>,
    },
    /// Request to an intermediary node to indirectly probe a suspect target.
    PingReq {
        seq: u64,
        target: Member,
        sender: Member,
        gossip: Vec<Member>,
    },
    /// Cluster bootstrap join request sent to seed nodes.
    JoinRequest { sender: Member },
    /// Cluster bootstrap join response returned by a seed node with current members and cluster_id.
    /// Cluster bootstrap join response returned by a seed node with current members and cluster_id.
    JoinResponse {
        cluster_id: Uuid,
        members: Vec<Member>,
    },
    /// Dynamic configuration update broadcast over QUIC.
    ConfigUpdate {
        tracing_filter: String,
        probe_interval_ms: u64,
        probe_timeout_ms: u64,
        suspect_timeout_ms: u64,
        indirect_ping_targets: u32,
        gossip_fanout: u32,
        env_key: String,
        env_val: String,
        sender: Member,
    },
    /// Direct acknowledgement of configuration update.
    ConfigAck {
        success: bool,
        sender: Member,
    },
}

impl Message {
    /// Returns a reference to the sender member if present in this message.
    pub fn sender(&self) -> Option<&Member> {
        match self {
            Self::Ping { sender, .. }
            | Self::Ack { sender, .. }
            | Self::PingReq { sender, .. }
            | Self::JoinRequest { sender }
            | Self::ConfigUpdate { sender, .. }
            | Self::ConfigAck { sender, .. } => Some(sender),
            Self::JoinResponse { .. } => None,
        }
    }

    /// Serializes this message into FlatBuffers binary bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut builder = planus::Builder::new();

        let payload = match self {
            Self::Ping {
                seq,
                sender,
                gossip,
            } => {
                let sender_rec = member_to_record(sender);
                let gossip_recs: Vec<_> = gossip.iter().map(member_to_record).collect();
                proto::MessagePayload::Ping(Box::new(proto::Ping {
                    seq: *seq,
                    sender: Some(Box::new(sender_rec)),
                    gossip: Some(gossip_recs),
                }))
            }
            Self::Ack {
                seq,
                sender,
                gossip,
            } => {
                let sender_rec = member_to_record(sender);
                let gossip_recs: Vec<_> = gossip.iter().map(member_to_record).collect();
                proto::MessagePayload::Ack(Box::new(proto::Ack {
                    seq: *seq,
                    sender: Some(Box::new(sender_rec)),
                    gossip: Some(gossip_recs),
                }))
            }
            Self::PingReq {
                seq,
                target,
                sender,
                gossip,
            } => {
                let target_rec = member_to_record(target);
                let sender_rec = member_to_record(sender);
                let gossip_recs: Vec<_> = gossip.iter().map(member_to_record).collect();
                proto::MessagePayload::PingReq(Box::new(proto::PingReq {
                    seq: *seq,
                    target: Some(Box::new(target_rec)),
                    sender: Some(Box::new(sender_rec)),
                    gossip: Some(gossip_recs),
                }))
            }
            Self::JoinRequest { sender } => {
                let sender_rec = member_to_record(sender);
                proto::MessagePayload::JoinRequest(Box::new(proto::JoinRequest {
                    sender: Some(Box::new(sender_rec)),
                }))
            }
            Self::JoinResponse {
                cluster_id,
                members,
            } => {
                let cluster_bytes = cluster_id.to_bytes();
                let proto_cluster_id = proto::aaron::node::Uuid {
                    high: u64::from_be_bytes(cluster_bytes[0..8].try_into().unwrap()),
                    low: u64::from_be_bytes(cluster_bytes[8..16].try_into().unwrap()),
                };
                let member_recs: Vec<_> = members.iter().map(member_to_record).collect();
                proto::MessagePayload::JoinResponse(Box::new(proto::JoinResponse {
                    cluster_id: Some(proto_cluster_id),
                    members: Some(member_recs),
                }))
            }
            Self::ConfigUpdate {
                tracing_filter,
                probe_interval_ms,
                probe_timeout_ms,
                suspect_timeout_ms,
                indirect_ping_targets,
                gossip_fanout,
                env_key,
                env_val,
                sender,
            } => {
                let sender_rec = member_to_record(sender);
                proto::MessagePayload::ConfigUpdate(Box::new(proto::ConfigUpdate {
                    tracing_filter: Some(tracing_filter.clone()),
                    probe_interval_ms: *probe_interval_ms,
                    probe_timeout_ms: *probe_timeout_ms,
                    suspect_timeout_ms: *suspect_timeout_ms,
                    indirect_ping_targets: *indirect_ping_targets,
                    gossip_fanout: *gossip_fanout,
                    env_key: Some(env_key.clone()),
                    env_val: Some(env_val.clone()),
                    sender: Some(Box::new(sender_rec)),
                }))
            }
            Self::ConfigAck { success, sender } => {
                let sender_rec = member_to_record(sender);
                proto::MessagePayload::ConfigAck(Box::new(proto::ConfigAck {
                    success: *success,
                    sender: Some(Box::new(sender_rec)),
                }))
            }
        };

        let msg = proto::MembershipMessage {
            payload: Some(payload),
        };
        let offset = msg.prepare(&mut builder);
        builder.finish(offset, None).to_vec()
    }

    /// Deserializes a FlatBuffers binary buffer into a strongly-typed `Message`.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, MessageError> {
        let msg_ref = proto::MembershipMessageRef::read_as_root(bytes)?;
        let payload_ref = msg_ref.payload()?.ok_or(MessageError::EmptyPayload)?;

        match payload_ref {
            proto::MessagePayloadRef::Ping(ping) => {
                let seq = ping.seq()?;
                let sender_ref = ping
                    .sender()?
                    .ok_or(MessageError::MissingField("Ping.sender"))?;
                let sender = record_ref_to_member(sender_ref)?;

                let mut gossip = Vec::new();
                if let Some(vec) = ping.gossip()? {
                    for r in vec {
                        gossip.push(record_ref_to_member(r?)?);
                    }
                }
                Ok(Self::Ping {
                    seq,
                    sender,
                    gossip,
                })
            }
            proto::MessagePayloadRef::Ack(ack) => {
                let seq = ack.seq()?;
                let sender_ref = ack
                    .sender()?
                    .ok_or(MessageError::MissingField("Ack.sender"))?;
                let sender = record_ref_to_member(sender_ref)?;

                let mut gossip = Vec::new();
                if let Some(vec) = ack.gossip()? {
                    for r in vec {
                        gossip.push(record_ref_to_member(r?)?);
                    }
                }
                Ok(Self::Ack {
                    seq,
                    sender,
                    gossip,
                })
            }
            proto::MessagePayloadRef::PingReq(ping_req) => {
                let seq = ping_req.seq()?;
                let target_ref = ping_req
                    .target()?
                    .ok_or(MessageError::MissingField("PingReq.target"))?;
                let target = record_ref_to_member(target_ref)?;

                let sender_ref = ping_req
                    .sender()?
                    .ok_or(MessageError::MissingField("PingReq.sender"))?;
                let sender = record_ref_to_member(sender_ref)?;

                let mut gossip = Vec::new();
                if let Some(vec) = ping_req.gossip()? {
                    for r in vec {
                        gossip.push(record_ref_to_member(r?)?);
                    }
                }
                Ok(Self::PingReq {
                    seq,
                    target,
                    sender,
                    gossip,
                })
            }
            proto::MessagePayloadRef::JoinRequest(join_req) => {
                let sender_ref = join_req
                    .sender()?
                    .ok_or(MessageError::MissingField("JoinRequest.sender"))?;
                let sender = record_ref_to_member(sender_ref)?;
                Ok(Self::JoinRequest { sender })
            }
            proto::MessagePayloadRef::JoinResponse(join_resp) => {
                let cluster_ref = join_resp
                    .cluster_id()?
                    .ok_or(MessageError::MissingField("JoinResponse.cluster_id"))?;
                let cluster_id = Uuid::new(cluster_ref.high(), cluster_ref.low());

                let mut members = Vec::new();
                if let Some(vec) = join_resp.members()? {
                    for r in vec {
                        members.push(record_ref_to_member(r?)?);
                    }
                }
                Ok(Self::JoinResponse {
                    cluster_id,
                    members,
                })
            }
            proto::MessagePayloadRef::ConfigUpdate(update) => {
                let tracing_filter = update.tracing_filter()?.unwrap_or_default().to_string();
                let probe_interval_ms = update.probe_interval_ms()?;
                let probe_timeout_ms = update.probe_timeout_ms()?;
                let suspect_timeout_ms = update.suspect_timeout_ms()?;
                let indirect_ping_targets = update.indirect_ping_targets()?;
                let gossip_fanout = update.gossip_fanout()?;
                let env_key = update.env_key()?.unwrap_or_default().to_string();
                let env_val = update.env_val()?.unwrap_or_default().to_string();
                let sender_ref = update
                    .sender()?
                    .ok_or(MessageError::MissingField("ConfigUpdate.sender"))?;
                let sender = record_ref_to_member(sender_ref)?;
                Ok(Self::ConfigUpdate {
                    tracing_filter,
                    probe_interval_ms,
                    probe_timeout_ms,
                    suspect_timeout_ms,
                    indirect_ping_targets,
                    gossip_fanout,
                    env_key,
                    env_val,
                    sender,
                })
            }
            proto::MessagePayloadRef::ConfigAck(ack) => {
                let success = ack.success()?;
                let sender_ref = ack
                    .sender()?
                    .ok_or(MessageError::MissingField("ConfigAck.sender"))?;
                let sender = record_ref_to_member(sender_ref)?;
                Ok(Self::ConfigAck { success, sender })
            }
        }
    }
}

fn member_to_record(m: &Member) -> proto::MemberRecord {
    let id_bytes = m.node_id.id().to_bytes();
    let cluster_bytes = m.node_id.cluster_id.map(|c| c.to_bytes());

    let proto_node_id = proto::aaron::node::NodeId {
        id: Some(proto::aaron::node::Uuid {
            high: u64::from_be_bytes(id_bytes[0..8].try_into().unwrap()),
            low: u64::from_be_bytes(id_bytes[8..16].try_into().unwrap()),
        }),
        incarnation: m.node_id.incarnation,
        cluster_id: cluster_bytes.map(|c| proto::aaron::node::Uuid {
            high: u64::from_be_bytes(c[0..8].try_into().unwrap()),
            low: u64::from_be_bytes(c[8..16].try_into().unwrap()),
        }),
    };

    let proto_status = match m.status {
        MemberStatus::Alive => proto::MemberStatus::Alive,
        MemberStatus::Suspect => proto::MemberStatus::Suspect,
        MemberStatus::Dead => proto::MemberStatus::Dead,
        MemberStatus::Left => proto::MemberStatus::Left,
    };

    proto::MemberRecord {
        node_id: Some(Box::new(proto_node_id)),
        addr: Some(m.addr.to_string()),
        status: proto_status,
        incarnation: m.incarnation,
    }
}

fn record_ref_to_member(r: proto::MemberRecordRef<'_>) -> Result<Member, MessageError> {
    let node_ref = r
        .node_id()?
        .ok_or(MessageError::MissingField("MemberRecord.node_id"))?;

    let id_ref = node_ref
        .id()?
        .ok_or(MessageError::MissingField("NodeId.id"))?;

    let id = Uuid::new(id_ref.high(), id_ref.low());
    let cluster_id = node_ref.cluster_id()?.map(|c| Uuid::new(c.high(), c.low()));

    let node_id = NodeId::new(id, node_ref.incarnation()?, cluster_id);

    let addr_str = r
        .addr()?
        .ok_or(MessageError::MissingField("MemberRecord.addr"))?;
    let addr = SocketAddr::from_str(addr_str).map_err(|err| MessageError::InvalidSocketAddr {
        raw: addr_str.to_string(),
        err,
    })?;

    let status = match r.status()? {
        proto::MemberStatus::Alive => MemberStatus::Alive,
        proto::MemberStatus::Suspect => MemberStatus::Suspect,
        proto::MemberStatus::Dead => MemberStatus::Dead,
        proto::MemberStatus::Left => MemberStatus::Left,
    };

    let incarnation = r.incarnation()?;

    Ok(Member {
        node_id,
        addr,
        status,
        incarnation,
    })
}
