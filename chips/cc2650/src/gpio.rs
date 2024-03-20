use core::ops::{Index, IndexMut};
use kernel::hil::gpio::{self, Input};
use tock_cells::optional_cell::OptionalCell;

use crate::{driverlib, peripheral_interrupts as irqs};

pub struct GPIOPin {
    pin: u32,
    pin_mask: u32,
    client: OptionalCell<&'static dyn gpio::Client>,
}

impl GPIOPin {
    const fn new(pin: u32) -> GPIOPin {
        GPIOPin {
            pin,
            pin_mask: 1 << pin,
            client: OptionalCell::empty(),
        }
    }

    pub fn set_client(&self, client: &'static dyn gpio::Client) {
        self.client.set(client);
    }

    pub fn handle_interrupt(&self) {
        self.client.map(|client| {
            client.fired();
        });
    }
}

impl gpio::Input for GPIOPin {
    fn read(&self) -> bool {
        unsafe { driverlib::GPIO_readDio(self.pin) != 0 }
    }
}

impl gpio::Output for GPIOPin {
    fn toggle(&self) -> bool {
        unsafe { driverlib::GPIO_toggleDio(self.pin) };
        self.read()
    }

    fn set(&self) {
        // unsafe { driverlib::GPIO_setDio(self.pin) }
        if self.pin == 25 {
            let gpio = unsafe { cc2650::Peripherals::steal().GPIO };
            gpio.dout27_24.modify(|_r, w| w.dio25().set_bit());
        }
    }

    fn clear(&self) {
        if self.pin == 25 {
            // unsafe { driverlib::GPIO_clearDio(self.pin) }
            let gpio = unsafe { cc2650::Peripherals::steal().GPIO };
            gpio.dout27_24.modify(|_r, w| w.dio25().clear_bit());
        }
    }
}

/// Pinmux implementation (IOC)
impl GPIOPin {
    pub fn enable_gpio(&self) {
        if self.pin == 25 {
            let ioc = unsafe { cc2650::Peripherals::steal().IOC };
            ioc.iocfg25
                .modify(|_r, w| w.port_id().gpio().ie().clear_bit().iostr().max());
        } else {
            let pin_config = unsafe { driverlib::IOCPortConfigureGet(self.pin) };
            unsafe {
                driverlib::IOCPortConfigureSet(self.pin, driverlib::IOC_PORT_GPIO, pin_config)
            };
        }
    }

    fn enable_output(&self) {
        if self.pin == 25 {
            let gpio = unsafe { cc2650::Peripherals::steal().GPIO };
            gpio.doe31_0.modify(|_r, w| w.dio25().set_bit());
        } else {
            unsafe { driverlib::GPIO_setOutputEnableDio(self.pin, driverlib::GPIO_OUTPUT_ENABLE) };
        }
    }

    fn enable_input(&self) {
        let mut pin_config = unsafe { driverlib::IOCPortConfigureGet(self.pin) };
        pin_config |= driverlib::IOC_INPUT_ENABLE;
        unsafe { driverlib::IOCPortConfigureSet(self.pin, driverlib::IOC_PORT_GPIO, pin_config) };
    }
}

impl gpio::Configure for GPIOPin {
    fn floating_state(&self) -> gpio::FloatingState {
        let pin_config = unsafe { driverlib::IOCPortConfigureGet(self.pin) };
        match (
            pin_config & driverlib::IOC_IOPULL_DOWN,
            pin_config & driverlib::IOC_IOPULL_UP,
            pin_config & driverlib::IOC_NO_IOPULL,
        ) {
            (driverlib::IOC_IOPULL_DOWN, 0, 0) => gpio::FloatingState::PullDown,
            (0, driverlib::IOC_IOPULL_UP, 0) => gpio::FloatingState::PullUp,
            (0, 0, driverlib::IOC_NO_IOPULL) => gpio::FloatingState::PullNone,
            _ => unreachable!("invalid floating state value"),
        }
    }

    fn set_floating_state(&self, mode: gpio::FloatingState) {
        let mode = match mode {
            gpio::FloatingState::PullDown => driverlib::IOC_IOPULL_DOWN,
            gpio::FloatingState::PullUp => driverlib::IOC_IOPULL_UP,
            gpio::FloatingState::PullNone => driverlib::IOC_NO_IOPULL,
        };

        unsafe { driverlib::IOCIOPortPullSet(self.pin, mode) }
    }

    fn deactivate_to_low_power(&self) {
        self.set_floating_state(gpio::FloatingState::PullNone);
        self.disable_input();
        self.disable_output();
    }

    fn is_output(&self) -> bool {
        unsafe { driverlib::GPIO_getOutputEnableDio(self.pin) != 0 }
    }

    fn make_output(&self) -> gpio::Configuration {
        self.enable_gpio();
        self.enable_output();

        gpio::Configuration::Output
    }

    fn disable_output(&self) -> gpio::Configuration {
        unsafe { driverlib::GPIO_setOutputEnableDio(self.pin, 0) };
        self.configuration()
    }

    fn is_input(&self) -> bool {
        unsafe { driverlib::IOCPortConfigureGet(self.pin) & driverlib::IOC_INPUT_ENABLE != 0 }
    }

    fn make_input(&self) -> gpio::Configuration {
        self.enable_gpio();
        self.enable_input();
        gpio::Configuration::Input
    }

    fn disable_input(&self) -> gpio::Configuration {
        let mut pin_config = unsafe { driverlib::IOCPortConfigureGet(self.pin) };
        pin_config &= !driverlib::IOC_INPUT_ENABLE;
        unsafe { driverlib::IOCPortConfigureSet(self.pin, driverlib::IOC_PORT_GPIO, pin_config) };
        self.configuration()
    }

    fn configuration(&self) -> gpio::Configuration {
        let input = self.is_input();
        let output = self.is_output();
        let config = (input, output);
        match config {
            (false, false) => gpio::Configuration::LowPower,
            (false, true) => gpio::Configuration::Output,
            (true, false) => gpio::Configuration::Input,
            (true, true) => gpio::Configuration::InputOutput,
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
