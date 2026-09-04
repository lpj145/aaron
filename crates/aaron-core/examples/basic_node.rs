use aaron_core::{Context, Node, service_fn};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), aaron_core::BoxError> {
    println!("=== Starting Minimal Aaron Node ===");

    Node::new("basic-worker")
        .with(service_fn("worker", |ctx: Context| async move {
            println!("Worker service started! Node ID: {}", ctx.identity.id());
            println!(
                "Node hostname: {}, detected IPs: {:?}",
                ctx.env.hostname, ctx.env.ipv4
            );

            for i in 1..=3 {
                tokio::time::sleep(Duration::from_millis(300)).await;
                println!("Worker processing heartbeat step {i}/3...");
            }

            println!("Worker completed tasks. Signaling graceful shutdown.");
            ctx.shutdown();
            Ok(())
        }))
        .run()
        .await
}
