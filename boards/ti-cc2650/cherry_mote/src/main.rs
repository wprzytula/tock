#![no_std]
#![cfg_attr(not(doc), no_main)]

use cc2650_chip::uart::UartPinConfig;
use kernel::{create_capability, hil::led::LedHigh, static_init};
use ti_cc2650_common::NUM_PROCS;

const LED_PIN_RED: u32 = io::LED_PANIC_PIN;

mod io;

#[derive(Clone, Copy)]
struct PinConfig;
impl UartPinConfig for PinConfig {
    fn tx() -> u32 {
        cc2650_chip::driverlib::IOID_3
    }

    fn rx() -> u32 {
        cc2650_chip::driverlib::IOID_2
    }

    fn rts() -> u32 {
        cc2650_chip::driverlib::IOID_8
    }

    fn cts() -> u32 {
        cc2650_chip::driverlib::IOID_4
    }
}

/// Main function called after RAM initialized.
#[no_mangle]
pub unsafe fn main() {
    let main_loop_capability = create_capability!(kernel::capabilities::MainLoopCapability);

    let red_led = static_init!(
        LedHigh<'static, cc2650_chip::gpio::GPIOPin>,
        LedHigh::new(&cc2650_chip::gpio::PORT[LED_PIN_RED])
    );

    let leds = static_init!(
        [&'static LedHigh<'static, cc2650_chip::gpio::GPIOPin>; 1],
        [red_led]
    );

    let (board_kernel, smartrf, chip) = ti_cc2650_common::start(PinConfig, leds);

    println!("Hello world from board with loaded processes!");
    println!("Proceeding to main kernel loop...!");

    board_kernel.kernel_loop(
        &smartrf,
        chip,
        None::<&kernel::ipc::IPC<{ NUM_PROCS as u8 }>>,
        &main_loop_capability,
    );
}
