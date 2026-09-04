use aaron::{
    admin::{AdminConfig, AdminService},
    membership::{MembershipConfig, MembershipService},
    tracing::TracingService,
    Context, Node, Uuid, service_fn,
};
use std::time::Duration;
use tracing::{error, info};

#[tokio::main]
async fn main() {
    println!("=== Aaron Node with Vue.js Admin Dashboard ===\n");
    println!("Starting node with embedded Vue.js admin console on http://127.0.0.1:8080 ...\n");

    let cluster_id = Uuid::new(0x1234_5678, 0x9ABC_DEF0);

    let mem_config = MembershipConfig {
        bind_addr: "127.0.0.1:17946".to_string(),
        seeds: vec![],
        cluster_id: Some(cluster_id),
        probe_interval: Duration::from_millis(500),
        probe_timeout: Duration::from_millis(150),
        suspect_timeout: Duration::from_millis(1000),
        indirect_ping_targets: 3,
        gossip_fanout: 3,
    };

    let (membership, handle) = MembershipService::pair_with_config(mem_config);
    let tracing_svc = TracingService::new();

    let admin_config = AdminConfig {
        bind_addr: "127.0.0.1:8080".parse().unwrap(),
        enabled: true,
        static_dir: None,
    };

    let admin_svc = AdminService::with_config(admin_config)
        .with_membership_handle(handle);

    Node::new("admin-console")
        .with(tracing_svc)
        .with(membership)
        .with(admin_svc)
        .with(service_fn("data-seeder", |ctx: Context| async move {
            // Seed sample application records into LSM Store
            let app_ks = ctx.store.keyspace("app")?;
            app_ks.insert("system/status", "healthy")?;
            app_ks.insert("users/admin", r#"{"id":1,"name":"Marcos","role":"SuperAdmin"}"#)?;
            app_ks.insert("services/api_gateway", r#"{"routes":12,"rate_limit":5000}"#)?;
            ctx.store.persist()?;

            info!(
                "Sample application data seeded to store keyspace 'app'. Dashboard is ready at http://127.0.0.1:8080"
            );
            Ok(())
        }))
        .run()
        .await
        .unwrap_or_else(|err| error!("Node ended with error: {err}"));
}
