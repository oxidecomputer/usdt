use std::thread::sleep;
use std::time::Duration;

use usdt::dtrace_provider;

dtrace_provider!("probe-test/test.d");

fn main() {
    let duration = Duration::from_secs(1);
    loop {
        test_start!();
        sleep(duration);
        test_stop!(1.0);
    }
}
