#![no_std]
#![cfg_attr(not(doc), no_main)]

mod startup;

pub use components::tmp431::SetThermometerClient;

pub use startup::{
    start, NoThermometer, CHIP, HFREQ, NUM_PROCS, PROCESSES, PROCESS_PRINTER, STACK_MEMORY,
};

pub mod console_lite {
    pub const DRIVER_NUM: usize = 2137;
}
