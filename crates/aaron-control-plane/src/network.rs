use crate::message::RaftMessage;
use crate::types::{ControlPlaneNode, TypeConfig};
use aaron_core::{read_frame_with_limit, write_frame_with_limit, QuicManager, DEFAULT_MAX_RAFT_FRAME_SIZE};
use openraft::error::{
    InstallSnapshotError, NetworkError, RPCError, RaftError, Unreachable,
};
use openraft::network::RPCOption;
use openraft::raft::{
    AppendEntriesRequest, AppendEntriesResponse, InstallSnapshotRequest, InstallSnapshotResponse,
    VoteRequest, VoteResponse,
};
use openraft::{RaftNetwork, RaftNetworkFactory, RPCTypes};
use std::sync::Arc;
use std::time::Duration;
use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::sync::{Mutex, RwLock};

#[derive(Clone)]
pub struct ControlPlaneNetworkFactory {
    quic: QuicManager,
    routing_table: Arc<RwLock<HashMap<u64, SocketAddr>>>,
}

impl ControlPlaneNetworkFactory {
    pub fn new(quic: QuicManager) -> Self {
        Self {
            quic,
            routing_table: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn with_routing_table(quic: QuicManager, routing_table: Arc<RwLock<HashMap<u64, SocketAddr>>>) -> Self {
        Self { quic, routing_table }
    }

    pub fn routing_table(&self) -> Arc<RwLock<HashMap<u64, SocketAddr>>> {
        self.routing_table.clone()
    }
}

impl RaftNetworkFactory<TypeConfig> for ControlPlaneNetworkFactory {
    type Network = ControlPlaneNetwork;

    async fn new_client(&mut self, target: u64, node: &ControlPlaneNode) -> Self::Network {
        ControlPlaneNetwork {
            target,
            target_node: node.clone(),
            quic: self.quic.clone(),
            cached_conn: Arc::new(Mutex::new(None)),
            routing_table: self.routing_table.clone(),
        }
    }
}

pub struct ControlPlaneNetwork {
    target: u64,
    target_node: ControlPlaneNode,
    quic: QuicManager,
    cached_conn: Arc<Mutex<Option<quinn::Connection>>>,
    routing_table: Arc<RwLock<HashMap<u64, SocketAddr>>>,
}

impl ControlPlaneNetwork {
    async fn get_or_connect(&self) -> Result<quinn::Connection, RPCError<u64, ControlPlaneNode, RaftError<u64>>> {
        let mut guard = self.cached_conn.lock().await;
        if let Some(conn) = &*guard
            && conn.close_reason().is_none() {
                return Ok(conn.clone());
            }

        let target_addr_str = {
            let table = self.routing_table.read().await;
            if let Some(live_addr) = table.get(&self.target) {
                live_addr.to_string()
            } else {
                self.target_node.addr.clone()
            }
        };

        let conn = self
            .quic
            .connect_node(&target_addr_str, self.target_node.node_uuid())
            .await
            .map_err(|e| RPCError::Unreachable(Unreachable::new(&std::io::Error::other(e.to_string()))))?;

        *guard = Some(conn.clone());
        Ok(conn)
    }

    async fn send_rpc_bytes(
        &self,
        action: RPCTypes,
        bytes: &[u8],
        timeout: Duration,
    ) -> Result<Vec<u8>, RPCError<u64, ControlPlaneNode, RaftError<u64>>> {
        let send_future = async {
            let conn = self.get_or_connect().await?;

            let target_addr_opt = {
                let table = self.routing_table.read().await;
                table.get(&self.target).cloned()
            };

            let (mut send, mut recv) = match conn.open_bi().await {
                Ok(streams) => streams,
                Err(_) => {
                    // Stale connection - invalidate cache, evict from pool, and reconnect
                    *self.cached_conn.lock().await = None;
                    if let Some(ref addr) = target_addr_opt {
                        self.quic.disconnect(addr).await;
                    }
                    let fresh_conn = self.get_or_connect().await?;
                    fresh_conn
                        .open_bi()
                        .await
                        .map_err(|e| RPCError::Network(NetworkError::new(&std::io::Error::other(e.to_string()))))?
                }
            };

            if let Err(e) = write_frame_with_limit(&mut send, bytes, DEFAULT_MAX_RAFT_FRAME_SIZE).await {
                *self.cached_conn.lock().await = None;
                if let Some(ref addr) = target_addr_opt {
                    self.quic.disconnect(addr).await;
                }
                return Err(RPCError::Network(NetworkError::new(&std::io::Error::other(e.to_string()))));
            }

            let _ = send.finish();

            let resp_bytes = match read_frame_with_limit(&mut recv, DEFAULT_MAX_RAFT_FRAME_SIZE).await {
                Ok(Some(b)) => b,
                Ok(None) => {
                    *self.cached_conn.lock().await = None;
                    if let Some(ref addr) = target_addr_opt {
                        self.quic.disconnect(addr).await;
                    }
                    return Err(RPCError::Network(NetworkError::new(&std::io::Error::other("unexpected EOF from peer"))));
                }
                Err(e) => {
                    *self.cached_conn.lock().await = None;
                    if let Some(ref addr) = target_addr_opt {
                        self.quic.disconnect(addr).await;
                    }
                    return Err(RPCError::Network(NetworkError::new(&std::io::Error::other(e.to_string()))));
                }
            };

            Ok::<Vec<u8>, RPCError<u64, ControlPlaneNode, RaftError<u64>>>(resp_bytes)
        };

        match tokio::time::timeout(timeout, send_future).await {
            Ok(res) => res,
            Err(_) => {
                *self.cached_conn.lock().await = None;
                Err(RPCError::Timeout(openraft::error::Timeout {
                    action,
                    id: self.target,
                    target: self.target,
                    timeout,
                }))
            }
        }
    }
}

impl RaftNetwork<TypeConfig> for ControlPlaneNetwork {
    async fn append_entries(
        &mut self,
        req: AppendEntriesRequest<TypeConfig>,
        option: RPCOption,
    ) -> Result<AppendEntriesResponse<u64>, RPCError<u64, ControlPlaneNode, RaftError<u64>>> {
        let timeout = option.hard_ttl();
        let msg = RaftMessage::Append(req);
        let bytes = msg.to_bytes();

        let resp_bytes = self.send_rpc_bytes(RPCTypes::AppendEntries, &bytes, timeout).await?;
        let parsed = RaftMessage::from_bytes(&resp_bytes)
            .map_err(|e| RPCError::Network(NetworkError::new(&std::io::Error::other(e.to_string()))))?;

        match parsed {
            RaftMessage::AppendResp(resp) => Ok(resp),
            _ => Err(RPCError::Network(NetworkError::new(&std::io::Error::other("unexpected RPC response type")))),
        }
    }

    async fn install_snapshot(
        &mut self,
        req: InstallSnapshotRequest<TypeConfig>,
        option: RPCOption,
    ) -> Result<
        InstallSnapshotResponse<u64>,
        RPCError<u64, ControlPlaneNode, RaftError<u64, InstallSnapshotError>>,
    > {
        let timeout = option.hard_ttl();
        let msg = RaftMessage::Snapshot(req);
        let bytes = msg.to_bytes();

        let resp_bytes = self.send_rpc_bytes(RPCTypes::InstallSnapshot, &bytes, timeout).await.map_err(|e| match e {
            RPCError::Timeout(t) => RPCError::Timeout(t),
            RPCError::Unreachable(u) => RPCError::Unreachable(u),
            RPCError::Network(n) => RPCError::Network(n),
            RPCError::PayloadTooLarge(p) => RPCError::PayloadTooLarge(p),
            RPCError::RemoteError(r) => RPCError::Network(NetworkError::new(&std::io::Error::other(r.to_string()))),
        })?;

        let parsed = RaftMessage::from_bytes(&resp_bytes)
            .map_err(|e| RPCError::Network(NetworkError::new(&std::io::Error::other(e.to_string()))))?;

        match parsed {
            RaftMessage::SnapshotResp(resp) => Ok(resp),
            _ => Err(RPCError::Network(NetworkError::new(&std::io::Error::other("unexpected RPC response type")))),
        }
    }

    async fn vote(
        &mut self,
        req: VoteRequest<u64>,
        option: RPCOption,
    ) -> Result<VoteResponse<u64>, RPCError<u64, ControlPlaneNode, RaftError<u64>>> {
        let timeout = option.hard_ttl();
        let msg = RaftMessage::Vote(req);
        let bytes = msg.to_bytes();

        let resp_bytes = self.send_rpc_bytes(RPCTypes::Vote, &bytes, timeout).await?;
        let parsed = RaftMessage::from_bytes(&resp_bytes)
            .map_err(|e| RPCError::Network(NetworkError::new(&std::io::Error::other(e.to_string()))))?;

        match parsed {
            RaftMessage::VoteResp(resp) => Ok(resp),
            _ => Err(RPCError::Network(NetworkError::new(&std::io::Error::other("unexpected RPC response type")))),
        }
    }
}
