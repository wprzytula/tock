#![crate_name = "cc2650"]
#![crate_type = "rlib"]
#![no_std]
#![warn(unreachable_pub)]

#[cfg(feature = "ccfg")]
mod ccfg;
pub mod chip;
mod crt1;
mod driverlib;
pub mod fcfg;
pub mod gpio;
pub mod gpt;
mod peripheral_interrupts;
pub mod prcm;
#[cfg(feature = "uart_lite")]
mod scif;
pub mod uart;
pub mod udma;

pub use crate::crt1::init;
