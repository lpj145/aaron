use membership_service::MembershipConfig;
use node::{Env, ServiceConfig, Uuid};
use std::time::Duration;

#[test]
fn test_membership_config_defaults() {
    let env = Env::detect();
    let cfg = MembershipConfig::from_env(&env).unwrap();

    assert_eq!(cfg.bind_addr, "0.0.0.0:7946");
    assert!(cfg.seeds.is_empty());
    assert_eq!(cfg.cluster_id, None);
    assert_eq!(cfg.probe_interval, Duration::from_millis(1000));
    assert_eq!(cfg.probe_timeout, Duration::from_millis(200));
    assert_eq!(cfg.suspect_timeout, Duration::from_millis(1000));
    assert_eq!(cfg.indirect_ping_targets, 3);
    assert_eq!(cfg.gossip_fanout, 3);
}

#[test]
fn test_membership_config_from_env_seeds_and_cluster_id_parsing() {
    let env = Env::detect();
    let expected_uuid = Uuid::new(0x1111_2222, 0x3333_4444);

    env.set("MEMBERSHIP_BIND_ADDR", "127.0.0.1:8000").unwrap();
    env.set(
        "MEMBERSHIP_SEEDS",
        "10.0.0.1:7946, 10.0.0.2:7946 , 10.0.0.3:7946",
    )
    .unwrap();
    env.set("MEMBERSHIP_CLUSTER_ID", format!("{expected_uuid}"))
        .unwrap();

    let cfg = MembershipConfig::from_env(&env).unwrap();
    assert_eq!(cfg.bind_addr, "127.0.0.1:8000");
    assert_eq!(
        cfg.seeds,
        vec![
            "10.0.0.1:7946".to_string(),
            "10.0.0.2:7946".to_string(),
            "10.0.0.3:7946".to_string()
        ]
    );
    assert_eq!(cfg.cluster_id, Some(expected_uuid));
}

#[test]
fn test_membership_config_schema_reflection() {
    let schema = MembershipConfig::schema();
    assert_eq!(schema.len(), 3);
    assert_eq!(schema[0].name, "MEMBERSHIP_BIND_ADDR");
    assert_eq!(schema[1].name, "MEMBERSHIP_SEEDS");
    assert_eq!(schema[2].name, "MEMBERSHIP_CLUSTER_ID");
}
