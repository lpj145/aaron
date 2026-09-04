use aaron::{
    membership::{MembershipConfig, MembershipEvent, MembershipService},
    tracing::TracingService,
    Context, Node, Uuid, service_fn,
};
use std::time::Duration;
use tracing::{error, info};

#[tokio::main]
async fn main() {
    println!("=== Aaron Node: Cluster Membership with Admin Handle Example ===\n");

    let cluster_id = Uuid::new(0x1234_5678, 0x9ABC_DEF0);

    let config = MembershipConfig {
        bind_addr: "127.0.0.1:17946".to_string(),
        seeds: vec![],
        cluster_id: Some(cluster_id),
        probe_interval: Duration::from_millis(500),
        probe_timeout: Duration::from_millis(150),
        suspect_timeout: Duration::from_millis(1000),
        indirect_ping_targets: 3,
        gossip_fanout: 3,
    };

    // 1. Create paired MembershipService and MembershipHandle
    let (membership, handle) = MembershipService::pair_with_config(config);

    // 2. Build and run Node with Tracing, Membership, and an Admin Query Worker
    Node::new("cluster-admin")
        .with(TracingService::new())
        .with(membership)
        .with(service_fn("cluster_admin", move |ctx: Context| {
            let handle = handle.clone();
            async move {
                info!("Cluster Admin Service waiting for membership to initialize...");
                handle.wait_ready().await;

                // Query cluster ID
                let cid = handle.cluster_id().await;
                info!("Active Cluster ID: {:?}", cid);

                // Query local member
                let local = handle.local_member().await;
                info!("Local Node Member: {:?}", local);

                // Query all active members in topology
                let members = handle.active_members().await;
                info!("Current Active Cluster Members Count: {}", members.len());
                for m in &members {
                    info!(
                        "  -> Member: id={}, addr={}, status={}",
                        m.node_id.id(),
                        m.addr,
                        m.status
                    );
                }

                // Subscribe to real-time cluster membership events
                let mut sub = ctx.event_hub.subscribe::<MembershipEvent>().await;
                tokio::spawn(async move {
                    while let Ok(event) = sub.recv().await {
                        info!(">>> Received Topology Event: {event}");
                    }
                });

                tokio::time::sleep(Duration::from_secs(1)).await;
                info!("Cluster Admin sample completed successfully. Shutting down.");
                ctx.shutdown();
                Ok(())
            }
        }))
        .run()
        .await
        .unwrap_or_else(|err| error!("Node ended with error: {err}"));
}
