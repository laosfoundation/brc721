use bitcoin::consensus::{Decodable, Encodable};
use bitcoin::p2p::address::Address;
use bitcoin::p2p::message::{NetworkMessage, RawNetworkMessage};
use bitcoin::p2p::message_blockdata::Inventory;
use bitcoin::p2p::message_network::VersionMessage;
use bitcoin::p2p::{Magic, PROTOCOL_VERSION};
use bitcoin::{Block, BlockHash};
use std::collections::HashMap;
use std::io::{BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct P2PFetcher {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    magic: Magic,
}

impl P2PFetcher {
    pub fn connect(addr: &str, magic: Magic) -> std::io::Result<Self> {
        log::info!("p2p connect to {}", addr);
        let writer = TcpStream::connect(addr)?;
        writer.set_nodelay(true)?;
        let reader = BufReader::new(writer.try_clone()?);
        let mut me = Self {
            reader,
            writer,
            magic,
        };
        me.handshake()?;
        Ok(me)
    }

    fn send_msg(&mut self, msg: NetworkMessage) -> std::io::Result<()> {
        let raw = RawNetworkMessage::new(self.magic, msg);
        raw.consensus_encode(&mut self.writer)?;
        self.writer.flush()
    }

    fn read_msg(&mut self) -> std::io::Result<RawNetworkMessage> {
        match RawNetworkMessage::consensus_decode(&mut self.reader) {
            Ok(m) => Ok(m),
            Err(e) => Err(std::io::Error::other(e.to_string())),
        }
    }

    fn handshake(&mut self) -> std::io::Result<()> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let recv_sa: SocketAddr = "0.0.0.0:0".parse().unwrap();
        let send_sa: SocketAddr = "0.0.0.0:0".parse().unwrap();
        let version = VersionMessage {
            version: PROTOCOL_VERSION,
            services: Default::default(),
            timestamp: now,
            receiver: Address::new(&recv_sa, Default::default()),
            sender: Address::new(&send_sa, Default::default()),
            nonce: rand_nonce(),
            user_agent: "brc721".into(),
            start_height: 0,
            relay: true,
        };
        log::debug!("p2p >> version");
        self.send_msg(NetworkMessage::Version(version))?;
        let mut got_verack = false;
        let mut got_version = false;
        for _ in 0..50 {
            let msg = self.read_msg()?;
            log::trace!("p2p << {}", msg.cmd());
            match msg.payload() {
                NetworkMessage::Version(_) => {
                    got_version = true;
                    let _ = self.send_msg(NetworkMessage::Verack);
                    log::debug!("p2p >> verack");
                }
                NetworkMessage::Verack => {
                    got_verack = true;
                }
                NetworkMessage::SendHeaders => {}
                NetworkMessage::Ping(n) => {
                    let _ = self.send_msg(NetworkMessage::Pong(*n));
                    log::trace!("p2p >> pong");
                }
                _ => {}
            }
            if got_verack && got_version {
                break;
            }
        }
        Ok(())
    }

    pub fn fetch_blocks(&mut self, hashes: &[BlockHash]) -> std::io::Result<Vec<Block>> {
        if hashes.is_empty() {
            return Ok(Vec::new());
        }
        let inv: Vec<Inventory> = hashes.iter().copied().map(Inventory::Block).collect();
        log::debug!("p2p >> getdata {} blocks", inv.len());
        self.send_msg(NetworkMessage::GetData(inv))?;
        let mut want: HashMap<BlockHash, usize> = HashMap::new();
        for (i, h) in hashes.iter().enumerate() {
            want.insert(*h, i);
        }
        let mut gathered: Vec<Option<Block>> = vec![None; hashes.len()];
        let mut remaining = hashes.len();
        while remaining > 0 {
            let msg = self.read_msg()?;
            match msg.payload() {
                NetworkMessage::Block(block) => {
                    log::trace!("p2p << block {}", block.block_hash());
                    let bh = block.block_hash();
                    if let Some(&idx) = want.get(&bh) {
                        if gathered[idx].is_none() {
                            gathered[idx] = Some(block.clone());
                            remaining -= 1;
                        }
                    }
                }
                NetworkMessage::Ping(n) => {
                    let _ = self.send_msg(NetworkMessage::Pong(*n));
                }
                _ => {}
            }
        }
        Ok(gathered.into_iter().map(|o| o.unwrap()).collect())
    }
}

fn rand_nonce() -> u64 {
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    (t & 0xFFFF_FFFF_FFFF_FFFF) as u64
}

pub fn magic_from_network_name(name: &str) -> Magic {
    match name.to_lowercase().as_str() {
        "mainnet" | "bitcoin" => Magic::BITCOIN,
        "testnet" => Magic::TESTNET3,
        "signet" => Magic::SIGNET,
        "regtest" => Magic::REGTEST,
        _ => Magic::BITCOIN,
    }
}

pub fn default_p2p_port_for_network(name: &str) -> u16 {
    match name.to_lowercase().as_str() {
        "mainnet" | "bitcoin" => 8333,
        "testnet" => 18333,
        "signet" => 38333,
        "regtest" => 18444,
        _ => 8333,
    }
}
