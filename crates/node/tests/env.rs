use node::Env;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

#[test]
fn test_env_detect_system_info() {
    let env = Env::detect();
    assert!(!env.hostname.is_empty(), "hostname should not be empty");
    assert!(
        !env.ipv4.is_empty(),
        "ipv4 list should contain at least 1 address"
    );
    assert!(
        !env.ipv6.is_empty(),
        "ipv6 list should contain at least 1 address"
    );
}

#[test]
fn test_env_typed_get() {
    let env = Env::detect();
    env.set("PORT", " 8080 ").unwrap();
    env.set("MAX_CONNECTIONS", "5000").unwrap();
    env.set("TIMEOUT_MS", "-100").unwrap();
    env.set("RATIO", "2.5").unwrap();
    env.set("DEBUG", "true").unwrap();
    env.set("APP_NAME", "my-awesome-service").unwrap();
    env.set("BIND_IP", "127.0.0.1").unwrap();
    env.set("SOCKET", "127.0.0.1:9000").unwrap();
    env.set("DATA_DIR", "/var/lib/aaron").unwrap();

    assert_eq!(env.get::<u16>("PORT"), Some(8080));
    assert_eq!(env.get::<u64>("MAX_CONNECTIONS"), Some(5000));
    assert_eq!(env.get::<i32>("TIMEOUT_MS"), Some(-100));
    assert_eq!(env.get::<f64>("RATIO"), Some(2.5));
    assert_eq!(env.get::<bool>("DEBUG"), Some(true));
    assert_eq!(
        env.get::<String>("APP_NAME"),
        Some("my-awesome-service".to_string())
    );
    assert_eq!(
        env.get::<IpAddr>("BIND_IP"),
        Some(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)))
    );
    assert_eq!(
        env.get::<SocketAddr>("SOCKET"),
        Some("127.0.0.1:9000".parse().unwrap())
    );
    assert_eq!(
        env.get::<PathBuf>("DATA_DIR"),
        Some(PathBuf::from("/var/lib/aaron"))
    );
}

#[test]
fn test_env_set_and_get() {
    let env = Env::detect();
    env.set("CUSTOM_KEY", "custom_val").unwrap();
    assert_eq!(
        env.get::<String>("CUSTOM_KEY"),
        Some("custom_val".to_string())
    );

    // Overwrite
    env.set("CUSTOM_KEY", 12345).unwrap();
    assert_eq!(env.get::<u32>("CUSTOM_KEY"), Some(12345));
}

#[test]
fn test_env_missing_and_invalid_parse() {
    let env = Env::detect();
    env.set("INVALID_NUMBER", "not_a_number").unwrap();

    assert_eq!(env.get::<String>("NON_EXISTENT_VAR"), None);
    assert_eq!(env.get::<u32>("INVALID_NUMBER"), None);
}

#[test]
fn test_env_tracking_and_example_generation() {
    let env = Env::detect();

    // Access multiple times (including non-existent and invalid)
    let _ = env.get::<String>("DATABASE_URL");
    let _ = env.get::<String>("DATABASE_URL");
    let _ = env.get::<u16>("SERVER_PORT");
    let _ = env.get::<bool>("DEBUG_MODE");

    let tracked = env.tracked();
    assert_eq!(tracked.len(), 3);
    assert_eq!(tracked[0].name, "DATABASE_URL");
    assert_eq!(tracked[1].name, "SERVER_PORT");
    assert_eq!(tracked[2].name, "DEBUG_MODE");

    let example = env.generate_env_example();
    assert!(example.contains("# Auto-generated .env.example"));
    assert!(example.contains("# Type: String\nDATABASE_URL=\n"));
    assert!(example.contains("# Type: u16\nSERVER_PORT=\n"));
    assert!(example.contains("# Type: bool\nDEBUG_MODE=\n"));
}

#[test]
fn test_env_write_example_to_file() {
    let env = Env::detect();
    let _ = env.get::<String>("APP_SECRET");
    let _ = env.get::<u16>("PORT");

    let temp_dir = std::env::temp_dir();
    let file_path = temp_dir.join(format!("test_example_{}.env", std::process::id()));

    env.write_env_example(&file_path).unwrap();
    assert!(file_path.exists());

    let content = std::fs::read_to_string(&file_path).unwrap();
    assert!(content.contains("APP_SECRET="));
    assert!(content.contains("PORT="));

    let _ = std::fs::remove_file(file_path);
}

#[test]
fn test_env_dotenv_loading() {
    let temp_dir = std::env::temp_dir();
    let dotenv_path = temp_dir.join(".env");
    std::fs::write(&dotenv_path, "TEST_DOTENV_KEY=dotenv_detected_val\n").unwrap();

    let _ = dotenvy::from_path(&dotenv_path);
    let env = Env::detect();
    assert_eq!(
        env.get::<String>("TEST_DOTENV_KEY"),
        Some("dotenv_detected_val".to_string())
    );

    let _ = std::fs::remove_file(dotenv_path);
}

#[tokio::test]
async fn test_env_concurrent_reads_and_writes() {
    let env = Arc::new(Env::detect());
    let mut handles = Vec::new();

    for i in 0..50 {
        let env_clone = Arc::clone(&env);
        handles.push(tokio::spawn(async move {
            let key = format!("CONCURRENT_KEY_{}", i % 10);
            let _ = env_clone.set(&key, format!("value_{}", i));
            let _ = env_clone.get::<String>(&key);
            let _ = env_clone.get::<u16>("SHARED_PORT");
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }
}
