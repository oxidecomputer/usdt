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

cargo --version
rustc --version

export RUST_BACKTRACE=1

banner test
ptime -m cargo test --release --verbose --package empty --no-default-features
