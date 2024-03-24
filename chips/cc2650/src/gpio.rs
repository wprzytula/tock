use core::ops::{Index, IndexMut};
use kernel::hil;
use tock_cells::optional_cell::OptionalCell;

use crate::driverlib;

mod internals {
    use core::ops::Deref;

    pub(super) struct Gpio(*const cc2650::gpio::RegisterBlock);
    unsafe impl Send for Gpio {}
    unsafe impl Sync for Gpio {}

    // taken straight from cc2650 crate
    const GPIO_REGISTER_BLOCK_ADDR: usize = 1073881088;
    pub(super) static GPIO: Gpio = Gpio(GPIO_REGISTER_BLOCK_ADDR as *const _);

    impl Deref for Gpio {
        type Target = cc2650::gpio::RegisterBlock;

        fn deref(&self) -> &Self::Target {
            unsafe { &*self.0 }
        }
    }
}
use internals::GPIO;

pub struct GPIOPin {
    pin: u32,
    pin_mask: u32,
    client: OptionalCell<&'static dyn hil::gpio::Client>,
}

impl GPIOPin {
    const fn new(pin: u32) -> GPIOPin {
        debug_assert!(pin < 32);
        GPIOPin {
            pin,
            pin_mask: 1 << pin,
            client: OptionalCell::empty(),
        }
    }

    pub fn set_client(&self, client: &'static dyn hil::gpio::Client) {
        self.client.set(client);
    }

    pub fn handle_interrupt(&self) {
        self.client.map(|client| {
            client.fired();
        });
    }
}

impl hil::gpio::Input for GPIOPin {
    fn read(&self) -> bool {
        // unsafe { driverlib::GPIO_readDio(self.pin) != 0 }
        GPIO.din31_0.read().bits() & self.pin_mask != 0
    }
}

impl hil::gpio::Output for GPIOPin {
    fn toggle(&self) -> bool {
        // unsafe { driverlib::GPIO_toggleDio(self.pin) };
        GPIO.douttgl31_0
            .modify(|_r, w| unsafe { w.bits(self.pin_mask) });
        GPIO.dout31_0.read().bits() & self.pin_mask != 0
    }

    fn set(&self) {
        // unsafe { driverlib::GPIO_setDio(self.pin) }
        GPIO.doutset31_0.write(|w| unsafe { w.bits(self.pin_mask) });
    }

    fn clear(&self) {
        // unsafe { driverlib::GPIO_clearDio(self.pin) }
        GPIO.doutclr31_0.write(|w| unsafe { w.bits(self.pin_mask) });
    }
}

/// Pinmux implementation (IOC)
impl GPIOPin {
    pub fn enable_gpio(&self) {
        // let ioc = unsafe { cc2650::Peripherals::steal().IOC };
        // let modifier = |_r, w| w.port_id().gpio().ie().clear_bit().iostr().max();
        // let ioc_register_block: *const cc2650::ioc::RegisterBlock = ioc.deref();
        // let pin_block = unsafe { &*ioc_register_block.add(self.pin as usize) };
        // pin_block.

        // Driverlib is better here: cc2650 crate requires either matching over 32 options or a lot of unsafe.
        // OTOH both IOCPortConfigure{G,S}et are present in ROM.
        let pin_config = unsafe { driverlib::IOCPortConfigureGet(self.pin) };
        unsafe { driverlib::IOCPortConfigureSet(self.pin, driverlib::IOC_PORT_GPIO, pin_config) };
    }

    fn enable_output(&self) {
        // unsafe { driverlib::GPIO_setOutputEnableDio(self.pin, driverlib::GPIO_OUTPUT_ENABLE) };
        GPIO.doe31_0
            .modify(|_r, w| unsafe { w.bits(self.pin_mask) });
    }

    fn enable_input(&self) {
        // Driverlib is better here: cc2650 crate requires either matching over 32 options or a lot of unsafe.
        // OTOH both IOCPortConfigure{G,S}et are present in ROM.
        let mut pin_config = unsafe { driverlib::IOCPortConfigureGet(self.pin) };
        pin_config |= driverlib::IOC_INPUT_ENABLE;
        unsafe { driverlib::IOCPortConfigureSet(self.pin, driverlib::IOC_PORT_GPIO, pin_config) };
    }
}

impl hil::gpio::Configure for GPIOPin {
    fn floating_state(&self) -> hil::gpio::FloatingState {
        // Driverlib is better here: cc2650 crate requires either matching over 32 options or a lot of unsafe.
        // OTOH IOCPortConfigureGet is present in ROM.
        let pin_config = unsafe { driverlib::IOCPortConfigureGet(self.pin) };
        match (
            pin_config & driverlib::IOC_IOPULL_DOWN,
            pin_config & driverlib::IOC_IOPULL_UP,
            pin_config & driverlib::IOC_NO_IOPULL,
        ) {
            (driverlib::IOC_IOPULL_DOWN, 0, 0) => hil::gpio::FloatingState::PullDown,
            (0, driverlib::IOC_IOPULL_UP, 0) => hil::gpio::FloatingState::PullUp,
            (0, 0, driverlib::IOC_NO_IOPULL) => hil::gpio::FloatingState::PullNone,
            _ => unreachable!("invalid floating state value"),
        }
    }

    fn set_floating_state(&self, mode: hil::gpio::FloatingState) {
        // Driverlib is better here: IOCIOPortPullSet is present in ROM.
        let mode = match mode {
            hil::gpio::FloatingState::PullDown => driverlib::IOC_IOPULL_DOWN,
            hil::gpio::FloatingState::PullUp => driverlib::IOC_IOPULL_UP,
            hil::gpio::FloatingState::PullNone => driverlib::IOC_NO_IOPULL,
        };

        unsafe { driverlib::IOCIOPortPullSet(self.pin, mode) }
    }

    fn deactivate_to_low_power(&self) {
        self.set_floating_state(hil::gpio::FloatingState::PullNone);
        self.disable_input();
        self.disable_output();
    }

    fn is_output(&self) -> bool {
        // unsafe { driverlib::GPIO_getOutputEnableDio(self.pin) != 0 }
        GPIO.doe31_0.read().bits() & self.pin_mask != 0
    }

    fn make_output(&self) -> hil::gpio::Configuration {
        self.enable_gpio();
        self.enable_output();

        hil::gpio::Configuration::Output
    }

    fn disable_output(&self) -> hil::gpio::Configuration {
        unsafe { driverlib::GPIO_setOutputEnableDio(self.pin, 0) };
        self.configuration()
    }

    fn is_input(&self) -> bool {
        unsafe { driverlib::IOCPortConfigureGet(self.pin) & driverlib::IOC_INPUT_ENABLE != 0 }
    }

    fn make_input(&self) -> hil::gpio::Configuration {
        self.enable_gpio();
        self.enable_input();
        hil::gpio::Configuration::Input
    }

    fn disable_input(&self) -> hil::gpio::Configuration {
        let mut pin_config = unsafe { driverlib::IOCPortConfigureGet(self.pin) };
        pin_config &= !driverlib::IOC_INPUT_ENABLE;
        unsafe { driverlib::IOCPortConfigureSet(self.pin, driverlib::IOC_PORT_GPIO, pin_config) };
        self.configuration()
    }

    fn configuration(&self) -> hil::gpio::Configuration {
        let input = self.is_input();
        let output = self.is_output();
        let config = (input, output);
        match config {
            (false, false) => hil::gpio::Configuration::LowPower,
            (false, true) => hil::gpio::Configuration::Output,
            (true, false) => hil::gpio::Configuration::Input,
            (true, true) => hil::gpio::Configuration::InputOutput,
        }
    }
}

pub struct Port<const N: usize> {
    pub pins: [GPIOPin; N],
}

impl<const N: usize> Index<u32> for Port<N> {
    type Output = GPIOPin;

    fn index(&self, index: u32) -> &GPIOPin {
        &self.pins[index as usize]
    }
}

impl<const N: usize> IndexMut<u32> for Port<N> {
    fn index_mut(&mut self, index: u32) -> &mut GPIOPin {
        &mut self.pins[index as usize]
    }
}

impl<const N: usize> Port<N> {
    pub const fn new(pins: [GPIOPin; N]) -> Self {
        Self { pins }
    }
}

const NUM_PINS: usize = 32;

pub static mut PORT: Port<NUM_PINS> = Port::new([
    GPIOPin::new(0),
    GPIOPin::new(1),
    GPIOPin::new(2),
    GPIOPin::new(3),
    GPIOPin::new(4),
    GPIOPin::new(5),
    GPIOPin::new(6),
    GPIOPin::new(7),
    GPIOPin::new(8),
    GPIOPin::new(9),
    GPIOPin::new(10),
    GPIOPin::new(11),
    GPIOPin::new(12),
    GPIOPin::new(13),
    GPIOPin::new(14),
    GPIOPin::new(15),
    GPIOPin::new(16),
    GPIOPin::new(17),
    GPIOPin::new(18),
    GPIOPin::new(19),
    GPIOPin::new(20),
    GPIOPin::new(21),
    GPIOPin::new(22),
    GPIOPin::new(23),
    GPIOPin::new(24),
    GPIOPin::new(25),
    GPIOPin::new(26),
    GPIOPin::new(27),
    GPIOPin::new(28),
    GPIOPin::new(29),
    GPIOPin::new(30),
    GPIOPin::new(31),
]);
