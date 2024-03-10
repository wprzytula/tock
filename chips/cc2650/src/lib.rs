#![crate_name = "cc2650"]
#![crate_type = "rlib"]
#![no_std]

pub mod chip;
mod crt1;
mod driverlib;
mod peripheral_interrupts;

pub use crate::crt1::init;
