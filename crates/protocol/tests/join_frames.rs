//! Join-frame codec behavior: round-trips, version gating, truncation, and
//! field-limit edges. Byte layouts are pinned separately in `golden.rs`.

use protocol::{Accept, Connect, JoinError, Reject, Reply};

#[test]
fn connect_round_trip_empty_fields() {
    let frame = Connect {
        token: String::new(),
        session_id: 0,
        name: String::new(),
    };
    let bytes = frame.encode().expect("encodes");
    assert_eq!(Connect::decode(&bytes).expect("decodes"), frame);
    assert_eq!(bytes.len(), 10, "empty strings leave only fixed fields");
}

#[test]
fn connect_round_trip_non_ascii_name() {
    let frame = Connect {
        token: "tökən".into(),
        session_id: u32::MAX,
        name: "pläyer ✨".into(),
    };
    let bytes = frame.encode().expect("encodes");
    assert_eq!(Connect::decode(&bytes).expect("decodes"), frame);
}

#[test]
fn connect_rejects_overlong_fields() {
    let long_token = "x".repeat(257);
    let frame = Connect {
        token: long_token,
        session_id: 1,
        name: "p".into(),
    };
    match frame.encode() {
        Err(JoinError::FieldTooLarge { len: 257, max: 256 }) => {}
        other => panic!("expected FieldTooLarge, got {other:?}"),
    }
}

#[test]
fn unknown_version_is_rejected() {
    let mut bytes = Connect {
        token: String::new(),
        session_id: 1,
        name: "p".into(),
    }
    .encode()
    .expect("encodes");
    bytes[1] = 9; // a version from the future
    match Connect::decode(&bytes) {
        Err(JoinError::UnsupportedVersion {
            got: 9,
            supported: 1,
        }) => {}
        other => panic!("expected UnsupportedVersion, got {other:?}"),
    }
}

#[test]
fn unknown_kind_is_rejected() {
    let mut bytes = Accept { client_id: 1 }.encode();
    bytes[0] = 0x7f;
    match Reply::decode(&bytes) {
        Err(JoinError::UnknownKind { got: 0x7f }) => {}
        other => panic!("expected UnknownKind, got {other:?}"),
    }
}

#[test]
fn truncated_frames_are_rejected_without_partial_output() {
    let full = Connect {
        token: "secret".into(),
        session_id: 3,
        name: "player".into(),
    }
    .encode()
    .expect("encodes");
    for cut in 0..full.len() {
        assert!(
            Connect::decode(&full[..cut]).is_err(),
            "prefix of {cut} bytes must not decode"
        );
    }
    let accept = Accept { client_id: 9 }.encode();
    assert!(Accept::decode(&accept[..2]).is_err());
    let reject = Reject {
        reason: "no".into(),
    }
    .encode();
    assert!(Reject::decode(&reject[..3]).is_err());
}

#[test]
fn non_utf8_string_is_rejected() {
    let mut bytes = Connect {
        token: String::new(),
        session_id: 1,
        name: "ok".into(),
    }
    .encode()
    .expect("encodes");
    let name_len = bytes.len() - 2;
    bytes.truncate(name_len); // cut inside the declared name
    bytes.extend_from_slice(&[0xff, 0xfe]); // invalid UTF-8 in its place
    match Connect::decode(&bytes) {
        Err(JoinError::BadUtf8) => {}
        other => panic!("expected BadUtf8, got {other:?}"),
    }
}

#[test]
fn trailing_bytes_are_ignored() {
    let mut bytes = Accept { client_id: 4 }.encode();
    bytes.extend_from_slice(b"junk");
    assert_eq!(Accept::decode(&bytes).expect("decodes").client_id, 4);
}
