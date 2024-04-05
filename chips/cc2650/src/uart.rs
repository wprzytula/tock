// 19.4.5 FIFO Operation
// The UART has two 32-entry FIFOs; one for transmit and one for receive. Both FIFOs are accessed
// through the UART Data Register (UART:DR). Read operations of the UART:DR register return a 12-bit
// value consisting of 8 data bits and 4 error flags, while write operations place 8-bit data in the TX FIFO.
// Out of reset, both FIFOs are disabled and act as 1-byte-deep holding registers. The FIFOs are enabled by
// setting the UART:LCRH FEN register bit.
// FIFO status can be monitored through the UART Flag Register (UART:FR) and the UART Receive Status
// Register (UART:RSR). Hardware monitors empty, full, and overrun conditions. The UART:FR register
// contains empty and full flags (TXFE, TXFF, RXFE, and RXFF bits), and the UART:RSR register shows
// overrun status through the OE bit. If the FIFOs are disabled, the empty and full flags are set according to
// the status of the 1-byte deep holding registers.
// The trigger points at which the FIFOs generate interrupts are controlled through the UART Interrupt FIFO
// Level Select Register (UART:IFLS). Both FIFOs can be individually configured to trigger interrupts at
// different levels. Available configurations include ⅛, ¼, ½, ¾, and ⅞. For example, if the ¼ option is
// selected for the receive FIFO, the UART generates a receive interrupt after 4 data bytes are received. Out
// of reset, both FIFOs are configured to trigger an interrupt at the ½ mark.

use core::{arch::asm, cell::Cell};

use kernel::{hil, ErrorCode};
use tock_cells::{map_cell::MapCell, optional_cell::OptionalCell};

use crate::{driverlib, udma};

// 48 MHz
const CLOCK_FREQ: u32 = 48_000_000;
pub const BAUD_RATE: u32 = 115_200;

/// Stores an ongoing TX/RX transaction
struct Transaction {
    /// The buffer containing the bytes to transmit as it should be returned to
    /// the client
    buffer: &'static mut [u8],
    /// The total amount to transmit
    length: usize,
    /// The index of the byte currently being sent
    index: usize,
}

pub struct UartFull<'a> {
    uart: cc2650::UART0,
    udma: udma::Udma,
    tx_client: OptionalCell<&'a dyn hil::uart::TransmitClient>,
    rx_client: OptionalCell<&'a dyn hil::uart::ReceiveClient>,
    tx_transaction: MapCell<Transaction>,
    rx_transaction: MapCell<Transaction>,
    rx_abort_in_progress: Cell<bool>,
}

impl<'a> UartFull<'a> {
    /// Constructor
    // This should only be constructed once
    pub fn new(uart: cc2650::UART0) -> Self {
        Self {
            uart,
            // Currently no better idea how to make it cleaner.
            // Stealing implies that this code won't work in debug builds,
            // because of debug_assert! inside...
            udma: udma::Udma::new(unsafe { cc2650::Peripherals::steal().UDMA0 }),
            tx_client: OptionalCell::empty(),
            rx_client: OptionalCell::empty(),
            tx_transaction: MapCell::empty(),
            rx_transaction: MapCell::empty(),
            rx_abort_in_progress: Cell::new(false),
        }
    }

    /// The idea is that this is only called once per MCU reboot.
    #[inline]
    pub fn initialize(&self) {
        /*
        // 2. Configure the IOC module to map UART signals to the correct GPIO pins.
        // RF1.7_UART_RX EM -> DIO_2
        peripherals
            .IOC
            .iocfg2
            .modify(|_r, w| w.port_id().uart0_rx().ie().set_bit());
        // RF1.9_UART_TX EM -> DIO_3
        peripherals
            .IOC
            .iocfg3
            .modify(|_r, w| w.port_id().uart0_tx().ie().clear_bit());
        */
        unsafe {
            driverlib::IOCPinTypeUart(
                driverlib::UART0_BASE,
                driverlib::IOID_2,
                driverlib::IOID_3,
                driverlib::IOID_UNUSED,
                driverlib::IOID_UNUSED,
            )
        };

        /*
        // For this example, the UART clock is assumed to be 24 MHz, and the desired UART configuration is:
        // • Baud rate: 115 200
        // • Data length of 8 bits
        // • One stop bit
        // • No parity
        // • FIFOs disabled
        // • No interrupts
        //
        // The first thing to consider when programming the UART is the BRD because the UART:IBRD and
        // UART:FBRD registers must be written before the UART:LCRH register.
        // The BRD can be calculated using the equation:
        //      BRD = 24 000 000 / (16 × 115 200) = 13.0208
        // The result of Equation 3 indicates that the UART:IBRD DIVINT field must be set to 13 decimal or 0xD.
        //
        // Equation 4 calculates the value to be loaded into the UART:FBRD register.
        //      UART:FBRD.DIVFRAC = integer (0.0208 × 64 + 0.5) = 1
        //
        // With the BRD values available, the UART configuration is written to the module in the following order:
        let uart = &peripherals.UART0;

        // 1. Disable the UART by clearing the UART:CTL UARTEN bit.
        uart.ctl.modify(|_r, w| w.uarten().dis());

        // 2. Write the integer portion of the BRD to the UART:IBRD register.
        // uart.ibrd.modify(|_r, w| unsafe { w.divint().bits(13) });
        uart.ibrd.modify(|_r, w| unsafe { w.divint().bits(26) }); // for 48 MHz

        // 3. Write the fractional portion of the BRD to the UART:FBRD register.
        // uart.fbrd.modify(|_r, w| unsafe { w.divfrac().bits(1) });
        uart.fbrd.modify(|_r, w| unsafe { w.divfrac().bits(3) }); // for 48 MHz

        // 4. Write the desired serial parameters to the UART:LCRH register (in this case, a value of 0x0000 0060).
        uart.lcrh.modify(|_r, w| w.pen().dis().wlen()._8());

        // 5. Enable the UART by setting the UART:CTL UARTEN bit.
        uart.ctl
            .modify(|_r, w| w.uarten().en().txe().en().rxe().en());
        */

        unsafe {
            driverlib::UARTConfigSetExpClk(
                driverlib::UART0_BASE,
                CLOCK_FREQ,
                BAUD_RATE,
                driverlib::UART_CONFIG_PAR_NONE
                    | driverlib::UART_CONFIG_STOP_ONE
                    | driverlib::UART_CONFIG_WLEN_8,
            )
        };

        self.udma.uart_channels_configure();
    }

    fn set_baud_rate(&self, baud_rate: u32) {
        let div = (((CLOCK_FREQ * 8) / baud_rate) + 1) / 2;
        self.uart
            .ibrd
            .write(|w| unsafe { w.divint().bits((div / 64).try_into().unwrap()) });
        self.uart
            .fbrd
            .write(|w| unsafe { w.divfrac().bits((div % 64).try_into().unwrap()) })
    }

    /// The idea is that this is run each time MCU stops deep sleep.
    pub fn enable(&self) {
        // Disable, because they should be enabled only upon a transfer/receive request.
        self.udma.uart_disable_tx();
        self.udma.uart_disable_rx();

        // UARTEnable is static inline, so better use our own version.
        // unsafe { driverlib::UARTEnable(driverlib::UART0_BASE) }

        // Enable the FIFO.
        self.uart.lcrh.modify(|_r, w| w.fen().en());

        // Enable RX, TX, and the UART.
        self.uart
            .ctl
            .modify(|_r, w| w.uarten().en().txe().en() /*.rxe().en()*/);
    }

    #[allow(dead_code)]
    pub fn disable(&self) {
        self.dma_stop_tx();
        self.udma.uart_disable_tx();
        self.udma.uart_disable_rx();
        unsafe { driverlib::UARTDisable(driverlib::UART0_BASE) };
    }

    fn dma_start_tx(&self) {
        self.uart.dmactl.modify(|_r, w| w.txdmae().set_bit());
    }

    fn dma_stop_tx(&self) {
        self.udma.uart_disable_tx();
        self.uart.dmactl.modify(|_r, w| w.txdmae().clear_bit());
    }

    fn enable_rx_interrupts(&self) {
        // Set interrupts:
        // - receive interrupt
        // - reception timeout interrupt
        self.uart
            .imsc
            .modify(|_r, w| w.rxim().set_bit().rtim().set_bit())
    }

    fn enable_tx_interrupts(&self) {
        // Set interrupts:
        // - transmit interrupt
        self.uart.imsc.modify(|_r, w| w.txim().set_bit())
    }

    fn disable_rx_interrupts(&self) {
        // Unset interrupts:
        // - receive interrupt
        // - reception timeout interrupt
        self.uart
            .imsc
            .modify(|_r, w| w.rxim().clear_bit().rtim().clear_bit())
    }

    fn disable_tx_interrupts(&self) {
        // Unset interrupts:
        // - transmit interrupt
        self.uart.imsc.modify(|_r, w| w.txim().clear_bit())
    }

    /// UART interrupt handler that listens for both TX and RX end events
    #[inline(never)]
    pub(crate) fn handle_interrupt(&self) {
        // TODO: handle rx timeout

        let tx_completed = self.udma.uart_request_done_tx();
        if tx_completed {
            self.udma.uart_request_done_tx_clear()
        }

        // FIXME: debug prints
        let ris = self.uart.ris.read();
        if ris.txris().bit_is_set() {
            unsafe {
                asm!("nop");
            }
            // kernel::debug!("TXRIS set");
        }
        if ris.oeris().bit_is_set() {
            unsafe {
                asm!("nop");
            }
            // kernel::debug!("OERIS set");
        }

        let mis = self.uart.mis.read();
        if mis.txmis().bit_is_set() {
            unsafe {
                asm!("nop");
            }
            // kernel::debug!("TXMIS set");
        }
        if mis.oemis().bit_is_set() {
            unsafe {
                asm!("nop");
            }
            // kernel::debug!("OEMIS set");
        }

        if self.udma.uart_is_enabled_tx() {
            // kernel::debug!("UART TX is enabled.");
        }
        // FIXME END: debug prints

        // clear interrupt flags
        self.uart.icr.write(|w| {
            w
                // .beic()              // break error
                // .set_bit()
                // .ctsmic()            // Clear-To-Send ...
                // .set_bit()
                // .feic()              // framing error
                // .set_bit()
                // .oeic()              // buffer overrun error
                // .set_bit()
                // .peic()              // parity error
                // .set_bit()
                .rtic() // reception timeout
                .set_bit()
                .rxic() // receive
                .set_bit()
                .txic() // transmit
                .set_bit()
        });

        // TX transfer finished
        if !self.udma.uart_is_enabled_tx() {
            self.tx_transaction.take().map(
                |Transaction {
                     buffer,
                     length,
                     index,
                 }| {
                    let remaining_len = length - index;
                    if remaining_len == 0 {
                        // Transaction has completed.
                        self.dma_stop_tx();
                        self.tx_client.map(move |client| {
                            client.transmitted_buffer(buffer, length, Ok(()));
                        });
                    } else {
                        // There are more transfers to be done for this request,
                        // due to the upper bound on uDMA transfer size (1024).
                        let next_transfer_len =
                            usize::min(remaining_len, driverlib::UDMA_XFER_SIZE_MAX as usize);

                        self.udma
                            .uart_transfer_tx(&buffer[index..index + next_transfer_len]);

                        self.tx_transaction.put(Transaction {
                            buffer,
                            length,
                            index: index + next_transfer_len,
                        });
                    }
                },
            );
        }

        /* FIXME: RX
        if self.rx_ready() {
            self.disable_rx_interrupts();

            // Get the number of bytes in the buffer that was received this time
            let rx_bytes = self.registers.rxd_amount.get() as usize;

            // Check if this ENDRX is due to an abort. If so, we want to
            // do the receive callback immediately.
            if self.rx_abort_in_progress.get() {
                self.rx_abort_in_progress.set(false);
                self.rx_client.map(|client| {
                    self.rx_buffer.take().map(|rx_buffer| {
                        client.received_buffer(
                            rx_buffer,
                            self.offset.get() + rx_bytes,
                            Err(ErrorCode::CANCEL),
                            hil::uart::Error::None,
                        );
                    });
                });
            } else {
                // In the normal case, we need to either pass call the callback
                // or do another read to get more bytes.

                // Update how many bytes we still need to receive and
                // where we are storing in the buffer.
                self.rx_remaining_bytes
                    .set(self.rx_remaining_bytes.get().saturating_sub(rx_bytes));
                self.offset.set(self.offset.get() + rx_bytes);

                let rem = self.rx_remaining_bytes.get();
                if rem == 0 {
                    // Signal client that the read is done
                    self.rx_client.map(|client| {
                        self.rx_buffer.take().map(|rx_buffer| {
                            client.received_buffer(
                                rx_buffer,
                                self.offset.get(),
                                Ok(()),
                                uart::Error::None,
                            );
                        });
                    });
                } else {
                    // Setup how much we can read. We already made sure that
                    // this will fit in the buffer.
                    let to_read = core::cmp::min(rem, 255);
                    self.registers
                        .rxd_maxcnt
                        .write(Counter::COUNTER.val(to_read as u32));

                    // Actually do the receive.
                    self.set_rx_dma_pointer_to_buffer();
                    self.registers.task_startrx.write(Task::ENABLE::SET);
                    self.enable_rx_interrupts();
                }
            }
        }
        */
    }

    /// Transmit one byte at the time
    pub unsafe fn send_byte(&self, byte: u8) {
        self.uart.dr.write(|w| unsafe { w.data().bits(byte) })
    }

    // Pulls a byte out of the RX FIFO.
    #[inline]
    fn read(&self) -> u8 {
        self.uart.dr.read().data().bits()
    }

    /// Check if the UART transmission is done
    fn tx_fifo_empty(&self) -> bool {
        self.uart.fr.read().txfe().bit_is_set()
    }

    // Check if no more bytes can be enqueued in TX FIFO
    fn tx_fifo_full(&self) -> bool {
        self.uart.fr.read().txff().bit_is_set()
    }

    /// Check if either the rx_buffer is full or the UART has timed out
    fn rx_ready(&self) -> bool {
        self.uart.fr.read().rxff().bit_is_clear()
    }

    // fn set_rx_dma_pointer_to_buffer(&self) {
    //     self.rx_transaction.map(|rx| {
    //         self.registers
    //             .rxd_ptr
    //             .set(rx.buffer[self.index..].as_ptr() as u32);
    //     });
    // }

    // Helper function used by both transmit_word and transmit_buffer
    fn setup_buffer_transmit(&self, buf: &'static mut [u8], tx_len: usize) {
        let first_transfer_len = usize::min(tx_len, driverlib::UDMA_XFER_SIZE_MAX as usize);

        self.udma.uart_transfer_tx(&buf[..first_transfer_len]);
        self.dma_start_tx();

        let tx = Transaction {
            buffer: buf,
            length: tx_len,
            index: first_transfer_len,
        };
        self.tx_transaction.put(tx);
    }
}

impl<'a> hil::uart::Transmit<'a> for UartFull<'a> {
    fn set_transmit_client(&self, client: &'a dyn hil::uart::TransmitClient) {
        self.tx_client.set(client);
    }

    fn transmit_buffer(
        &self,
        tx_data: &'static mut [u8],
        tx_len: usize,
    ) -> Result<(), (ErrorCode, &'static mut [u8])> {
        if tx_len == 0 || tx_len > tx_data.len() {
            Err((ErrorCode::SIZE, tx_data))
        } else if self.tx_transaction.is_some() {
            Err((ErrorCode::BUSY, tx_data))
        } else {
            self.setup_buffer_transmit(tx_data, tx_len);
            Ok(())
        }
    }

    fn transmit_word(&self, _data: u32) -> Result<(), ErrorCode> {
        Err(ErrorCode::FAIL)
    }

    fn transmit_abort(&self) -> Result<(), ErrorCode> {
        // Experimental:
        // because of DMA, this may (and probably will) return bigger
        // amount of data transferred than it really was.

        if let Some(Transaction { buffer, index, .. }) = self.tx_transaction.take() {
            self.dma_stop_tx();
            self.udma.uart_disable_tx();
            self.tx_client
                .map(|client| client.transmitted_buffer(buffer, index, Err(ErrorCode::CANCEL)));
            Err(ErrorCode::BUSY)
        } else {
            Ok(())
        }
    }
}

impl<'a> hil::uart::Configure for UartFull<'a> {
    fn configure(&self, params: hil::uart::Parameters) -> Result<(), ErrorCode> {
        // These could probably be implemented, but are currently ignored,
        // so throw an error.

        // TODO: implement missing CTS/RTS
        if params.stop_bits != hil::uart::StopBits::One {
            return Err(ErrorCode::NOSUPPORT);
        }
        if params.parity != hil::uart::Parity::None {
            return Err(ErrorCode::NOSUPPORT);
        }
        if params.hw_flow_control {
            return Err(ErrorCode::NOSUPPORT);
        }

        if params.baud_rate == 0 {
            return Err(ErrorCode::INVAL);
        }
        self.set_baud_rate(params.baud_rate);

        Ok(())
    }
}

impl<'a> hil::uart::Receive<'a> for UartFull<'a> {
    fn set_receive_client(&self, client: &'a dyn hil::uart::ReceiveClient) {
        self.rx_client.set(client);
    }

    /* fn receive_buffer(
        &self,
        rx_buf: &'static mut [u8],
        rx_len: usize,
    ) -> Result<(), (ErrorCode, &'static mut [u8])> {
        if self.rx_buffer.is_some() {
            return Err((ErrorCode::BUSY, rx_buf));
        }
        // truncate rx_len if necessary
        let truncated_length = core::cmp::min(rx_len, rx_buf.len());

        self.rx_remaining_bytes.set(truncated_length);
        self.offset.set(0);
        self.rx_buffer.replace(rx_buf);
        self.set_rx_dma_pointer_to_buffer();

        let truncated_uart_max_length = core::cmp::min(truncated_length, 255);

        self.registers
            .rxd_maxcnt
            .write(Counter::COUNTER.val(truncated_uart_max_length as u32));
        self.registers.task_stoprx.write(Task::ENABLE::SET);
        self.registers.task_startrx.write(Task::ENABLE::SET);

        self.enable_rx_interrupts();
        Ok(())
    } */

    fn receive_word(&self) -> Result<(), ErrorCode> {
        Err(ErrorCode::FAIL)
    }

    fn receive_buffer(
        &self,
        rx_buffer: &'static mut [u8],
        rx_len: usize,
    ) -> Result<(), (ErrorCode, &'static mut [u8])> {
        Err((ErrorCode::NOSUPPORT, rx_buffer))
    }

    fn receive_abort(&self) -> Result<(), ErrorCode> {
        Err(ErrorCode::NOSUPPORT)
    }

    /* fn receive_abort(&self) -> Result<(), ErrorCode> {
        // Trigger the STOPRX event to cancel the current receive call.
        if self.rx_buffer.is_none() {
            Ok(())
        } else {
            self.rx_abort_in_progress.set(true);
            self.registers.task_stoprx.write(Task::ENABLE::SET);
            Err(ErrorCode::BUSY)
        }
    } */
}
