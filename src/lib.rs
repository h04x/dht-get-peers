use std::{
    collections::HashMap,
    io,
    net::{IpAddr, SocketAddr, ToSocketAddrs, UdpSocket},
    time::Duration,
};

use lava_torrent::{bencode::BencodeElem, LavaTorrentError};
use thiserror::Error;

pub fn bytes_to_sock(bytes: [u8; 6]) -> SocketAddr {
    let (ip, port) = bytes.split_at(4);
    let ip = IpAddr::from(<[u8; 4]>::try_from(ip).unwrap());
    let port = u16::from_be_bytes(port.try_into().unwrap());
    SocketAddr::new(ip, port)
}

pub fn get_peers_msg(local_node_id: [u8; 20], info_hash: [u8; 20]) -> BencodeElem {
    let t = (String::from("t"), BencodeElem::String(String::from("aa")));
    let y = (String::from("y"), BencodeElem::String(String::from("q")));
    let q = (
        String::from("q"),
        BencodeElem::String(String::from("get_peers")),
    );

    let id = (
        String::from("id"),
        BencodeElem::Bytes(local_node_id.to_vec()),
    );
    let info_hash = (
        String::from("info_hash"),
        BencodeElem::Bytes(info_hash.to_vec()),
    );

    let a = (
        String::from("a"),
        BencodeElem::Dictionary(HashMap::from([id, info_hash])),
    );

    BencodeElem::Dictionary(HashMap::from([t, y, q, a]))
}

#[derive(Debug, Clone)]
pub struct CompactNode {
    pub id: [u8; 20],
    pub addr: SocketAddr,
}

impl CompactNode {
    pub fn from_bytes(bytes: [u8; 26]) -> CompactNode {
        let (id, addr) = bytes.split_at(20);
        let addr = bytes_to_sock(addr.try_into().unwrap());
        CompactNode {
            id: id.try_into().unwrap(),
            addr,
        }
    }
}

#[derive(Debug)]
pub enum PeersNodes {
    Peers(Vec<SocketAddr>),
    Nodes(Vec<CompactNode>),
}

impl PeersNodes {
    pub fn peers_from_list(elems: &[BencodeElem]) -> PeersNodes {
        let mut peers = Vec::new();
        for e in elems {
            match e {
                BencodeElem::Bytes(b) => {
                    if let Ok(bytes) = <[u8; 6]>::try_from(b.clone()) {
                        peers.push(bytes_to_sock(bytes));
                    }
                }
                BencodeElem::String(s) => {
                    if let Ok(bytes) = <[u8; 6]>::try_from(s.bytes().collect::<Vec<_>>()) {
                        peers.push(bytes_to_sock(bytes));
                    }
                }
                _ => (),
            }
        }
        PeersNodes::Peers(peers)
    }
    pub fn nodes_from_bytes(bytes: &[u8]) -> Result<PeersNodes, ()> {
        let chunks = bytes.chunks_exact(26);
        if !chunks.remainder().is_empty() {
            return Err(());
        }
        Ok(PeersNodes::Nodes(
            chunks
                .map(|c| CompactNode::from_bytes(c.try_into().unwrap()))
                .collect(),
        ))
    }
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("bencode parse error")]
    LavaTorrentError(#[from] LavaTorrentError),
    #[error("deserialize error")]
    DeserializeError(u8),
    #[error("proto error")]
    ProtoError(BencodeElem),
}

pub fn get_peers_responce_decode(recv: &[u8]) -> Result<PeersNodes, Error> {
    let bencode = BencodeElem::from_bytes(recv)?
        .first()
        .cloned()
        .ok_or(Error::DeserializeError(1))?;

    let dict = match bencode {
        BencodeElem::Dictionary(d) => d,
        _ => return Err(Error::DeserializeError(2)),
    };

    if let Some(e) = dict.get("e") {
        return Err(Error::ProtoError(e.clone()));
    }

    let r = dict.get("r").ok_or(Error::DeserializeError(3))?;

    let dict = match r {
        BencodeElem::Dictionary(d) => d,
        _ => return Err(Error::DeserializeError(4)),
    };

    let peers = dict.get("values");
    let nodes = dict.get("nodes");

    Ok(match (peers, nodes) {
        (Some(BencodeElem::List(p)), None)
        | (Some(BencodeElem::List(p)), Some(BencodeElem::Bytes(_))) => {
            PeersNodes::peers_from_list(p)
        }
        (None, Some(BencodeElem::Bytes(n))) => {
            PeersNodes::nodes_from_bytes(n).map_err(|_| Error::DeserializeError(6))?
        }
        _ => return Err(Error::DeserializeError(7)),
    })
}

#[derive(Error, Debug)]
pub enum GetPeersError {
    #[error("io error")]
    Io(#[from] io::Error),
}

fn xor(l: [u8; 20], r: [u8; 20]) -> [u8; 20] {
    l.iter()
        .zip(r)
        .map(|(x, y)| x ^ y)
        .collect::<Vec<_>>()
        .try_into()
        .unwrap()
}

pub fn get_peers(info_hash: [u8; 20]) -> Result<Vec<SocketAddr>, GetPeersError> {
    let bootstrappers = [
        "router.utorrent.com:6881",
        "dht.aelitis.com:6881",
        "router.bittorrent.com:6881",
        "dht.transmissionbt.com:6881",
    ];

    // resolve arr of host:port to SocketAddr and collect em to flat arr
    let bootstrappers = bootstrappers
        .into_iter()
        .filter_map(|h| h.to_socket_addrs().ok())
        .flatten()
        .collect::<Vec<_>>();

    get_peers_bs(info_hash, bootstrappers.as_slice())
}

pub fn get_peers_bs<T: ToSocketAddrs>(
    info_hash: [u8; 20],
    bootstrappers: T,
) -> Result<Vec<SocketAddr>, GetPeersError> {
    // hardcoded source node id
    let my_node_id = [7; 20];

    // how many closest nodes we select after each iterations
    const SELECTED: usize = 5;

    // collect visited nodes for prevent repetition
    let mut visited = HashMap::new();

    let mut recv_buf = [0; 1024];

    // nodes, that polled on each iteration, initiate by bootstrappers
    let mut nodes = bootstrappers.to_socket_addrs()?.collect::<Vec<_>>();

    // collect peers
    let mut peers = Vec::new();

    let sock = UdpSocket::bind("0.0.0.0:0")?;
    sock.set_read_timeout(Some(Duration::from_secs(1)))?;

    let get_peers_msg = get_peers_msg(my_node_id, info_hash).encode();

    // Polls n nodes. When there is peers in the answer, finish lookup and exit.
    // Otherwise collect new nodes from response, calculate distance,
    // select n closest to info_hash and poll again
    loop {
        // (id, addr) pairs, collect during iteration
        let mut iter_nodes = Vec::new();

        // send get_peers to each node
        for addr in &nodes {
            sock.send_to(&get_peers_msg, addr)?;
        }

        // and wait em response
        for _ in 0..nodes.len() {
            if let Ok(len) = sock.recv(&mut recv_buf) {
                match get_peers_responce_decode(&recv_buf[..len]) {
                    Ok(PeersNodes::Nodes(n)) => iter_nodes.extend_from_slice(&n),
                    Ok(PeersNodes::Peers(p)) => peers.extend_from_slice(&p),
                    Err(_) => (), //TODO: debug
                }
            }
        }

        if !peers.is_empty() {
            return Ok(peers);
        }

        if nodes.is_empty() {
            return Ok(vec![]);
        }

        // remove already visited
        iter_nodes.retain(|n| !visited.contains_key(&n.addr));

        // remove duplicates
        iter_nodes.dedup_by(|a, b| a.addr == b.addr);

        // order in XOR metric
        iter_nodes.sort_by(|a, b| {
            let a = xor(info_hash, a.id);
            let b = xor(info_hash, b.id);
            a.cmp(&b)
        });

        iter_nodes.truncate(SELECTED);

        // mark remaining nodes as visited
        iter_nodes.iter().for_each(|n| {
            visited.insert(n.addr, ());
        });

        nodes = iter_nodes.iter().map(|i| i.addr).collect();
    }
}
