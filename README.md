# dht-get-peers
Simply request peers from Bittorent DHT network

## Example
```Rust
fn main() {
    // debian-11.6.0
    let info_hash =
        b"\x6d\x47\x95\xde\xe7\x0a\xeb\x88\xe0\x3e\x53\x36\xca\x7c\x9f\xcf\x0a\x1e\x20\x6d";

    let r = get_peers(*info_hash);
    dbg!(r);
}
```