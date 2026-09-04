use std::fmt;
use std::hash::{Hash, Hasher};
use std::time::{SystemTime, UNIX_EPOCH};

#[allow(
    clippy::all,
    clippy::pedantic,
    clippy::nursery,
    unused_imports,
    dead_code
)]
mod generated {
    include!(concat!(env!("OUT_DIR"), "/node_generated.rs"));
}

pub use generated::aaron::node::{NodeId, NodeIdBuilder, NodeIdRef, Uuid, UuidRef};

impl Uuid {
    pub const NIL: Self = Self::new(0, 0);

    /// Creates a new `Uuid` from high and low 64-bit unsigned integers.
    pub const fn new(high: u64, low: u64) -> Self {
        Self { high, low }
    }

    /// Generates a new random/time-ordered 128-bit unique UUID (UUIDv7-style) with CSPRNG entropy.
    pub fn random() -> Self {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64);

        let high_time = (now_ms & 0x0000_FFFF_FFFF_FFFF) << 16;

        let mut rand_bytes = [0u8; 16];
        if rustls::crypto::ring::default_provider()
            .secure_random
            .fill(&mut rand_bytes)
            .is_ok()
        {
            let rand1 = u64::from_le_bytes(rand_bytes[0..8].try_into().unwrap());
            let rand2 = u64::from_le_bytes(rand_bytes[8..16].try_into().unwrap());
            let high = high_time | ((rand1 >> 48) & 0x0FFF) | 0x7000;
            let low = (rand2 & 0x3FFF_FFFF_FFFF_FFFF) | 0x8000_0000_0000_0000;
            return Self { high, low };
        }

        // Fallback to high-resolution time, pid, and atomic counter hash
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let count = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos() as u64);

        let pid = std::process::id() as u64;

        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (pid, nanos, count, gethostname::gethostname()).hash(&mut hasher);
        let rand1 = hasher.finish();
        (nanos.wrapping_add(count), pid).hash(&mut hasher);
        let rand2 = hasher.finish();

        let high = high_time | ((rand1 >> 48) & 0x0FFF) | 0x7000;
        let low = (rand2 & 0x3FFF_FFFF_FFFF_FFFF) | 0x8000_0000_0000_0000;

        Self { high, low }
    }

    /// Creates a `Uuid` from a 128-bit integer.
    pub const fn from_u128(val: u128) -> Self {
        Self {
            high: (val >> 64) as u64,
            low: val as u64,
        }
    }

    /// Converts this `Uuid` to a 128-bit integer.
    pub const fn to_u128(&self) -> u128 {
        ((self.high as u128) << 64) | (self.low as u128)
    }

    /// Creates a `Uuid` from a 16-byte array (big-endian).
    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        let high = u64::from_be_bytes(bytes[0..8].try_into().unwrap());
        let low = u64::from_be_bytes(bytes[8..16].try_into().unwrap());
        Self { high, low }
    }

    /// Converts this `Uuid` into a 16-byte array (big-endian).
    pub fn to_bytes(&self) -> [u8; 16] {
        let mut bytes = [0u8; 16];
        bytes[0..8].copy_from_slice(&self.high.to_be_bytes());
        bytes[8..16].copy_from_slice(&self.low.to_be_bytes());
        bytes
    }
}

impl fmt::Display for Uuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}{:016x}", self.high, self.low)
    }
}

impl std::str::FromStr for Uuid {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let clean = s.replace('-', "");
        if clean.len() != 32 || !clean.is_ascii() {
            return Err(format!(
                "invalid UUID string: expected 32 ASCII hex chars, got '{}'",
                s
            ));
        }
        let high_str = clean
            .get(0..16)
            .ok_or_else(|| format!("invalid UUID boundary in '{s}'"))?;
        let low_str = clean
            .get(16..32)
            .ok_or_else(|| format!("invalid UUID boundary in '{s}'"))?;
        let high = u64::from_str_radix(high_str, 16)
            .map_err(|e| format!("invalid UUID high 64-bit hex: {e}"))?;
        let low = u64::from_str_radix(low_str, 16)
            .map_err(|e| format!("invalid UUID low 64-bit hex: {e}"))?;
        Ok(Self::new(high, low))
    }
}

impl serde::Serialize for Uuid {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if serializer.is_human_readable() {
            serializer.serialize_str(&self.to_string())
        } else {
            (self.high, self.low).serialize(serializer)
        }
    }
}

impl<'de> serde::Deserialize<'de> for Uuid {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        if deserializer.is_human_readable() {
            let s = String::deserialize(deserializer)?;
            s.parse::<Self>().map_err(serde::de::Error::custom)
        } else {
            let (high, low) = <(u64, u64)>::deserialize(deserializer)?;
            Ok(Self::new(high, low))
        }
    }
}

impl NodeId {
    /// Creates a new `NodeId` with specified ID, incarnation timestamp, and optional cluster ID.
    pub const fn new(id: Uuid, incarnation: u64, cluster_id: Option<Uuid>) -> Self {
        Self {
            id: Some(id),
            incarnation,
            cluster_id,
        }
    }

    /// Creates a new `NodeId` using the current system timestamp (in milliseconds) as incarnation.
    pub fn with_current_incarnation(id: Uuid, cluster_id: Option<Uuid>) -> Self {
        let incarnation = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64);

        Self::new(id, incarnation, cluster_id)
    }

    /// Returns the node's unique ID, or a default zeroed UUID if not set.
    pub fn id(&self) -> Uuid {
        self.id.unwrap_or_default()
    }

    /// Serializes this `NodeId` table into a FlatBuffers binary buffer using Planus.
    pub fn to_flatbuffer_bytes(&self) -> Vec<u8> {
        use planus::WriteAsOffset;
        let mut builder = planus::Builder::new();
        let offset = self.prepare(&mut builder);
        builder.finish(offset, None).to_vec()
    }

    /// Deserializes a `NodeId` from a FlatBuffers binary buffer using Planus.
    pub fn from_flatbuffer_bytes(bytes: &[u8]) -> Result<Self, planus::Error> {
        use planus::ReadAsRoot;
        let node_ref = NodeIdRef::read_as_root(bytes)?;
        node_ref.try_into()
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let node_hex = self.id.map_or_else(|| "0".repeat(32), |u| format!("{u}"));
        if let Some(cluster) = self.cluster_id {
            write!(
                f,
                "NodeId(id: {}, inc: {}, cluster: {})",
                node_hex, self.incarnation, cluster
            )
        } else {
            write!(f, "NodeId(id: {}, inc: {})", node_hex, self.incarnation)
        }
    }
}
