use core::{arch::asm, fmt::Write};

use cortexm3::{nvic, CortexM3, CortexMVariant as _};
use kernel::platform::chip::InterruptService as _;

use crate::{
    fcfg::Fcfg,
    gpt::Gpt,
    peripheral_interrupts as irq,
    prcm::{self, Prcm},
    uart::UartFull,
    udma::Udma,
};

#[cfg(feature = "uart_lite")]
use crate::uart::UartLite;

pub struct Cc2650<'a> {
    userspace_kernel_boundary: cortexm3::syscall::SysCall,
    pub gpt: Gpt<'a>,
    pub uart_full: UartFull<'a>,
    #[cfg(feature = "uart_lite")]
    pub uart_lite: UartLite<'a>,
    pub prcm: Prcm,
    pub fcfg: Fcfg,
}
const MASK_AON_PROG: (u128, u128) = cortexm3::interrupt_mask!(irq::AON_PROG);

impl<'a> Cc2650<'a> {
    pub unsafe fn new() -> Self {
        let peripherals = cc2650::Peripherals::take().unwrap();

        let prcm = Prcm::new(peripherals.PRCM);
        // Power on peripherals (eg. GPIO) and Serial
        prcm.enable_domains(prcm::PowerDomains::empty().peripherals().serial().rfc());

        // Enable the GPIO, UART and GPT clocks
        prcm.enable_clocks(prcm::Clocks::empty().gpio().uart().gpt().dma().rfc());

        let udma = Udma::new(peripherals.UDMA0);
        udma.enable();

        let gpt = Gpt::new(peripherals.GPT0);

        #[cfg(feature = "uart_lite")]
        let uart_lite = {
            let uart_lite = UartLite::new(
                peripherals.AON_RTC,
                peripherals.AON_WUC,
                peripherals.AUX_AIODIO0,
                peripherals.AUX_AIODIO1,
                peripherals.AUX_EVCTL,
                peripherals.AUX_SCE,
                peripherals.AUX_TIMER,
                peripherals.AUX_WUC,
            );
            uart_lite.initialize();
            uart_lite
        };

        let uart_full = UartFull::new(peripherals.UART0);
        uart_full.initialize();
        uart_full.enable();

        let fcfg = Fcfg::new(peripherals.FCFG1);

        Self {
            userspace_kernel_boundary: cortexm3::syscall::SysCall::new(),
            gpt,
            uart_full,
            #[cfg(feature = "uart_lite")]
            uart_lite,
            prcm,
            fcfg,
        }
    }
}

impl kernel::platform::chip::Chip for Cc2650<'_> {
    // type MPU = cortexm3::mpu::MPU;
    type MPU = ();
    type UserspaceKernelBoundary = cortexm3::syscall::SysCall;

    fn mpu(&self) -> &Self::MPU {
        &()
    }

    fn userspace_kernel_boundary(&self) -> &Self::UserspaceKernelBoundary {
        &self.userspace_kernel_boundary
    }

    fn service_pending_interrupts(&self) {
        unsafe {
            while let Some(interrupt) = nvic::next_pending_with_mask(MASK_AON_PROG) {
                let supported = self.service_interrupt(interrupt);
                assert!(supported, "Got unsupported interrupt: {}", interrupt);

                let n = nvic::Nvic::new(interrupt);
                n.clear_pending();
                if interrupt != irq::AON_PROG {
                    n.enable();
                }
            }
        }
    }

    fn has_pending_interrupts(&self) -> bool {
        unsafe { nvic::has_pending_with_mask(MASK_AON_PROG) }
    }

    fn sleep(&self) {
        unsafe {
            cortexm3::support::wfi();
        }
    }

    unsafe fn atomic<F, R>(&self, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        cortexm3::support::atomic(f)
    }

    unsafe fn print_state(&self, writer: &mut dyn Write) {
        CortexM3::print_cortexm_state(writer);
    }
}

impl kernel::platform::chip::InterruptService for Cc2650<'_> {
    unsafe fn service_interrupt(&self, interrupt: u32) -> bool {
        match interrupt {
            irq::GPIO => todo!(),
            irq::I2C => todo!(),
            irq::RF_CORE_PE_1 => todo!(),
            irq::AON_RTC => todo!(),
            irq::UART0 => self.uart_full.handle_interrupt(),
            irq::AUX_SWEV0 => (), // FIXME: don't just ignore AUX_SWEV0
            irq::SSI0 => todo!(),
            irq::SSI1 => todo!(),
            irq::RF_CORE_PE_2 => todo!(),
            irq::RF_CORE_HW => todo!(),
            irq::RF_CMD_ACK => todo!(),
            irq::I2S => todo!(),
            irq::WATCHDOG => todo!(),
            irq::GPT0A => self.gpt.handle_interrupt(),
            irq::GPT0B => unreachable!(),
            irq::GPT1A => unreachable!(),
            irq::GPT1B => unreachable!(),
            irq::GPT2A => unreachable!(),
            irq::GPT2B => unreachable!(),
            irq::GPT3A => unreachable!(),
            irq::GPT3B => unreachable!(),
            irq::CRYPTO => todo!(),
            irq::DMA_SD => todo!(),
            irq::DMA_ERROR => todo!(),
            irq::FLASH => todo!(),
            irq::SW_EVENT_0 => todo!(),
            irq::AUX_COMBINED => todo!(),

            // We need to ignore JTAG events since some debuggers emit these
            // These `nop`s are for debug purposes only.
            irq::AON_PROG => asm! {
                "nop",
                "nop",
                "nop",
            },

            irq::DYNAMIC_PROG => todo!(),
            irq::AUX_COMP_A => todo!(),
            irq::AUX_ADC => todo!(),
            irq::TRNG => todo!(),
            _ => return false,
        }

        true
    }
}
