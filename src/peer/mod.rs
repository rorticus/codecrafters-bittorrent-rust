pub mod bitfield;
pub mod handshake;
pub mod message;

pub use bitfield::Bitfield;
pub use handshake::Handshake;
pub use message::PeerMessage;

use std::io::Read;
use std::io::Write;
use std::net::SocketAddrV4;
use std::net::TcpStream;

#[derive(Debug)]
pub struct PeerConnection {
    pub socket: TcpStream,
    pub peer_id: [u8; 20],
    pub peer_choking: bool,
    pub am_interested: bool,
    pub bitfield: Option<Bitfield>,
}

fn random_peer_id() -> [u8; 20] {
    std::array::from_fn(|_| rand::random())
}

impl PeerConnection {
    pub fn connect(info_hash: [u8; 20], peer: &str) -> anyhow::Result<Self> {
        let handshake_out = Handshake {
            length: 19,
            protocol: "BitTorrent protocol".to_string(),
            info_hash,
            peer_id: random_peer_id(),
        };
        let handshake_bytes = handshake_out.to_bytes();

        let peer_addr: SocketAddrV4 = peer.parse()?;

        let mut stream = TcpStream::connect(&peer_addr)?;

        // write the handshake
        stream.write_all(&handshake_bytes)?;

        // read the handshake
        let mut buf = [0u8; 1024];
        let n = stream.read(&mut buf)?;
        let received = &buf[..n];

        let handshake_in = Handshake::from_bytes(received)?;

        return Ok(PeerConnection {
            socket: stream,
            peer_id: handshake_in.peer_id,
            bitfield: None,
            am_interested: false,
            peer_choking: true,
        });
    }

    pub fn recv_message(&mut self) -> Result<PeerMessage, anyhow::Error> {
        // read the 4 byte length prefix
        let mut len_buf = [0u8; 4];
        self.socket.read_exact(&mut len_buf)?;
        let len = u32::from_be_bytes(len_buf) as usize;

        if len == 0 {
            // keepalive message
            return Ok(PeerMessage::KeepAlive);
        }

        // read exactly len bytes
        let mut buf = vec![0u8; len];
        self.socket.read_exact(&mut buf)?;

        return Ok(PeerMessage::try_from(buf.as_slice())?);
    }

    pub fn send_message(self: &mut Self, message: &PeerMessage) -> anyhow::Result<()> {
        let bytes = message.to_wire();

        self.socket.write_all(&bytes)?;

        Ok(())
    }

    pub fn handle_one(&mut self) -> Result<PeerMessage, anyhow::Error> {
        let msg = self.recv_message()?;

        match &msg {
            PeerMessage::Choke => self.peer_choking = true,
            PeerMessage::Unchoke => self.peer_choking = false,
            PeerMessage::Have(_i) => {
                // todo
            }
            PeerMessage::Bitfield(b) => self.bitfield = Some(b.clone()),
            PeerMessage::KeepAlive => {
                // noop
            }
            _ => {}
        }

        Ok(msg)
    }
}
