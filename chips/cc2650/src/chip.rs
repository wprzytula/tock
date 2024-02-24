use core::fmt::Write;

use cortexm3::{nvic, CortexM3, CortexMVariant as _};

pub struct Cc2650 {
    userspace_kernel_boundary: cortexm3::syscall::SysCall,
}

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
            while let Some(interrupt) = nvic::next_pending() {
                // let irq = NvicIrq::from_u32(interrupt)
                //     .expect("Pending IRQ flag not enumerated in NviqIrq");
                // match irq {
                //     NvicIrq::Gpio => gpio::PORT.handle_interrupt(),
                //     NvicIrq::AonRtc => rtc::RTC.handle_interrupt(),
                //     NvicIrq::Uart0 => uart::UART0.handle_interrupt(),
                //     NvicIrq::I2c0 => i2c::I2C0.handle_interrupt(),
                //     // We need to ignore JTAG events since some debuggers emit these
                //     NvicIrq::AonProg => (),
                //     _ => panic!("Unhandled interrupt {:?}", irq),
                // }
                // unimplemented!("Interrupt handling");
                let n = nvic::Nvic::new(interrupt);
                n.clear_pending();
                n.enable();
            }
        }
    }

    fn has_pending_interrupts(&self) -> bool {
        unsafe { nvic::has_pending() }
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
