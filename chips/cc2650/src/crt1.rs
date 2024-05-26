use cortexm3::{
    initialize_ram_jump_to_main,
    nvic::{self, Nvic},
    unhandled_interrupt, CortexM3, CortexMVariant,
};

use crate::driverlib;

extern "C" {
    // Symbols defined in the linker file
    static mut _erelocate: u32;
    static mut _etext: u32;
    static mut _ezero: u32;
    static mut _srelocate: u32;
    static mut _szero: u32;

    // _estack is not really a function, but it makes the types work
    // You should never actually invoke it!!
    fn _estack();
}

unsafe extern "C" fn hard_fault_handler() {
    panic!("HARD FAULT...");
}

unsafe extern "C" fn aon_programmable_handler() {
    let nvic = Nvic::new(crate::peripheral_interrupts::AON_PROG);
    nvic.disable();
    nvic.clear_pending();
}

#[cfg_attr(
    all(target_arch = "arm", target_os = "none"),
    link_section = ".vectors"
)]
// used Ensures that the symbol is kept until the final binary
// #[cfg_attr(all(target_arch = "arm", target_os = "none"), used)]
#[used]
static BASE_VECTORS: [unsafe extern "C" fn(); 16] = [
    _estack,
    initialize_ram_jump_to_main,
    unhandled_interrupt,       // NMI
    hard_fault_handler,        // Hard Fault
    unhandled_interrupt,       // MPU fault
    unhandled_interrupt,       // Bus fault
    unhandled_interrupt,       // Usage fault
    unhandled_interrupt,       // Reserved
    unhandled_interrupt,       // Reserved
    unhandled_interrupt,       // Reserved
    unhandled_interrupt,       // Reserved
    CortexM3::SVC_HANDLER,     // SVC
    unhandled_interrupt,       // Debug monitor,
    unhandled_interrupt,       // Reserved
    unhandled_interrupt,       // PendSV
    CortexM3::SYSTICK_HANDLER, // Systick
];

#[cfg_attr(
    all(target_arch = "arm", target_os = "none"),
    link_section = ".vectors"
)]
// used Ensures that the symbol is kept until the final binary
// #[cfg_attr(all(target_arch = "arm", target_os = "none"), used)]
#[used]
static IRQS: [unsafe extern "C" fn(); 34] = [
    CortexM3::GENERIC_ISR,            // GPIO Int handler
    CortexM3::GENERIC_ISR,            // I2C
    CortexM3::GENERIC_ISR,            // RF Core Command & Packet Engine 1
    unhandled_interrupt,              // unassigned
    CortexM3::GENERIC_ISR,            // AON RTC
    CortexM3::GENERIC_ISR,            // UART0 Rx and Tx
    crate::scif::Scif::ready_handler, // AUX Software Event 0
    CortexM3::GENERIC_ISR,            // SSI0 Rx and Tx
    CortexM3::GENERIC_ISR,            // SSI1 Rx and Tx
    CortexM3::GENERIC_ISR,            // RF Core & Packet Engine 2
    CortexM3::GENERIC_ISR,            // RF Core Hardware
    CortexM3::GENERIC_ISR,            // RF Core Command Acknowledge
    CortexM3::GENERIC_ISR,            // I2S
    crate::scif::Scif::alert_handler, // AUX Software Event 1
    CortexM3::GENERIC_ISR,            // Watchdog timer
    CortexM3::GENERIC_ISR,            // Timer 0 subtimer A
    CortexM3::GENERIC_ISR,            // Timer 0 subtimer B
    CortexM3::GENERIC_ISR,            // Timer 1 subtimer A
    CortexM3::GENERIC_ISR,            // Timer 1 subtimer B
    CortexM3::GENERIC_ISR,            // Timer 2 subtimer A
    CortexM3::GENERIC_ISR,            // Timer 2 subtimer B
    CortexM3::GENERIC_ISR,            // Timer 3 subtimer A
    CortexM3::GENERIC_ISR,            // Timer 3 subtimer B
    CortexM3::GENERIC_ISR,            // Crypto Core Result available
    CortexM3::GENERIC_ISR,            // uDMA Software
    CortexM3::GENERIC_ISR,            // uDMA Error
    CortexM3::GENERIC_ISR,            // Flash controller
    CortexM3::GENERIC_ISR,            // Software Event 0
    CortexM3::GENERIC_ISR,            // AUX combined event
    aon_programmable_handler,         // AON programmable 0
    CortexM3::GENERIC_ISR,            // Dynamic Programmable interrupt
    // source (Default: PRCM)
    CortexM3::GENERIC_ISR, // AUX Comparator A
    CortexM3::GENERIC_ISR, // AUX ADC new sample or ADC DMA
    // done, ADC underflow, ADC overflow
    CortexM3::GENERIC_ISR, // TRNG event
];

#[no_mangle]
pub unsafe extern "C" fn init() {
    driverlib::SetupTrimDevice();

    nvic::enable_all();
    {
        // disable debugger interrupt to facilitate debugging
        let n = nvic::Nvic::new(crate::peripheral_interrupts::AON_PROG);
        n.clear_pending();
        n.disable();
        n.clear_pending();
    }
}
