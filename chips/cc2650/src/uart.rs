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

mod full {
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

    use super::Transaction;

    // 48 MHz
    const CLOCK_FREQ: u32 = 48_000_000;
    pub const BAUD_RATE: u32 = 115_200;

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

        fn set_hw_flow_control(&self, on: bool) {
            self.uart
                .ctl
                .modify(|_r, w| w.ctsen().bit(on).rtsen().bit(on))
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
                .modify(|_r, w| w.uarten().en().txe().en().rxe().en());
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

        fn dma_start_rx(&self) {
            self.uart.dmactl.modify(|_r, w| w.rxdmae().set_bit());
        }

        fn dma_stop_tx(&self) {
            self.udma.uart_disable_tx();
            self.uart.dmactl.modify(|_r, w| w.txdmae().clear_bit());
        }

        fn dma_stop_rx(&self) {
            self.udma.uart_disable_rx();
            self.uart.dmactl.modify(|_r, w| w.rxdmae().clear_bit());
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
            let rx_completed = self.udma.uart_request_done_rx();
            if rx_completed {
                // kernel::debug!("RX DMA transfer has completed");
                self.udma.uart_request_done_rx_clear()
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
                kernel::debug!("OERIS set");
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
            if tx_completed && !self.udma.uart_is_enabled_tx() {
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

            // RX transfer finished
            if rx_completed && !self.udma.uart_is_enabled_rx() {
                self.rx_transaction.take().map(
                    |Transaction {
                         buffer,
                         length,
                         index,
                     }| {
                        let remaining_len = length - index;
                        if remaining_len == 0 {
                            // Transaction has completed.
                            self.dma_stop_rx();
                            self.rx_client.map(move |client| {
                                // kernel::debug!("Notifying RX client");
                                client.received_buffer(
                                    buffer,
                                    length,
                                    Ok(()),
                                    kernel::hil::uart::Error::None,
                                );
                            });
                        } else {
                            // There are more receptions to be done for this request,
                            // due to the upper bound on uDMA transfer size (1024).
                            let next_transfer_len =
                                usize::min(remaining_len, driverlib::UDMA_XFER_SIZE_MAX as usize);

                            self.udma
                                .uart_transfer_rx(&mut buffer[index..index + next_transfer_len]);

                            self.rx_transaction.put(Transaction {
                                buffer,
                                length,
                                index: index + next_transfer_len,
                            });
                        }
                    },
                );
            }
        }

        /// Transmit one byte at the time
        pub unsafe fn send_byte(&self, byte: u8) {
            self.uart.dr.write(|w| unsafe { w.data().bits(byte) })
        }

        // Pulls a byte out of the RX FIFO.
        #[inline]
        unsafe fn read(&self) -> u8 {
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

        // FIXME: transmit_word unused
        // Helper function used by both transmit_word and transmit_buffer
        fn setup_buffer_transmit(&self, buf: &'static mut [u8], tx_len: usize) {
            // truncate tx_len if necessary
            let truncated_length = core::cmp::min(tx_len, buf.len());

            let first_transfer_len =
                usize::min(truncated_length, driverlib::UDMA_XFER_SIZE_MAX as usize);

            self.udma.uart_transfer_tx(&buf[..first_transfer_len]);
            self.dma_start_tx();

            let tx = Transaction {
                buffer: buf,
                length: truncated_length,
                index: first_transfer_len,
            };
            self.tx_transaction.put(tx);
        }

        // FIXME: receive_word unused
        // Helper function used by both receive_word and receive_buffer
        fn setup_buffer_receive(&self, rx_buf: &'static mut [u8], rx_len: usize) {
            // truncate rx_len if necessary
            let truncated_length = core::cmp::min(rx_len, rx_buf.len());

            let first_transfer_len =
                usize::min(truncated_length, driverlib::UDMA_XFER_SIZE_MAX as usize);

            self.udma
                .uart_transfer_rx(&mut rx_buf[..first_transfer_len]);
            self.dma_start_rx();

            let rx = Transaction {
                buffer: rx_buf,
                length: truncated_length,
                index: first_transfer_len,
            };
            self.rx_transaction.put(rx);
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

            if params.stop_bits != hil::uart::StopBits::One {
                return Err(ErrorCode::NOSUPPORT);
            }
            if params.parity != hil::uart::Parity::None {
                return Err(ErrorCode::NOSUPPORT);
            }

            self.set_hw_flow_control(params.hw_flow_control);

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

        fn receive_buffer(
            &self,
            rx_buf: &'static mut [u8],
            rx_len: usize,
        ) -> Result<(), (ErrorCode, &'static mut [u8])> {
            if rx_len == 0 || rx_len > rx_buf.len() {
                Err((ErrorCode::SIZE, rx_buf))
            } else if self.rx_transaction.is_some() {
                Err((ErrorCode::BUSY, rx_buf))
            } else {
                self.setup_buffer_receive(rx_buf, rx_len);
                Ok(())
            }
        }

        fn receive_word(&self) -> Result<(), ErrorCode> {
            Err(ErrorCode::FAIL)
        }

        fn receive_abort(&self) -> Result<(), ErrorCode> {
            // Experimental:
            // because of DMA, this may (and probably will) return bigger
            // amount of data received than it really was.
            if let Some(Transaction { buffer, index, .. }) = self.rx_transaction.take() {
                self.dma_stop_rx();
                self.udma.uart_disable_rx();
                self.rx_client.map(|client| {
                    client.received_buffer(
                        buffer,
                        index,
                        Err(ErrorCode::CANCEL),
                        kernel::hil::uart::Error::Aborted,
                    )
                });
                Err(ErrorCode::BUSY)
            } else {
                Ok(())
            }
        }
    }

    mod panic_writer {
        use core::{fmt, ops::Deref};
        use kernel::debug::IoWrite;

        struct UartFull(*const cc2650::uart0::RegisterBlock);
        unsafe impl Send for UartFull {}
        unsafe impl Sync for UartFull {}

        // taken straight from cc2650 crate
        const UART_REGISTER_BLOCK_ADDR: usize = 0x40001000;
        const UART: UartFull = UartFull(UART_REGISTER_BLOCK_ADDR as *const _);

        impl Deref for UartFull {
            type Target = cc2650::uart0::RegisterBlock;

            fn deref(&self) -> &Self::Target {
                unsafe { &*self.0 }
            }
        }

        pub struct PanicWriter;

        impl PanicWriter {
            // Best-effort turn off other users of UART to prevent colisions
            // when printing panic message.
            pub fn capture_uart(&mut self) {
                UART.dmactl.write(|w| {
                    w.rxdmae()
                        .clear_bit()
                        .txdmae()
                        .clear_bit()
                        .dmaonerr()
                        .clear_bit()
                })
            }

            // SAFETY: make sure that other users of UART were turned off
            // and prevented further interaction.
            pub unsafe fn write_byte(&mut self, byte: u8) {
                while UART.fr.read().txff().bit_is_set() {
                    // Wait until send queue is nonfull
                }
                UART.dr.write(|w| unsafe { w.data().bits(byte) })
            }
        }

        impl fmt::Write for PanicWriter {
            fn write_str(&mut self, s: &str) -> fmt::Result {
                self.write(s.as_bytes());
                Ok(())
            }
        }

        impl IoWrite for PanicWriter {
            fn write(&mut self, buf: &[u8]) -> usize {
                for byte in buf.iter().copied() {
                    unsafe { self.write_byte(byte) };
                }
                buf.len()
            }
        }
    }
    pub use panic_writer::PanicWriter;
}
use core::fmt;

pub use full::{PanicWriter as PanicWriterFull, UartFull, BAUD_RATE};

#[cfg(feature = "uart_lite")]
pub mod lite {
    use core::{
        fmt::{self, Write as _},
        sync::atomic::{AtomicUsize, Ordering},
    };

    use kernel::{
        hil::{
            self,
            uart::{Configure, Receive, Transmit},
        },
        ErrorCode,
    };
    use tock_cells::{optional_cell::OptionalCell, volatile_cell::VolatileCell};

    use crate::{
        driverlib,
        scif::{
            safe_packed_ref, SCIFData, SCIFIntData, SCIFTaskCtrl, SCIFTaskStructType, Scif,
            AUXIOMODE_INPUT, AUXIOMODE_OUTPUT,
        },
    };

    /// Maximum number of characters that can be stored in the UART TX FIFO
    const SCIF_UART_TX_BUFFER_LEN: usize = 768;
    const SCIF_UART_TX_FIFO_MAX_COUNT: u32 = (SCIF_UART_TX_BUFFER_LEN - 1) as u32;

    /// UART Emulator I/O mapping: UART RX
    #[allow(unused)]
    const SCIF_UART_EMULATOR_DIO_UART_RX: u32 = 29;
    /// UART Emulator I/O mapping: UART TX
    #[allow(unused)]
    const SCIF_UART_EMULATOR_DIO_UART_TX: u32 = 28;

    const SCIF_UART_EMULATOR_TASK_ID: u32 = 0;

    const SCIF_UART_BAUD_RATE: u32 = 230400;
    const LOST_BUFFER_SIZE: usize = 16;
    const SC_UART_FREE_THRESHOLD: usize = (2 * SCIF_UART_TX_FIFO_MAX_COUNT / 4) as usize;

    // All shared data structures in AUX RAM need to be packed

    type TxBuffer = [VolatileCell<u16>; SCIF_UART_TX_BUFFER_LEN];

    /// UART Emulator: Task input data structure
    #[repr(packed)]
    struct SCIFUartEmulatorInput {
        /// TX FIFO ring buffer
        tx_buffer: TxBuffer,
    }

    /// UART Emulator: Task state structure
    #[repr(packed)]
    struct SCIFUartEmulatorState {
        /// TX FIFO head index (updated by the application)
        tx_head: VolatileCell<u16>,
        /// TX FIFO tail index (updated by the Sensor Controller)
        tx_tail: VolatileCell<u16>,
    }

    /// Sensor Controller task data (configuration, input buffer(s), output buffer(s) and internal state)
    #[repr(packed)]
    struct SCIFTaskData {
        uart_emulator: UARTEmulator,
    }

    #[repr(packed)]
    struct UARTEmulator {
        input: SCIFUartEmulatorInput,
        state: SCIFUartEmulatorState,
    }

    /// Sensor Controller task generic control (located in AUX RAM)
    #[allow(non_snake_case)] // because this is effectively a constant
    const fn SCIF_TASK_DATA() -> &'static SCIFTaskData {
        unsafe { core::mem::transmute(0x400E00E6 as *mut SCIFTaskData) }
    }

    // const SCIF_TASK_DATA: &'static SCIFTaskData =
    //     unsafe { core::mem::transmute(0x400E00E6 as *mut SCIFTaskData) };

    // Initialized internal driver data, to be used in the call to \ref scifInit()
    // extern const SCIF_DATA_T scifDriverSetup;

    // UART TX FIFO access functions
    // uint32_t scifUartGetTxFifoCount(void);
    // void scifUartTxPutTwoChars(char c1, char c2);
    // void scifUartTxPutChar(char c);
    // void scifUartTxPutChars(char* pBuffer, uint32_t count);

    /// Firmware image to be uploaded to the AUX RAM
    #[rustfmt::skip]
    static AUX_RAM_IMAGE: [u16; 941] = [
    /*0x0000*/ 0x1408, 0x040C, 0x1408, 0x042C, 0x1408, 0x0447, 0x1408, 0x044D, 0x4436, 0x2437, 0xAEFE, 0xADB7, 0x6442, 0x7000, 0x7C6B, 0x6870, 
    /*0x0020*/ 0x0068, 0x1425, 0x6871, 0x0069, 0x1425, 0x6872, 0x006A, 0x1425, 0x786B, 0xF801, 0xFA01, 0xBEF2, 0x786E, 0x6870, 0xFD0E, 0x6872, 
    /*0x0040*/ 0xED92, 0xFD06, 0x7C6E, 0x642D, 0x0451, 0x786B, 0x8F1F, 0xED8F, 0xEC01, 0xBE01, 0xADB7, 0x8DB7, 0x6630, 0x6542, 0x0000, 0x186E, 
    /*0x0060*/ 0x9D88, 0x9C01, 0xB60D, 0x1067, 0xAF19, 0xAA00, 0xB609, 0xA8FF, 0xAF39, 0xBE06, 0x0C6B, 0x8869, 0x8F08, 0xFD47, 0x9DB7, 0x086B, 
    /*0x0080*/ 0x8801, 0x8A01, 0xBEEC, 0x262F, 0xAEFE, 0x4630, 0x0451, 0x5527, 0x6642, 0x0000, 0x0C6B, 0x140B, 0x0451, 0x6742, 0x86FF, 0x03FF, 
    /*0x00A0*/ 0x0C6D, 0x786C, 0xCD47, 0x686D, 0xCD06, 0xB605, 0x0000, 0x0C6C, 0x7C6F, 0x652D, 0x0C6D, 0x786D, 0xF801, 0xE92B, 0xFD0E, 0xBE01, 
    /*0x00C0*/ 0x6436, 0xBDB7, 0x241A, 0xA6FE, 0xADB7, 0x641A, 0xADB7, 0x0000, 0x0375, 0x0376, 0x0378, 0x0000, 0x0000, 0xFFFF, 0x0000, 0x0000, 
    /*0x00E0*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x0100*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x0120*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x0140*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x0160*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x0180*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x01A0*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x01C0*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x01E0*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x0200*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x0220*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x0240*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x0260*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x0280*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x02A0*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x02C0*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x02E0*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x0300*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x0320*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x0340*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x0360*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x0380*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x03A0*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x03C0*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x03E0*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x0400*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x0420*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x0440*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x0460*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x0480*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x04A0*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x04C0*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x04E0*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x0500*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x0520*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x0540*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x0560*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x0580*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x05A0*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x05C0*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x05E0*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x0600*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x0620*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x0640*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x0660*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x0680*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x06A0*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x06C0*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 
    /*0x06E0*/ 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0xADB7, 0x1779, 0xADB7, 0xADB7, 0x8603, 0x0380, 0x0FAC, 0x6000, 0x0BAC, 0xCDB1, 0x8DB7, 
    /*0x0700*/ 0xED36, 0xBE06, 0x2B73, 0x1B74, 0x9D2A, 0xB605, 0x9873, 0xEF09, 0x8603, 0x138C, 0x1FAC, 0x077D, 0x460E, 0x7008, 0x8603, 0x1392, 
    /*0x0720*/ 0x1FAC, 0x077D, 0xEC01, 0xBE02, 0x460E, 0x8E01, 0x660E, 0xEDA9, 0xF8FF, 0xBE03, 0x8603, 0x139E, 0x1FAC, 0x077D, 0x8603, 0x1380, 
    /*0x0740*/ 0x1FAC, 0x660E, 0xED36, 0xBED9, 0x1B74, 0x9801, 0x8603, 0x9A00, 0xAE01, 0x1000, 0x1F74, 0x077D, 0x0000
    ];

    /// Look-up table that converts from AUX I/O index to MCU IOCFG offset
    static AUX_TO_INDEX_TO_MCU_IOCFG_OFFSET_LUT: [u8; 0x10] = [
        120, 116, 112, 108, 104, 100, 96, 92, 28, 24, 20, 16, 12, 8, 4, 0,
    ];

    /** \brief Look-up table of data structure information for each task
     *
     * There is one entry per data structure (\c cfg, \c input, \c output and \c state) per task:
     * - [31:20] Data structure size (number of 16-bit words)
     * - [19:12] Buffer count (when 2+, first data structure is preceded by buffering control variables)
     * - [11:0] Address of the first data structure
     */
    static SCIF_TASK_DATA_STRUCT_INFO_LUT: [u32; 0x4] = [
        //  cfg         input       output      state
        0x00000000, 0x300010E6, 0x00000000, 0x002016E6, // UART Emulator
    ];

    // No task-specific initialization functions

    // No task-specific uninitialization functions

    /** \brief Initilializes task resource hardware dependencies
     *
     * This function is called by the internal driver initialization function, \ref scifInit().
     */
    unsafe fn scif_task_resource_init(scif: &Scif) {
        scif.scif_init_io(2, AUXIOMODE_OUTPUT, 1, 1);
        scif.scif_init_io(1, AUXIOMODE_INPUT, 1, 0);
    } // scifTaskResourceInit

    /** \brief Uninitilializes task resource hardware dependencies
     *
     * This function is called by the internal driver uninitialization function, \ref scifUninit().
     */
    #[cfg(feature = "full_scif")]
    unsafe fn scif_task_resource_uninit(scif: &Scif) {
        scif.scif_uninit_io(2, 1);
        scif.scif_uninit_io(1, 1);
    } // scifTaskResourceUninit

    impl Scif {
        /** \brief Sets the UART baud rate
         *
         * This function must be called to start baud rate generation before or after starting the UART
         * emulation task. This function can be called during operation to change the baud rate on-the-fly.
         *
         * \param[in]      baudRate
         *     The new baud rate (e.g. 115200 or 9600). 0 disables baud rate generation.
         */
        pub(crate) fn uart_set_baud_rate(&self, baud_rate: u32) {
            // Start baud rate generation?
            if baud_rate > 0 {
                // Calculate the AUX timer 0 period
                // t0Period = 24000000 / baudRate;
                let mut t0_period = 24000000 / baud_rate;

                // The period must be 256 clock cycles or less, so up the prescaler until it is
                // t0PrescalerExp = 0;
                let mut t0_prescaler_exp = 0;
                // while (t0Period > 256) {
                while t0_period > 256 {
                    // t0PrescalerExp += 1;
                    t0_prescaler_exp += 1;
                    // t0Period >>= 1;
                    t0_period >>= 1;
                }

                // Stop baud rate generation while reconfiguring
                // HWREG(AUX_TIMER_BASE + AUX_TIMER_O_T0CTL) = 0;
                self.aux_timer.t0ctl.write(|w| w.en().clear_bit());

                // Set period and prescaler, and select reload mode
                // HWREG(AUX_TIMER_BASE + AUX_TIMER_O_T0CFG) = (t0PrescalerExp << AUX_TIMER_T0CFG_PRE_S) | AUX_TIMER_T0CFG_RELOAD_M;
                self.aux_timer
                    .t0cfg
                    .write(|w| unsafe { w.pre().bits(t0_prescaler_exp).reload().set_bit() });
                // HWREG(AUX_TIMER_BASE + AUX_TIMER_O_T0TARGET) = t0Period - 1;
                self.aux_timer
                    .t0target
                    .write(|w| unsafe { w.value().bits(t0_period as u16 - 1) });

                // Start baud rate generation
                // HWREG(AUX_TIMER_BASE + AUX_TIMER_O_T0CTL) = 1;
                self.aux_timer.t0ctl.write(|w| w.en().set_bit());

            // Baud rate 0 -> stop baud rate generation
            } else {
                // HWREG(AUX_TIMER_BASE + AUX_TIMER_O_T0CTL) = 0;
                self.aux_timer.t0ctl.write(|w| w.en().clear_bit());
            }
        }

        /** \brief Re-initializes I/O pins used by the specified tasks
         *
         * It is possible to stop a Sensor Controller task and let the System CPU borrow and operate its I/O
         * pins. For example, the Sensor Controller can operate an SPI interface in one application state while
         * the System CPU with SSI operates the SPI interface in another application state.
         *
         * This function must be called before \ref scifExecuteTasksOnceNbl() or \ref scifStartTasksNbl() if
         * I/O pins belonging to Sensor Controller tasks have been borrowed System CPU peripherals.
         *
         * \param[in]      bvTaskIds
         *     Bit-vector of task IDs for the task I/Os to be re-initialized
         */
        #[allow(unused)]
        unsafe fn scif_reinit_task_io(&self, bv_task_ids: u32) {
            if bv_task_ids & (1 << SCIF_UART_EMULATOR_TASK_ID) != 0 {
                self.scif_reinit_io(2, 1);
                self.scif_reinit_io(1, 1);
            }
        } // scifReinitTaskIo
    }

    pub struct UartLite<'a> {
        scif: Scif,
        tx_client: OptionalCell<&'a dyn hil::uart::TransmitClient>,
    }

    impl<'a> UartLite<'a> {
        pub(crate) fn new(
            aon_rtc: cc2650::AON_RTC,
            aon_wuc: cc2650::AON_WUC,
            aux_aiodio0: cc2650::AUX_AIODIO0,
            aux_aiodio1: cc2650::AUX_AIODIO1,
            aux_evctl: cc2650::AUX_EVCTL,
            aux_sce: cc2650::AUX_SCE,
            aux_timer: cc2650::AUX_TIMER,
            aux_wuc: cc2650::AUX_WUC,
        ) -> Self {
            let scif = Scif::new(
                aon_rtc,
                aon_wuc,
                aux_aiodio0,
                aux_aiodio1,
                aux_evctl,
                aux_sce,
                aux_timer,
                aux_wuc,
            );

            Self {
                scif,
                tx_client: OptionalCell::empty(),
            }
        }

        /// Driver setup data, to be used in the call to \ref scifInit()
        fn scif_driver_data() -> SCIFData {
            SCIFData {
                int_data: unsafe { core::mem::transmute(0x400E00D6 as *mut SCIFIntData) },
                task_ctrl: unsafe { core::mem::transmute(0x400E00DC as *mut SCIFTaskCtrl) },
                #[cfg(feature = "full_scif")]
                task_execute_schedule: 0x400E00CE as *mut u16,
                bv_dirty_tasks: 0x0000,
                aux_ram_image: &AUX_RAM_IMAGE,
                task_data_struct_info_lut: &SCIF_TASK_DATA_STRUCT_INFO_LUT,
                aux_io_index_to_mcu_iocfg_offset_lut: &AUX_TO_INDEX_TO_MCU_IOCFG_OFFSET_LUT,
                fptr_task_resource_init: scif_task_resource_init,
                #[cfg(feature = "full_scif")]
                fptr_task_resource_uninit: scif_task_resource_uninit,
            }
        }

        pub(crate) fn initialize(&self) {
            unsafe {
                driverlib::AONWUCAuxWakeupEvent(driverlib::AONWUC_AUX_WAKEUP);
                while driverlib::AONWUCPowerStatusGet() & driverlib::AONWUC_AUX_POWER_ON == 0 {}
                let clocks = driverlib::AUX_WUC_SMPH_CLOCK;
                aux_ctrl_register_consumer(clocks);
                driverlib::AONWUCMcuPowerDownConfig(driverlib::AONWUC_CLOCK_SRC_LF);
                driverlib::AONWUCAuxPowerDownConfig(driverlib::AONWUC_CLOCK_SRC_LF);

                self.scif.scif_init(Self::scif_driver_data()).unwrap();
                self.scif.scif_reset_task_structs(
                    1 << SCIF_UART_EMULATOR_TASK_ID,
                    (1 << SCIFTaskStructType::SCIFStructCfg as u32)
                        | (1 << SCIFTaskStructType::SCIFStructInput as u32)
                        | (1 << SCIFTaskStructType::SCIFStructOutput as u32),
                );
                self.scif
                    .scif_execute_tasks_once_nbl(1 << SCIF_UART_EMULATOR_TASK_ID)
                    .unwrap();

                self.scif.uart_set_baud_rate(SCIF_UART_BAUD_RATE);
            }
        }
    }

    unsafe fn aux_ctrl_register_consumer(clocks: u32) {
        let interrupts_were_disabled = driverlib::IntMasterDisable();

        driverlib::AONWUCAuxWakeupEvent(driverlib::AONWUC_AUX_WAKEUP);
        while driverlib::AONWUCPowerStatusGet() & driverlib::AONWUC_AUX_POWER_ON == 0 {}

        driverlib::AUXWUCClockEnable(clocks);
        while driverlib::AUXWUCClockStatus(clocks) != driverlib::AUX_WUC_CLOCK_READY {}

        if !interrupts_were_disabled {
            driverlib::IntMasterEnable();
        }
    }

    /** \brief Calculates the number of cells currently used in the TX FIFO
     *
     * The TX FIFO is divided into 2-byte cells and only a whole cell can be used.
     *
     * The count is decremented when all characters from the current cell (stop bits of the last one) are transmitted.
     *
     * \return
     *     The number of cells used in the TX FIFO, waiting to be transmitted
     */
    fn scif_uart_get_tx_fifo_count() -> u16 {
        unsafe {
            let state = safe_packed_ref!(SCIF_TASK_DATA().uart_emulator.state);
            let mut tx_head = safe_packed_ref!(state.tx_head).get();
            let tx_tail = safe_packed_ref!(state.tx_tail).get();
            if tx_head < tx_tail {
                tx_head += SCIF_UART_TX_BUFFER_LEN as u16;
            }
            tx_head - tx_tail
        }
    } // scifUartGetTxFifoCount

    fn scif_uart_get_tx_fifo_free_slots() -> u16 {
        SCIF_UART_TX_FIFO_MAX_COUNT as u16 - scif_uart_get_tx_fifo_count()
    } // scifUartGetTxFifoCount

    /** \brief Transmits two characters
     *
     * This function must not be called when the TX FIFO is full. Both characters use only one TX FIFO cell.
     * Calling this function when the FIFO is full will cause overflow, without warning. The number of free cells in the FIFO is:
     * \code
     * SCIF_UART_TX_FIFO_MAX_COUNT - scifUartGetTxFifoCount()
     * \endcode
     *
     * \param[in]      c1
     *     The first character to transmit
     * \param[in]      c2
     *     The second character to transmit
     */
    unsafe fn scif_uart_tx_put_two_chars(c1: u8, c2: u8) {
        // Put the character
        let mut tx_head = safe_packed_ref!(SCIF_TASK_DATA().uart_emulator.state.tx_head).get();
        let entry = (c2 as u16) << 8 | c1 as u16;
        safe_packed_ref!(SCIF_TASK_DATA().uart_emulator.input.tx_buffer)[tx_head as usize]
            .set(entry);

        // Update the TX FIFO head index
        tx_head += 1;
        if tx_head == (core::mem::size_of::<TxBuffer>() / core::mem::size_of::<u16>()) as u16 {
            tx_head = 0;
        }
        safe_packed_ref!(SCIF_TASK_DATA().uart_emulator.state.tx_head).set(tx_head);
    } // scifUartTxPutTwoChars

    /** \brief Transmits one character
     *
     * This function must not be called when the TX FIFO is full.
     * Calling this function when the FIFO is full will cause overflow, without warning. The number of free cells in the FIFO is:
     * \code
     * SCIF_UART_TX_FIFO_MAX_COUNT - scifUartGetTxFifoCount()
     * \endcode
     *
     * \param[in]      c
     *     The character to transmit
     */
    unsafe fn scif_uart_tx_put_char(c: u8) {
        scif_uart_tx_put_two_chars(c, 0);
    } // scifUartTxPutChar

    /** \brief Transmits the specified number of character
     *
     * This function must not be called with count higher than the number of free entries in the TX FIFO.
     * Calling this function with too high count will cause overflow, without warning. The number of free
     * entries in the FIFO is:
     * \code
     * SCIF_UART_TX_FIFO_MAX_COUNT - scifUartGetTxFifoCount()
     * \endcode
     *
     * \param[in,out]  *pBuffer
     *     Pointer to the character source buffer
     * \param[in]      count
     *     Number of characters to put
     */
    unsafe fn scif_uart_tx_put_chars(buff: &[u8], count: u32) {
        let mut entry: u16;

        // For each character ...
        let mut tx_head: u32 =
            safe_packed_ref!(SCIF_TASK_DATA().uart_emulator.state.tx_head).get() as u32;
        for n in (0..count as usize).step_by(2) {
            // Get it
            if n + 1 == count as usize {
                entry = buff[n] as u16;
            } else {
                entry = buff[n + 1] as u16;
                entry <<= 8;
                entry |= buff[n] as u16;
            }

            safe_packed_ref!(SCIF_TASK_DATA().uart_emulator.input.tx_buffer)[tx_head as usize]
                .set(entry);

            // Update the TX FIFO head index
            tx_head += 1;
            if tx_head == SCIF_UART_TX_BUFFER_LEN as u32 {
                tx_head = 0;
            }
        }
        safe_packed_ref!(SCIF_TASK_DATA().uart_emulator.state.tx_head).set(tx_head as u16);
    } // scifUartTxPutChars

    /// Intended use: panic writer in CherryMote.
    // NOTICE: the current implementation is blocking; i.e. it wait until there is enough
    // space in the cyclic buffer, so that the whole message is sent.
    unsafe fn transmit_blocking(message: &[u8]) {
        let tx_len = message.len();
        let mut idx = 0;
        while idx < tx_len {
            let remaining = tx_len - idx;
            let written;

            if remaining > 2 && 2 * scif_uart_get_tx_fifo_free_slots() as usize >= remaining {
                written = remaining;
                scif_uart_tx_put_chars(&message[idx..idx + written], written as u32);
            } else if remaining == 1 {
                written = 1;
                while 2 * scif_uart_get_tx_fifo_free_slots() < written as u16 {}
                scif_uart_tx_put_char(message[idx]);
            } else {
                // remaining >= 2
                written = 2;
                while 2 * scif_uart_get_tx_fifo_free_slots() < written as u16 {}
                scif_uart_tx_put_two_chars(message[idx], message[idx + 1])
            }

            idx += written;
        }
    }

    pub(super) fn transmit_lossy(s: &[u8]) -> Result<(), ErrorCode> {
        let mut len = s.len();

        static BYTES_LOST: AtomicUsize = AtomicUsize::new(0);
        let mut lost_buffer = [0_u8; LOST_BUFFER_SIZE];

        // Based on: https://stackoverflow.com/a/39491059
        struct LostBytesWriter<'a> {
            buf: &'a mut [u8],
            offset: usize,
        }

        impl<'a> LostBytesWriter<'a> {
            fn new(buf: &'a mut [u8]) -> Self {
                LostBytesWriter { buf, offset: 0 }
            }
        }

        impl<'a> fmt::Write for LostBytesWriter<'a> {
            fn write_str(&mut self, s: &str) -> fmt::Result {
                let bytes = s.as_bytes();

                // Skip over already-copied data
                let remainder = &mut self.buf[self.offset..];
                // Check if there is space remaining (return error instead of panicking)
                if remainder.len() < bytes.len() {
                    return Err(core::fmt::Error);
                }
                // Make the two slices the same length
                let remainder = &mut remainder[..bytes.len()];
                // Copy
                remainder.copy_from_slice(bytes);

                // Update offset to avoid overwriting
                self.offset += bytes.len();

                Ok(())
            }
        }

        let free_bytes = 2 * scif_uart_get_tx_fifo_free_slots() as usize;
        let bytes_lost = BYTES_LOST.load(Ordering::Relaxed);
        let mut i = 0;

        if bytes_lost > 0 {
            // We have already lost some bytes and haven't reported that yet.
            if free_bytes >= SC_UART_FREE_THRESHOLD {
                // We have enough space to report the past loss.
                // Let's try creating the loss message.

                let message_size = {
                    let lost_buffer = &mut lost_buffer;
                    let len_before = lost_buffer.len();
                    // Safety: number of bytes written won't ever exceed size of the buffer (16).
                    unsafe {
                        write!(LostBytesWriter::new(lost_buffer), "\nLOST:{}\n", bytes_lost)
                            .unwrap_unchecked()
                    };
                    let len_after = lost_buffer.len();

                    len + len_before - len_after
                };

                if free_bytes < message_size {
                    // If we can't fit both the LOST message and our new message, just continue
                    // accounting the loss.
                    BYTES_LOST.fetch_add(len, Ordering::Relaxed);
                    return Err(ErrorCode::BUSY);
                } else {
                    // Report the past loss, zeroing the loss counter.
                    unsafe {
                        scif_uart_tx_put_chars(&lost_buffer, message_size as u32);
                    }
                    BYTES_LOST.store(0, Ordering::Relaxed);
                }
            } else {
                // Not only did we lose bytes that we haven't reported yet,
                // but also we can't even report them yet. Too bad.
                BYTES_LOST.fetch_add(len, Ordering::Relaxed);
                return Err(ErrorCode::BUSY);
            }
        } else {
            // We didn't lose any bytes that we haven't yet reported.
            if free_bytes < len {
                // Some bytes can't fit the buffer; remember that they are lost.
                BYTES_LOST.fetch_add(len - free_bytes, Ordering::Relaxed);
                len = free_bytes;
            }
        }

        // Actually write the message (at least its prefix that fits).

        // Put pairs of bytes.
        while i + 1 < len
        /* && s[i] != b'\0' && s[i + 1] != b'\0' */
        {
            unsafe {
                scif_uart_tx_put_two_chars(s[i], s[i + 1]);
            }
            i += 2;
        }
        // Put the last, odd byte if present.
        if i < len
        /* && s[i] != b'\0' */
        {
            unsafe {
                scif_uart_tx_put_char(s[i]);
            }
            i += 1
        }
        assert_eq!(i, len);

        Ok(())
    }

    impl<'a> Transmit<'a> for UartLite<'a> {
        fn set_transmit_client(&self, client: &'a dyn kernel::hil::uart::TransmitClient) {
            self.tx_client.set(client)
        }

        // NOTICE: the current implementation is nonblocking; i.e. if there is not enough
        // space in the cyclic buffer, then the message is truncated. Eventually,
        // when there is again some space, an additional message about number of bytes lost
        // is transmitted.
        fn transmit_buffer(
            &self,
            tx_buffer: &'static mut [u8],
            tx_len: usize,
        ) -> Result<(), (ErrorCode, &'static mut [u8])> {
            if tx_len > tx_buffer.len() {
                return Err((ErrorCode::SIZE, tx_buffer));
            }

            match transmit_lossy(tx_buffer) {
                Ok(()) => {
                    self.tx_client
                        .map(|client| client.transmitted_buffer(tx_buffer, tx_len, Ok(())));
                    Ok(())
                }
                Err(err) => Err((err, tx_buffer)),
            }
        }

        fn transmit_word(&self, _word: u32) -> Result<(), ErrorCode> {
            Err(ErrorCode::FAIL)
        }

        fn transmit_abort(&self) -> Result<(), ErrorCode> {
            Ok(())
        }
    }

    // Minimal implementation, for UartLite to implement hil::uart::Uart.
    impl<'a> Configure for UartLite<'a> {
        fn configure(&self, params: hil::uart::Parameters) -> Result<(), ErrorCode> {
            match params {
                hil::uart::Parameters {
                    baud_rate: SCIF_UART_BAUD_RATE,
                    width: hil::uart::Width::Eight,
                    parity: hil::uart::Parity::None,
                    hw_flow_control: false,
                    stop_bits: hil::uart::StopBits::One,
                } => Ok(()),
                _ => Err(ErrorCode::INVAL),
            }
        }
    }

    // Mock implementation, for UartLite to implement hil::uart::Uart.
    impl<'a> Receive<'a> for UartLite<'a> {
        fn set_receive_client(&self, _client: &'a dyn hil::uart::ReceiveClient) {
            // noop
        }

        fn receive_buffer(
            &self,
            rx_buffer: &'static mut [u8],
            _rx_len: usize,
        ) -> Result<(), (ErrorCode, &'static mut [u8])> {
            Err((ErrorCode::NOSUPPORT, rx_buffer))
        }

        fn receive_word(&self) -> Result<(), ErrorCode> {
            Err(ErrorCode::NOSUPPORT)
        }

        fn receive_abort(&self) -> Result<(), ErrorCode> {
            Err(ErrorCode::NOSUPPORT)
        }
    }
    mod panic_writer {
        use core::fmt;

        use kernel::debug::IoWrite;

        pub struct PanicWriter;

        impl IoWrite for PanicWriter {
            fn write(&mut self, buf: &[u8]) -> usize {
                unsafe { super::transmit_blocking(buf) };
                buf.len()
            }
        }

        impl fmt::Write for PanicWriter {
            fn write_str(&mut self, s: &str) -> fmt::Result {
                self.write(s.as_bytes());
                Ok(())
            }
        }
    }
    pub use panic_writer::PanicWriter;
}
#[cfg(feature = "uart_lite")]
pub use lite::{PanicWriter as PanicWriterLite, UartLite};

use kernel::debug::IoWrite as _;
#[cfg(feature = "uart_lite")]

pub struct PanicWriterLiteAndFull;
impl PanicWriterLiteAndFull {
    pub fn capture_uart(&mut self) {
        PanicWriterFull.capture_uart();
    }
}

impl kernel::debug::IoWrite for PanicWriterLiteAndFull {
    fn write(&mut self, buf: &[u8]) -> usize {
        PanicWriterLite.write(buf);
        PanicWriterFull.write(buf);
        buf.len()
    }
}

impl fmt::Write for PanicWriterLiteAndFull {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write(s.as_bytes());
        Ok(())
    }
}
