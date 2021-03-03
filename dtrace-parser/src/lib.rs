//! A small library for parsing DTrace provider files.
// Copyright 2021 Oxide Computer Company

use thiserror::Error;

mod build;
pub mod parser;

pub use build::{build_providers, expand};

use crate::parser::Rule;

/// Type representing errors that occur when parsing a D file.
#[derive(Error, Debug)]
pub enum DTraceError {
    #[error("unexpected token type, expected {expected:?}, found {found:?}")]
    UnexpectedToken { expected: Rule, found: Rule },
    #[error("this set of pairs contains no tokens")]
    EmptyPairsIterator,
    #[error("probe names must be unique: duplicated \"{0:?}\"")]
    DuplicateProbeName((String, String)),
    #[error(transparent)]
    IO(#[from] std::io::Error),
    #[error("failed to parse according to the DTrace grammar:\n{0}")]
    ParseError(String),
    #[error("failed to build Rust/C FFI glue: {0}")]
    BuildError(String),
}
