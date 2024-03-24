use crate::driverlib;

#[derive(Clone, Copy)]
#[repr(u32)]
enum PowerDomain {
    Rfc = driverlib::PRCM_DOMAIN_RFCORE,
    Serial = driverlib::PRCM_DOMAIN_SERIAL,
    Peripherals = driverlib::PRCM_DOMAIN_PERIPH,
    Sysbus = driverlib::PRCM_DOMAIN_SYSBUS,
    Vims = driverlib::PRCM_DOMAIN_VIMS,
    Cpu = driverlib::PRCM_DOMAIN_CPU,
    Timer = driverlib::PRCM_DOMAIN_TIMER,
    Clkctrl = driverlib::PRCM_DOMAIN_CLKCTRL,
    Mcu = driverlib::PRCM_DOMAIN_MCU,
}

#[derive(Clone, Copy, Default)]
pub struct PowerDomains(u32);

impl PowerDomains {
    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn rfc(self) -> Self {
        Self(self.0 | PowerDomain::Rfc as u32)
    }

    pub const fn serial(self) -> Self {
        Self(self.0 | PowerDomain::Serial as u32)
    }

    pub const fn peripherals(self) -> Self {
        Self(self.0 | PowerDomain::Peripherals as u32)
    }

    pub const fn sysbus(&self) -> Self {
        Self(self.0 | PowerDomain::Sysbus as u32)
    }

    pub const fn vims(self) -> Self {
        Self(self.0 | PowerDomain::Vims as u32)
    }

    pub const fn cpu(self) -> Self {
        Self(self.0 | PowerDomain::Cpu as u32)
    }

    pub const fn timer(self) -> Self {
        Self(self.0 | PowerDomain::Timer as u32)
    }

    pub const fn clkctrl(self) -> Self {
        Self(self.0 | PowerDomain::Clkctrl as u32)
    }

    pub const fn mcu(self) -> Self {
        Self(self.0 | PowerDomain::Mcu as u32)
    }

    pub const fn all(self) -> Self {
        Self::empty()
            .rfc()
            .serial()
            .sysbus()
            .peripherals()
            .vims()
            .cpu()
            .timer()
            .clkctrl()
            .mcu()
    }
}

impl Into<u32> for PowerDomains {
    fn into(self) -> u32 {
        self.0
    }
}

// TODO: rewrite Power with the type-state pattern.

pub struct Power(());

impl Power {
    #[inline]
    pub fn enable_domains(domains: PowerDomains) {
        unsafe { driverlib::PRCMPowerDomainOn(domains.into()) };
        while !Power::are_enabled(domains) {}
    }

    #[inline]
    pub fn disable_domains(domains: PowerDomains) {
        unsafe { driverlib::PRCMPowerDomainOff(domains.into()) }
    }

    #[inline]
    fn are_enabled(domains: PowerDomains) -> bool {
        let status = unsafe { driverlib::PRCMPowerDomainStatus(domains.into()) };
        status & driverlib::PRCM_DOMAIN_POWER_ON != 0
    }

    // TODO: Driverlib version is probably better, consider throwing this away
    pub fn power_on_domains() {
        let prcm = unsafe { cc2650::Peripherals::steal().PRCM };
        // Enable the PERIPH and SERIAL power domains and wait for them to be powered up
        prcm.pdctl0
            .modify(|_r, w| w.periph_on().set_bit().serial_on().set_bit());
        loop {
            let stat = prcm.pdstat0.read();
            if stat.periph_on().bit_is_set() && stat.serial_on().bit_is_set() {
                break;
            }
        }
    }
}

pub struct Clock(());

impl Clock {
    unsafe fn reload_clock_controller() {
        // driverlib::PRCMLoadSet();
        // while !driverlib::PRCMLoadGet() {}

        // Load settings into CLKCTRL and wait for LOAD_DONE
        let prcm = cc2650::Peripherals::steal().PRCM;
        prcm.clkloadctl.modify(|_r, w| w.load().set_bit());
        loop {
            if prcm.clkloadctl.read().load_done().bit_is_set() {
                break;
            }
        }
    }

    unsafe fn enable_gpio_clock() {
        // unsafe {
        //     driverlib::PRCMPeripheralRunEnable(driverlib::PRCM_PERIPH_GPIO);
        //     driverlib::PRCMPeripheralSleepEnable(driverlib::PRCM_PERIPH_GPIO);
        //     driverlib::PRCMPeripheralDeepSleepEnable(driverlib::PRCM_PERIPH_GPIO);
        // }

        // Enable the GPIO clock
        cc2650::Peripherals::steal()
            .PRCM
            .gpioclkgr
            .write(|w| w.clk_en().set_bit());
        cc2650::Peripherals::steal()
            .PRCM
            .gpioclkgs
            .write(|w| w.clk_en().set_bit());
        cc2650::Peripherals::steal()
            .PRCM
            .gpioclkgds
            .write(|w| w.clk_en().set_bit());
    }

    pub fn enable_gpio() {
        unsafe {
            Self::enable_gpio_clock();
            Self::reload_clock_controller();
        }
    }

    unsafe fn enable_gpt_clock() {
        // Enable the GPT0 clock
        cc2650::Peripherals::steal()
            .PRCM
            .gptclkgr
            .write(|w| w.clk_en().gpt0());
        cc2650::Peripherals::steal()
            .PRCM
            .gptclkgs
            .write(|w| w.clk_en().gpt0());
        cc2650::Peripherals::steal()
            .PRCM
            .gptclkgds
            .write(|w| w.clk_en().gpt0());
    }

    pub fn enable_gpt() {
        unsafe {
            Self::enable_gpt_clock();
            Self::reload_clock_controller();
        }
    }

    unsafe fn enable_uart_clock() {
        // Enable the UART clock
        cc2650::Peripherals::steal()
            .PRCM
            .uartclkgr
            .write(|w| w.clk_en().set_bit());
        cc2650::Peripherals::steal()
            .PRCM
            .uartclkgs
            .write(|w| w.clk_en().set_bit());
        cc2650::Peripherals::steal()
            .PRCM
            .uartclkgds
            .write(|w| w.clk_en().set_bit());
    }

    pub fn enable_uart() {
        unsafe {
            Self::enable_uart_clock();
            Self::reload_clock_controller();
        }
    }
}
