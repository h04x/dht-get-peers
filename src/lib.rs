use std::{
    collections::HashMap,
    io,
    net::{SocketAddr, ToSocketAddrs, UdpSocket},
    time::Duration,
};

use krpc_message::{FromBencode, Message, Node, Payload, ToBencode};
use log::debug;
use thiserror::Error;

#[derive(Debug)]
pub enum PeersOrNodes {
    Peers(Vec<SocketAddr>),
    Nodes(Vec<Node>),
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("deserialize error")]
    DeserializeError(#[from] krpc_message::decoding::Error),
    #[error("expecting response")]
    ExpectResponse,
    #[error("expecting peers or/and nodes in response")]
    ExpectPeersNodes,
}

pub fn get_peers_responce_decode(recv: &[u8]) -> Result<PeersOrNodes, Error> {
    let msg = Message::from_bencode(recv)?;

    let Payload::Response(response) = msg.payload else {
        return Err(Error::ExpectResponse);
    };

    Ok(match (response.values, response.nodes) {
        (Some(peers), None) | (Some(peers), Some(_)) => {
            PeersOrNodes::Peers(peers.into_iter().map(|s| s.into()).collect())
        }
        (None, Some(nodes)) => PeersOrNodes::Nodes(nodes),
        _ => return Err(Error::ExpectPeersNodes),
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
    // hardcoded our node id
    let my_node_id = [7; 20];

    // how many nodes we select after each iterations
    const LIM_SELECTED: usize = 5;

    // collect visited nodes for prevent repetition
    let mut visited = HashMap::new();

    let mut recv_buf = [0; 1024];

    // nodes, which polled on each iteration, initiate by bootstrappers
    let mut nodes = bootstrappers.to_socket_addrs()?.collect::<Vec<_>>();

    // collected peers
    let mut peers = Vec::new();

    let sock = UdpSocket::bind("0.0.0.0:0")?;
    sock.set_read_timeout(Some(Duration::from_secs(1)))?;

    let get_peers_msg = Message::get_peers(24929, my_node_id, info_hash)
        .to_bencode()
        .expect("unable to generate get_peers msg");

    // Polls n nodes. When there is peers in the response, return peers.
    // Otherwise collect new nodes from response, filter,
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
                    Ok(PeersOrNodes::Nodes(n)) => iter_nodes.extend_from_slice(&n),
                    Ok(PeersOrNodes::Peers(p)) => peers.extend_from_slice(&p),
                    Err(e) => debug!(
                        "parse response failed with '{:?}', raw message: {:?}",
                        e,
                        Message::from_bencode(&recv_buf[..len])
                    ),
                }
            }
        }

        if !peers.is_empty() {
            peers.dedup();
            return Ok(peers);
        }

        if nodes.is_empty() {
            return Ok(vec![]);
        }

        // remove already visited
        iter_nodes.retain(|n| !visited.contains_key(&n.addr));

        // remove duplicates
        iter_nodes.dedup_by(|a, b| a.addr == b.addr);

        // sort in XOR order asc
        iter_nodes.sort_by(|a, b| {
            let a = xor(info_hash, a.id.bytes);
            let b = xor(info_hash, b.id.bytes);
            a.cmp(&b)
        });

        iter_nodes.truncate(LIM_SELECTED);

        // mark remaining nodes as visited
        iter_nodes.iter().for_each(|n| {
            visited.insert(n.addr, ());
        });

        nodes = iter_nodes.iter().map(|i| i.addr.into()).collect();
    }
}
