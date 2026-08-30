use node::{BoxError, CancellationToken, EventHub, QuicManager, read_frame, write_frame};
use quinn::{RecvStream, SendStream};
use std::time::Duration;
use tracing::{debug, error, info, trace, warn};

use crate::event::MembershipEvent;
use crate::member::Member;
use crate::message::Message;
use crate::stage::egress::EgressTransport;
use crate::table::{MembershipChange, MembershipTable};

/// Ingress handler processing incoming QUIC streams on the membership listener.
#[derive(Clone)]
pub struct IngressHandler {
    table: MembershipTable,
    event_hub: EventHub,
    quic: QuicManager,
    gossip_fanout: usize,
    probe_timeout: Duration,
}

impl IngressHandler {
    /// Creates a new `IngressHandler`.
    pub fn new(
        table: MembershipTable,
        event_hub: EventHub,
        quic: QuicManager,
        gossip_fanout: usize,
        probe_timeout: Duration,
    ) -> Self {
        Self {
            table,
            event_hub,
            quic,
            gossip_fanout,
            probe_timeout,
        }
    }

    /// Listens for incoming QUIC connections on the endpoint until cancellation.
    pub async fn listen(&self, endpoint: quinn::Endpoint, token: CancellationToken) {
        while let Some(incoming) = tokio::select! {
            _ = token.cancelled() => None,
            conn = endpoint.accept() => conn,
        } {
            let handler = self.clone();
            let conn_token = token.clone();
            tokio::spawn(async move {
                if let Ok(connection) = incoming.await {
                    handler.handle_connection(connection, conn_token).await;
                }
            });
        }
    }

    /// Processes inbound bi-directional streams for an active QUIC connection.
    async fn handle_connection(&self, connection: quinn::Connection, token: CancellationToken) {
        loop {
            let stream_res = tokio::select! {
                _ = token.cancelled() => break,
                res = connection.accept_bi() => res,
            };

            let (send, recv) = match stream_res {
                Ok(streams) => streams,
                Err(_) => break,
            };

            let handler = self.clone();
            tokio::spawn(async move {
                if let Err(err) = handler.handle_bi_stream(send, recv).await {
                    debug!(target: "membership::ingress", error = %err, "Error handling bi-stream");
                }
            });
        }
    }

    /// Handles an incoming bi-directional QUIC stream.
    pub async fn handle_bi_stream(
        &self,
        mut send: SendStream,
        mut recv: RecvStream,
    ) -> Result<(), BoxError> {
        let frame_bytes = match read_frame(&mut recv).await? {
            Some(bytes) => bytes,
            None => return Ok(()), // Clean EOF
        };

        let msg = Message::from_bytes(&frame_bytes)?;
        let local_cluster = self.table.cluster_id().await;

        // Gatekeeper: Reject any non-JoinRequest message from an unauthorized/mismatched cluster
        if let Some(expected_cid) = local_cluster
            && !matches!(msg, Message::JoinRequest { .. })
            && let Some(sender) = msg.sender()
            && sender.node_id.cluster_id != Some(expected_cid)
        {
            warn!(
                target: "membership::ingress",
                expected_cluster = %expected_cid,
                sender_cluster = ?sender.node_id.cluster_id,
                from = %sender.addr,
                "Dropped message from unauthorized or mismatched cluster"
            );
            return Ok(());
        }

        match msg {
            Message::Ping {
                seq,
                sender,
                gossip,
            } => {
                trace!(
                    target: "membership::ingress",
                    from = %sender.addr,
                    seq = seq,
                    "Received Ping"
                );

                // 1. Process sender and piggybacked gossip updates
                self.process_member_update(sender).await;
                for update in gossip {
                    self.process_member_update(update).await;
                }

                // 2. Formulate Ack response with local state & gossip piggyback
                let local_member = self.table.local_member().await;
                let gossip_payload = self.table.collect_gossip_payload(self.gossip_fanout).await;

                let ack = Message::Ack {
                    seq,
                    sender: local_member,
                    gossip: gossip_payload,
                };

                let ack_bytes = ack.to_bytes();
                write_frame(&mut send, &ack_bytes).await?;
                let _ = send.finish();
            }
            Message::PingReq {
                seq,
                target,
                sender,
                gossip,
            } => {
                debug!(
                    target: "membership::ingress",
                    from = %sender.addr,
                    target = %target.addr,
                    seq = seq,
                    "Received PingReq (indirect probe request)"
                );

                // 1. Process sender and gossip
                self.process_member_update(sender).await;
                for update in gossip {
                    self.process_member_update(update).await;
                }

                // 2. Security validation: target MUST be a known active member of our cluster
                if self.table.get(&target.node_id.id()).await.is_none() {
                    warn!(
                        target: "membership::ingress",
                        target = %target.addr,
                        target_id = %target.node_id.id(),
                        "Rejected PingReq: Target node is not a known member of the cluster"
                    );
                    return Ok(());
                }

                // 3. Perform indirect probe to target
                let local_member = self.table.local_member().await;
                let ping = Message::Ping {
                    seq,
                    sender: local_member.clone(),
                    gossip: self.table.collect_gossip_payload(self.gossip_fanout).await,
                };

                // Forward probe with configured probe timeout
                let probe_result =
                    EgressTransport::ping(&self.quic, target.addr, ping, self.probe_timeout).await;

                if let Ok(Message::Ack {
                    seq: ack_seq,
                    sender: target_ack_sender,
                    gossip: target_gossip,
                }) = probe_result
                {
                    // Target responded! Authoritatively confirm target as Alive and forward Ack back
                    self.confirm_member_alive(target_ack_sender.clone()).await;
                    for u in target_gossip {
                        self.process_member_update(u).await;
                    }

                    let forward_ack = Message::Ack {
                        seq: ack_seq,
                        sender: target_ack_sender,
                        gossip: self.table.collect_gossip_payload(self.gossip_fanout).await,
                    };

                    write_frame(&mut send, &forward_ack.to_bytes()).await?;
                    let _ = send.finish();
                }
            }
            Message::JoinRequest { sender } => {
                let local_cluster = self.table.cluster_id().await;
                let cluster_id = match local_cluster {
                    Some(cid) => {
                        if sender.node_id.cluster_id != Some(cid) {
                            warn!(
                                target: "membership::ingress",
                                expected_cluster = %cid,
                                sender_cluster = ?sender.node_id.cluster_id,
                                from = %sender.addr,
                                "Rejected unauthorized JoinRequest: cluster_id mismatch"
                            );
                            return Ok(());
                        }
                        cid
                    }
                    None => {
                        let new_cid = sender.node_id.cluster_id.unwrap_or_else(node::Uuid::random);
                        self.table.set_cluster_id(new_cid).await;
                        new_cid
                    }
                };

                info!(
                    target: "membership::ingress",
                    from = %sender.addr,
                    id = %sender.node_id.id(),
                    cluster_id = %cluster_id,
                    "Admitting authorized node to cluster"
                );

                self.process_member_update(sender).await;

                let all_members = self.table.all_active_members().await;
                let join_resp = Message::JoinResponse {
                    cluster_id,
                    members: all_members,
                };

                write_frame(&mut send, &join_resp.to_bytes()).await?;
                let _ = send.finish();
            }
            Message::Ack {
                seq,
                sender,
                gossip,
            } => {
                trace!(
                    target: "membership::ingress",
                    from = %sender.addr,
                    seq = seq,
                    "Received Ack on inbound stream"
                );
                self.process_member_update(sender).await;
                for update in gossip {
                    self.process_member_update(update).await;
                }
            }
            Message::JoinResponse {
                cluster_id: _,
                members,
            } => {
                for m in members {
                    self.process_member_update(m).await;
                }
            }
        }

        Ok(())
    }

    /// Processes a member update, applying SWIM state precedence and publishing events to EventHub.
    pub async fn process_member_update(&self, update: Member) {
        if let Some(change) = self.table.upsert(update).await {
            let event = match change {
                MembershipChange::Joined(m) => {
                    info!(target: "membership", member = %m, "Node joined cluster");
                    MembershipEvent::Joined(m)
                }
                MembershipChange::Alive(m) => {
                    debug!(target: "membership", member = %m, "Node reaffirmed Alive");
                    MembershipEvent::Alive(m)
                }
                MembershipChange::Suspect(m) => {
                    warn!(target: "membership", member = %m, "Node Suspect");
                    MembershipEvent::Suspect(m)
                }
                MembershipChange::Dead(m) => {
                    error!(target: "membership", member = %m, "Node declared Dead");
                    MembershipEvent::Dead(m)
                }
                MembershipChange::Left(m) => {
                    info!(target: "membership", member = %m, "Node Left cluster");
                    MembershipEvent::Left(m)
                }
                MembershipChange::Refuted(m) => {
                    info!(
                        target: "membership",
                        new_incarnation = m.incarnation,
                        "Local node refuted false suspicion"
                    );
                    MembershipEvent::Refuted(m)
                }
            };
            self.event_hub.publish(event).await;
        }
    }

    /// Authoritatively confirms a peer as Alive from a direct or indirect Ack response.
    pub async fn confirm_member_alive(&self, update: Member) {
        if let Some(change) = self.table.confirm_alive(update).await {
            let event = match change {
                MembershipChange::Joined(m) => {
                    info!(target: "membership", member = %m, "Node joined cluster");
                    MembershipEvent::Joined(m)
                }
                MembershipChange::Alive(m) => {
                    debug!(target: "membership", member = %m, "Node reaffirmed Alive");
                    MembershipEvent::Alive(m)
                }
                MembershipChange::Suspect(m) => {
                    warn!(target: "membership", member = %m, "Node Suspect");
                    MembershipEvent::Suspect(m)
                }
                MembershipChange::Dead(m) => {
                    error!(target: "membership", member = %m, "Node declared Dead");
                    MembershipEvent::Dead(m)
                }
                MembershipChange::Left(m) => {
                    info!(target: "membership", member = %m, "Node Left cluster");
                    MembershipEvent::Left(m)
                }
                MembershipChange::Refuted(m) => {
                    info!(
                        target: "membership",
                        new_incarnation = m.incarnation,
                        "Local node refuted false suspicion"
                    );
                    MembershipEvent::Refuted(m)
                }
            };
            self.event_hub.publish(event).await;
        }
    }
}
