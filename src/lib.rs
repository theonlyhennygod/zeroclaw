#![warn(clippy::all, clippy::pedantic)]
#![allow(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::unnecessary_literal_bound,
    clippy::module_name_repetitions,
    clippy::struct_field_names,
    clippy::must_use_candidate,
    clippy::new_without_default,
    clippy::return_self_not_must_use
)]

pub mod config;
#[allow(dead_code)] // TODO: extract shared runtime APIs into a dedicated core crate.
pub mod heartbeat;
#[allow(dead_code)] // TODO: extract shared runtime APIs into a dedicated core crate.
pub mod memory;
#[allow(dead_code)] // TODO: extract shared runtime APIs into a dedicated core crate.
pub mod observability;
#[allow(dead_code)] // TODO: extract shared runtime APIs into a dedicated core crate.
pub mod providers;
#[allow(dead_code)] // TODO: extract shared runtime APIs into a dedicated core crate.
pub mod runtime;
#[allow(dead_code)] // TODO: extract shared runtime APIs into a dedicated core crate.
pub mod security;
