use core::fmt;

mod internals {
    use core::ops::Deref;

    pub(super) struct UartFull(*const cc2650::uart0::RegisterBlock);
    unsafe impl Send for UartFull {}
    unsafe impl Sync for UartFull {}

    // taken straight from cc2650 crate
    const UART_REGISTER_BLOCK_ADDR: usize = 0x40001000;
    pub(super) static UART: UartFull = UartFull(UART_REGISTER_BLOCK_ADDR as *const _);

    impl Deref for UartFull {
        type Target = cc2650::uart0::RegisterBlock;

        fn deref(&self) -> &Self::Target {
            unsafe { &*self.0 }
        }
    }
}
use internals::UART;

struct PanicWriter;

impl PanicWriter {
    // Best-effort turn off other users of UART to prevent colisions
    // when printing panic message.
    fn capture_uart(&mut self) {
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
    unsafe fn write_byte(&mut self, byte: u8) {
        while UART.fr.read().txff().bit_is_set() {
            // Wait until send queue is nonfull
        }
        UART.dr.write(|w| unsafe { w.data().bits(byte) })
    }
}

impl fmt::Write for PanicWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            unsafe { self.write_byte(byte) };
        }
        Ok(())
    }
}

impl kernel::debug::IoWrite for PanicWriter {
    fn write(&mut self, buf: &[u8]) -> usize {
        for byte in buf.iter().copied() {
            unsafe { self.write_byte(byte) };
        }
        buf.len()
    }
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::io::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    PanicWriter.write_fmt(args).unwrap();
}

#[cfg(not(test))]
use core::panic::PanicInfo;

#[cfg(not(test))]
#[no_mangle]
#[inline(never)]
#[panic_handler]
/// Panic handler
pub unsafe fn panic_fmt(pi: &PanicInfo) -> ! {
    use core::ptr::addr_of;

    use crate::{CHIP, PROCESSES, PROCESS_PRINTER};
    use cc2650_chip::gpio::PORT;
    use kernel::debug;

    let led_kernel_pin = &PORT[25];
    let led = &mut kernel::hil::led::LedHigh::new(led_kernel_pin);
    let writer = &mut PanicWriter;

    writer.capture_uart();
    debug::panic(
        &mut [led],
        writer,
        pi,
        &cortexm3::support::nop,
        &*addr_of!(PROCESSES),
        &*addr_of!(CHIP),
        &*addr_of!(PROCESS_PRINTER), // &None::<&'static kernel::process::ProcessPrinterText>,
    )
}
