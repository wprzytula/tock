// Licensed under the Apache License, Version 2.0 or the MIT License.
// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright Tock Contributors 2022.

//! Provides userspace with access to chip configuration.
//!
//! Userspace Interface
//! -------------------
//!
//! ### `command` System Call
//!
//! The `command` system call support one argument `cmd` which is used to
//! specify the specific operation, currently the following cmd's are supported:
//!
//! * `0`: check whether the driver exists
//! * `1`: read the chip-configured IEEE MAC address
//!
//!
//! The possible return from the 'command' system call indicates the following:
//!
//! * `Ok(())`:    The operation has been successful.
//! * `NOSUPPORT`: Invalid `cmd`.
//!
//! Usage
//! -----
//!
//! You need a device that provides the `hil::chip_config::ChipConfiguration` trait.
//! ```

use kernel::hil;
use kernel::syscall::{CommandReturn, SyscallDriver};
use kernel::{ErrorCode, ProcessId};

/// Syscall driver number.
use capsules_core::driver;
pub const DRIVER_NUM: usize = driver::NUM::ChipConfiguration as usize;

pub struct ChipConfiguration<'a, T: hil::chip_config::ChipConfiguration> {
    driver: &'a T,
}

impl<'a, T: hil::chip_config::ChipConfiguration> ChipConfiguration<'a, T> {
    pub fn new(driver: &'a T) -> ChipConfiguration<'a, T> {
        ChipConfiguration { driver }
    }
}

impl<'a, T: hil::chip_config::ChipConfiguration> SyscallDriver for ChipConfiguration<'a, T> {
    fn command(
        &self,
        command_num: usize,
        _: usize,
        _: usize,
        _processid: ProcessId,
    ) -> CommandReturn {
        match command_num {
            // driver existence check
            0 => CommandReturn::success(),

            // get IEEE MAC
            1 => CommandReturn::success_u64(self.driver.ieee_mac()),
            _ => CommandReturn::failure(ErrorCode::NOSUPPORT),
        }
    }

    fn allocate_grant(&self, _: ProcessId) -> Result<(), kernel::process::Error> {
        Ok(())
    }
}
