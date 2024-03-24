use core::fmt;

use cc2650::Peripherals;

struct Writer;

impl Writer {
    fn write_byte(&mut self, byte: u8) {
        let uart = unsafe { Peripherals::steal().UART0 };
        while uart.fr.read().txfe().bit_is_clear() {
            // Wait until send queue is empty
        }
        uart.dr.write(|w| unsafe { w.data().bits(byte) })
    }
}

impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            self.write_byte(byte);
        }
        Ok(())
    }
}

impl kernel::debug::IoWrite for Writer {
    fn write(&mut self, buf: &[u8]) -> usize {
        for byte in buf.iter().copied() {
            self.write_byte(byte);
        }
        buf.len()
    }
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::io::print::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    Writer.write_fmt(args).unwrap();
}

impl kernel::hil::led::Led for Writer {
    fn init(&self) {
        ()
    }

    fn on(&self) {
        ()
    }

    fn off(&self) {
        ()
    }

    fn toggle(&self) {
        ()
    }

    fn read(&self) -> bool {
        true
    }
}

#[cfg(not(test))]
use core::panic::PanicInfo;

#[cfg(not(test))]
#[no_mangle]
#[inline(never)]
#[panic_handler]
/// Panic handler
pub unsafe fn panic_fmt(pi: &PanicInfo) -> ! {
    use crate::{CHIP, PROCESSES, PROCESS_PRINTER};
    use cc2650_chip::gpio::PORT;
    use kernel::debug;

    let led_kernel_pin = &PORT[25];
    let led = &mut kernel::hil::led::LedHigh::new(led_kernel_pin);
    let writer = &mut Writer;
    debug::panic(
        &mut [led],
        writer,
        pi,
        &cortexm3::support::nop,
        &PROCESSES,
        &CHIP,
        &PROCESS_PRINTER, // &None::<&'static kernel::process::ProcessPrinterText>,
    )
}
