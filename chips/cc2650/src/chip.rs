use core::{arch::asm, fmt::Write};

use cortexm3::{nvic, CortexM3, CortexMVariant as _};

use crate::peripheral_interrupts as irq;

pub struct Cc2650 {
    userspace_kernel_boundary: cortexm3::syscall::SysCall,
}
const MASK_AON_PROG: (u128, u128) = cortexm3::interrupt_mask!(irq::AON_PROG);

impl Cc2650 {
    pub unsafe fn new() -> Cc2650 {
        Cc2650 {
            userspace_kernel_boundary: cortexm3::syscall::SysCall::new(),
        }
    }
}

impl kernel::platform::chip::Chip for Cc2650 {
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
                    irq::GPT0A => todo!(),
                    irq::GPT0B => todo!(),
                    irq::GPT1A => todo!(),
                    irq::GPT1B => todo!(),
                    irq::GPT2A => todo!(),
                    irq::GPT2B => todo!(),
                    irq::GPT3A => todo!(),
                    irq::GPT3B => todo!(),
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