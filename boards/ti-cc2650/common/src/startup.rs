use core::ptr::{addr_of, addr_of_mut};

use capsules_core::{console, led::LedDriver};
use capsules_system::{process_policies::PanicFaultPolicy, process_printer::ProcessPrinterText};
use cc2650_chip::{chip::Cc2650, uart};

use kernel::{
    capabilities,
    component::Component as _,
    create_capability, debug,
    hil::uart::Configure as _,
    platform::{KernelResources, SyscallDriverLookup},
    scheduler::round_robin::RoundRobinSched,
    static_init,
};

#[cfg(feature = "uart_lite")]
use crate::console_lite;

// High frequency oscillator speed
pub const HFREQ: u32 = 48 * 1_000_000;

// How should the kernel respond when a process faults.
const FAULT_RESPONSE: PanicFaultPolicy = PanicFaultPolicy {};

/// Dummy buffer that causes the linker to reserve enough space for the stack.
#[no_mangle]
#[link_section = ".stack_buffer"]
pub static mut STACK_MEMORY: [u8; 0x1000] = [0; 0x1000];

// Number of concurrent processes this platform supports.
pub const NUM_PROCS: usize = 2;
pub static mut PROCESSES: [Option<&'static dyn kernel::process::Process>; NUM_PROCS] = [None, None];

pub static mut CHIP: Option<&'static Cc2650> = None;
pub static mut PROCESS_PRINTER: Option<&'static ProcessPrinterText> = None;

pub struct Platform<const NUM_LEDS: usize> {
    scheduler: &'static RoundRobinSched<'static>,
    systick: cortexm3::systick::SysTick,
    leds: capsules_core::led::LedDriver<
        'static,
        kernel::hil::led::LedHigh<'static, cc2650_chip::gpio::GPIOPin>,
        NUM_LEDS,
    >,
    alarm: &'static capsules_core::alarm::AlarmDriver<
        'static,
        capsules_core::virtualizers::virtual_alarm::VirtualMuxAlarm<
            'static,
            cc2650_chip::gpt::Gpt<'static>,
        >,
    >,
    console: &'static capsules_core::console::Console<'static>,
    #[cfg(feature = "uart_lite")]
    console_lite: &'static capsules_core::console_lite::ConsoleLite<'static>,
}

impl<const NUM_LEDS: usize> SyscallDriverLookup for Platform<NUM_LEDS> {
    fn with_driver<F, R>(&self, driver_num: usize, f: F) -> R
    where
        F: FnOnce(Option<&dyn kernel::syscall::SyscallDriver>) -> R,
    {
        match driver_num {
            capsules_core::led::DRIVER_NUM => f(Some(&self.leds)),
            capsules_core::alarm::DRIVER_NUM => f(Some(self.alarm)),
            capsules_core::console::DRIVER_NUM => f(Some(self.console)),
            #[cfg(feature = "uart_lite")]
            console_lite::DRIVER_NUM => f(Some(self.console_lite)),
            _ => f(None),
        }
    }
}

impl<'a, const NUM_LEDS: usize> KernelResources<Cc2650<'a>> for Platform<NUM_LEDS> {
    type SyscallDriverLookup = Self;
    type SyscallFilter = ();
    type ProcessFault = ();
    type Scheduler = RoundRobinSched<'static>;
    type SchedulerTimer = cortexm3::systick::SysTick;
    type WatchDog = ();
    type ContextSwitchCallback = ();

    fn syscall_driver_lookup(&self) -> &Self::SyscallDriverLookup {
        self
    }
    fn syscall_filter(&self) -> &Self::SyscallFilter {
        &()
    }
    fn process_fault(&self) -> &Self::ProcessFault {
        &()
    }
    fn scheduler(&self) -> &Self::Scheduler {
        self.scheduler
    }
    fn scheduler_timer(&self) -> &Self::SchedulerTimer {
        &self.systick
    }
    fn watchdog(&self) -> &Self::WatchDog {
        &()
    }
    fn context_switch_callback(&self) -> &Self::ContextSwitchCallback {
        &()
    }
}

/// This is in a separate, inline(never) function so that its stack frame is
/// removed when this function returns. Otherwise, the stack space used for
/// these static_inits is wasted.
#[inline(never)]
pub unsafe fn start<const NUM_LEDS: usize>(
    leds: &'static [&'static kernel::hil::led::LedHigh<'static, cc2650_chip::gpio::GPIOPin>;
                 NUM_LEDS],
) -> (
    &'static kernel::Kernel,
    Platform<NUM_LEDS>,
    &'static Cc2650<'static>,
) {
    cc2650_chip::init();

    // Create capabilities that the board needs to call certain protected kernel
    // functions.
    let process_management_capability =
        create_capability!(capabilities::ProcessManagementCapability);

    /* PERIPHERALS CONFIGURATION */
    let chip = static_init!(Cc2650, Cc2650::new());

    let board_kernel = static_init!(kernel::Kernel, kernel::Kernel::new(&*addr_of!(PROCESSES)));

    // Powering on domains and clock gating is done in Cc2650::new().

    CHIP = Some(chip);
    /* END PERIPHERALS CONFIGURATION */

    /* CAPSULES CONFIGURATION */
    // LEDs
    let leds = LedDriver::new(&leds);

    // Alarm
    let alarm_mux = components::alarm::AlarmMuxComponent::new(&chip.gpt).finalize(
        components::alarm_mux_component_static!(cc2650_chip::gpt::Gpt),
    );

    let alarm = components::alarm::AlarmDriverComponent::new(
        board_kernel,
        capsules_core::alarm::DRIVER_NUM,
        &alarm_mux,
    )
    .finalize(components::alarm_component_static!(cc2650_chip::gpt::Gpt));

    // UART I/O
    let uart_full_mux =
        components::console::UartMuxComponent::new(&chip.uart_full, uart::BAUD_RATE)
            .finalize(components::uart_mux_component_static!()); // 64 is the default

    // This is to turn HW flow control on again, after UartMux::new() turns it off.
    chip.uart_full
        .configure(kernel::hil::uart::Parameters {
            baud_rate: uart::BAUD_RATE,
            width: kernel::hil::uart::Width::Eight,
            stop_bits: kernel::hil::uart::StopBits::One,
            parity: kernel::hil::uart::Parity::None,
            hw_flow_control: true,
        })
        .unwrap();

    let console = components::console::ConsoleComponent::new(
        board_kernel,
        console::DRIVER_NUM,
        &uart_full_mux,
    )
    .finalize(components::console_component_static!(64, 64)); // (64, 64) is the default

    #[cfg(feature = "uart_lite")]
    kernel::deferred_call::DeferredCallClient::register(&chip.uart_lite);

    #[cfg(feature = "uart_lite")]
    let console_lite = {
        components::console::ConsoleLiteComponent::new(
            board_kernel,
            console_lite::DRIVER_NUM,
            &chip.uart_lite, // Does not use callbacks from UART, so no need to set client.
        )
        .finalize(components::console_lite_component_static!())
    };

    // Debug writer

    #[cfg(not(feature = "debug_to_lite"))]
    components::debug_writer::DebugWriterComponent::new(uart_full_mux).finalize({
        let uart = kernel::static_buf!(capsules_core::virtualizers::virtual_uart::UartDevice);
        let ring = kernel::static_buf!(kernel::collections::ring_buffer::RingBuffer<'static, u8>);
        // 256B buffer to save RAM (2kB is the default). This means 64B for output buffer (the one passed to uart::transmit_buffer)
        // and 192B for internal buffer (the one storing debug prints to be yet done). With UART-lite, internal buffer can be
        // as small as the biggest debug message issued and everything should still work correctly.
        let buffer = kernel::static_buf!([u8; /* 256 */ 1024 * 2]);
        let debug = kernel::static_buf!(kernel::debug::DebugWriter);
        let debug_wrapper = kernel::static_buf!(kernel::debug::DebugWriterWrapper);

        (uart, ring, buffer, debug, debug_wrapper)
    });

    #[cfg(feature = "debug_to_lite")]
    {
        let debugger_uart = &chip.uart_lite;

        const INTERNAL_BUF_SIZE: usize = 128;
        const OUTPUT_BUF_SIZE: usize = 128;
        const BUF_SIZE: usize = INTERNAL_BUF_SIZE + OUTPUT_BUF_SIZE;
        let buf = static_init!([u8; BUF_SIZE], [0_u8; BUF_SIZE]);

        let (output_buf, internal_buf) = buf.split_at_mut(OUTPUT_BUF_SIZE);

        // Create virtual device for kernel debug.
        let ring_buffer = kernel::static_init!(
            kernel::collections::ring_buffer::RingBuffer<'static, u8>,
            kernel::collections::ring_buffer::RingBuffer::new(internal_buf,)
        );
        let debugger = kernel::static_init!(
            kernel::debug::DebugWriter,
            kernel::debug::DebugWriter::new(debugger_uart, output_buf, ring_buffer,)
        );

        // Debugger is the exclusive callback client of UART-Lite. ConsoleLite uses it synchronously.
        kernel::hil::uart::Transmit::set_transmit_client(debugger_uart, debugger);

        let debug_wrapper = static_init!(
            kernel::debug::DebugWriterWrapper,
            kernel::debug::DebugWriterWrapper::new(debugger)
        );
        unsafe {
            kernel::debug::set_debug_writer_wrapper(debug_wrapper);
        }
    }

    /* END CAPSULES CONFIGURATION */

    /* PLATFORM CONFIGURATION */
    // Process Printer consumes 6,5 kB of flash.
    let process_printer = components::process_printer::ProcessPrinterTextComponent::new()
        .finalize(components::process_printer_text_component_static!());
    PROCESS_PRINTER = Some(process_printer);

    let scheduler = components::sched::round_robin::RoundRobinComponent::new(&*addr_of!(PROCESSES))
        .finalize(components::round_robin_component_static!(NUM_PROCS));
    let platform = Platform {
        scheduler,
        systick: cortexm3::systick::SysTick::new_with_calibration(HFREQ),
        leds,
        alarm,
        console,
        #[cfg(feature = "uart_lite")]
        console_lite,
    };
    /* END PLATFORM CONFIGURATION */

    /* LOAD PROCESSES */
    // These symbols are defined in the linker script.
    extern "C" {
        /// Beginning of the ROM region containing app images.
        static _sapps: u8;
        /// End of the ROM region containing app images.
        static _eapps: u8;
        /// Beginning of the RAM region for app memory.
        static mut _sappmem: u8;
        /// End of the RAM region for app memory.
        static _eappmem: u8;
    }

    debug!("Hello world from initialised board!");
    debug!("Proceeding to loading processes...!");

    kernel::process::load_processes(
        board_kernel,
        chip,
        core::slice::from_raw_parts(
            core::ptr::addr_of!(_sapps),
            core::ptr::addr_of!(_eapps) as usize - core::ptr::addr_of!(_sapps) as usize,
        ),
        core::slice::from_raw_parts_mut(
            core::ptr::addr_of_mut!(_sappmem),
            core::ptr::addr_of!(_eappmem) as usize - core::ptr::addr_of!(_sappmem) as usize,
        ),
        &mut *addr_of_mut!(PROCESSES),
        &FAULT_RESPONSE,
        &process_management_capability,
    )
    .unwrap_or_else(|err| {
        debug!("Error loading processes!");
        debug!("{:?}", err);
    });
    /* END LOAD PROCESSES */

    (board_kernel, platform, chip)
}
