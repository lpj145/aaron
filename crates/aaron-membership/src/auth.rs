use aaron_core::Uuid;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};

type HmacSha256 = Hmac<Sha256>;
const DOMAIN: &[u8] = b"aaron-membership-join-v1";

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as u64)
}

pub fn sign(cluster_id: Uuid, node_id: Uuid, incarnation: u64, timestamp_ms: u64) -> [u8; 32] {
    let mut mac =
        HmacSha256::new_from_slice(&cluster_id.to_bytes()).expect("UUID is a valid HMAC key");
    mac.update(DOMAIN);
    mac.update(&node_id.to_bytes());
    mac.update(&incarnation.to_be_bytes());
    mac.update(&timestamp_ms.to_be_bytes());
    mac.finalize().into_bytes().into()
}

pub fn verify(
    cluster_id: Uuid,
    node_id: Uuid,
    incarnation: u64,
    timestamp_ms: u64,
    received: &[u8],
    window_ms: u64,
) -> bool {
    let now = now_ms();
    let age = now.abs_diff(timestamp_ms);
    if age > window_ms || received.len() != 32 {
        return false;
    }
    let mut mac =
        HmacSha256::new_from_slice(&cluster_id.to_bytes()).expect("UUID is a valid HMAC key");
    mac.update(DOMAIN);
    mac.update(&node_id.to_bytes());
    mac.update(&incarnation.to_be_bytes());
    mac.update(&timestamp_ms.to_be_bytes());
    mac.verify_slice(received).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifies_a_current_proof() {
        let cluster = Uuid::new(1, 2);
        let node = Uuid::new(3, 4);
        let timestamp = now_ms();
        let mac = sign(cluster, node, 7, timestamp);

        assert!(verify(cluster, node, 7, timestamp, &mac, 2_000));
    }

    #[test]
    fn rejects_wrong_cluster_and_expired_proofs() {
        let cluster = Uuid::new(1, 2);
        let node = Uuid::new(3, 4);
        let timestamp = now_ms();
        let mac = sign(cluster, node, 7, timestamp);

        assert!(!verify(Uuid::new(9, 10), node, 7, timestamp, &mac, 2_000));
        assert!(!verify(
            cluster,
            node,
            7,
            timestamp.saturating_sub(2_001),
            &mac,
            2_000
        ));
        assert!(!verify(cluster, node, 7, timestamp, &mac[..31], 2_000));
    }
}
