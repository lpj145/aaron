use aaron_core::Uuid;
use std::str::FromStr;

#[test]
fn test_uuid_from_str_utf8_boundary_and_fuzz_robustness() {
    // 1. Strings with multi-byte UTF-8 characters whose boundaries hit byte 16
    let edge_cases = vec![
        "0123456789abcdef0123456789abcde✨", // 32 chars, but multi-byte in UTF-8
        "0123456789abcde🦀0123456789abcdef", // Emoji right at byte 15-19 boundary
        "0123456789abcdefé123456789abcdef",  // Multi-byte char at byte 16
        "0123456789abcdeñ123456789abcdef0",  // Multi-byte char inside clean slice
        "漢字漢字漢字漢字漢字漢字漢字漢字",  // Exactly 32 bytes of multi-byte UTF-8
        "",
        "123",
        "0123456789abcdef0123456789abcdef0123456789abcdef",
        "01234567-89ab-cdef-0123-456789abcdef-extra",
        "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz", // 32 chars, but invalid hex
        "0123456789abcdef0123456789abcdeg", // 1 invalid hex char at end
        "g123456789abcdef0123456789abcdef", // 1 invalid hex char at start
    ];

    for raw in edge_cases {
        let res = Uuid::from_str(raw);
        assert!(
            res.is_err(),
            "Malformed input '{raw}' must return Err without panicking"
        );
    }

    // 2. Automated pseudo-random fuzzer generating arbitrary byte slices
    let mut rng: u64 = 0xDEAD_BEEF_CAFE_BABE;
    for _ in 0..1000 {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;

        let len = (rng as usize) % 40;
        let mut bytes = Vec::with_capacity(len);
        for i in 0..len {
            bytes.push(((rng >> (i % 8)) & 0xFF) as u8);
        }

        if let Ok(s) = std::str::from_utf8(&bytes) {
            // Must not panic on any arbitrary UTF-8 string
            let _ = Uuid::from_str(s);
        }
    }
}

#[test]
fn test_uuid_from_str_valid_roundtrips() {
    for _ in 0..500 {
        let uuid = Uuid::random();
        let formatted_hyphenated = format!(
            "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
            (uuid.high >> 32) as u32,
            (uuid.high >> 16) as u16,
            uuid.high as u16,
            (uuid.low >> 48) as u16,
            uuid.low & 0x0000_FFFF_FFFF_FFFF
        );
        let formatted_hex = format!("{uuid}");

        let parsed_hyphen =
            Uuid::from_str(&formatted_hyphenated).expect("Valid hyphenated UUID must parse");
        assert_eq!(parsed_hyphen, uuid);

        let parsed_hex = Uuid::from_str(&formatted_hex).expect("Valid hex UUID must parse");
        assert_eq!(parsed_hex, uuid);
    }
}
