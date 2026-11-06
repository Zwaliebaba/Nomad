//! Every message variant survives framing, whole or split into pieces of any size.

use std::collections::BTreeSet;
use std::fmt::Debug;

use nomad_protocol::{
    AuthToken, ByeReason, ClientMessage, FrameDecoder, HEADER_LEN, PROTOCOL_VERSION, ServerMessage,
    WireMessage, decode_body, decode_frame, encode_frame, hello_protocol,
};
use nomad_types::Command;

use crate::samples::{
    CLIENT_VARIANTS, ORDER_VARIANTS, SERVER_VARIANTS, client_messages, client_variant,
    fleet_orders, order_variant, server_messages, server_variant,
};

pub fn frame<M: WireMessage>(message: &M) -> Vec<u8> {
    let mut out = Vec::new();
    encode_frame(message, &mut out).expect("a sample message encodes");
    out
}

fn assert_round_trips<M: WireMessage + PartialEq + Debug>(messages: &[M]) {
    let mut stream = Vec::new();
    for message in messages {
        let bytes = frame(message);
        let header: [u8; HEADER_LEN] = bytes[..HEADER_LEN]
            .try_into()
            .expect("a frame has a header");
        let body_len = usize::try_from(u32::from_le_bytes(header)).expect("u32 fits in usize");
        assert_eq!(HEADER_LEN + body_len, bytes.len(), "{message:?}");

        let (decoded, used) = decode_frame::<M>(&bytes).expect("a sample message decodes");
        assert_eq!(&decoded, message);
        assert_eq!(used, bytes.len());
        assert_eq!(
            &decode_body::<M>(&bytes[HEADER_LEN..]).expect("a sample message decodes"),
            message
        );
        stream.extend_from_slice(&bytes);
    }

    // The whole stream in one piece, byte by byte, and in uneven pieces.
    for piece_len in [stream.len(), 1, 3, 7, 64] {
        let mut decoder = FrameDecoder::<M>::new();
        let mut decoded = Vec::new();
        for piece in stream.chunks(piece_len) {
            decoder.push(piece);
            while let Some(message) = decoder.next_message().expect("a sample message decodes") {
                decoded.push(message);
            }
        }
        assert_eq!(decoded, messages, "pieces of {piece_len} bytes");
        assert_eq!(decoder.buffered_len(), 0);
        decoder
            .finish()
            .expect("the stream ends on a frame boundary");
    }
}

#[test]
fn samples_cover_every_variant() {
    let client: BTreeSet<usize> = client_messages().iter().map(client_variant).collect();
    assert_eq!(client, (0..CLIENT_VARIANTS).collect());
    let server: BTreeSet<usize> = server_messages().iter().map(server_variant).collect();
    assert_eq!(server, (0..SERVER_VARIANTS).collect());
    let orders: BTreeSet<usize> = fleet_orders().iter().map(order_variant).collect();
    assert_eq!(orders, (0..ORDER_VARIANTS).collect());
    // Every fleet order is also sent as a command.
    let commanded = client_messages()
        .iter()
        .filter_map(|m| match m {
            ClientMessage::Command {
                command: Command::Fleet { order, .. },
                ..
            } => Some(order_variant(order)),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(commanded, orders);
}

#[test]
fn every_client_message_round_trips() {
    assert_round_trips(&client_messages());
}

#[test]
fn every_server_message_round_trips() {
    assert_round_trips(&server_messages());
}

#[test]
fn next_frame_yields_raw_bodies() {
    let messages = client_messages();
    let mut decoder = FrameDecoder::<ClientMessage>::new();
    for message in &messages {
        decoder.push(&frame(message));
    }
    for message in &messages {
        let body = decoder.next_frame().unwrap().unwrap();
        assert_eq!(&decode_body::<ClientMessage>(&body).unwrap(), message);
    }
    assert_eq!(decoder.next_frame().unwrap(), None);
}

#[test]
fn handshake_layout_is_frozen() {
    // ClientMessage::Hello is variant 0 with the protocol version first.
    let hello = frame(&ClientMessage::Hello {
        protocol: PROTOCOL_VERSION,
        build: "b".to_owned(),
    });
    let body = &hello[HEADER_LEN..];
    assert_eq!(body[0], 0, "Hello is variant 0");
    assert_eq!(hello_protocol(body).unwrap(), PROTOCOL_VERSION);

    // ServerMessage::Bye { VersionMismatch { server } } is variant 0 of both enums.
    let bye = frame(&ServerMessage::Bye {
        reason: ByeReason::VersionMismatch { server: 300 },
    });
    // 300 as a postcard varint: 0b1010_1100, 0b0000_0010.
    assert_eq!(&bye[HEADER_LEN..], &[0, 0, 0xAC, 0x02]);
}

#[test]
fn auth_token_is_not_logged() {
    let token = AuthToken::new("secret-value".to_owned());
    let message = ClientMessage::Auth {
        token: token.clone(),
    };
    let logged = format!("{message:?}");
    assert!(!logged.contains("secret-value"), "{logged}");
    assert!(logged.contains("<redacted>"));
    assert_eq!(token.as_str(), "secret-value");
}
