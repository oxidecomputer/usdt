//! Expose USDT probe points from Rust programs.
// Copyright 2021 Oxide Computer Company

use std::path::Path;
use crate::DTraceError;

pub fn build_providers<P: AsRef<Path>>(_source: P) -> Result<(), DTraceError> {
    Ok(())
}
