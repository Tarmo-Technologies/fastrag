//! CWE taxonomy utilities. Provides a descendant-closure lookup compiled
//! from MITRE's CWE-1000 Research View.

pub mod data;
pub mod taxonomy;

#[cfg(feature = "compile-tool")]
pub mod compile;

pub use taxonomy::{Taxonomy, TaxonomyError};
