use core::ptr::{addr_of, addr_of_mut};

use capsules_core::{console, led::LedDriver};
use capsules_extra::ieee802154::phy_driver;
use capsules_system::{process_policies::PanicFaultPolicy, process_printer::ProcessPrinterText};
use cc2650_chip::{
    chip::{Cc2650, PinConfig},
    ieee802154_radio::Radio,
    uart,
};

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
    ieee802154: &'static capsules_extra::ieee802154::phy_driver::RadioDriver<
        'static,
        cc2650_chip::ieee802154_radio::Radio<'static>,
    >,
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
            capsules_extra::ieee802154::DRIVER_NUM => f(Some(self.ieee802154)),
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
    pin_config: impl PinConfig,
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
    let chip = static_init!(Cc2650, Cc2650::new(pin_config));

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

    //--------------------------------------------------------------------------
    // IEEE 802.15.4 and UDP
    //--------------------------------------------------------------------------

    kernel::deferred_call::DeferredCallClient::register(&chip.radio);

    // let ieee802154 = {
    //     let grant_cap = create_capability!(capabilities::MemoryAllocationCapability);
    //     let kernel_tx_buf = static_init!([u8; 130], [0_u8; 130]);

    //     static_init!(
    //         phy_driver::RadioDriver<'static, Radio<'static>>,
    //         phy_driver::RadioDriver::new(
    //             &chip.radio,
    //             board_kernel.create_grant(phy_driver::DRIVER_NUM, &grant_cap),
    //             kernel_tx_buf
    //         )
    //     )
    // };

    let ieee802154 = components::ieee802154::Ieee802154RawComponent::new(
        board_kernel,
        capsules_extra::ieee802154::DRIVER_NUM,
        &chip.radio,
    )
    .finalize(components::ieee802154_raw_component_static!(
        cc2650_chip::ieee802154_radio::Radio,
    ));

    // use capsules_core::virtualizers::virtual_alarm::VirtualMuxAlarm;
    // use capsules_extra::net::{
    //     ieee802154::MacAddress,
    //     ipv6::{ip_utils::IPAddr, ipv6_send::IP6SendStruct},
    //     network_capabilities::NetworkCapability,
    //     udp::udp_send::UDPSendClient,
    // };
    // use core::mem::MaybeUninit;

    // let device_id: [u8; 8] = chip.fcfg.ieee_mac().to_le_bytes();
    // let device_id_bottom_16: u16 = u16::from_le_bytes([device_id[0], device_id[1]]);

    // Constants related to the configuration of the 15.4 network stack
    // const PAN_ID: u16 = 0xABCD;
    // const DST_MAC_ADDR: MacAddress = MacAddress::Short(49138);
    // const DEFAULT_CTX_PREFIX_LEN: u8 = 8; //Length of context for 6LoWPAN compression
    // const DEFAULT_CTX_PREFIX: [u8; 16] = [0x0_u8; 16]; //Context for 6LoWPAN Compression

    // let (ieee802154_radio, mux_mac) = components::ieee802154::Ieee802154Component::new(
    //     board_kernel,
    //     capsules_extra::ieee802154::DRIVER_NUM,
    //     &chip.radio,
    //     aes_mux,
    //     PAN_ID,
    //     device_id_bottom_16,
    //     device_id,
    // )
    // .finalize(components::ieee802154_component_static!(
    //     cc2650_chip::ieee802154_radio::Radio,
    //     cc2650_chip::aes::AesECB<'static>
    // ));

    // let local_ip_ifaces = static_init!(
    //     [IPAddr; 3],
    //     [
    //         IPAddr::generate_from_mac(MacAddress::Long(device_id)),
    //         IPAddr([
    //             0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d,
    //             0x1e, 0x1f,
    //         ]),
    //         IPAddr::generate_from_mac(MacAddress::Short(device_id_bottom_16)),
    //     ]
    // );

    // use capsules_extra::net::ipv6::ipv6_send::IP6Sender;
    // use capsules_extra::net::udp::udp_send::{MuxUdpSender, UDPSender};

    // struct Safe<T>(T);
    // unsafe impl<T> Send for Safe<T> {}
    // unsafe impl<T> Sync for Safe<T> {}
    // static CREATE_CAP: Safe<&dyn capabilities::NetworkCapabilityCreationCapability> = Safe(
    //     &create_capability!(capabilities::NetworkCapabilityCreationCapability),
    // );
    // static UDP_VIS: capsules_extra::net::network_capabilities::UdpVisibilityCapability =
    //     capsules_extra::net::network_capabilities::UdpVisibilityCapability::new(CREATE_CAP.0);

    // static NETWORK_CAP: capsules_extra::net::network_capabilities::NetworkCapability =
    //     NetworkCapability::new(
    //         capsules_extra::net::network_capabilities::AddrRange::Any,
    //         capsules_extra::net::network_capabilities::PortRange::Any,
    //         capsules_extra::net::network_capabilities::PortRange::Any,
    //         CREATE_CAP.0,
    //     );

    // static mut BUF: [u8; 4] = [2, 1, 3, 7];

    // let (udp_send_mux, udp_recv_mux, udp_port_table) = components::udp_mux::UDPMuxComponent::new(
    //     mux_mac,
    //     DEFAULT_CTX_PREFIX_LEN,
    //     DEFAULT_CTX_PREFIX,
    //     DST_MAC_ADDR,
    //     MacAddress::Long(device_id),
    //     local_ip_ifaces,
    //     alarm_mux,
    // )
    // .finalize(components::udp_mux_component_static!(
    //     cc2650_chip::gpt::Gpt,
    //     components::ieee802154::Ieee802154ComponentMacDeviceType<
    //         cc2650_chip::ieee802154_radio::Radio<'static>,
    //         cc2650_chip::aes::AesECB<'static>,
    //     >
    // ));

    // let udp_send = static_init!(
    //     capsules_extra::net::udp::udp_send::UDPSendStruct<
    //         IP6SendStruct<'static, VirtualMuxAlarm<cc2650_chip::gpt::Gpt>>,
    //     >,
    //     capsules_extra::net::udp::udp_send::UDPSendStruct::new(udp_send_mux, &UDP_VIS)
    // );
    // udp_send
    //     .send_to(
    //         local_ip_ifaces[0],
    //         42,
    //         kernel::utilities::leasable_buffer::SubSliceMut::new(&mut *addr_of_mut!(BUF)),
    //         &NETWORK_CAP,
    //     )
    //     .unwrap();

    // UDP driver initialization happens here
    // let _udp_driver = components::udp_driver::UDPDriverComponent::new(
    //     board_kernel,
    //     capsules_extra::net::udp::driver::DRIVER_NUM,
    //     udp_send_mux,
    //     udp_recv_mux,
    //     udp_port_table,
    //     local_ip_ifaces,
    // )
    // .finalize(components::udp_driver_component_static!(
    //     cc2650_chip::gpt::Gpt
    // ));

    // let thread_driver = components::thread_network::ThreadNetworkComponent::new(
    //     board_kernel,
    //     capsules_extra::net::thread::driver::DRIVER_NUM,
    //     udp_send_mux,
    //     udp_recv_mux,
    //     udp_port_table,
    //     aes_mux,
    //     device_id,
    //     alarm_mux,
    // )
    // .finalize(components::thread_network_component_static!(
    //     cc2650_chip::gpt::Gpt,
    //     cc2650_chip::aes::AesECB<'static>,
    // ));

    // ieee802154_radio.set_key_procedure(thread_driver);
    // ieee802154_radio.set_device_procedure(thread_driver);

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
        ieee802154,
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
