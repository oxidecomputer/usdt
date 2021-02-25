use std::thread::sleep;
use std::time::Duration;

use usdt::dtrace_provider;

dtrace_provider!("probe-test/test.d");

fn main() {
    let duration = Duration::from_secs(1);
    let mut counter: u8 = 0;
    loop {
        test_start!(counter);
        sleep(duration);
        test_stop!("the probe has fired", counter);
        counter = counter.wrapping_add(1);
    }
}
