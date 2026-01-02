//! VM Template Management
//!
//! Template library with OVA/OVF import/export capabilities.

pub mod types;
pub mod library;
pub mod ova;

pub use types::*;
pub use library::TemplateLibrary;
pub use ova::{OvaImporter, OvaExporter};