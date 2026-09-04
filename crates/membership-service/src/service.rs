use node::{BoxError, Context, Service, ServiceConfig};
use std::net::SocketAddr;
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
            .publish(node::NodeEvents::BindClusterId { cluster_id })
            .await;

        // 3. Bind QUIC server endpoint with TLS certificate bound to this node's UUID
        let endpoint = ctx
            .network
            .quic
            .listen_for_node(&config.bind_addr, ctx.identity.id())
            .await?;

        let mut local_addr = endpoint.local_addr()?;
        if local_addr.ip().is_unspecified() {
            local_addr = ctx.env.resolve_socket_addr(local_addr);
        }

        info!(
            target: "membership",
            bind_addr = %config.bind_addr,
            local_addr = %local_addr,
            node_id = %ctx.identity.id(),
            cluster_id = %cluster_id,
            "MembershipService listening over QUIC"
        );

        // 4. Initialize core SWIM components with established Cluster ID
        let mut local_identity = ctx.identity.clone();
        local_identity.cluster_id = Some(cluster_id);

        let mut local_tags = Vec::new();

        // 4.1 Primary application service identity (e.g. service:bank, service:treasurer)
        let primary_service_tag = format!("service:{}", ctx.service_name);
        if !local_tags.contains(&primary_service_tag) {
            local_tags.push(primary_service_tag);
        }

        // 4.2 Explicit node tags from Node::with_tag/with_tags
        for t in ctx.tags().await {
            if !local_tags.contains(&t) {
                local_tags.push(t);
            }
        }

        // 4.3 Service capabilities dynamically declared by registered services
        let svcs = ctx.services().await;
        for s in svcs {
            for cap in s.capabilities {
                if !local_tags.contains(&cap) {
                    local_tags.push(cap);
                }
            }
        }

        // 4.2 Collect node hostname / pod name
        let host = ctx
            .env
            .get::<String>("POD_NAME")
            .or_else(|| ctx.env.get::<String>("HOSTNAME"))
            .or_else(|| std::env::var("HOSTNAME").ok())
            .or_else(|| std::env::var("HOST").ok());
        if let Some(h) = host {
            let trimmed = h.trim();
            if !trimmed.is_empty() {
                local_tags.push(format!("host:{trimmed}"));
            }
        }

        // 4.3 Collect any custom user tags from AARON_TAGS env var
        if let Ok(custom) = std::env::var("AARON_TAGS") {
            for t in custom.split(',') {
                let trimmed = t.trim();
                if !trimmed.is_empty() && !local_tags.contains(&trimmed.to_string()) {
                    local_tags.push(trimmed.to_string());
                }
            }
        }

        let table = MembershipTable::new_with_tags(local_identity, local_addr, local_tags);
        let config_arc = std::sync::Arc::new(tokio::sync::RwLock::new(config.clone()));

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
            config_arc.clone(),
        );

        // Initialize public MembershipHandle
        self.handle
            .initialize(
                table.clone(),
                ctx.network.quic.clone(),
                ctx.event_hub.clone(),
                config_arc.clone(),
            )
            .await;

        // 5. Spawn background Ingress listener FIRST so the node immediately accepts incoming joins/probes
        let ingress_handle = {
            let ingress = ingress.clone();
            let token = ctx.token.clone();
            let ep = endpoint.clone();
            tokio::spawn(async move {
                ingress.listen(ep, token).await;
            })
        };

        // 6. Bootstrap cluster join by contacting configured seed nodes in background (with continuous auto-healing)
        let local_member = table.local_member().await;
        let seed_join_handle = {
            let seeds = config.seeds.clone();
            let quic = ctx.network.quic.clone();
            let token = ctx.token.clone();
            let ingress = ingress.clone();
            let table = table.clone();
            tokio::spawn(async move {
                if seeds.is_empty() {
                    return;
                }
                let mut attempt = 0;
                loop {
                    if token.is_cancelled() {
                        break;
                    }

                    attempt += 1;
                    for seed_str in &seeds {
                        let seed_trim = seed_str.trim();
                        let seed_addrs: Vec<SocketAddr> = if let Ok(addr) = seed_trim.parse::<SocketAddr>() {
                            vec![addr]
                        } else {
                            match tokio::net::lookup_host(seed_trim).await {
                                Ok(addrs) => addrs.collect(),
                                Err(_) => vec![],
                            }
                        };

                        for seed_addr in seed_addrs {
                            if seed_addr == local_addr {
                                continue; // Skip self
                            }

                            // Check if this seed is already known and active in our table
                            let is_seed_active = {
                                let active = table.all_active_members().await;
                                active.iter().any(|m| m.addr == seed_addr)
                            };

                            if is_seed_active {
                                continue;
                            }

                            info!(target: "membership", seed = %seed_addr, cluster_id = %cluster_id, attempt = attempt, "Attempting to join/merge cluster via seed node");
                            match EgressTransport::join(
                                &quic,
                                seed_addr,
                                local_member.clone(),
                                Duration::from_millis(1500),
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
                                        "Successfully joined/merged cluster via seed"
                                    );
                                    for m in members {
                                        ingress.process_member_update(m).await;
                                    }
                                }
                                Err(err) => {
                                    warn!(
                                        target: "membership",
                                        seed = %seed_addr,
                                        attempt = attempt,
                                        error = %err,
                                        "Failed to contact seed node"
                                    );
                                }
                            }
                        }
                    }

                    tokio::select! {
                        _ = token.cancelled() => break,
                        _ = tokio::time::sleep(Duration::from_secs(3)) => {}
                    }
                }
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
        let _ = seed_join_handle.await;
        let _ = dynamic_join_handle.await;

        Ok(())
    }
}
