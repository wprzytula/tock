use core::fmt::{self, Write as _};
#[cfg(not(test))]
use core::panic::PanicInfo;

use cc2650_chip::uart::{PanicWriterFull, PanicWriterLite};
use kernel::debug::IoWrite;

pub(crate) const LED_PANIC_PIN: u32 = 20;

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
    PanicWriterLite.write_fmt(args).unwrap();
}

struct PanicWriterLiteAndFull;
impl IoWrite for PanicWriterLiteAndFull {
    fn write(&mut self, buf: &[u8]) -> usize {
        PanicWriterFull.write(buf);
        PanicWriterLite.write(buf);
        buf.len()
    }
}

impl fmt::Write for PanicWriterLiteAndFull {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write(s.as_bytes());
        Ok(())
    }
}

#[cfg(not(test))]
#[no_mangle]
#[inline(never)]
#[panic_handler]
/// Panic handler
pub unsafe fn panic_fmt(pi: &PanicInfo) -> ! {
    use core::ptr::addr_of;

    use cc2650_chip::gpio::PORT;
    use kernel::debug;
    use ti_cc2650_common::{CHIP, PROCESSES, PROCESS_PRINTER};

    let led_kernel_pin = &PORT[LED_PANIC_PIN];
    let led = &mut kernel::hil::led::LedHigh::new(led_kernel_pin);
    let writer = &mut PanicWriterLiteAndFull;

    debug::panic(
        &mut [led],
        writer,
        pi,
        &cortexm3::support::nop,
        &*addr_of!(PROCESSES),
        &*addr_of!(CHIP),
        &*addr_of!(PROCESS_PRINTER),
    )
}
