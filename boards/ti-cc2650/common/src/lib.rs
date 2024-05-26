#![no_std]
#![cfg_attr(not(doc), no_main)]

mod startup;

pub use startup::{start, CHIP, HFREQ, NUM_PROCS, PROCESSES, PROCESS_PRINTER, STACK_MEMORY};
