#![crate_name = "cc2650"]
#![crate_type = "rlib"]
#![no_std]

pub mod chip;
mod crt1;
mod driverlib;

pub use crate::crt1::init;
