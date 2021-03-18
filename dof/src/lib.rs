//! Tools for extracting and parsing data in DTrace Object Format (DOF).
//!
//! The `dof` crate provides types and functions for reading and writing the DTrace Object Format,
//! an ELF-like serialization format used to store information about DTrace providers and probes in
//! object files. DOF sections are generated from source code at compile time, and used to
//! communicate information to the in-kernel portions of DTrace about the providers and probes
//! defined in the source.
//!
//! Low-level bindings to the DOF C structures are contained in the [`dof_bindings`] module,
//! however most client code with interact with the more convenient stuctures defined in the crate
//! root. The [`Section`] type describes a complete DOF section as contained in an object file. It
//! contains one or more [`Provider`]s, each of which contains one or more [`Probe`]s.
//!
//! A [`Probe`] describes the names of the related components, such as the function in which it is
//! called, the provider to which it belongs, and the probe name itself. It also contains
//! information about the location of the probe callsite in the object file itself. This is used by
//! DTrace to enable and disable the probe dynamically.
//!
//! Users of the crate will most likely be interested in deserializing existing DOF data from an
//! object file. The function [`extract_dof_sections`] may be used to pull all sections (and all
//! providers and probes) from either an ELF or Mach-O object file.
//!
//! The [`Section::from_bytes`] and [`Section::as_bytes`] methods can be used for ser/des of a
//! section directly to DOF itself, i.e., ignoring the larger object file format.
//!
//! Most useful methods and types are exported in the crate root. However, the lower-level Rust
//! bindings to the raw C-structs are also exposed in the [`dof_bindings`] module, and may be
//! extracted from a DOF byte slice with the [`des::deserialize_raw_sections`] function.
// Copyright 2021 Oxide Computer Company

#[cfg(feature = "des")]
pub mod des;
pub mod dof;
pub mod dof_bindings;
#[cfg(feature = "des")]
pub mod fmt;
pub mod ser;

#[cfg(feature = "des")]
pub use crate::des::{
    collect_dof_sections, deserialize_section, extract_dof_sections, is_dof_section,
};
pub use crate::dof::*;
pub use crate::ser::serialize_section;
