#!/bin/bash
#:
#: name = "illumos / build-and-test"
#: variety = "basic"
#: target = "helios"
#: rust_toolchain = "1.85"
#: output_rules = []
#:

set -o errexit
set -o pipefail
set -o xtrace

rustup show active-toolchain || rustup toolchain install
cargo --version
rustc --version

export RUST_BACKTRACE=1

banner clippy
ptime -m cargo clippy \
        --workspace \
        -- -D warnings -A clippy::style

banner test
ptime -m cargo test \
        --release \
        --no-fail-fast \
        --verbose \
        --workspace
