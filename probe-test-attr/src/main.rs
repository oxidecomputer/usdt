#![feature(asm)]

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Arg {
    x: u8,
}

#[usdt::provider(format = "foo_{provider}_{probe}")]
mod provider_name {
    use super::Arg;
    fn probe_foo(x: u8, name: String, arg: Arg) {}
}

fn main() {
    usdt::register_probes().unwrap();
    for x in 0.. {
        std::thread::sleep(std::time::Duration::from_secs(1));
        foo_provider_name_probe_foo!(|| (0, "something", Arg { x }));
    }
}
