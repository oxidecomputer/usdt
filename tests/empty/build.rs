use usdt::Builder;

fn main() {
    Builder::new("provider.d")
        .build()
        .expect("Failed to build provider");
}
