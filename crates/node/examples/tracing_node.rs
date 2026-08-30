use node::{Context, Node, service_fn};
use std::time::Duration;
use tracing::{debug, error, info, trace, warn};
use tracing_service::{ChangeLogLevel, TracingService};

#[tokio::main]
async fn main() {
    println!("=== Starting Aaron Node with TracingService ===");
    println!("Tip: Set LOG_FORMAT=pretty to use pretty formatted logs (default is JSON)\n");

    Node::new()
        .with(TracingService::new())
        .with(service_fn("worker", |ctx: Context| async move {
            info!("Worker service started. Initial log level is 'info'.");
            debug!("This debug message is hidden at 'info' level.");

            tokio::time::sleep(Duration::from_millis(500)).await;

            info!("Publishing ChangeLogLevel::debug() event to EventHub...");
            ctx.event_hub.publish(ChangeLogLevel::debug()).await;

            tokio::time::sleep(Duration::from_millis(100)).await;
            debug!(">>> This DEBUG message is now visible after dynamic reload!");

            tokio::time::sleep(Duration::from_millis(500)).await;

            info!("Publishing ChangeLogLevel::trace() event to EventHub...");
            ctx.event_hub.publish(ChangeLogLevel::trace()).await;

            tokio::time::sleep(Duration::from_millis(100)).await;
            trace!(">>> This TRACE message is now visible after dynamic reload!");
            warn!(">>> Warnings and errors remain visible across all levels.");

            ctx.shutdown();
            Ok(())
        }))
        .run()
        .await
        .unwrap_or_else(|err| error!("Node ended with error: {err}"));
}
