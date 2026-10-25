//! Property-style sweeps over the codec: encode → split → compare, across
//! representative values of every field, including non-default header sizes.

use protocol::{BASE_HEADER_SIZE, Error, Header, MAX_PAYLOAD_LEN};

fn header_variants() -> Vec<Header> {
    let mut out = Vec::new();
    for header_size in [BASE_HEADER_SIZE, 25, 32, 64] {
        for channel in [0u8, 1, 128, 255] {
            for (session_id, client_id) in [(0u32, 0u32), (u32::MAX, 1), (7, u32::MAX)] {
                for schema in [0u16, 65535] {
                    let mut header = Header::new(channel, session_id, client_id, schema, 0);
                    header.header_size = header_size;
                    out.push(header);
                }
            }
        }
    }
    out
}

fn payloads() -> Vec<Vec<u8>> {
    vec![
        Vec::new(),
        b"x".to_vec(),
        vec![7u8; 1500],
        vec![9u8; MAX_PAYLOAD_LEN],
    ]
}

#[test]
fn round_trip_across_field_combinations() {
    let payloads = payloads();
    for header in header_variants() {
        for payload in &payloads {
            let mut header = header.clone();
            header.payload_len = payload.len() as u32;

            let packet = header.encode_packet(payload).expect("encode");
            let (decoded, got) = Header::split_packet(&packet).expect("split");
            assert_eq!(decoded, header);
            assert_eq!(got, payload);

            // Trailing bytes are ignored.
            let mut with_tail = packet.clone();
            with_tail.extend_from_slice(b"trailing bytes");
            let (_, got_again) = Header::split_packet(&with_tail).expect("split with tail");
            assert_eq!(got_again, payload);
        }
    }
}

#[test]
fn payload_limits_are_enforced() {
    // At the limit: fine.
    let full = vec![0u8; MAX_PAYLOAD_LEN];
    let header = Header::new(0, 1, 1, 1, full.len() as u32);
    assert!(header.encode_packet(&full).is_ok());

    // Over the limit on encode...
    let too_big = vec![0u8; MAX_PAYLOAD_LEN + 1];
    let header = Header::new(0, 1, 1, 1, too_big.len() as u32);
    assert_eq!(
        header.encode_packet(&too_big),
        Err(Error::PayloadTooLarge {
            len: too_big.len(),
            max: MAX_PAYLOAD_LEN,
        })
    );

    // ...and on decode, so a crafted header cannot lie about its size.
    let raw = header.encode();
    assert!(matches!(
        Header::decode(&raw),
        Err(Error::PayloadTooLarge { .. })
    ));
}

#[test]
fn payload_length_mismatch_is_an_error() {
    let header = Header::new(0, 1, 1, 1, 3);
    assert!(matches!(
        header.encode_packet(b"ab"),
        Err(Error::PayloadLengthMismatch {
            declared: 3,
            actual: 2
        })
    ));
}

#[test]
fn zero_length_header_is_truncated() {
    assert_eq!(
        Header::decode(&[]),
        Err(Error::Truncated { needed: 24, got: 0 })
    );
}
