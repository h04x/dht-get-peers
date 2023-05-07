use dht_get_peers::get_peers;
use log::debug;

fn main() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Trace)
        .try_init();

    // debian-11.6.0
    let info_hash = b"\x6d\x47\x95\xde\xe7\x0a\xeb\x88\xe0\x3e\
         \x53\x36\xca\x7c\x9f\xcf\x0a\x1e\x20\x6d";

    dbg!(get_peers(*info_hash));
}
