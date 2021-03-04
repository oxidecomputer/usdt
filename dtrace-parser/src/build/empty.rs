//! Expose USDT probe points from Rust programs.
// Copyright 2021 Oxide Computer Company

use crate::DTraceError;
use std::path::Path;

pub fn build_providers<P: AsRef<Path>>(_source: P) -> Result<(), DTraceError> {
    Ok(())
}
