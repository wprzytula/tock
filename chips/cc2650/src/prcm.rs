use crate::driverlib::{self};

#[derive(Clone, Copy)]
#[repr(u32)]
pub enum PowerDomain {
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

pub struct Power(());

impl Power {
    pub fn enable_domain(domain: PowerDomain) {
        unsafe { driverlib::PRCMPowerDomainOn(domain as u32) };
        while !Power::is_enabled(domain) {}
    }

    pub fn disable_domain(domain: PowerDomain) {
        unsafe { driverlib::PRCMPowerDomainOff(domain as u32) }
    }

    pub fn is_enabled(domain: PowerDomain) -> bool {
        let status = unsafe { driverlib::PRCMPowerDomainStatus(domain as u32) };
        status & driverlib::PRCM_DOMAIN_POWER_ON != 0
    }

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
}
