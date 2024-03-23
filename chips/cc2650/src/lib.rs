#![crate_name = "cc2650"]
#![crate_type = "rlib"]
#![no_std]

pub mod chip;
mod crt1;
mod driverlib;
pub mod gpio;
mod peripheral_interrupts;
pub mod prcm;
pub mod uart;

pub use crate::crt1::init;
