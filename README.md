# dht-get-peers
Request a peers from Bittorent DHT network [BEP05](https://www.bittorrent.org/beps/bep_0005.html)

## Example
```Rust
use dht_get_peers::get_peers;

fn main() {
    // debian-11.6.0
    let info_hash = b"\x6d\x47\x95\xde\xe7\x0a\xeb\x88\xe0\x3e\
         \x53\x36\xca\x7c\x9f\xcf\x0a\x1e\x20\x6d";

    dbg!(get_peers(*info_hash));
}
```