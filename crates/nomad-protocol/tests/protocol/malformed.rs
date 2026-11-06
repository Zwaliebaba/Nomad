//! Truncated, oversized and garbage input and unknown versions are rejected with the right
//! typed error, and nothing panics (doc 20 §9, `CODING_STANDARDS.md` §5).

use std::fmt::Debug;

use nomad_protocol::{
    ClientMessage, FrameDecoder, HEADER_LEN, MAX_CLIENT_FRAME_LEN, MAX_FRAME_LEN,
    MAX_SERVER_FRAME_LEN, MalformedBody, PROTOCOL_VERSION, ProtocolError, ServerMessage,
    WireMessage, check_version, decode_body, decode_frame, encode_frame, hello_protocol,
};
use nomad_types::{Command, FleetId, FleetOrder, Queueing};

use crate::round_trip::frame;
use crate::samples::{at, client_messages, id, server_messages};

/// A frame with the given body, whatever it contains.
fn raw_frame(body: &[u8]) -> Vec<u8> {
    let mut bytes = u32::try_from(body.len())
        .expect("test bodies are small")
        .to_le_bytes()
        .to_vec();
    bytes.extend_from_slice(body);
    bytes
}

fn malformed<M: WireMessage + Debug>(bytes: &[u8]) -> MalformedBody {
    match decode_frame::<M>(bytes) {
        Err(ProtocolError::Malformed(reason)) => reason,
        other => panic!("expected a malformed body, got {other:?}"),
    }
}

fn assert_truncation_detected<M: WireMessage + Debug>(messages: &[M]) {
    for message in messages {
        let bytes = frame(message);
        for cut in 0..bytes.len() {
            let prefix = &bytes[..cut];
            let needed = if cut < HEADER_LEN {
                HEADER_LEN
            } else {
                bytes.len()
            };
            let expected = ProtocolError::Truncated {
                needed,
                available: cut,
            };
            assert_eq!(
                decode_frame::<M>(prefix).expect_err("the input is rejected"),
                expected
            );

            let mut decoder = FrameDecoder::<M>::new();
            decoder.push(prefix);
            assert!(
                decoder
                    .next_message()
                    .expect("a prefix is not an error")
                    .is_none(),
                "cut at {cut}"
            );
            assert_eq!(decoder.buffered_len(), cut);
            if cut == 0 {
                decoder
                    .finish()
                    .expect("an empty stream ends on a frame boundary");
            } else {
                assert_eq!(
                    decoder.finish().expect_err("the input is rejected"),
                    expected
                );
            }
        }
    }
}

#[test]
fn truncated_frames_are_rejected() {
    assert_truncation_detected(&client_messages());
    assert_truncation_detected(&server_messages());
}

fn assert_oversized_header_rejected<M: WireMessage + Debug>(len: u32) {
    let max = M::MAX_FRAME_LEN;
    let expected = ProtocolError::Oversized {
        len: u64::from(len),
        max,
    };
    // Rejected from the header alone: no body is needed or buffered.
    let header = len.to_le_bytes();
    assert_eq!(
        decode_frame::<M>(&header).expect_err("the input is rejected"),
        expected
    );

    let mut decoder = FrameDecoder::<M>::new();
    decoder.push(&header);
    decoder.push(&[0; 32]);
    assert_eq!(
        decoder.next_message().expect_err("the input is rejected"),
        expected
    );
    // The framing is lost for good: later input is ignored and the error repeats.
    decoder.push(&frame(&ClientMessage::Ping { nonce: 7 }));
    assert_eq!(decoder.buffered_len(), 0);
    assert_eq!(
        decoder.next_message().expect_err("the input is rejected"),
        expected
    );
    assert_eq!(
        decoder.next_frame().expect_err("the input is rejected"),
        expected
    );
    assert_eq!(
        decoder.finish().expect_err("the input is rejected"),
        expected
    );
}

#[test]
fn oversized_frames_are_rejected() {
    assert_eq!(MAX_FRAME_LEN, MAX_SERVER_FRAME_LEN);
    assert_oversized_header_rejected::<ClientMessage>(MAX_CLIENT_FRAME_LEN + 1);
    assert_oversized_header_rejected::<ClientMessage>(u32::MAX);
    assert_oversized_header_rejected::<ServerMessage>(MAX_SERVER_FRAME_LEN + 1);
    assert_oversized_header_rejected::<ServerMessage>(u32::MAX);

    // A frame of exactly the limit is accepted; it is merely incomplete here.
    let header = MAX_CLIENT_FRAME_LEN.to_le_bytes();
    assert!(matches!(
        decode_frame::<ClientMessage>(&header),
        Err(ProtocolError::Truncated { .. })
    ));
    // A client-sized limit does not apply to server messages.
    let header = (MAX_CLIENT_FRAME_LEN + 1).to_le_bytes();
    assert!(matches!(
        decode_frame::<ServerMessage>(&header),
        Err(ProtocolError::Truncated { .. })
    ));
}

#[test]
fn oversized_messages_are_not_encoded() {
    // 5,000 waypoints of 17 bytes do not fit in a 64 KiB client frame.
    let message = ClientMessage::Command {
        client_seq: 1,
        command: Command::Fleet {
            fleet: FleetId(id(1)),
            order: FleetOrder::Patrol {
                route: vec![at(2, 1.0, 2.0); 5_000],
            },
            queue: Queueing::Replace,
        },
    };
    let mut out = vec![1, 2, 3];
    let error = encode_frame(&message, &mut out).unwrap_err();
    assert!(
        matches!(error, ProtocolError::Oversized { len, max }
            if len > u64::from(MAX_CLIENT_FRAME_LEN) && max == MAX_CLIENT_FRAME_LEN),
        "{error:?}"
    );
    assert_eq!(out, [1, 2, 3], "nothing is appended");
}

#[test]
fn malformed_bodies_are_rejected() {
    use postcard::Error as Postcard;

    // An empty body.
    assert_eq!(
        malformed::<ClientMessage>(&raw_frame(&[])),
        MalformedBody::Decode(Postcard::DeserializeUnexpectedEnd)
    );
    // A variant that does not exist.
    assert!(matches!(
        malformed::<ClientMessage>(&raw_frame(&[200])),
        MalformedBody::Decode(_)
    ));
    assert!(matches!(
        malformed::<ServerMessage>(&raw_frame(&[9])),
        MalformedBody::Decode(_)
    ));
    // Hello with a build string that is not UTF-8.
    assert_eq!(
        malformed::<ClientMessage>(&raw_frame(&[0, 1, 2, 0xFF, 0xFE])),
        MalformedBody::Decode(Postcard::DeserializeBadUtf8)
    );
    // Hello whose build string claims more bytes than the frame has.
    assert_eq!(
        malformed::<ClientMessage>(&raw_frame(&[0, 1, 100, b'a'])),
        MalformedBody::Decode(Postcard::DeserializeUnexpectedEnd)
    );
    // A notification whose optional subject has an invalid tag.
    assert_eq!(
        malformed::<ServerMessage>(&raw_frame(&[7, 5, 2])),
        MalformedBody::Decode(Postcard::DeserializeBadOption)
    );
    // A command whose u64 sequence number is a varint longer than 10 bytes.
    let mut body = vec![2];
    body.extend_from_slice(&[0xFF; 12]);
    assert_eq!(
        malformed::<ClientMessage>(&raw_frame(&body)),
        MalformedBody::Decode(Postcard::DeserializeBadVarint)
    );
    // Ping followed by a stray byte in the same frame.
    let mut body = frame(&ClientMessage::Ping { nonce: 7 })[HEADER_LEN..].to_vec();
    body.push(0);
    assert_eq!(
        malformed::<ClientMessage>(&raw_frame(&body)),
        MalformedBody::TrailingBytes(1)
    );
    assert_eq!(
        decode_body::<ClientMessage>(&body).unwrap_err(),
        ProtocolError::Malformed(MalformedBody::TrailingBytes(1))
    );
}

#[test]
fn a_malformed_frame_does_not_desynchronise_the_stream() {
    let mut decoder = FrameDecoder::<ClientMessage>::new();
    decoder.push(&raw_frame(&[200, 1, 2, 3]));
    decoder.push(&frame(&ClientMessage::Ping { nonce: 7 }));
    assert!(matches!(
        decoder.next_message(),
        Err(ProtocolError::Malformed(MalformedBody::Decode(_)))
    ));
    assert_eq!(
        decoder.next_message().unwrap(),
        Some(ClientMessage::Ping { nonce: 7 })
    );
    assert_eq!(decoder.next_message().unwrap(), None);
    decoder.finish().unwrap();
}

#[test]
fn version_mismatch_is_detected() {
    check_version(PROTOCOL_VERSION).unwrap();
    let newer = PROTOCOL_VERSION + 1;
    let expected = ProtocolError::UnknownVersion {
        found: newer,
        expected: PROTOCOL_VERSION,
    };
    assert_eq!(check_version(newer).unwrap_err(), expected);
    assert!(check_version(0).is_err());

    // A Hello from a newer client whose remaining fields this build cannot decode: the
    // version still reads from the frozen prefix, so the mismatch is reported as such.
    let mut future_hello = vec![0, u8::try_from(newer).unwrap()];
    future_hello.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x01]);
    assert!(decode_body::<ClientMessage>(&future_hello).is_err());
    let found = hello_protocol(&future_hello).unwrap();
    assert_eq!(check_version(found).unwrap_err(), expected);

    // Anything other than a Hello is not a handshake.
    let ping = &frame(&ClientMessage::Ping { nonce: 7 })[HEADER_LEN..];
    assert_eq!(
        hello_protocol(ping).unwrap_err(),
        ProtocolError::Malformed(MalformedBody::NotHello)
    );
    assert!(matches!(
        hello_protocol(&[]),
        Err(ProtocolError::Malformed(MalformedBody::Decode(_)))
    ));
    assert!(matches!(
        hello_protocol(&[0]),
        Err(ProtocolError::Malformed(MalformedBody::Decode(_)))
    ));
}

/// `SplitMix64`: a small, seeded generator, so the garbage test is reproducible.
struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut x = self.0;
        x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        x ^ (x >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        let n = u64::try_from(n).expect("a test bound fits in u64");
        usize::try_from(self.next() % n).expect("a value below a usize bound fits in usize")
    }

    fn bytes(&mut self, len: usize) -> Vec<u8> {
        (0..len).map(|_| self.next().to_le_bytes()[0]).collect()
    }
}

/// One fuzz input, built in one of several ways so that both framing and body decoding are
/// exercised.
fn garbage(rng: &mut SplitMix, valid: &[Vec<u8>]) -> Vec<u8> {
    let pick = |rng: &mut SplitMix| valid[rng.below(valid.len())].clone();
    match rng.below(5) {
        // Pure noise: mostly bad headers.
        0 => {
            let len = rng.below(64);
            rng.bytes(len)
        }
        // A correct header around a random body.
        1 => {
            let len = rng.below(200);
            raw_frame(&rng.bytes(len))
        }
        // A valid frame with a few bytes of its body changed.
        2 => {
            let mut bytes = pick(rng);
            for _ in 0..=rng.below(4) {
                if bytes.len() > HEADER_LEN {
                    let at = HEADER_LEN + rng.below(bytes.len() - HEADER_LEN);
                    bytes[at] = rng.bytes(1)[0];
                }
            }
            bytes
        }
        // A valid frame, cut short or with its declared length changed.
        3 => {
            let mut bytes = pick(rng);
            if rng.below(2) == 0 {
                bytes.truncate(rng.below(bytes.len() + 1));
            } else {
                let len = u32::try_from(rng.below(bytes.len() + 8)).expect("test frames are small");
                bytes[..HEADER_LEN].copy_from_slice(&len.to_le_bytes());
            }
            bytes
        }
        // Two valid frames with noise between them.
        _ => {
            let mut bytes = pick(rng);
            let noise = rng.below(6);
            bytes.extend(rng.bytes(noise));
            bytes.extend(pick(rng));
            bytes
        }
    }
}

/// How `decode_frame` classified each fuzz input, to show that every path was taken.
#[derive(Debug, Default)]
struct Outcomes {
    decoded: u32,
    oversized: u32,
    truncated: u32,
    undecodable: u32,
    trailing: u32,
}

/// Feeds `bytes` through every decoding entry point. Panics only on a broken property.
fn decode_everything<M: WireMessage + Debug>(
    rng: &mut SplitMix,
    bytes: &[u8],
    outcomes: &mut Outcomes,
) {
    match decode_frame::<M>(bytes) {
        Ok((message, used)) => {
            assert!(used <= bytes.len());
            // Whatever decodes re-encodes, and its canonical bytes are stable.
            let canonical = frame(&message);
            let (again, _) = decode_frame::<M>(&canonical).expect("a canonical frame decodes");
            assert_eq!(frame(&again), canonical);
            outcomes.decoded += 1;
        }
        Err(ProtocolError::Oversized { .. }) => outcomes.oversized += 1,
        Err(ProtocolError::Truncated { needed, available }) => {
            assert_eq!(available, bytes.len());
            assert!(needed > available);
            outcomes.truncated += 1;
        }
        Err(ProtocolError::Malformed(MalformedBody::Decode(_))) => outcomes.undecodable += 1,
        Err(ProtocolError::Malformed(MalformedBody::TrailingBytes(_))) => outcomes.trailing += 1,
        Err(other) => panic!("unexpected error from decode_frame: {other:?}"),
    }
    let _ = decode_body::<M>(bytes);
    let _ = hello_protocol(bytes);
    if let Some(body) = bytes.get(HEADER_LEN..) {
        let _ = hello_protocol(body);
    }

    let mut decoder = FrameDecoder::<M>::new();
    let mut rest = bytes;
    while !rest.is_empty() {
        let n = (1 + rng.below(16)).min(rest.len());
        decoder.push(&rest[..n]);
        rest = &rest[n..];
        loop {
            match decoder.next_message() {
                Ok(Some(_)) | Err(ProtocolError::Malformed(_)) => {}
                Ok(None) => break,
                Err(ProtocolError::Oversized { .. }) => {
                    assert_eq!(decoder.buffered_len(), 0);
                    break;
                }
                Err(other) => panic!("unexpected error from the decoder: {other:?}"),
            }
        }
        assert!(decoder.buffered_len() <= bytes.len());
    }
    let _ = decoder.finish();
}

#[test]
fn seeded_garbage_never_panics() {
    const CASES: u32 = 4_000;
    let client: Vec<Vec<u8>> = client_messages().iter().map(frame).collect();
    let server: Vec<Vec<u8>> = server_messages().iter().map(frame).collect();
    let mut rng = SplitMix(0x4E4F_4D41_4400_0001);
    let mut outcomes = Outcomes::default();
    for case in 0..CASES {
        let valid = if case % 2 == 0 { &client } else { &server };
        let bytes = garbage(&mut rng, valid);
        decode_everything::<ClientMessage>(&mut rng, &bytes, &mut outcomes);
        decode_everything::<ServerMessage>(&mut rng, &bytes, &mut outcomes);
    }
    let Outcomes {
        decoded,
        oversized,
        truncated,
        undecodable,
        trailing,
    } = outcomes;
    assert!(
        [decoded, oversized, truncated, undecodable, trailing]
            .iter()
            .all(|&n| n > 0),
        "every outcome occurs: {outcomes:?}"
    );
}
