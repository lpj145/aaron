use std::{
    any::TypeId,
    collections::HashMap,
    net::{Ipv4Addr, Ipv6Addr},
    path::Path,
    str::FromStr,
    sync::{RwLock, RwLockReadGuard, RwLockWriteGuard},
};

use if_addrs::get_if_addrs;

use crate::BoxError;

/// Information about an environment variable that was accessed and tracked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackedVar {
    /// Name of the environment variable.
    pub name: String,
    /// Type ID of the type requested for this variable.
    pub type_id: TypeId,
    /// Type name as a string (e.g. `"String"`, `"u16"`, `"bool"`).
    pub type_name: &'static str,
}

/// Manages system environment variables, network detection, and configuration tracking.
///
/// `Env` automatically detects local network interfaces (IPv4/IPv6), hostname, and process
/// environment variables (loading `.env` files via `dotenvy` if present). It also tracks which
/// environment variables are accessed at runtime to automatically generate `.env.example` templates.
///
/// # Example
///
/// ```rust
/// use node::Env;
///
/// let env = Env::detect();
///
/// // Read typed configuration
/// let port: u16 = env.get("PORT").unwrap_or(8080);
/// let hostname = &env.hostname;
///
/// // Set or override an environment variable
/// env.set("APP_ENV", "production").unwrap();
///
/// // Generate a .env.example with all accessed keys and their expected types
/// let example = env.generate_env_example();
/// println!("{example}");
/// ```
pub struct Env {
    /// Tracked variables accessed through [`get`](Self::get).
    tracked: RwLock<Vec<TrackedVar>>,
    /// In-memory storage of environment key-value pairs.
    envs: RwLock<HashMap<String, String>>,
    /// System hostname.
    pub hostname: String,
    /// Detected local IPv4 addresses.
    pub ipv4: Vec<String>,
    /// Detected local IPv6 addresses.
    pub ipv6: Vec<String>,
}

impl Env {
    /// Detects current system environment, local network IPs, process variables,
    /// and automatically loads `.env` if found in the current or ancestor directory.
    pub fn detect() -> Env {
        let _ = dotenvy::dotenv();

        let hostname = gethostname::gethostname().to_string_lossy().into_owned();
        let (ipv4, ipv6) = detect_ips();
        let envs = std::env::vars().collect();

        Self {
            tracked: RwLock::new(Vec::new()),
            envs: RwLock::new(envs),
            hostname,
            ipv4,
            ipv6,
        }
    }

    fn read_envs(&self) -> RwLockReadGuard<'_, HashMap<String, String>> {
        self.envs.read().unwrap_or_else(|e| e.into_inner())
    }

    fn write_envs(&self) -> RwLockWriteGuard<'_, HashMap<String, String>> {
        self.envs.write().unwrap_or_else(|e| e.into_inner())
    }

    fn read_tracked(&self) -> RwLockReadGuard<'_, Vec<TrackedVar>> {
        self.tracked.read().unwrap_or_else(|e| e.into_inner())
    }

    fn write_tracked(&self) -> RwLockWriteGuard<'_, Vec<TrackedVar>> {
        self.tracked.write().unwrap_or_else(|e| e.into_inner())
    }

    /// Retrieves and parses an environment variable into type `T`, tracking the access.
    ///
    /// Returns `Some(T)` if the variable exists and can be parsed successfully, or `None` otherwise.
    pub fn get<T>(&self, name: &str) -> Option<T>
    where
        T: FromStr + 'static,
    {
        let already_tracked = {
            let tracked = self.read_tracked();
            tracked.iter().any(|v| v.name == name)
        };

        if !already_tracked {
            let mut tracked = self.write_tracked();
            if !tracked.iter().any(|v| v.name == name) {
                tracked.push(TrackedVar {
                    name: name.to_string(),
                    type_id: TypeId::of::<T>(),
                    type_name: std::any::type_name::<T>(),
                });
            }
        }

        let envs = self.read_envs();
        let val_str = envs.get(name)?;
        val_str
            .parse::<T>()
            .or_else(|_| val_str.trim().parse::<T>())
            .ok()
    }

    /// Retrieves the raw string value of an environment variable without tracking its type.
    pub fn get_raw(&self, name: &str) -> Option<String> {
        let envs = self.read_envs();
        envs.get(name).cloned()
    }

    /// Sets or overrides an environment variable in the internal store.
    pub fn set(&self, name: &str, value: impl ToString) -> Result<(), BoxError> {
        self.write_envs()
            .insert(name.to_string(), value.to_string());
        Ok(())
    }

    /// Returns a list of all environment variables tracked by calls to [`get`](Self::get).
    pub fn tracked(&self) -> Vec<TrackedVar> {
        self.read_tracked().clone()
    }

    /// Generates content for a `.env.example` file based on all tracked environment variables.
    pub fn generate_env_example(&self) -> String {
        let tracked = self.tracked();
        let mut out = String::new();
        out.push_str("# Auto-generated .env.example\n\n");
        let mut seen = std::collections::HashSet::new();
        for var in tracked {
            let clean_name = var.name.replace(['\r', '\n'], "");
            if clean_name.is_empty() || !seen.insert(clean_name.clone()) {
                continue;
            }
            let type_str = simplify_type_name(var.type_name);
            out.push_str(&format!("# Type: {type_str}\n{clean_name}=\n\n"));
        }
        out
    }

    /// Writes the generated `.env.example` file to the specified path.
    pub fn write_env_example(&self, path: impl AsRef<Path>) -> Result<(), BoxError> {
        let content = self.generate_env_example();
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Returns the primary detected local IPv4 address as `Ipv4Addr`.
    pub fn primary_ipv4(&self) -> Option<Ipv4Addr> {
        self.ipv4.first().and_then(|s| s.parse().ok())
    }

    /// Returns the primary detected local IPv6 address as `Ipv6Addr`.
    pub fn primary_ipv6(&self) -> Option<Ipv6Addr> {
        self.ipv6.first().and_then(|s| s.parse().ok())
    }

    /// Resolves an unspecified IP (0.0.0.0 or ::) to the primary non-loopback IP detected on network interfaces.
    pub fn resolve_ip(&self, ip: std::net::IpAddr) -> std::net::IpAddr {
        if ip.is_unspecified() {
            match ip {
                std::net::IpAddr::V4(_) => self
                    .primary_ipv4()
                    .map(std::net::IpAddr::V4)
                    .unwrap_or(std::net::IpAddr::V4(Ipv4Addr::LOCALHOST)),
                std::net::IpAddr::V6(_) => self
                    .primary_ipv6()
                    .map(std::net::IpAddr::V6)
                    .unwrap_or(std::net::IpAddr::V6(Ipv6Addr::LOCALHOST)),
            }
        } else {
            ip
        }
    }

    /// Resolves an unspecified SocketAddr (e.g. 0.0.0.0:7946) to the primary non-loopback IP with the same port.
    pub fn resolve_socket_addr(&self, addr: std::net::SocketAddr) -> std::net::SocketAddr {
        let mut resolved = addr;
        resolved.set_ip(self.resolve_ip(addr.ip()));
        resolved
    }
}

fn simplify_type_name(type_name: &str) -> String {
    let cleaned = type_name
        .replace("alloc::string::String", "String")
        .replace("alloc::vec::Vec", "Vec")
        .replace("core::option::Option", "Option");

    if !cleaned.contains('<')
        && let Some(pos) = cleaned.rfind("::")
    {
        return cleaned[pos + 2..].to_string();
    }
    cleaned
}

/// Detects local IPv4 and IPv6 addresses as strings from active network interfaces.
fn detect_ips() -> (Vec<String>, Vec<String>) {
    let (v4, v6) = detect_ip_addrs();
    (
        v4.into_iter().map(|ip| ip.to_string()).collect(),
        v6.into_iter().map(|ip| ip.to_string()).collect(),
    )
}

/// Detects local typed `Ipv4Addr` and `Ipv6Addr` from active network interfaces.
fn detect_ip_addrs() -> (Vec<Ipv4Addr>, Vec<Ipv6Addr>) {
    let mut ipv4 = Vec::new();
    let mut ipv6_global = Vec::new();
    let mut ipv6_link_local = Vec::new();

    if let Ok(ifaces) = get_if_addrs() {
        for iface in ifaces {
            if iface.is_loopback() {
                continue;
            }
            match iface.addr {
                if_addrs::IfAddr::V4(v4) => {
                    if !ipv4.contains(&v4.ip) {
                        ipv4.push(v4.ip);
                    }
                }
                if_addrs::IfAddr::V6(v6) => {
                    if is_unicast_link_local_v6(&v6.ip) {
                        if !ipv6_link_local.contains(&v6.ip) {
                            ipv6_link_local.push(v6.ip);
                        }
                    } else if !ipv6_global.contains(&v6.ip) {
                        ipv6_global.push(v6.ip);
                    }
                }
            }
        }
    }

    let mut ipv6 = ipv6_global;
    if ipv6.is_empty() {
        ipv6 = ipv6_link_local;
    }

    if ipv4.is_empty() {
        ipv4.push(Ipv4Addr::LOCALHOST);
    }
    if ipv6.is_empty() {
        ipv6.push(Ipv6Addr::LOCALHOST);
    }

    (ipv4, ipv6)
}

/// Checks if an IPv6 address is unicast link-local (`fe80::/10`).
fn is_unicast_link_local_v6(ip: &Ipv6Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 0xfe && (octets[1] & 0xc0) == 0x80
}
