use crate::config::ShardConfig;
use crate::coordinator::ShardCoordinator;
use crate::handle::ShardHandle;
use control_plane_service::ControlPlaneHandle;
use node::{BoxError, Context, Service, ServiceConfig};

/// Serviço do Control Plane responsável pelo Estágio 1: Designação de Shards.
pub struct ShardService {
    config_override: Option<ShardConfig>,
    control_plane: ControlPlaneHandle,
    handle: ShardHandle,
}

impl ShardService {
    pub fn coordinator(control_plane: ControlPlaneHandle) -> (Self, ShardHandle) {
        let nil_uuid = node::Uuid::NIL;
        let handle = ShardHandle::new(nil_uuid, 1024);
        (
            Self {
                config_override: None,
                control_plane,
                handle: handle.clone(),
            },
            handle,
        )
    }

    pub fn with_config(mut self, config: ShardConfig) -> Self {
        self.config_override = Some(config);
        self
    }
}

impl Service for ShardService {
    type Config = ShardConfig;

    fn name(&self) -> &str {
        "shard-service"
    }

    async fn run(&self, ctx: Context) -> Result<(), BoxError> {
        let config = match &self.config_override {
            Some(c) => c.clone(),
            None => ShardConfig::from_env(&ctx.env)?,
        };

        let my_id = ctx.identity.id();
        self.handle.set_local_node_id(my_id).await;
        self.handle.set_total_shards(config.total_shards).await;

        let coord = ShardCoordinator::new(config, self.control_plane.clone(), self.handle.clone());
        coord.run_loop(ctx).await;
        Ok(())
    }
}
