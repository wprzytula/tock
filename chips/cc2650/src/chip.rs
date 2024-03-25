use core::{arch::asm, fmt::Write};

use cortexm3::{nvic, CortexM3, CortexMVariant as _};

use crate::{
    gpt::Gpt,
    peripheral_interrupts as irq,
    prcm::{self, Prcm},
    uart::UartFull,
};

pub struct Cc2650<'a> {
    userspace_kernel_boundary: cortexm3::syscall::SysCall,
    pub gpt: Gpt<'a>,
    pub uart_full: UartFull<'a>,
    pub prcm: Prcm,
}
const MASK_AON_PROG: (u128, u128) = cortexm3::interrupt_mask!(irq::AON_PROG);

impl<'a> Cc2650<'a> {
    pub unsafe fn new() -> Self {
        let peripherals = cc2650::Peripherals::take().unwrap();

        let prcm = Prcm::new(peripherals.PRCM);
        // Power on peripherals (eg. GPIO) and Serial
        prcm.enable_domains(prcm::PowerDomains::empty().peripherals().serial());

        // Enable the GPIO, UART and GPT clocks
        prcm.enable_clocks(prcm::Clocks::empty().gpio().uart().gpt());

        let gpt = Gpt::new(peripherals.GPT0);

        let uart_full = UartFull::new(peripherals.UART0);
        uart_full.initialize();

        Self {
            userspace_kernel_boundary: cortexm3::syscall::SysCall::new(),
            gpt,
            uart_full,
            prcm,
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
                match interrupt {
                    irq::GPIO => todo!(),
                    irq::I2C => todo!(),
                    irq::RF_CORE_PE_1 => todo!(),
                    irq::AON_RTC => todo!(),
                    irq::UART0 => todo!(),
                    irq::UART1 => todo!(),
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
                    irq::AON_PROG => asm! {
                        "nop",
                        "nop",
                        "nop",
                    },

                    irq::DYNAMIC_PROG => todo!(),
                    irq::AUX_COMP_A => todo!(),
                    irq::AUX_ADC => todo!(),
                    irq::TRNG => todo!(),
                    _ => panic!("Unhandled interrupt {}", interrupt),
                }
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
