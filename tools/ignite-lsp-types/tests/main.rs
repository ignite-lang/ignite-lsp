//! Every integration test of this crate in one test executable
//! (`docs/vision.md`, invariant 9). Each file is a module; harness names
//! are `<file>::<test>`.

mod adversary;
mod common;
mod drift;
mod refusal;
