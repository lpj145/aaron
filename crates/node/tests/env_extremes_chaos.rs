//! Chaos/exploratory tests for `Env` under extreme and hostile configuration values.
//!
//! `Env` is the single entry point every service uses to read configuration, and
//! `generate_env_example()` turns that same data back into a file meant to be re-loaded by
//! `dotenvy`. These tests push unicode, control characters, oversized values, colliding
//! field names across services, and true thread concurrency through that round trip.
//!
//! Nothing in `src/` is modified — these only explore and document behavior.

use node::{BoxError, ConfigError, ConfigField, Context, Env, Node, Service, ServiceConfig};
use std::sync::{Arc, Barrier};

/// Values a hostile or merely sloppy deployment can put in a `.env` file / process
/// environment. None of these may panic, hang, or be mis-parsed into a valid number.
#[test]
fn test_extreme_env_values_parse_without_panicking() {
    let env = Env::detect();

    let giant = "9".repeat(4 * 1024 * 1024); // 4MB of digits
    let cases: Vec<(&str, String)> = vec![
        ("CHAOS_NUL", "por\0ta".to_string()),
        ("CHAOS_NEWLINES", "8080\nEXTRA=1".to_string()),
        ("CHAOS_CRLF", "8080\r\n".to_string()),
        ("CHAOS_TAB_PAD", "\t 8080 \t".to_string()),
        ("CHAOS_EMOJI", "🚀🔥".to_string()),
        ("CHAOS_RTL_OVERRIDE", "\u{202E}8080\u{202C}".to_string()),
        ("CHAOS_COMBINING", "8\u{0301}080".to_string()),
        ("CHAOS_FULLWIDTH_DIGITS", "８０８０".to_string()),
        ("CHAOS_GIANT", giant.clone()),
        ("CHAOS_EMPTY", String::new()),
        ("CHAOS_ONLY_WS", "   ".to_string()),
        ("CHAOS_MINUS_ZERO", "-0".to_string()),
        ("CHAOS_PLUS_PORT", "+8080".to_string()),
        ("CHAOS_HEX_PORT", "0x1F90".to_string()),
        ("CHAOS_BOOL_YES", "YES".to_string()),
    ];

    for (name, value) in &cases {
        env.set(name, value).unwrap();
    }

    for (name, value) in &cases {
        // Every accessor must survive every value.
        let as_string: Option<String> = env.get(name);
        assert_eq!(
            as_string.as_deref(),
            Some(value.as_str()),
            "String round-trip changed the value of {name}"
        );
        let _: Option<u16> = env.get(name);
        let _: Option<bool> = env.get(name);
        let _: Option<f64> = env.get(name);
        let _ = env.get_raw(name);
    }

    // Values that must NOT be accepted as a port number.
    for name in [
        "CHAOS_NUL",
        "CHAOS_NEWLINES",
        "CHAOS_EMOJI",
        "CHAOS_COMBINING",
        "CHAOS_GIANT",
        "CHAOS_EMPTY",
        "CHAOS_ONLY_WS",
        "CHAOS_HEX_PORT",
    ] {
        let parsed: Option<u16> = env.get(name);
        assert_eq!(
            parsed, None,
            "{name} must not parse as a u16 port, got {parsed:?}"
        );
    }

    // `Env::get` retries with `.trim()`, so surrounding whitespace/CRLF is tolerated.
    assert_eq!(env.get::<u16>("CHAOS_TAB_PAD"), Some(8080));
    assert_eq!(env.get::<u16>("CHAOS_CRLF"), Some(8080));

    // A fullwidth-digit "port" is a classic homoglyph trap: it looks like 8080 in a
    // console but must not parse as one.
    assert_eq!(
        env.get::<u16>("CHAOS_FULLWIDTH_DIGITS"),
        None,
        "fullwidth digits must not parse as an ASCII port number"
    );

    // Bidi override characters render as a different port than they parse as.
    assert_eq!(env.get::<u16>("CHAOS_RTL_OVERRIDE"), None);
}

/// Two services below both declare `SHARED_PORT`, with different types and defaults.
/// A service's schema is a static property of its `Config` type, so each collision
/// participant needs its own config struct.
macro_rules! declaring_service {
    ($svc:ident, $cfg:ident, $name:literal, $field:expr) => {
        struct $cfg;

        impl ServiceConfig for $cfg {
            fn schema() -> Vec<ConfigField> {
                vec![$field]
            }
            fn from_env(_env: &Env) -> Result<Self, ConfigError> {
                Ok(Self)
            }
        }

        struct $svc;

        impl Service for $svc {
            type Config = $cfg;

            fn name(&self) -> &str {
                $name
            }

            async fn run(&self, _ctx: Context) -> Result<(), BoxError> {
                Ok(())
            }
        }
    };
}

declaring_service!(
    AlphaService,
    AlphaConfig,
    "svc-alpha",
    ConfigField::new("SHARED_PORT", "u16")
        .default("8080")
        .description("port used by alpha")
);
declaring_service!(
    BetaService,
    BetaConfig,
    "svc-beta",
    ConfigField::new("SHARED_PORT", "String")
        .default("not-a-port")
        .description("same var, different type, per beta")
);
declaring_service!(
    NumericService,
    NumericConfig,
    "svc-numeric",
    ConfigField::new("COLLIDING_VAR", "u16").required()
);
declaring_service!(
    BoolService,
    BoolConfig,
    "svc-bool",
    ConfigField::new("COLLIDING_VAR", "bool").required()
);

/// Two independent services declaring the *same* environment variable name with different
/// types and different defaults. `Node` only enforces uniqueness of *service* names, so
/// nothing stops this collision — but the generated `.env.example` emits the key twice with
/// two different values, and whichever line comes last silently wins when the file is loaded.
#[test]
fn test_colliding_field_names_across_services_produce_a_contradictory_env_example() {
    let node = Node::new().with(AlphaService).with(BetaService);

    let example = node.generate_env_example();
    let assignments: Vec<&str> = example
        .lines()
        .filter(|l| l.starts_with("SHARED_PORT="))
        .collect();

    assert_eq!(
        assignments.len(),
        1,
        "generate_env_example() emitted the same key {} times with conflicting values ({:?}); \
         loading that file gives whichever assignment happens to come last, so one service \
         silently gets the other's configuration",
        assignments.len(),
        assignments
    );
}

/// Cross-service collisions also break `validate_env`: the same raw value is checked against
/// both declared types, so a value valid for one service is reported as invalid for the
/// other and the whole node refuses to start — with no hint that the real problem is a
/// duplicated field name rather than a bad value.
#[test]
fn test_colliding_field_types_make_a_valid_value_fail_validation() {
    let env = Env::detect();
    env.set("COLLIDING_VAR", "8080").unwrap();

    let node = Node::new().with(NumericService).with(BoolService);

    let result = node.validate_env(&env);
    assert!(
        result.is_err(),
        "expected the u16/bool collision on COLLIDING_VAR to be caught by validate_env"
    );
    let errors = result.unwrap_err();
    assert_eq!(
        errors.len(),
        1,
        "expected exactly one type error, got: {errors:?}"
    );
    // Documents that the error blames the *value*, never mentioning that two services
    // declared the same variable with incompatible types.
    assert!(
        matches!(&errors[0], ConfigError::InvalidValue { var_name, .. } if var_name == "COLLIDING_VAR")
    );
}

/// `generate_env_example()` writes `NAME=` lines straight from tracked variable names with
/// no escaping. A name carrying a newline (reachable whenever a variable name comes from
/// any dynamic source) therefore splits into two lines, injecting an attacker-chosen
/// assignment into a file that is meant to be re-loaded by `dotenvy`.
#[test]
fn test_env_example_generation_is_not_line_injectable_via_variable_names() {
    let env = Env::detect();
    let hostile_name = "GOOD_VAR\nINJECTED_ADMIN=true";
    env.set(hostile_name, "x").unwrap();
    let _: Option<String> = env.get(hostile_name); // tracked -> appears in the example

    let example = env.generate_env_example();
    let injected = example
        .lines()
        .any(|l| l.trim_start().starts_with("INJECTED_ADMIN="));

    assert!(
        !injected,
        "generate_env_example() emitted a standalone `INJECTED_ADMIN=...` line: a variable \
         name containing a newline is written verbatim, so it splits into an extra \
         assignment that dotenvy will happily load back.\n--- generated ---\n{example}"
    );
}

/// True multi-threaded hammering of `Env`: concurrent `set`, typed `get` (which mutates the
/// tracked-variable list), `tracked()` and `generate_env_example()`. Explores whether the
/// tracking list can end up with duplicate entries for the same name under a race between
/// its check-then-insert, and whether any accessor can deadlock or poison a lock.
#[test]
fn test_concurrent_env_access_never_duplicates_tracked_variables() {
    const THREADS: usize = 32;
    const NAMES: usize = 8;

    let env = Arc::new(Env::detect());
    for i in 0..NAMES {
        env.set(&format!("RACE_VAR_{i}"), i).unwrap();
    }

    let barrier = Arc::new(Barrier::new(THREADS));
    let mut handles = Vec::with_capacity(THREADS);
    for t in 0..THREADS {
        let env = env.clone();
        let barrier = barrier.clone();
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            for i in 0..NAMES {
                let name = format!("RACE_VAR_{i}");
                match t % 4 {
                    0 => {
                        let _: Option<u16> = env.get(&name);
                    }
                    1 => {
                        let _: Option<String> = env.get(&name);
                    }
                    2 => {
                        env.set(&name, t).unwrap();
                    }
                    _ => {
                        let _ = env.tracked();
                        let _ = env.generate_env_example();
                    }
                }
            }
        }));
    }
    for h in handles {
        h.join()
            .expect("an Env accessor panicked or poisoned a lock under concurrency");
    }

    let tracked = env.tracked();
    for i in 0..NAMES {
        let name = format!("RACE_VAR_{i}");
        let count = tracked.iter().filter(|v| v.name == name).count();
        assert_eq!(
            count, 1,
            "{name} appears {count} times in the tracked list — the check-then-insert in \
             Env::get raced and registered the same variable more than once, which would \
             duplicate it in the generated .env.example"
        );
    }
}
