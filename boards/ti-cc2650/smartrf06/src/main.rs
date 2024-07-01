#![no_std]
#![cfg_attr(not(doc), no_main)]

use core::ptr::addr_of_mut;

use cc2650_chip::uart::UartPinConfig;
use kernel::{
    create_capability, deferred_call::DeferredCall, hil::led::LedHigh, static_init,
    utilities::cells::TakeCell,
};
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
        cc2650_chip::driverlib::IOID_21
    }

    fn cts() -> u32 {
        cc2650_chip::driverlib::IOID_0
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

    kernel::debug!("Hello world from board with loaded processes!");

    // experiment(chip);

    kernel::debug!("debug: Proceeding to main kernel loop...!");
    exec_deferred_calls();

    board_kernel.kernel_loop(
        &smartrf,
        chip,
        None::<&kernel::ipc::IPC<{ NUM_PROCS as u8 }>>,
        &main_loop_capability,
    );
}

fn exec_deferred_calls() {
    // use kernel::platform::chip::Chip as _;

    while DeferredCall::has_tasks() {
        DeferredCall::service_next_pending();
    }

    // unsafe {
    //     if let Some(chip) = ti_cc2650_common::CHIP {
    //         chip.service_pending_interrupts();
    //     }
    // }
}

unsafe fn experiment(chip: &'static cc2650_chip::chip::Cc2650<'static>) {
    kernel::debug!("Proceeding to radio experiment...!");

    use kernel::hil::radio::{RadioConfig as _, RadioData as _};
    const BUF_LEN: usize = 16;
    const PAN_ID: u16 = 0xABCD;

    {
        static mut BUF: [u8; 5] = [0_u8; 5];

        let (err, _) = chip
            .radio
            .transmit(addr_of_mut!(BUF).as_mut().unwrap_unchecked(), BUF.len())
            .unwrap_err();
        assert!(matches!(err, kernel::ErrorCode::OFF));
    }

    chip.radio.start().unwrap();
    // chip.radio.stop().unwrap();
    // chip.radio.start().unwrap();
    // chip.radio.stop().unwrap();

    struct ExchangeFrames {
        chip: &'static cc2650_chip::chip::Cc2650<'static>,
        tx_buf: TakeCell<'static, [u8]>,
        rx_buf: TakeCell<'static, [u8]>,
    }
    impl ExchangeFrames {
        fn new(chip: &'static cc2650_chip::chip::Cc2650<'static>) -> Self {
            unsafe {
                let rx_buf = static_init!([u8; BUF_LEN], [0_u8; BUF_LEN]);
                let tx_buf = static_init!(
                    [u8; BUF_LEN],
                    [
                        b'A', b'l', b'a', b' ', b'm', b'a', b'k', b'o', b't', b'a', b'.', b'.',
                        b'.', b'.', b'.', b'.',
                    ]
                );

                Self {
                    chip,
                    tx_buf: TakeCell::new(tx_buf),
                    rx_buf: TakeCell::new(rx_buf),
                }
            }
        }

        fn config(&'static self) {
            let radio = &self.chip.radio;

            let device_id: [u8; 8] = self.chip.fcfg.ieee_mac().to_le_bytes();
            let device_id_bottom_16: u16 = u16::from_le_bytes([device_id[0], device_id[1]]);

            kernel::debug!("device_id: {:?}, short: {}", device_id, device_id_bottom_16);
            exec_deferred_calls();

            // radio.set_power_client(self);
            radio.set_config_client(self);

            radio.set_pan(PAN_ID);
            radio.set_tx_power(-3).unwrap();
            radio.set_address(device_id_bottom_16);
            radio.set_address_long(device_id);
            radio.set_channel(kernel::hil::radio::RadioChannel::Channel26);

            kernel::debug!("Right before commiting the config.");
            exec_deferred_calls();
            radio.config_commit();
            kernel::debug!("Right after commiting the config.");
            kernel::debug!("Config commit scheduled. Waiting for callback.");
            exec_deferred_calls();
        }

        fn exchange(&'static self) {
            let radio = &self.chip.radio;
            kernel::debug!("begin exchange");
            exec_deferred_calls();

            radio.set_receive_client(self);
            radio.set_transmit_client(self);

            radio.set_receive_buffer(self.rx_buf.take().unwrap());
            let _ = radio.transmit(self.tx_buf.take().unwrap(), BUF_LEN);
            kernel::debug!("transmit called");
            exec_deferred_calls();
            kernel::debug!("executed deferred calls after transmit called");
        }
    }

    impl kernel::hil::radio::ConfigClient for ExchangeFrames {
        fn config_done(&self, result: Result<(), kernel::ErrorCode>) {
            kernel::debug!("config done");
            exec_deferred_calls();
            result.unwrap();
            unsafe { &*(self as *const Self) }.exchange(); // A little hacking
        }
    }

    impl kernel::hil::radio::TxClient for ExchangeFrames {
        fn send_done(
            &self,
            buf: &'static mut [u8],
            acked: bool,
            result: Result<(), kernel::ErrorCode>,
        ) {
            kernel::debug!("send done");
            result.unwrap();
            assert!(acked);
            self.tx_buf.put(Some(buf));
            panic!("transmitted frame.");
        }
    }

    impl kernel::hil::radio::RxClient for ExchangeFrames {
        fn receive(
            &self,
            buf: &'static mut [u8],
            frame_len: usize,
            _lqi: u8,
            crc_valid: bool,
            result: Result<(), kernel::ErrorCode>,
        ) {
            kernel::debug!("receive done");
            exec_deferred_calls();
            assert_eq!(frame_len, BUF_LEN);
            result.unwrap();
            assert!(crc_valid);
            self.tx_buf.map(|tx_buf| {
                self.rx_buf.map(|rx_buf| {
                    assert_eq!(tx_buf, rx_buf);
                })
            });
            self.rx_buf.put(Some(buf));
            exec_deferred_calls();
            panic!("received frame.");
        }
    }

    impl Drop for ExchangeFrames {
        fn drop(&mut self) {
            struct NoopClient;
            impl kernel::hil::radio::ConfigClient for NoopClient {
                fn config_done(&self, _result: Result<(), kernel::ErrorCode>) {
                    // pass
                }
            }
            kernel::debug!("DROP should have never happened!");
            self.chip.radio.set_config_client(&NoopClient);
        }
    }

    let experiment = static_init!(ExchangeFrames, ExchangeFrames::new(chip));
    experiment.config();
    exec_deferred_calls();
    // experiment.exchange(); <-- happens asynchronously
}

#[no_mangle]
#[inline(never)]
pub unsafe extern "C" fn panic() {
    panic!()
}
