// Licensed under the Apache License, Version 2.0 or the MIT License.
// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright Tock Contributors 2024.

//! Provides userspace with access to a serial interface.
//!
//! Setup
//! -----
//!
//! You need a device that provides the `hil::uart::UART` trait.
//!
//! ```rust
//! # use kernel::static_init;
//! # use capsules::console::Console;
//!
//! let console = static_init!(
//!     Console<usart::USART>,
//!     Console::new(&usart::USART0,
//!                  115200,
//!                  &mut console::WRITE_BUF,
//!                  &mut console::READ_BUF,
//!                  board_kernel.create_grant(&grant_cap)));
//! hil::uart::UART::set_client(&usart::USART0, console);
//! ```
//!
//! Usage
//! -----
//!
//! The user must perform three steps in order to write a buffer:
//!
//! ```c
//! // (Optional) Set a callback to be invoked when the buffer has been written
//! subscribe(CONSOLE_DRIVER_NUM, 1, my_callback);
//! // Share the buffer from userspace with the driver
//! allow(CONSOLE_DRIVER_NUM, buffer, buffer_len_in_bytes);
//! // Initiate the transaction
//! command(CONSOLE_DRIVER_NUM, 1, len_to_write_in_bytes)
//! ```
//!
//! When the buffer has been written successfully, the buffer is released from
//! the driver. Successive writes must call `allow` each time a buffer is to be
//! written.

use kernel::deferred_call::{DeferredCall, DeferredCallClient};
use kernel::grant::{AllowRoCount, AllowRwCount, Grant, GrantKernelData, UpcallCount};
use kernel::hil::uart;
use kernel::processbuffer::ReadableProcessBuffer;
use kernel::syscall::{CommandReturn, SyscallDriver};
use kernel::utilities::cells::OptionalCell;
use kernel::{ErrorCode, ProcessId};

/// Syscall driver number.
pub const DRIVER_NUM: usize = 2137;

/// IDs for subscribed upcalls.
mod upcall {
    /// Write buffer completed callback
    pub const WRITE_DONE: usize = 1;
    /// Number of upcalls. Even though we only use one, indexing starts at 0 so
    /// to be able to use index 1 we need to specify two upcalls.
    pub const COUNT: u8 = 3;
}

/// Ids for read-only allow buffers
mod ro_allow {
    /// Readonly buffer for write buffer
    ///
    /// Before the allow syscall was handled by the kernel,
    /// console used allow number "1", so to preserve compatibility
    /// we still use allow number 1 now.
    pub const WRITE: usize = 1;
    /// The number of allow buffers the kernel stores for this grant
    pub const COUNT: u8 = 2;
}

/// Ids for read-write allow buffers
mod rw_allow {
    /// The number of allow buffers the kernel stores for this grant
    pub const COUNT: u8 = 0;
}

struct Operation {
    process_id: ProcessId,
    write_len: usize,
}

pub struct ConsoleLite<'a> {
    uart: &'a dyn uart::UartLite<'a>,
    apps: Grant<
        (),
        UpcallCount<{ upcall::COUNT }>,
        AllowRoCount<{ ro_allow::COUNT }>,
        AllowRwCount<{ rw_allow::COUNT }>,
    >,
    deferred_call: DeferredCall,
    tx_in_progress: OptionalCell<Operation>,
}

impl<'a> ConsoleLite<'a> {
    pub fn new(
        uart: &'a dyn uart::UartLite<'a>,
        grant: Grant<
            (),
            UpcallCount<{ upcall::COUNT }>,
            AllowRoCount<{ ro_allow::COUNT }>,
            AllowRwCount<{ rw_allow::COUNT }>,
        >,
    ) -> ConsoleLite<'a> {
        ConsoleLite {
            uart,
            apps: grant,
            deferred_call: DeferredCall::new(),
            tx_in_progress: OptionalCell::empty(),
        }
    }

    fn send_new(
        &self,
        process_id: ProcessId,
        kernel_data: &GrantKernelData,
        len: usize,
    ) -> Result<(), ErrorCode> {
        if self.tx_in_progress.is_some() {
            return Err(ErrorCode::BUSY);
        }

        let write_len = kernel_data
            .get_readonly_processbuffer(ro_allow::WRITE)
            .map_or(0, |write| write.len())
            .min(len);
        self.send(write_len, kernel_data);
        self.tx_in_progress.set(Operation {
            process_id,
            write_len,
        });
        self.deferred_call.set();
        Ok(())
    }

    fn send(&self, write_len: usize, kernel_data: &GrantKernelData) {
        if let Ok(write) = kernel_data.get_readonly_processbuffer(ro_allow::WRITE) {
            let _ = write.enter(|data| {
                let remaining_data = match data.get_to(..write_len) {
                    Some(remaining_data) => remaining_data,
                    None => {
                        // A slice has changed under us and is now
                        // smaller than what we need to write. Our
                        // behavior in this case is documented as
                        // undefined; the simplest thing we can do
                        // that doesn't panic is to abort the write.
                        return 0;
                    }
                };

                let mut iter = remaining_data
                    .chunks(2)
                    .filter_map(|one_or_two_byte_slice| {
                        let mut iter = one_or_two_byte_slice.iter();
                        iter.next().map(|first| {
                            if let Some(second) = iter.next() {
                                kernel::hil::uart::UartLiteWord::TwoByte(first.get(), second.get())
                            } else {
                                kernel::hil::uart::UartLiteWord::EndingOneByte(first.get())
                            }
                        })
                    });

                let lite_input = kernel::hil::uart::UartLiteInput::new(&mut iter, write_len);

                self.uart.transmit_iterator(lite_input);

                0 // This is just to type check; it bears no meaning.
            });
        }
    }

    fn tx_done(&self) {
        if let Some(Operation {
            process_id,
            write_len,
        }) = self.tx_in_progress.take()
        {
            let _ = self.apps.enter(process_id, |_app, kernel_data| {
                let _ = kernel_data.schedule_upcall(upcall::WRITE_DONE, (write_len, 0, 0));
            });
        }
    }
}

impl SyscallDriver for ConsoleLite<'_> {
    /// Initiate serial transfers
    ///
    /// ### `command_num`
    ///
    /// - `0`: Driver existence check.
    /// - `1`: Transmits a buffer passed via `allow`, up to the length
    ///        passed in `arg1`
    fn command(
        &self,
        cmd_num: usize,
        arg1: usize,
        _: usize,
        processid: ProcessId,
    ) -> CommandReturn {
        let res = self
            .apps
            .enter(processid, |_app, kernel_data| {
                match cmd_num {
                    0 => Ok(()),
                    1 => {
                        // putstr
                        let len = arg1;
                        self.send_new(processid, kernel_data, len)
                    }
                    _ => Err(ErrorCode::NOSUPPORT),
                }
            })
            .map_err(ErrorCode::from);
        match res {
            Ok(Ok(())) => CommandReturn::success(),
            Ok(Err(e)) => CommandReturn::failure(e),
            Err(e) => CommandReturn::failure(e),
        }
    }

    fn allocate_grant(&self, processid: ProcessId) -> Result<(), kernel::process::Error> {
        self.apps.enter(processid, |_, _| {})
    }
}

impl DeferredCallClient for ConsoleLite<'_> {
    fn handle_deferred_call(&self) {
        self.tx_done();
    }

    fn register(&'static self) {
        self.deferred_call.register(self);
    }
}
