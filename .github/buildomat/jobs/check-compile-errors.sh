#!/bin/bash
#:
#: name = "illumos / check-compile-errors"
#: variety = "basic"
#: target = "helios"
#: rust_toolchain = "nightly-2021-11-24"
#: output_rules = []
#:

set -o errexit
set -o pipefail
set -o xtrace

cargo --version
rustc --version

export RUST_BACKTRACE=1

banner test
ptime -m cargo test \
        --release \
        --no-fail-fast \
        --verbose \
        --package compile-errors
