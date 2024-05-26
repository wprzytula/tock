#![allow(
    non_snake_case,
    non_upper_case_globals,
    non_camel_case_types,
    dead_code,
    unreachable_pub,
)]

#[path = "bindings.rs"]
mod driverlib_bindings;
pub use driverlib_bindings::*;
