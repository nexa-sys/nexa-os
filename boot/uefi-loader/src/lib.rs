#![no_std]

//! Stub module retained so dependent crates can keep `nexa-uefi-loader` in their
//! dependency graph. The real entry point now lives in `main.rs` because the
//! crate builds as a UEFI binary instead of a `cdylib`.
