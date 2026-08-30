use node::{BoxError, Context, Service, ServiceConfig};
use std::net::SocketAddr;
use std::str::FromStr;
use std::time::Duration;
use tracing::{info, warn};

use crate::config::MembershipConfig;
use crate::event::JoinClusterCommand;
use crate::handle::MembershipHandle;
use crate::stage::egress::EgressTransport;
use crate::stage::ingress::IngressHandler;
use crate::stage::probe::ProbeLoop;
use crate::table::MembershipTable;

/// A supervised Cluster Membership and Failure Detection Service (SWIM over QUIC).
#[derive(Clone, Default)]
pub struct MembershipService {
    config_override: Option<MembershipConfig>,
    handle: MembershipHandle,
}

impl MembershipService {
    /// Creates a new `MembershipService` resolving configuration from the node's environment.
    pub fn new() -> Self {
        Self {
            config_override: None,
            handle: MembershipHandle::new(),
        }
    }

    /// Creates a new `MembershipService` with explicit configuration settings.
    pub fn with_config(config: MembershipConfig) -> Self {
        Self {
            config_override: Some(config),
            handle: MembershipHandle::new(),
        }
    }

    /// Returns a clone of the [`MembershipHandle`] associated with this service.
    pub fn handle(&self) -> MembershipHandle {
        self.handle.clone()
    }

    /// Creates a paired `(MembershipService, MembershipHandle)` for convenient initialization.
    pub fn pair() -> (Self, MembershipHandle) {
        let service = Self::new();
        let handle = service.handle();
        (service, handle)
    }

    /// Creates a paired `(MembershipService, MembershipHandle)` with custom configuration.
    pub fn pair_with_config(config: MembershipConfig) -> (Self, MembershipHandle) {
        let service = Self::with_config(config);
        let handle = service.handle();
        (service, handle)
    }
}

impl Service for MembershipService {
    type Config = MembershipConfig;

    fn name(&self) -> &str {
        "membership-service"
    }

    async fn run(&self, ctx: Context) -> Result<(), BoxError> {
        // 1. Resolve configuration (override or environment)
        let config = match &self.config_override {
            Some(cfg) => cfg.clone(),
            None => MembershipConfig::from_env(&ctx.env)?,
        };

        // 2. Validate and enforce Cluster ID Authorization Policy
        let resolved_cluster_id = config.cluster_id.or(ctx.identity.cluster_id);
        let cluster_id = match resolved_cluster_id {
            Some(cid) => cid,
            None => {
                if !config.seeds.is_empty() {
                    return Err(Box::new(node::ConfigError::MissingRequired {
                        service: "membership-service".to_string(),
                        var_name: "MEMBERSHIP_CLUSTER_ID".to_string(),
                        description: "MEMBERSHIP_CLUSTER_ID is strictly required for joining nodes (when seeds are configured)".to_string(),
                    }) as BoxError);
                }
                // Bootstrap node: generate initial cluster authority
                let new_cid = node::Uuid::random();
                info!(target: "membership", cluster_id = %new_cid, "Initialized new cluster authority as bootstrap node");
                new_cid
            }
        };

        // Notify Node runtime to bind and persist cluster ID
        ctx.event_hub
            .publish(node::BindClusterIdCommand::new(cluster_id))
            .await;

        // 3. Bind QUIC server endpoint with TLS certificate bound to this node's UUID
        let endpoint = ctx
            .network
            .quic
            .listen_for_node(&config.bind_addr, ctx.identity.id())
            .await?;

        let local_addr = endpoint.local_addr()?;
        info!(
            target: "membership",
            bind_addr = %config.bind_addr,
            local_addr = %local_addr,
            node_id = %ctx.identity.id(),
            cluster_id = %cluster_id,
            "MembershipService listening over QUIC"
        );

        // 4. Initialize core SWIM components with established Cluster ID
        let mut local_identity = ctx.identity;
        local_identity.cluster_id = Some(cluster_id);

        let table = MembershipTable::new(local_identity, local_addr);
        let ingress = IngressHandler::new(
            table.clone(),
            ctx.event_hub.clone(),
            ctx.network.quic.clone(),
            config.gossip_fanout,
            config.probe_timeout,
        );
        let probe = ProbeLoop::new(
            table.clone(),
            ctx.event_hub.clone(),
            ctx.network.quic.clone(),
            config.clone(),
        );

        // Initialize public MembershipHandle
        self.handle
            .initialize(
                table.clone(),
                ctx.network.quic.clone(),
                ctx.event_hub.clone(),
            )
            .await;

        // 5. Bootstrap cluster join by contacting configured seed nodes
        let local_member = table.local_member().await;
        for seed_str in &config.seeds {
            if let Ok(seed_addr) = SocketAddr::from_str(seed_str) {
                if seed_addr == local_addr {
                    continue; // Skip self
                }

                info!(target: "membership", seed = %seed_addr, cluster_id = %cluster_id, "Attempting to join cluster via seed node");
                match EgressTransport::join(
                    &ctx.network.quic,
                    seed_addr,
                    local_member.clone(),
                    Duration::from_millis(1000),
                )
                .await
                {
                    Ok((seed_cluster_id, members)) => {
                        if seed_cluster_id != cluster_id {
                            warn!(
                                target: "membership",
                                seed = %seed_addr,
                                expected_cluster = %cluster_id,
                                seed_cluster = %seed_cluster_id,
                                "Seed node returned mismatched cluster_id, rejecting join"
                            );
                            continue;
                        }

                        info!(
                            target: "membership",
                            seed = %seed_addr,
                            cluster_id = %cluster_id,
                            discovered = members.len(),
                            "Successfully joined cluster"
                        );
                        for m in members {
                            ingress.process_member_update(m).await;
                        }
                        break;
                    }
                    Err(err) => {
                        warn!(
                            target: "membership",
                            seed = %seed_addr,
                            error = %err,
                            "Failed to contact seed node, trying next"
                        );
                    }
                }
            }
        }

        // 6. Spawn background Ingress listener
        let ingress_handle = {
            let ingress = ingress.clone();
            let token = ctx.token.clone();
            let ep = endpoint.clone();
            tokio::spawn(async move {
                ingress.listen(ep, token).await;
            })
        };

        // 7. Spawn background listener for dynamic JoinClusterCommand via EventHub
        let mut join_cmd_sub = ctx.event_hub.subscribe::<JoinClusterCommand>().await;
        let dynamic_join_handle = {
            let handle_clone = self.handle.clone();
            let token_clone = ctx.token.clone();
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = token_clone.cancelled() => break,
                        cmd = join_cmd_sub.recv() => {
                            match cmd {
                                Ok(cmd) => {
                                    info!(target: "membership", seed = %cmd.seed_addr, "Processing dynamic JoinClusterCommand via EventHub");
                                    if let Err(err) = handle_clone.join(cmd.seed_addr).await {
                                        warn!(target: "membership", seed = %cmd.seed_addr, error = %err, "Dynamic JoinClusterCommand failed");
                                    }
                                }
                                Err(_) => break,
                            }
                        }
                    }
                }
            })
        };

        // 8. Run Probe / Failure Detector loop until cancellation
        probe.run(ctx.token.clone()).await;

        // 9. Graceful teardown
        endpoint.close(0u32.into(), b"shutdown");
        let _ = ingress_handle.await;
        let _ = dynamic_join_handle.await;

        Ok(())
    }
}
