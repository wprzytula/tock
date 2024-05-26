#![crate_name = "cc2650"]
#![crate_type = "rlib"]
#![no_std]

pub mod aes;
#[cfg(feature = "ccfg")]
mod ccfg;
pub mod chip;
mod crt1;
mod driverlib;
pub mod fcfg;
pub mod gpio;
pub mod gpt;
pub mod ieee802154_radio;
mod peripheral_interrupts;
pub mod prcm;
mod scif;
pub mod uart;
pub mod udma;

pub use crate::crt1::init;
