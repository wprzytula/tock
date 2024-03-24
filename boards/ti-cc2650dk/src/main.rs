#![no_std]
#![cfg_attr(not(doc), no_main)]

use cc2650_chip::prcm;
use cc2650_chip::{chip::Cc2650, uart};

use kernel::{
    capabilities,
    component::Component as _,
    create_capability, debug,
    hil::time::Alarm,
    platform::{KernelResources, SyscallDriverLookup},
    scheduler::round_robin::RoundRobinSched,
    static_init,
};

mod io;

// High frequency oscillator speed
pub const HFREQ: u32 = 48 * 1_000_000;

// How should the kernel respond when a process faults.
const FAULT_RESPONSE: kernel::process::PanicFaultPolicy = kernel::process::PanicFaultPolicy {};

/// Dummy buffer that causes the linker to reserve enough space for the stack.
#[no_mangle]
#[link_section = ".stack_buffer"]
pub static mut STACK_MEMORY: [u8; 0x1000] = [0; 0x1000];

// Number of concurrent processes this platform supports.
const NUM_PROCS: usize = 2;
static mut PROCESSES: [Option<&'static dyn kernel::process::Process>; NUM_PROCS] = [None, None];

static mut CHIP: Option<&'static Cc2650> = None;
static mut PROCESS_PRINTER: Option<&'static kernel::process::ProcessPrinterText> = None;

struct Platform {
    scheduler: &'static RoundRobinSched<'static>,
    systick: cortexm3::systick::SysTick,
    led: &'static capsules_core::led::LedDriver<
        'static,
        kernel::hil::led::LedHigh<'static, cc2650_chip::gpio::GPIOPin>,
        1,
    >,
    alarm: &'static capsules_core::alarm::AlarmDriver<'static, cc2650_chip::gpt::Gpt<'static>>,
}

impl SyscallDriverLookup for Platform {
    fn with_driver<F, R>(&self, driver_num: usize, f: F) -> R
    where
        F: FnOnce(Option<&dyn kernel::syscall::SyscallDriver>) -> R,
    {
        match driver_num {
            capsules_core::led::DRIVER_NUM => f(Some(self.led)),
            capsules_core::alarm::DRIVER_NUM => f(Some(self.alarm)),
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
    let peripherals = cc2650::Peripherals::take().unwrap();
    cc2650_chip::init();

    // Create capabilities that the board needs to call certain protected kernel
    // functions.
    let process_management_capability =
        create_capability!(capabilities::ProcessManagementCapability);
    let memory_allocation_capability = create_capability!(capabilities::MemoryAllocationCapability);

    // Power on peripherals (eg. GPIO) and Serial
    prcm::Power::enable_domains(prcm::PowerDomains::empty().peripherals().serial());

    let board_kernel = static_init!(kernel::Kernel, kernel::Kernel::new(&PROCESSES));

    // let dynamic_deferred_call_clients =
    //     static_init!([DynamicDeferredCallClientState; 2], Default::default());
    // let dynamic_deferred_caller = static_init!(
    //     DynamicDeferredCall,
    //     DynamicDeferredCall::new(dynamic_deferred_call_clients)
    // );
    // DynamicDeferredCall::set_global_instance(dynamic_deferred_caller);

    // Enable the GPIO, UART and GPT clocks
    prcm::Clock::enable_clocks(&peripherals.PRCM, prcm::Clocks::empty().gpio().uart().gpt());

    uart::init_uart_full(&peripherals.UART0);

    let chip = static_init!(Cc2650, Cc2650::new());
    CHIP = Some(chip);

    // Capsules go here!
    // LEDs
    let led = components::led::LedsComponent::new().finalize(components::led_component_static!(
        kernel::hil::led::LedHigh<'static, cc2650_chip::gpio::GPIOPin>,
        kernel::hil::led::LedHigh::new(&cc2650_chip::gpio::PORT[25]),
    ));

    // Alarm
    // let alarm = components::alarm::AlarmDriverComponent::new(board_kernel, capsules_core::alarm::DRIVER_NUM, mux)

    let alarm = static_init!(
        capsules_core::alarm::AlarmDriver<'static, cc2650_chip::gpt::Gpt<'static>>,
        capsules_core::alarm::AlarmDriver::new(
            &chip.gpt,
            board_kernel.create_grant(
                capsules_core::alarm::DRIVER_NUM,
                &memory_allocation_capability
            )
        )
    );
    chip.gpt.set_alarm_client(alarm);

    let process_printer = components::process_printer::ProcessPrinterTextComponent::new()
        .finalize(components::process_printer_text_component_static!());
    PROCESS_PRINTER = Some(process_printer);

    let scheduler = components::sched::round_robin::RoundRobinComponent::new(&PROCESSES)
        .finalize(components::round_robin_component_static!(NUM_PROCS));
    let smartrf = Platform {
        scheduler,
        systick: cortexm3::systick::SysTick::new_with_calibration(HFREQ),
        led,
        alarm,
    };

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
        &mut PROCESSES,
        &FAULT_RESPONSE,
        &process_management_capability,
    )
    .unwrap_or_else(|err| {
        debug!("Error loading processes!");
        debug!("{:?}", err);
    });

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
