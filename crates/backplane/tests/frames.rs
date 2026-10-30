//! Round-trip and rejection tests for the backplane frame codec.

use backplane::{Error, Frame, MAX_FRAME_LEN, conn, conn::ConnError};
use tokio::io::AsyncWriteExt;

fn sample_frame() -> Frame {
    Frame::Register {
        claimed_id: 0,
        game_type: "echo-arena".into(),
        display_name: "Écho Aréna 🎮".into(),
        max_players: 8,
        payload_schema_version: 1,
    }
}

#[test]
fn round_trip_every_opcode() {
    let frames = vec![
        Frame::Hello {
            token: String::new(),
        },
        Frame::Hello {
            token: "s3cret".into(),
        },
        sample_frame(),
        Frame::Register {
            claimed_id: 42,
            game_type: "g".into(),
            display_name: "n".into(),
            max_players: 1,
            payload_schema_version: u16::MAX,
        },
        Frame::Registered { session_id: 7 },
        Frame::Heartbeat,
        Frame::ClientAttached {
            client_id: 3,
            player_name: "Zoë 🎮".into(),
        },
        Frame::ClientAttached {
            client_id: 4,
            player_name: String::new(),
        },
        Frame::ClientDetached { client_id: 3 },
        Frame::ToClient {
            packet: vec![1, 2, 3],
        },
        Frame::ToMany {
            client_ids: vec![1, 2, 3],
            packet: Vec::new(),
        },
        Frame::ToMany {
            client_ids: Vec::new(),
            packet: vec![9],
        },
        Frame::Broadcast { packet: vec![0] },
        Frame::FromClient { packet: Vec::new() },
    ];
    for frame in frames {
        let bytes = frame.encode_to_vec().expect("encode");
        assert_eq!(
            Frame::decode(&bytes[4..]).expect("decode"),
            frame,
            "{frame:?}"
        );
        assert_eq!(bytes[4], frame.opcode().to_u16() as u8);
    }
}

#[test]
fn client_attached_layout_includes_player_name() {
    let bytes = Frame::ClientAttached {
        client_id: 3,
        player_name: "A".into(),
    }
    .encode_to_vec()
    .expect("encode");
    assert_eq!(
        bytes,
        [9, 0, 0, 0, 5, 0, 3, 0, 0, 0, 1, 0, b'A'],
        "length, opcode, client id, then a length-prefixed UTF-8 name"
    );
}

#[test]
fn empty_packet_bodies_round_trip() {
    let bytes = Frame::ToClient { packet: Vec::new() }
        .encode_to_vec()
        .expect("encode");
    assert_eq!(
        Frame::decode(&bytes[4..]).expect("decode"),
        Frame::ToClient { packet: Vec::new() }
    );
}

#[test]
fn oversized_frame_is_rejected_on_encode() {
    let mut frame = Frame::ToMany {
        client_ids: Vec::new(),
        packet: Vec::new(),
    };
    if let Frame::ToMany { packet, .. } = &mut frame {
        *packet = vec![0u8; MAX_FRAME_LEN + 1];
    }
    assert!(matches!(frame.encode_to_vec(), Err(Error::TooLarge { .. })));
}

#[test]
fn unknown_opcode_is_rejected() {
    // Valid length prefix (2), unknown opcode 999.
    let bytes = 2u32
        .to_le_bytes()
        .iter()
        .chain(&[231, 3])
        .copied()
        .collect::<Vec<u8>>();
    assert_eq!(Frame::decode(&bytes[4..]), Err(Error::UnknownOpcode(999)));
}

#[test]
fn truncated_bodies_are_rejected() {
    // Register without its string fields.
    let bytes = Frame::Register {
        claimed_id: 0,
        game_type: String::new(),
        display_name: String::new(),
        max_players: 0,
        payload_schema_version: 0,
    }
    .encode_to_vec()
    .expect("encode");
    assert!(matches!(
        Frame::decode(&bytes[4..bytes.len() - 2]),
        Err(Error::MalformedBody(_))
    ));

    let attached = Frame::ClientAttached {
        client_id: 3,
        player_name: "player".into(),
    }
    .encode_to_vec()
    .expect("encode");
    assert!(matches!(
        Frame::decode(&attached[4..attached.len() - 1]),
        Err(Error::MalformedBody(_))
    ));
}

#[tokio::test]
async fn conn_round_trip_over_duplex() {
    let (mut client, mut server) = tokio::io::duplex(4096);
    let frame = Frame::Broadcast {
        packet: vec![5; 64],
    };
    conn::write_frame(&mut client, &frame)
        .await
        .expect("write frame");
    drop(client); // half-close: buffered data stays readable, then EOF
    let read = conn::read_frame(&mut server).await.expect("read frame");
    assert_eq!(
        read,
        Frame::Broadcast {
            packet: vec![5; 64]
        }
    );
}

#[tokio::test]
async fn conn_rejects_oversized_length_prefix() {
    let (mut client, mut server) = tokio::io::duplex(4096);
    client
        .write_all(&(MAX_FRAME_LEN as u32 + 1).to_le_bytes())
        .await
        .unwrap();
    drop(client);
    let err = conn::read_frame(&mut server).await.expect_err("oversized");
    assert!(matches!(err, ConnError::Protocol(Error::TooLarge { .. })));
}
