use node::{BoxError, ConfigError, ConfigField, Context, Env, Node, Service, ServiceConfig};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

#[derive(Debug, Clone)]
struct CustomP2pConfig {
    listen_port: u16,
    #[allow(dead_code)]
    max_peers: usize,
    #[allow(dead_code)]
    cluster_name: String,
}

impl ServiceConfig for CustomP2pConfig {
    fn schema() -> Vec<ConfigField> {
        vec![
            ConfigField::new("P2P_LISTEN_PORT", "u16")
                .required()
                .description("P2P listening port"),
            ConfigField::new("P2P_MAX_PEERS", "usize")
                .default("50")
                .description("Max peer count"),
            ConfigField::new("CLUSTER_NAME", "String")
                .default("default-cluster")
                .description("Cluster group identifier"),
        ]
    }

    fn from_env(env: &Env) -> Result<Self, ConfigError> {
        let listen_port =
            env.get::<u16>("P2P_LISTEN_PORT")
                .ok_or_else(|| ConfigError::MissingRequired {
                    service: "p2p-service".to_string(),
                    var_name: "P2P_LISTEN_PORT".to_string(),
                    description: "P2P listening port".to_string(),
                })?;

        let max_peers = env.get::<usize>("P2P_MAX_PEERS").unwrap_or(50);
        let cluster_name = env
            .get::<String>("CLUSTER_NAME")
            .unwrap_or_else(|| "default-cluster".to_string());

        Ok(Self {
            listen_port,
            max_peers,
            cluster_name,
        })
    }
}

struct ConfigTestService {
    run_called: Arc<AtomicBool>,
    port_seen: Arc<AtomicUsize>,
}

impl Service for ConfigTestService {
    type Config = CustomP2pConfig;

    fn name(&self) -> &str {
        "p2p-service"
    }

    async fn run(&self, ctx: Context) -> Result<(), BoxError> {
        self.run_called.store(true, Ordering::SeqCst);
        let cfg = CustomP2pConfig::from_env(&ctx.env)?;
        self.port_seen
            .store(cfg.listen_port as usize, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(10)).await;
        Ok(())
    }
}

#[tokio::test]
async fn test_service_config_and_run_execution() {
    let temp_dir = std::env::temp_dir().join(format!("test_config_exec_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);

    let run_called = Arc::new(AtomicBool::new(false));
    let port_seen = Arc::new(AtomicUsize::new(0));

    let svc = ConfigTestService {
        run_called: run_called.clone(),
        port_seen: port_seen.clone(),
    };

    let env = Env::detect();
    env.set("P2P_LISTEN_PORT", 9090).unwrap();

    let node = Node::new().with_dir_path(&temp_dir).with_env(env).with(svc);

    // Run node
    node.run().await.unwrap();

    assert!(run_called.load(Ordering::SeqCst), "run should be called");
    assert_eq!(port_seen.load(Ordering::SeqCst), 9090);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_node_env_validation_fail_fast_aborts_before_run() {
    let temp_dir = std::env::temp_dir().join(format!("test_fail_fast_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);

    let run_called = Arc::new(AtomicBool::new(false));

    let svc = ConfigTestService {
        run_called: run_called.clone(),
        port_seen: Arc::new(AtomicUsize::new(0)),
    };

    // Construct env without P2P_LISTEN_PORT
    let env = Env::detect();

    let node = Node::new().with_dir_path(&temp_dir).with_env(env).with(svc);

    let explicit_env = Env::detect();
    let validation = node.validate_env(&explicit_env);
    assert!(validation.is_err());
    let errors = validation.unwrap_err();
    assert_eq!(errors.len(), 1);
    assert!(matches!(errors[0], ConfigError::MissingRequired { .. }));

    // Run node should abort without calling run
    let res = node.run().await;
    assert!(res.is_err());
    assert!(
        !run_called.load(Ordering::SeqCst),
        "run must NOT be called when env validation fails"
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_node_env_validation_invalid_type_aborts() {
    let temp_dir = std::env::temp_dir().join(format!("test_invalid_type_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);

    let svc = ConfigTestService {
        run_called: Arc::new(AtomicBool::new(false)),
        port_seen: Arc::new(AtomicUsize::new(0)),
    };

    // Set invalid non-numeric value for u16 port in isolated Env
    let env = Env::detect();
    env.set("P2P_LISTEN_PORT", "not_a_valid_port_number")
        .unwrap();

    let node = Node::new().with_dir_path(&temp_dir).with_env(env).with(svc);

    let check_env = Env::detect();
    check_env
        .set("P2P_LISTEN_PORT", "not_a_valid_port_number")
        .unwrap();
    let validation = node.validate_env(&check_env);
    assert!(validation.is_err());
    let errors = validation.unwrap_err();
    assert_eq!(errors.len(), 1);
    assert!(matches!(errors[0], ConfigError::InvalidValue { .. }));

    let res = node.run().await;
    assert!(res.is_err());

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_node_generate_env_example_template() {
    let svc = ConfigTestService {
        run_called: Arc::new(AtomicBool::new(false)),
        port_seen: Arc::new(AtomicUsize::new(0)),
    };

    let node = Node::new().with(svc);
    let example = node.generate_env_example();

    assert!(example.contains("# === [p2p-service] ==="));
    assert!(example.contains("P2P_LISTEN_PORT="));
    assert!(example.contains("P2P_MAX_PEERS=50"));
    assert!(example.contains("CLUSTER_NAME=default-cluster"));
    assert!(example.contains("Type: u16 (Required)"));
    assert!(example.contains("Type: usize (Optional, default: 50)"));
}
