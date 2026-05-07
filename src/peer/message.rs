pub const MSG_CHOKE: u8 = 0;
pub const MSG_UNCHOKE: u8 = 1;
pub const MSG_INTERESTED: u8 = 2;
pub const MSG_NOT_INTERESTED: u8 = 3;
pub const MSG_HAVE: u8 = 4;
pub const MSG_BITFIELD: u8 = 5;
pub const MSG_REQUEST: u8 = 6;
pub const MSG_PIECE: u8 = 7;
pub const MSG_CANCEL: u8 = 8;

use crate::peer::bitfield::Bitfield;

#[derive(Debug, thiserror::Error)]
pub enum MessageParseError {
    #[error("bad message length expected {0} received {1}")]
    BadLength(usize, usize),

    #[error("invalid message id {0}")]
    InvalidMessageId(u8),
}

#[derive(Debug)]
pub enum PeerMessage {
    KeepAlive,
    Choke,
    Unchoke,
    Interested,
    NotInterested,
    Have(u32),
    Bitfield(Bitfield),
    Request {
        index: u32,
        begin: u32,
        length: u32,
    },
    Piece {
        index: u32,
        begin: u32,
        block: Vec<u8>,
    },
    Cancel {
        index: u32,
        begin: u32,
        length: u32,
    },
}

impl TryFrom<&[u8]> for PeerMessage {
    type Error = MessageParseError;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        if bytes.is_empty() {
            return Ok(PeerMessage::KeepAlive);
        }

        let message_id = bytes[0];
        let payload = if bytes.len() > 1 {
            &bytes[1..bytes.len()]
        } else {
            &[]
        };

        match message_id {
            // choke
            MSG_CHOKE => return Ok(PeerMessage::Choke),
            MSG_UNCHOKE => return Ok(PeerMessage::Unchoke),
            MSG_INTERESTED => return Ok(PeerMessage::Interested),
            MSG_NOT_INTERESTED => return Ok(PeerMessage::NotInterested),
            MSG_HAVE => {
                if payload.len() != 4 {
                    return Err(MessageParseError::BadLength(4, payload.len()));
                }
                return Ok(PeerMessage::Have(u32::from_be_bytes([
                    payload[0], payload[1], payload[2], payload[3],
                ])));
            }
            MSG_BITFIELD => return Ok(PeerMessage::Bitfield(Bitfield::from_bytes(payload))),
            MSG_REQUEST => {
                if payload.len() != 12 {
                    return Err(MessageParseError::BadLength(12, payload.len()));
                }

                let index = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);

                let begin = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]);

                let length = u32::from_be_bytes([payload[8], payload[9], payload[10], payload[11]]);

                return Ok(PeerMessage::Request {
                    index,
                    begin,
                    length,
                });
            }
            MSG_PIECE => {
                if payload.len() < 8 {
                    return Err(MessageParseError::BadLength(8, payload.len()));
                }

                let index = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);

                let begin = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]);

                let block: Vec<u8> = Vec::from(&payload[8..payload.len()]);

                return Ok(PeerMessage::Piece {
                    index,
                    begin,
                    block,
                });
            }
            MSG_CANCEL => {
                if payload.len() != 12 {
                    return Err(MessageParseError::BadLength(12, payload.len()));
                }

                let index = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);

                let begin = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]);

                let length = u32::from_be_bytes([payload[8], payload[9], payload[10], payload[11]]);

                return Ok(PeerMessage::Cancel {
                    index,
                    begin,
                    length,
                });
            }
            _ => Err(MessageParseError::InvalidMessageId(message_id)),
        }
    }
}

impl PeerMessage {
    fn frame(id: u8, payload: &[u8]) -> Vec<u8> {
        let mut bytes: Vec<u8> = Vec::new();

        let length = 1 + payload.len();

        bytes.extend((length as u32).to_be_bytes());
        bytes.push(id);

        if payload.len() > 0 {
            bytes.extend(payload);
        }

        return bytes;
    }

    pub fn to_wire(&self) -> Vec<u8> {
        match self {
            PeerMessage::KeepAlive => {
                return vec![];
            }
            PeerMessage::Choke => return PeerMessage::frame(MSG_CHOKE, &[]),
            PeerMessage::Unchoke => return PeerMessage::frame(MSG_UNCHOKE, &[]),
            PeerMessage::Interested => return PeerMessage::frame(MSG_INTERESTED, &[]),
            PeerMessage::NotInterested => return PeerMessage::frame(MSG_NOT_INTERESTED, &[]),
            PeerMessage::Have(index) => return PeerMessage::frame(MSG_HAVE, &index.to_be_bytes()),
            PeerMessage::Request {
                index,
                begin,
                length,
            } => {
                let mut bytes: Vec<u8> = Vec::new();

                bytes.extend(index.to_be_bytes());
                bytes.extend(begin.to_be_bytes());
                bytes.extend(length.to_be_bytes());

                return PeerMessage::frame(MSG_REQUEST, &bytes);
            }
            _ => {
                eprintln!("Couldn't convert {:?} to bytes", self);
                return vec![];
            }
        }
    }
}
