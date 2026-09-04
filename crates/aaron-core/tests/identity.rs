use aaron_core::{NodeId, NodeIdRef, Uuid};
use planus::{ReadAsRoot, WriteAsOffset};

#[test]
fn test_uuid_creation_and_conversions() {
    let uuid = Uuid::new(0x0123_4567_89ab_cdef, 0xfedc_ba98_7654_3210);
    assert_eq!(uuid.high, 0x0123_4567_89ab_cdef);
    assert_eq!(uuid.low, 0xfedc_ba98_7654_3210);

    // u128 conversion
    let val_u128 = uuid.to_u128();
    let uuid_from_u128 = Uuid::from_u128(val_u128);
    assert_eq!(uuid, uuid_from_u128);

    // Byte array roundtrip
    let bytes = uuid.to_bytes();
    let uuid_from_bytes = Uuid::from_bytes(bytes);
    assert_eq!(uuid, uuid_from_bytes);

    // Display format (hex)
    assert_eq!(format!("{uuid}"), "0123456789abcdeffedcba9876543210");
}

#[test]
fn test_node_id_with_incarnation_and_optional_cluster() {
    let node_raw_id = Uuid::new(100, 200);
    let cluster_id = Uuid::new(1, 1);
    let incarnation = 1_700_000_000_123u64;

    // 1. Without cluster ID
    let node_standalone = NodeId::new(node_raw_id, incarnation, None);
    assert_eq!(node_standalone.id(), node_raw_id);
    assert_eq!(node_standalone.incarnation, incarnation);
    assert_eq!(node_standalone.cluster_id, None);

    // 2. With cluster ID
    let node_in_cluster = NodeId::new(node_raw_id, incarnation, Some(cluster_id));
    assert_eq!(node_in_cluster.id(), node_raw_id);
    assert_eq!(node_in_cluster.incarnation, incarnation);
    assert_eq!(node_in_cluster.cluster_id, Some(cluster_id));

    // 3. with_current_incarnation
    let node_now = NodeId::with_current_incarnation(node_raw_id, Some(cluster_id));
    assert!(node_now.incarnation > 0);
}

#[test]
fn test_node_id_flatbuffers_serialization_via_planus() {
    let node_raw_id = Uuid::new(0xaaaa_bbbb_cccc_dddd, 0x1111_2222_3333_4444);
    let cluster_id = Uuid::new(0x9999_8888_7777_6666, 0x5555_4444_3333_2222);
    let original = NodeId::new(node_raw_id, 123456789, Some(cluster_id));

    let mut builder = planus::Builder::new();
    let offset = original.prepare(&mut builder);
    let slice = builder.finish(offset, None);

    // Read back via ReadAsRoot
    let node_ref = NodeIdRef::read_as_root(slice).unwrap();
    assert_eq!(node_ref.incarnation().unwrap(), 123456789);

    let id_ref = node_ref.id().unwrap().unwrap();
    assert_eq!(id_ref.high(), 0xaaaa_bbbb_cccc_dddd);
    assert_eq!(id_ref.low(), 0x1111_2222_3333_4444);

    let cluster_ref = node_ref.cluster_id().unwrap().unwrap();
    assert_eq!(cluster_ref.high(), 0x9999_8888_7777_6666);
    assert_eq!(cluster_ref.low(), 0x5555_4444_3333_2222);

    let deserialized: NodeId = node_ref.try_into().unwrap();
    assert_eq!(deserialized, original);
}

#[test]
fn test_node_id_flatbuffer_roundtrip_optional_cluster() {
    let node_raw_id = Uuid::new(42, 99);

    // Node without cluster_id
    let original = NodeId::new(node_raw_id, 98765, None);
    let bytes = original.to_flatbuffer_bytes();

    let recovered = NodeId::from_flatbuffer_bytes(&bytes).unwrap();
    assert_eq!(original, recovered);
    assert_eq!(recovered.cluster_id, None);

    // Node with cluster_id
    let cluster_id = Uuid::new(10, 20);
    let original_with_cluster = NodeId::new(node_raw_id, 98765, Some(cluster_id));
    let bytes_with_cluster = original_with_cluster.to_flatbuffer_bytes();

    let recovered_with_cluster = NodeId::from_flatbuffer_bytes(&bytes_with_cluster).unwrap();
    assert_eq!(original_with_cluster, recovered_with_cluster);
    assert_eq!(recovered_with_cluster.cluster_id, Some(cluster_id));
}

#[test]
fn test_node_id_invalid_corrupted_flatbuffer_bytes() {
    // Empty buffer
    assert!(NodeId::from_flatbuffer_bytes(&[]).is_err());

    // Truncated buffer (less than table prefix)
    assert!(NodeId::from_flatbuffer_bytes(&[0x01, 0x02]).is_err());

    // Random noise buffer
    let garbage = [0xFF; 32];
    assert!(NodeId::from_flatbuffer_bytes(&garbage).is_err());
}

#[test]
fn test_uuid_edge_cases_zero_and_max() {
    // Zero UUID
    let zero = Uuid::new(0, 0);
    assert_eq!(zero.to_u128(), 0);
    assert_eq!(Uuid::from_u128(0), zero);
    assert_eq!(format!("{zero}"), "00000000000000000000000000000000");

    // Max UUID
    let max = Uuid::new(u64::MAX, u64::MAX);
    assert_eq!(max.to_u128(), u128::MAX);
    assert_eq!(Uuid::from_u128(u128::MAX), max);
    assert_eq!(format!("{max}"), "ffffffffffffffffffffffffffffffff");
}
