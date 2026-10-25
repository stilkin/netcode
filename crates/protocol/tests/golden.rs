//! Golden vectors pinning the wire format (see `docs/wire-format.md`).
//!
//! These byte sequences are the contract. Regenerating one is a deliberate
//! protocol-evolution decision — never a side effect of refactoring.

use protocol::{Error, Header};

fn hex(s: &str) -> Vec<u8> {
    assert!(
        s.len().is_multiple_of(2),
        "golden vector must be whole bytes"
    );
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("valid hex"))
        .collect()
}

/// Valid v1 packet: session 42, client 7, schema 1, channel 0, payload
/// `de ad be ef`.
const V1_VALID: &str = "4e4855420100180000002a00000007000000010004000000deadbeef";

#[test]
fn golden_v1_valid_packet() {
    let bytes = hex(V1_VALID);
    let (header, payload) = Header::split_packet(&bytes).expect("valid packet splits");

    assert_eq!(header.version, 1);
    assert_eq!(header.header_size, 24);
    assert_eq!(header.channel, 0);
    assert_eq!(header.flags, 0);
    assert_eq!(header.session_id, 42);
    assert_eq!(header.client_id, 7);
    assert_eq!(header.payload_schema_version, 1);
    assert_eq!(header.payload_len, 4);
    assert_eq!(payload, [0xde, 0xad, 0xbe, 0xef]);

    // The vector is also the canonical encoding: round-tripping is byte-exact.
    assert_eq!(header.encode_packet(payload).expect("re-encode"), bytes);
}

/// A packet from the future: `header_size` 32 with 8 unknown header bytes
/// (`ca fe ba be de ad c0 de`), channel 5, 2-byte payload `00 42`.
const FUTURE_HEADER: &str = "4e4855420100200005002a00000007000000010002000000cafebabedeadc0de0042";

#[test]
fn golden_future_header_size_is_understood() {
    let bytes = hex(FUTURE_HEADER);
    let (header, payload) = Header::split_packet(&bytes).expect("future header splits");

    assert_eq!(header.header_size, 32);
    assert_eq!(header.channel, 5); // unknown channels stay opaque
    assert_eq!(header.payload_len, 2);
    assert_eq!(payload, [0x00, 0x42]); // read from offset 32, extras skipped
}

/// Same as `V1_VALID` but the magic has been zeroed.
const BAD_MAGIC: &str = "000000000100180000002a00000007000000010004000000deadbeef";

#[test]
fn golden_bad_magic_is_rejected() {
    let err = Header::split_packet(&hex(BAD_MAGIC)).expect_err("foreign packet");
    assert_eq!(err, Error::BadMagic);
}

/// Same as `V1_VALID` but declaring protocol version 2.
const UNSUPPORTED_VERSION: &str = "4e4855420200180000002a00000007000000010004000000deadbeef";

#[test]
fn golden_newer_version_is_rejected() {
    let err = Header::split_packet(&hex(UNSUPPORTED_VERSION)).expect_err("future version");
    assert_eq!(
        err,
        Error::UnsupportedVersion {
            got: 2,
            supported: 1
        }
    );
}

/// `V1_VALID` with the final payload byte missing.
const TRUNCATED: &str = "4e4855420100180000002a00000007000000010004000000deadbe";

#[test]
fn golden_truncated_packet_is_rejected() {
    let bytes = hex(TRUNCATED);
    let err = Header::split_packet(&bytes).expect_err("truncated packet");
    assert_eq!(
        err,
        Error::Truncated {
            needed: 28,
            got: 27
        }
    );

    // The header alone (24 bytes) still parses; only the payload is missing.
    let header = Header::decode(&bytes).expect("fixed header parses");
    assert_eq!(header.payload_len, 4);
}

/// Same as `V1_VALID` but declaring `header_size` 16, below the fixed minimum.
const HEADER_TOO_SMALL: &str = "4e4855420100100000002a00000007000000010004000000deadbeef";

#[test]
fn golden_shrunken_header_is_rejected() {
    let err = Header::split_packet(&hex(HEADER_TOO_SMALL)).expect_err("shrunken header");
    assert_eq!(err, Error::HeaderTooSmall { got: 16, base: 24 });
}
