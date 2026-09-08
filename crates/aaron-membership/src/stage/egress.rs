use aaron_core::{BoxError, QuicManager, read_frame, write_frame};
use std::net::SocketAddr;
use std::time::Duration;
use tracing::trace;

use crate::member::Member;
use crate::message::Message;

/// Egress transport client sending outbound SWIM messages over QUIC.
pub struct EgressTransport;

impl EgressTransport {
    /// Sends a generic request message to a remote peer over a QUIC stream and awaits the response message.
    async fn request_response(
        quic: &QuicManager,
        target_addr: SocketAddr,
        request: Message,
        timeout: Duration,
        op_name: &'static str,
    ) -> Result<Message, BoxError> {
        let stream_fut = async {
            let conn = quic.connect(target_addr, "aaron.node").await?;
            let (mut send, mut recv) = match conn.open_bi().await {
                Ok(stream) => stream,
                Err(err) => {
                    quic.pool().remove(&target_addr).await;
                    return Err(Box::new(err) as BoxError);
                }
            };

            let bytes = request.to_bytes();
            if let Err(e) = write_frame(&mut send, &bytes).await {
                quic.pool().remove(&target_addr).await;
                return Err(Box::new(e));
            }
            let _ = send.finish();

            let response_bytes = match read_frame(&mut recv).await {
                Ok(Some(bytes)) => bytes,
                Ok(None) => {
                    quic.pool().remove(&target_addr).await;
                    return Err(Box::new(std::io::Error::other(format!(
                        "unexpected EOF waiting for response to {op_name}"
                    ))) as BoxError);
                }
                Err(e) => {
                    quic.pool().remove(&target_addr).await;
                    return Err(Box::new(e));
                }
            };

            let response = Message::from_bytes(&response_bytes)?;
            Ok(response)
        };

        match tokio::time::timeout(timeout, stream_fut).await {
            Ok(res) => res,
            Err(_) => {
                quic.pool().remove(&target_addr).await;
                Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("{op_name} timed out after {timeout:?}"),
                )))
            }
        }
    }

    /// Sends a Ping message to a target peer over a new QUIC stream and awaits the Ack response.
    pub async fn ping(
        quic: &QuicManager,
        target_addr: SocketAddr,
        ping: Message,
        timeout: Duration,
    ) -> Result<Message, BoxError> {
        trace!(target: "membership::egress", target = %target_addr, "Sending Ping over QUIC");
        Self::request_response(quic, target_addr, ping, timeout, "ping").await
    }

    /// Sends a PingReq message to an intermediary peer, requesting an indirect probe of `target`.
    pub async fn ping_req(
        quic: &QuicManager,
        mediator_addr: SocketAddr,
        ping_req: Message,
        timeout: Duration,
    ) -> Result<Message, BoxError> {
        trace!(
            target: "membership::egress",
            mediator = %mediator_addr,
            "Sending PingReq indirect probe request over QUIC"
        );
        Self::request_response(quic, mediator_addr, ping_req, timeout, "ping_req").await
    }

    /// Sends a JoinRequest to a seed node over QUIC and returns the cluster ID and members.
    pub async fn join(
        quic: &QuicManager,
        seed_addr: SocketAddr,
        local_member: Member,
        timeout: Duration,
    ) -> Result<(aaron_core::Uuid, Vec<Member>), BoxError> {
        let cluster_id = local_member.node_id.cluster_id.ok_or_else(|| {
            std::io::Error::other("cluster_id is required for authenticated join")
        })?;
        let members =
            Self::join_authenticated(quic, seed_addr, local_member, cluster_id, timeout).await?;
        Ok((cluster_id, members))
    }

    /// Sends an HMAC-authenticated join request. The cluster UUID is never sent in
    /// the request or response; it is used only as the shared HMAC key.
    pub async fn join_authenticated(
        quic: &QuicManager,
        seed_addr: SocketAddr,
        local_member: Member,
        cluster_id: aaron_core::Uuid,
        timeout: Duration,
    ) -> Result<Vec<Member>, BoxError> {
        let timestamp_ms = crate::auth::now_ms();
        let mac = crate::auth::sign(
            cluster_id,
            local_member.node_id.id(),
            local_member.node_id.incarnation,
            timestamp_ms,
        );
        let req = Message::AuthenticatedJoinRequest {
            sender: local_member,
            timestamp_ms,
            mac: mac.to_vec(),
        };
        let response =
            Self::request_response(quic, seed_addr, req, timeout, "authenticated_join").await?;
        match response {
            Message::AuthenticatedJoinResponse { members } => Ok(members
                .into_iter()
                .map(|mut member| {
                    member.node_id.cluster_id = Some(cluster_id);
                    member
                })
                .collect()),
            other => Err(format!("expected AuthenticatedJoinResponse, got {other:?}").into()),
        }
    }

    /// Sends a ConfigUpdate message to a remote cluster peer over QUIC.
    pub async fn send_config_update(
        quic: &QuicManager,
        peer_addr: SocketAddr,
        update: Message,
        timeout: Duration,
    ) -> Result<(), BoxError> {
        trace!(target: "membership::egress", peer = %peer_addr, "Sending ConfigUpdate over QUIC");
        let response =
            Self::request_response(quic, peer_addr, update, timeout, "config_update").await?;
        match response {
            Message::ConfigAck { success, .. } if success => Ok(()),
            other => Err(format!("unexpected response to config_update: {other:?}").into()),
        }
    }
}
