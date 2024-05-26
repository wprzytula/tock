#![no_std]
#![cfg_attr(not(doc), no_main)]

use core::ptr::{addr_of, addr_of_mut};

use capsules_core::console;
use capsules_system::{process_policies::PanicFaultPolicy, process_printer::ProcessPrinterText};
use cc2650_chip::{chip::Cc2650, uart};

use kernel::{
    capabilities,
    component::Component as _,
    create_capability, debug,
    platform::{KernelResources, SyscallDriverLookup},
    scheduler::round_robin::RoundRobinSched,
    static_init,
};

mod io;

// High frequency oscillator speed
pub const HFREQ: u32 = 48 * 1_000_000;

// How should the kernel respond when a process faults.
const FAULT_RESPONSE: PanicFaultPolicy = PanicFaultPolicy {};

/// Dummy buffer that causes the linker to reserve enough space for the stack.
#[no_mangle]
#[link_section = ".stack_buffer"]
pub static mut STACK_MEMORY: [u8; 0x1000] = [0; 0x1000];

// Number of concurrent processes this platform supports.
const NUM_PROCS: usize = 2;
static mut PROCESSES: [Option<&'static dyn kernel::process::Process>; NUM_PROCS] = [None, None];

static mut CHIP: Option<&'static Cc2650> = None;
static mut PROCESS_PRINTER: Option<&'static ProcessPrinterText> = None;

struct Platform {
    scheduler: &'static RoundRobinSched<'static>,
    systick: cortexm3::systick::SysTick,
    led: &'static capsules_core::led::LedDriver<
        'static,
        kernel::hil::led::LedHigh<'static, cc2650_chip::gpio::GPIOPin>,
        1,
    >,
    alarm: &'static capsules_core::alarm::AlarmDriver<
        'static,
        capsules_core::virtualizers::virtual_alarm::VirtualMuxAlarm<
            'static,
            cc2650_chip::gpt::Gpt<'static>,
        >,
    >,
    console: &'static capsules_core::console::Console<'static>,
}

impl SyscallDriverLookup for Platform {
    fn with_driver<F, R>(&self, driver_num: usize, f: F) -> R
    where
        F: FnOnce(Option<&dyn kernel::syscall::SyscallDriver>) -> R,
    {
        match driver_num {
            capsules_core::led::DRIVER_NUM => f(Some(self.led)),
            capsules_core::alarm::DRIVER_NUM => f(Some(self.alarm)),
            capsules_core::console::DRIVER_NUM => f(Some(self.console)),
            _ => f(None),
        }
    }
}

impl<'a> KernelResources<Cc2650<'a>> for Platform {
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
unsafe fn start() -> (&'static kernel::Kernel, Platform, &'static Cc2650<'static>) {
    cc2650_chip::init();

    // Create capabilities that the board needs to call certain protected kernel
    // functions.
    let process_management_capability =
        create_capability!(capabilities::ProcessManagementCapability);

    /* PERIPHERALS CONFIGURATION */
    let chip = static_init!(Cc2650, Cc2650::new());

    let board_kernel = static_init!(kernel::Kernel, kernel::Kernel::new(&*addr_of!(PROCESSES)));

    // let dynamic_deferred_call_clients =
    //     static_init!([DynamicDeferredCallClientState; 2], Default::default());
    // let dynamic_deferred_caller = static_init!(
    //     DynamicDeferredCall,
    //     DynamicDeferredCall::new(dynamic_deferred_call_clients)
    // );
    // DynamicDeferredCall::set_global_instance(dynamic_deferred_caller);

    // Powering on domains and clock gating is done in Cc2650::new().

    CHIP = Some(chip);
    /* END PERIPHERALS CONFIGURATION */

    /* CAPSULES CONFIGURATION */
    // LEDs
    let led = components::led::LedsComponent::new().finalize(components::led_component_static!(
        kernel::hil::led::LedHigh<'static, cc2650_chip::gpio::GPIOPin>,
        kernel::hil::led::LedHigh::new(&cc2650_chip::gpio::PORT[io::LED_PANIC_PIN]),
    ));

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

    let uart_mux = components::console::UartMuxComponent::new(&chip.uart_full, uart::BAUD_RATE)
        .finalize(components::uart_mux_component_static!());
    let console =
        components::console::ConsoleComponent::new(board_kernel, console::DRIVER_NUM, &uart_mux)
            .finalize(components::console_component_static!(128, 128)); // (64, 64) is the default
    let _debug_writer = components::debug_writer::DebugWriterComponent::new(&uart_mux)
        .finalize(components::debug_writer_component_static!());
    /* END CAPSULES CONFIGURATION */

    /* PLATFORM CONFIGURATION */
    let process_printer = components::process_printer::ProcessPrinterTextComponent::new()
        .finalize(components::process_printer_text_component_static!());
    PROCESS_PRINTER = Some(process_printer);

    let scheduler = components::sched::round_robin::RoundRobinComponent::new(&*addr_of!(PROCESSES))
        .finalize(components::round_robin_component_static!(NUM_PROCS));
    let smartrf = Platform {
        scheduler,
        systick: cortexm3::systick::SysTick::new_with_calibration(HFREQ),
        led,
        alarm,
        console,
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

    println!("Hello world from initialised board!");
    println!("Proceeding to loading processes...!");
    debug!("Checking that kernel debug prints work...");

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

    (board_kernel, smartrf, chip)
}

/// Main function called after RAM initialized.
#[no_mangle]
pub unsafe fn main() {
    let main_loop_capability = create_capability!(capabilities::MainLoopCapability);

    let (board_kernel, smartrf, chip) = start();
    board_kernel.kernel_loop(
        &smartrf,
        chip,
        None::<&kernel::ipc::IPC<{ NUM_PROCS as u8 }>>,
        &main_loop_capability,
    );
}
