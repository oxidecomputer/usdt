use usdt::dtrace_provider;

dtrace_provider!("examples/provider.d");

fn main() {
    let x: u8 = 10;
    foo::bar(x);
}
