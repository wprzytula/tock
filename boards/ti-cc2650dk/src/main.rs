#![no_std]
#![cfg_attr(not(doc), no_main)]

use core::mem::MaybeUninit;
use core::ptr::{addr_of, addr_of_mut};

use capsules_core::console;
use capsules_core::virtualizers::virtual_alarm::VirtualMuxAlarm;
use capsules_extra::net::{
    ieee802154::MacAddress,
    ipv6::{ip_utils::IPAddr, ipv6_send::IP6SendStruct},
    network_capabilities::NetworkCapability,
    udp::udp_send::UDPSendClient,
};
use capsules_system::{process_policies::PanicFaultPolicy, process_printer::ProcessPrinterText};
use cc2650_chip::{chip::Cc2650, uart};

use kernel::{
    capabilities,
    component::Component as _,
    create_capability, debug,
    platform::{KernelResources, SyscallDriverLookup},
    scheduler::round_robin::RoundRobinSched,
    static_init,
    utilities::leasable_buffer::SubSlice,
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
        kernel::hil::led::LedHigh::new(&cc2650_chip::gpio::PORT[25]),
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

    let aes_mux = components::ieee802154::MuxAes128ccmComponent::new(&chip.aes).finalize(
        components::mux_aes128ccm_component_static!(cc2650_chip::aes::AesECB),
    );

    //--------------------------------------------------------------------------
    // IEEE 802.15.4 and UDP
    //--------------------------------------------------------------------------

    let device_id: [u8; 8] = chip.fcfg.ieee_mac().to_le_bytes();
    let device_id_bottom_16: u16 = u16::from_le_bytes([device_id[0], device_id[1]]);

    // Constants related to the configuration of the 15.4 network stack
    const PAN_ID: u16 = 0xABCD;
    const DST_MAC_ADDR: MacAddress = MacAddress::Short(49138);
    const DEFAULT_CTX_PREFIX_LEN: u8 = 8; //Length of context for 6LoWPAN compression
    const DEFAULT_CTX_PREFIX: [u8; 16] = [0x0_u8; 16]; //Context for 6LoWPAN Compression

    let (ieee802154_radio, mux_mac) = components::ieee802154::Ieee802154Component::new(
        board_kernel,
        capsules_extra::ieee802154::DRIVER_NUM,
        &chip.radio,
        aes_mux,
        PAN_ID,
        device_id_bottom_16,
        device_id,
    )
    .finalize(components::ieee802154_component_static!(
        cc2650_chip::ieee802154_radio::Radio,
        cc2650_chip::aes::AesECB<'static>
    ));

    let local_ip_ifaces = static_init!(
        [IPAddr; 3],
        [
            IPAddr::generate_from_mac(MacAddress::Long(device_id)),
            IPAddr([
                0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d,
                0x1e, 0x1f,
            ]),
            IPAddr::generate_from_mac(MacAddress::Short(device_id_bottom_16)),
        ]
    );

    use capsules_extra::net::ipv6::ipv6_send::IP6Sender;
    use capsules_extra::net::udp::udp_send::{MuxUdpSender, UDPSender};

    struct Safe<T>(T);
    unsafe impl<T> Send for Safe<T> {}
    unsafe impl<T> Sync for Safe<T> {}
    static create_cap: Safe<&dyn capabilities::NetworkCapabilityCreationCapability> = Safe(
        &create_capability!(capabilities::NetworkCapabilityCreationCapability),
    );
    static udp_vis: capsules_extra::net::network_capabilities::UdpVisibilityCapability =
        capsules_extra::net::network_capabilities::UdpVisibilityCapability::new(create_cap.0);

    static network_cap: capsules_extra::net::network_capabilities::NetworkCapability =
        NetworkCapability::new(
            capsules_extra::net::network_capabilities::AddrRange::Any,
            capsules_extra::net::network_capabilities::PortRange::Any,
            capsules_extra::net::network_capabilities::PortRange::Any,
            create_cap.0,
        );

    static mut BUF: [u8; 4] = [2, 1, 3, 7];

    let (udp_send_mux, udp_recv_mux, udp_port_table) = components::udp_mux::UDPMuxComponent::new(
        mux_mac,
        DEFAULT_CTX_PREFIX_LEN,
        DEFAULT_CTX_PREFIX,
        DST_MAC_ADDR,
        MacAddress::Long(device_id),
        local_ip_ifaces,
        alarm_mux,
    )
    .finalize(components::udp_mux_component_static!(
        cc2650_chip::gpt::Gpt,
        components::ieee802154::Ieee802154ComponentMacDeviceType<
            cc2650_chip::ieee802154_radio::Radio<'static>,
            cc2650_chip::aes::AesECB<'static>,
        >
    ));

    let udp_send = static_init!(
        capsules_extra::net::udp::udp_send::UDPSendStruct<
            IP6SendStruct<'static, VirtualMuxAlarm<cc2650_chip::gpt::Gpt>>,
        >,
        capsules_extra::net::udp::udp_send::UDPSendStruct::new(udp_send_mux, &udp_vis)
    );
    udp_send
        .send_to(
            local_ip_ifaces[0],
            42,
            kernel::utilities::leasable_buffer::SubSliceMut::new(&mut BUF),
            &network_cap,
        )
        .unwrap();

    // UDP driver initialization happens here
    let _udp_driver = components::udp_driver::UDPDriverComponent::new(
        board_kernel,
        capsules_extra::net::udp::driver::DRIVER_NUM,
        udp_send_mux,
        udp_recv_mux,
        udp_port_table,
        local_ip_ifaces,
    )
    .finalize(components::udp_driver_component_static!(
        cc2650_chip::gpt::Gpt
    ));

    let thread_driver = components::thread_network::ThreadNetworkComponent::new(
        board_kernel,
        capsules_extra::net::thread::driver::DRIVER_NUM,
        udp_send_mux,
        udp_recv_mux,
        udp_port_table,
        aes_mux,
        device_id,
        alarm_mux,
    )
    .finalize(components::thread_network_component_static!(
        cc2650_chip::gpt::Gpt,
        cc2650_chip::aes::AesECB<'static>,
    ));

    ieee802154_radio.set_key_procedure(thread_driver);
    ieee802154_radio.set_device_procedure(thread_driver);

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
