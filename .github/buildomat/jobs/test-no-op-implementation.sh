#!/bin/bash
#:
#: name = "illumos / test-no-op-implementation"
#: variety = "basic"
#: target = "helios"
#: rust_toolchain = "stable"
#: output_rules = []
#:

set -o errexit
set -o pipefail
set -o xtrace

rustup show active-toolchain || rustup toolchain install
cargo --version
rustc --version

export RUST_BACKTRACE=1

banner test
ptime -m cargo test \
        --release \
        --verbose \
        --no-default-features \
        --package empty
