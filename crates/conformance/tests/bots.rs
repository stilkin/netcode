//! Bot client behavior: join, exchange, and contract enforcement.

mod common;

use conformance::bot::{Bot, BotConfig, Packet};
use protocol::Header;

/// (conformance: Bot client) A bot joins a scripted session and observes
/// the greeting, its echo, and a broadcast.
#[tokio::test]
async fn bot_joins_and_verifies_the_session() {
    let rig = common::Rig::start().await;
    let session = rig.scripted_session().await;

    let cfg = BotConfig {
        lobby: rig.lobby.addr(),
        edge: rig.edge.edge_addr(),
        token: String::new(),
    };
    let mut bot = Bot::join(&cfg, session, "probe").await.expect("bot joins");

    bot.send_input(b"i0").expect("input sent");

    let mut greeted = false;
    let mut echoed = false;
    let mut broadcast = false;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while (!greeted || !echoed || !broadcast) && tokio::time::Instant::now() < deadline {
        match bot.next().await {
            Some(Packet::Event { payload, .. }) => {
                assert_eq!(payload, b"welcome".to_vec(), "scripted greeting");
                greeted = true;
            }
            Some(Packet::State { from, payload }) if from == bot.client_id => {
                assert_eq!(payload, b"i0".to_vec());
                echoed = true;
            }
            Some(Packet::State { from: 0, payload }) => {
                let tick = u64::from_le_bytes(payload[..8].try_into().expect("tick"));
                assert!(tick >= 1, "broadcast counters start at 1");
                broadcast = true;
            }
            _ => panic!("unexpected or missing traffic"),
        }
    }
    assert!(greeted && echoed && broadcast, "all three flows observed");
    assert!(
        bot.violations.is_empty(),
        "no violations: {:?}",
        bot.violations
    );

    rig.stop().await;
}

/// (conformance: Bot client) A rejected join surfaces the reason.
#[tokio::test]
async fn rejected_join_names_the_reason() {
    let rig = common::Rig::start().await;

    let cfg = BotConfig {
        lobby: rig.lobby.addr(),
        edge: rig.edge.edge_addr(),
        token: String::new(),
    };
    let err = match Bot::join(&cfg, 4242, "probe").await {
        Err(reason) => reason,
        Ok(_) => panic!("unknown session must not join"),
    };
    assert!(err.contains("4242"), "reason names the session: {err}");

    rig.stop().await;
}

/// (conformance: Bot client) Malformed stream bytes are a recorded
/// violation, not a silent drop: the enforcement core of the receive path.
#[test]
fn malformed_stream_bytes_are_violations() {
    // A well-formed event packet followed by junk.
    let packet = Header::new(1, 7, 3, 1, 2)
        .encode_packet(b"hi")
        .expect("packet");
    let junk: &[u8] = b"garbage that decodes as nothing valid!!";
    let mut buf = packet.clone();
    buf.extend_from_slice(junk);

    let mut violations = Vec::new();
    let first = conformance::bot::parse_stream(&mut buf, &mut violations).expect("first packet");
    assert!(matches!(first, Packet::Event { .. }));
    assert_eq!(buf.len(), junk.len(), "junk remains buffered");
    assert!(violations.is_empty(), "first packet is clean");

    // Parsing the junk records the violation and refuses to yield a packet.
    let second = conformance::bot::parse_stream(&mut buf, &mut violations);
    assert!(second.is_none(), "no packet comes out of junk");
    assert_eq!(violations.len(), 1, "violation recorded");
    assert!(violations[0].kind == "bad_stream_header");
}

/// (conformance: Scenario runner, substrate) Scenarios parse strictly.
#[test]
fn scenarios_parse_strictly() {
    let valid = r#"
        [hub]
        lobby = "127.0.0.1:8080"
        edge = "127.0.0.1:4433"
        [sessions]
        mode = "join"
        ids = [1, 2]
        [bots]
        count = 4
        [script]
        inputs_per_bot = 10
        [thresholds]
        join_success = 1.0
        max_loss_ratio = 0.05
    "#;
    conformance::scenario::Scenario::parse(valid).expect("valid scenario parses");

    let bad_mode = valid.replace("mode = \"join\"", "mode = \"summon\"");
    assert!(conformance::scenario::Scenario::parse(&bad_mode).is_err());

    let typo = valid.replace("inputs_per_bot", "inputs_per_boot");
    let err = conformance::scenario::Scenario::parse(&typo).unwrap_err();
    assert!(err.contains("inputs_per_boot"), "names the key: {err}");
}
