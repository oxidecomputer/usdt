use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let make_dir = manifest_dir.join("native");
    let artifact_dir = manifest_dir.join("../target/native");
    Command::new("make")
        .args(&["clean"])
        .current_dir(&make_dir)
        .status()
        .unwrap();
    Command::new("make")
        .args(&["lib"])
        .current_dir(&make_dir)
        .status()
        .unwrap();
    println!(
        "cargo:rustc-link-search=native={}",
        artifact_dir.to_str().unwrap()
    );
    println!("cargo:rustc-link-lib=static=usdt");
}
