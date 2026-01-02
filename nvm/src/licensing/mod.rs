//! Enterprise License Management
//!
//! Handles license validation, feature gating, and subscription management.

pub mod types;
pub mod validator;
pub mod features;

pub use types::*;
pub use validator::LicenseValidator;
pub use features::FeatureGate;
