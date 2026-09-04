use node::{BoxError, ConfigError, ConfigField, Context, Env, Node, Service, ServiceConfig};
use std::sync::Arc;

#[derive(Clone, Default)]
struct MockTypedService;

#[derive(Clone, Debug, Default)]
struct MockTypedConfig {
    port: u16,
    enabled: bool,
    max_memory: u64,
}

impl ServiceConfig for MockTypedConfig {
    fn schema() -> Vec<ConfigField> {
        vec![
            ConfigField::new("MOCK_PORT", "u16")
                .required()
                .description("Network port"),
            ConfigField::new("MOCK_ENABLED", "bool")
                .required()
                .description("Feature flag"),
            ConfigField::new("MOCK_MAX_MEM", "u64")
                .required()
                .description("Memory limit"),
        ]
    }

    fn from_env(env: &Env) -> Result<Self, ConfigError> {
        Ok(Self {
            port: env.get::<u16>("MOCK_PORT").unwrap_or(8080),
            enabled: env.get::<bool>("MOCK_ENABLED").unwrap_or(false),
            max_memory: env.get::<u64>("MOCK_MAX_MEM").unwrap_or(1024),
        })
    }
}

impl Service for MockTypedService {
    type Config = MockTypedConfig;

    fn name(&self) -> &str {
        "mock-typed-service"
    }

    async fn run(&self, ctx: Context) -> Result<(), BoxError> {
        let _cfg = MockTypedConfig::from_env(&ctx.env)?;
        Ok(())
    }
}

#[test]
fn test_validate_env_does_not_poison_tracked_type_metadata() {
    let env = Arc::new(Env::detect());
    env.set("MOCK_PORT", "9090").unwrap();
    env.set("MOCK_ENABLED", "true").unwrap();
    env.set("MOCK_MAX_MEM", "4096").unwrap();

    let node = Node::new("mock-node").with(MockTypedService);

    // 1. Validate environment at startup
    let validation_res = node.validate_env(&env);
    assert!(
        validation_res.is_ok(),
        "Environment validation must succeed"
    );

    // 2. Service parses typed config
    let config = MockTypedConfig::from_env(&env).expect("Config parse must succeed");
    assert_eq!(config.port, 9090);
    assert!(config.enabled);
    assert_eq!(config.max_memory, 4096);

    // 3. Generate .env.example
    let example = env.generate_env_example();

    // Verify type fidelity: Types must be u16, bool, u64 (NOT poisoned to String!)
    assert!(
        example.contains("# Type: u16"),
        "MOCK_PORT must be tracked as u16, but generated:\n{example}"
    );
    assert!(
        example.contains("# Type: bool"),
        "MOCK_ENABLED must be tracked as bool, but generated:\n{example}"
    );
    assert!(
        example.contains("# Type: u64"),
        "MOCK_MAX_MEM must be tracked as u64, but generated:\n{example}"
    );
}
