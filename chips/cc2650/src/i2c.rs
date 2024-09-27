/* 21.4 Initialization and Configuration
 * The following example shows how to configure the I2C module to transmit a single byte as a master. This
 * assumes the system clock is 48 MHz.
 * 1. Enable the serial power domain and enable the I2C module in PRCM by writing to the
 * PRCM:I2CCLKGR register, the PRCM:I2CCLKGS register, the PRCM:I2CCLKGDS register, or by
 * using the following driver library functions:
 * PRCMPeripheralRunEnable(uint32_t)
 * PRCMPeripheralSleepEnable(uint32_t)
 * PRCMPeripheralDeepSLeepEnable(uint32_t)
 * and loading the setting to clock controller by writing to the PRCM:CLKLOADCTL register
 * or by using the driverlib function PRCMLoadSet().
 * 2. Configure the IOC module to route the SDA and SCL signals from I/Os to the I2C module.
 * 3. Initialize the I2C master by writing the I2C:MCR register with a value of 0x0000 0010.
 * 4. Set the desired SCL clock speed of 100 kbps by writing the I2C:MTPR register with the correct value.
 * The value written to the I2C:MTPR register represents the number of system clock periods in one SCL
 * clock period. The TPR value is determined by the following equations:
 *
 * TPR = {PERDMACLK / [2 × (SCL_LP + SCL_HP) × SCL_CLK]} – 1
 * TPR = {48 MHz / [2 × (6 + 4) × 100000]} – 1
 * TPR = 23
 *
 * Write the I2C:MTPR register with the value of 0x0000 0017.
 * 5. Specify the slave address of the master and that the next operation is a transmit by writing the
 * I2C:MSA register with a value of 0x0000 0076, which sets the slave address to 0x3B.
 * 6. Place data (byte) to be transmitted in the data register by writing the I2C:MDR register with the desired
 * data.
 * 7. Initiate a single-byte transmit of the data from master to slave by writing the I2C:MCTRL register with a
 * value of 0x0000 0007 (Stop, Start, Run).
 * 8. Wait until the transmission completes by polling the I2C:MSTAT BUSBSY register bit until it is cleared.
 * 9. Check the I2C:MSTAT ERR register bit to confirm the transmit was acknowledged.
 */

use kernel::{
    deferred_call::{DeferredCall, DeferredCallClient},
    hil::{
        i2c::{Error, I2CHwMasterClient, I2CMaster},
        time::{ConvertTicks, Ticks, Time},
    },
};
use tock_cells::{optional_cell::OptionalCell, take_cell::TakeCell};

use crate::{driverlib, rtc::Rtc};

pub trait I2CPinConfig {
    fn sda() -> u32;
    fn scl() -> u32;
}

pub struct I2C<'a> {
    #[allow(dead_code)]
    i2c: cc2650::I2C0,
    timer: OptionalCell<&'a Rtc<'a>>,
    master_client: OptionalCell<&'a dyn I2CHwMasterClient>,
    buffer: TakeCell<'static, [u8]>,
    deferred_call: DeferredCall,
}

impl<'a> I2C<'a> {
    pub(crate) fn new(i2c: cc2650::I2C0) -> Self {
        Self {
            i2c,
            master_client: OptionalCell::empty(),
            buffer: TakeCell::empty(),
            deferred_call: DeferredCall::new(),
            timer: OptionalCell::empty(),
        }
    }

    pub fn initialize<PinCfg: I2CPinConfig>(&self, rtc: &'a Rtc<'a>, _pin_config: PinCfg) {
        self.timer.set(rtc);
        unsafe {
            // 2. Configure the IOC module to route the SDA and SCL signals from I/Os to the I2C module.
            driverlib::IOCPinTypeI2c(driverlib::IOC_BASE, PinCfg::sda(), PinCfg::scl());

            // let freq = 48_000_000;
            let freq = driverlib::SysCtrlClockGet();
            driverlib::I2CMasterInitExpClk(driverlib::I2C0_BASE, freq, true);

            /* // 3. Initialize the I2C master by writing the I2C:MCR register with a value of 0x0000 0010.
            self.i2c.mcr.write(|w| w.mfe().en());

            // 4. Set the desired SCL clock speed of 100 kbps by writing the I2C:MTPR register with the correct value.
            // The value written to the I2C:MTPR register represents the number of system clock periods in one SCL
            // clock period. The TPR value is determined by the following equations:
            //
            // TPR = {PERDMACLK / [2 × (SCL_LP + SCL_HP) × SCL_CLK]} – 1
            // TPR = {48 MHz / [2 × (6 + 4) × 100000]} – 1
            // TPR = 23
            self.i2c.mtpr.write(|w| w.tpr().bits(23)) */
        }
    }

    pub(crate) fn handle_interrupt(&self) {
        // kernel::debug!("I2C: handling interrupt");
    }

    // MCTRL is not visible in cc2650 crate due to being aliased (and thus shadowed) by MSTAT.
    // Let's work around that.
    #[allow(dead_code)]
    fn get_mctrl(&self) -> &cc2650::i2c0::MCTRL {
        unsafe {
            (core::ptr::addr_of!(self.i2c.mstat) as *const cc2650::i2c0::MCTRL)
                .as_ref()
                .unwrap_unchecked()
        }
    }

    #[allow(dead_code)]
    fn write_byte(&self, byte: u8) -> Result<(), Error> {
        // 5. Specify the slave address of the master and that the next operation is a transmit by writing the
        // I2C:MSA register with a value of 0x0000 0076, which sets the slave address to 0x3B.
        self.i2c
            .msa
            .write(|w| unsafe { w.rs().clear_bit().sa().bits(0x3B) });

        // 6. Place data (byte) to be transmitted in the data register by writing the I2C:MDR
        // register with the desired data.
        self.i2c.mdr.write(|w| unsafe { w.data().bits(byte) });

        // 7. Initiate a single-byte transmit of the data from master to slave by writing
        // the I2C:MCTRL register with a value of 0x0000 0007 (Stop, Start, Run).
        let mctrl = self.get_mctrl();
        mctrl.write(|w| w.stop().set_bit().start().set_bit().run().set_bit());

        // 8. Wait until the transmission completes by polling the I2C:MSTAT BUSBSY register bit until it is cleared.
        while self.i2c.mstat.read().busbsy().bit_is_set() {}

        // 9. Check the I2C:MSTAT ERR register bit to confirm the transmit was acknowledged.
        if self.i2c.mstat.read().err().bit_is_set() {
            Err(Error::DataNak)
        } else {
            Ok(())
        }
    }

    fn wait_busy_timed(timer: &Rtc<'_>) -> Result<(), Error> {
        unsafe {
            let started = timer.now();
            while driverlib::I2CMasterBusy(driverlib::I2C0_BASE) {
                let now = timer.now();
                let elapsed = now.wrapping_sub(started);

                // 10 ms
                if elapsed > timer.ticks_from_ms(10) {
                    // panic!("I2C: timed out waiting for Data Ack");
                    return Err(Error::DataNak);
                }
            }
            match driverlib::I2CMasterErr(driverlib::I2C0_BASE) {
                driverlib::I2C_MASTER_ERR_NONE => Ok(()),
                driverlib::I2C_MASTER_ERR_DATA_ACK => Err(Error::DataNak),
                driverlib::I2C_MASTER_ERR_ADDR_ACK => Err(Error::AddressNak),
                driverlib::I2C_MASTER_ERR_ARB_LOST => Err(Error::ArbitrationLost),
                _ => unreachable!(),
            }
        }
    }
}

const RUN_FLAG: u8 = 0x01;
const START_FLAG: u8 = 0x02;
const STOP_FLAG: u8 = 0x04;
const ACK_END_FLAG: u8 = 0x08;

impl<'a> I2CMaster<'a> for I2C<'a> {
    fn set_master_client(&self, master_client: &'a dyn I2CHwMasterClient) {
        self.master_client.set(master_client);
    }

    fn enable(&self) {
        // NOOP
    }

    fn disable(&self) {
        // NOOP
    }

    // Based on Contiki-NG impl
    fn write_read(
        &self,
        addr: u8,
        data: &'static mut [u8],
        write_len: usize,
        read_len: usize,
    ) -> Result<(), (Error, &'static mut [u8])> {
        if self.buffer.is_some() {
            return Err((Error::Busy, data));
        }
        // kernel::debug!("I2C: issuing write_read: [{:?}]", &data[..write_len]);

        let timer = self.timer.unwrap_or_panic();

        unsafe {
            /* Set slave address for write */
            driverlib::I2CMasterSlaveAddrSet(driverlib::I2C0_BASE, addr, false);

            /* Write first byte */
            driverlib::I2CMasterDataPut(driverlib::I2C0_BASE, data[0]);

            /* Assert RUN + START */
            driverlib::I2CMasterControl(
                driverlib::I2C0_BASE,
                driverlib::I2C_MASTER_CMD_BURST_SEND_START,
            );
            if let Err(err) = Self::wait_busy_timed(timer) {
                driverlib::I2CMasterControl(
                    driverlib::I2C0_BASE,
                    driverlib::I2C_MASTER_CMD_BURST_SEND_ERROR_STOP,
                );
                return Err((err, data));
            }
            for i in 1..write_len {
                /* Write next byte */
                driverlib::I2CMasterDataPut(driverlib::I2C0_BASE, data[i]);

                /* Clear START */
                driverlib::I2CMasterControl(
                    driverlib::I2C0_BASE,
                    driverlib::I2C_MASTER_CMD_BURST_SEND_CONT,
                );
                if let Err(err) = Self::wait_busy_timed(timer) {
                    driverlib::I2CMasterControl(
                        driverlib::I2C0_BASE,
                        driverlib::I2C_MASTER_CMD_BURST_SEND_ERROR_STOP,
                    );
                    return Err((err, data));
                }
            }

            /* Set slave address for read */
            driverlib::I2CMasterSlaveAddrSet(driverlib::I2C0_BASE, addr, true);

            /* Assert ACK */
            driverlib::I2CMasterControl(
                driverlib::I2C0_BASE,
                driverlib::I2C_MASTER_CMD_BURST_RECEIVE_START,
            );

            for i in 0..read_len - 1 {
                if let Err(err) = Self::wait_busy_timed(timer) {
                    driverlib::I2CMasterControl(
                        driverlib::I2C0_BASE,
                        driverlib::I2C_MASTER_CMD_BURST_SEND_ERROR_STOP,
                    );
                    return Err((err, data));
                }
                data[i] = driverlib::I2CMasterDataGet(driverlib::I2C0_BASE) as u8;
                driverlib::I2CMasterControl(
                    driverlib::I2C0_BASE,
                    driverlib::I2C_MASTER_CMD_BURST_RECEIVE_CONT,
                );
            }

            driverlib::I2CMasterControl(
                driverlib::I2C0_BASE,
                driverlib::I2C_MASTER_CMD_BURST_RECEIVE_FINISH,
            );
            if let Err(err) = Self::wait_busy_timed(timer) {
                driverlib::I2CMasterControl(
                    driverlib::I2C0_BASE,
                    driverlib::I2C_MASTER_CMD_BURST_SEND_ERROR_STOP,
                );
                return Err((err, data));
            }
            data[read_len - 1] = driverlib::I2CMasterDataGet(driverlib::I2C0_BASE) as u8;
        }

        // kernel::debug!("I2C: finished write_read: [{:?}]", &data[..read_len]);

        self.buffer.put(Some(data));
        self.deferred_call.set();

        Ok(())
    }

    // Based on whip6 impl
    fn write(
        &self,
        addr: u8,
        data: &'static mut [u8],
        len: usize,
    ) -> Result<(), (Error, &'static mut [u8])> {
        if self.buffer.is_some() {
            return Err((Error::Busy, data));
        }
        // kernel::debug!("I2C: issuing write: {:?}", &data[..len]);
        unsafe {
            driverlib::I2CMasterSlaveAddrSet(driverlib::I2C0_BASE, addr, false);
        }

        let has_start_flag = true;
        let has_stop_flag = true;
        const RUN_FLAG: u8 = 0x01;
        const START_FLAG: u8 = 0x02;
        const STOP_FLAG: u8 = 0x04;

        unsafe {
            let mut cmd = RUN_FLAG;
            if has_start_flag {
                cmd |= START_FLAG;
            }
            if len == 1 {
                if has_stop_flag {
                    cmd |= STOP_FLAG;
                }
                driverlib::I2CMasterDataPut(driverlib::I2C0_BASE, data[0]);
                driverlib::I2CMasterControl(driverlib::I2C0_BASE, cmd as u32);
                while driverlib::I2CMasterBusy(driverlib::I2C0_BASE) {}
                if driverlib::I2CMasterErr(driverlib::I2C0_BASE) != driverlib::I2C_MASTER_ERR_NONE {
                    return Err((Error::DataNak, data));
                }
            } else {
                for i in 0..len {
                    driverlib::I2CMasterDataPut(driverlib::I2C0_BASE, data[i]);
                    driverlib::I2CMasterControl(driverlib::I2C0_BASE, cmd as u32);
                    while driverlib::I2CMasterBusy(driverlib::I2C0_BASE) {}
                    if driverlib::I2CMasterErr(driverlib::I2C0_BASE)
                        != driverlib::I2C_MASTER_ERR_NONE
                    {
                        driverlib::I2CMasterControl(
                            driverlib::I2C0_BASE,
                            driverlib::I2C_MASTER_CMD_BURST_SEND_ERROR_STOP,
                        );
                        while driverlib::I2CMasterBusy(driverlib::I2C0_BASE) {}
                        return Err((Error::DataNak, data));
                    }
                    if i == 0 {
                        cmd &= !START_FLAG;
                    }
                    if i == len - 2 {
                        if has_stop_flag {
                            cmd |= STOP_FLAG;
                        }
                    }
                }
            }
        }

        self.buffer.put(Some(data));
        self.deferred_call.set();

        Ok(())
    }

    // Based on whip6 impl
    fn read(
        &self,
        addr: u8,
        buffer: &'static mut [u8],
        len: usize,
    ) -> Result<(), (Error, &'static mut [u8])> {
        if self.buffer.is_some() {
            return Err((Error::Busy, buffer));
        }
        unsafe {
            driverlib::I2CMasterSlaveAddrSet(driverlib::I2C0_BASE, addr, true);
        }

        let start_flag = true;
        let stop_flag = true;
        let ack_end_flag = true;

        unsafe {
            driverlib::I2CMasterSlaveAddrSet(driverlib::I2C0_BASE, addr, true);
            let mut cmd = RUN_FLAG;
            if start_flag {
                cmd |= START_FLAG;
            }
            if len == 1 {
                if stop_flag {
                    cmd |= STOP_FLAG;
                }
                if ack_end_flag {
                    cmd |= ACK_END_FLAG;
                }
                driverlib::I2CMasterControl(driverlib::I2C0_BASE, cmd as u32);
                while driverlib::I2CMasterBusy(driverlib::I2C0_BASE) {}
                if driverlib::I2CMasterErr(driverlib::I2C0_BASE) != driverlib::I2C_MASTER_ERR_NONE {
                    return Err((Error::DataNak, buffer));
                }
                buffer[0] = driverlib::I2CMasterDataGet(driverlib::I2C0_BASE) as u8;
            } else {
                cmd |= ACK_END_FLAG;
                for i in 0..len {
                    driverlib::I2CMasterControl(driverlib::I2C0_BASE, cmd as u32);
                    while driverlib::I2CMasterBusy(driverlib::I2C0_BASE) {}
                    if driverlib::I2CMasterErr(driverlib::I2C0_BASE)
                        != driverlib::I2C_MASTER_ERR_NONE
                    {
                        driverlib::I2CMasterControl(
                            driverlib::I2C0_BASE,
                            driverlib::I2C_MASTER_CMD_BURST_RECEIVE_ERROR_STOP,
                        );
                        while driverlib::I2CMasterBusy(driverlib::I2C0_BASE) {}
                        return Err((Error::DataNak, buffer));
                    }
                    buffer[i] = driverlib::I2CMasterDataGet(driverlib::I2C0_BASE) as u8;
                    if i == 0 {
                        cmd &= !START_FLAG;
                    }
                    if i == len - 2 {
                        if stop_flag {
                            cmd |= STOP_FLAG;
                        }
                        if !ack_end_flag {
                            cmd &= !ACK_END_FLAG;
                        }
                    }
                }
            }
        }

        // kernel::debug!("I2C: issued read: [{:?}]", &buffer[..len]);
        self.buffer.put(Some(buffer));
        self.deferred_call.set();

        Ok(())
    }
}

impl DeferredCallClient for I2C<'_> {
    fn handle_deferred_call(&self) {
        self.buffer.take().map(|buf| {
            self.master_client
                .map(|client| client.command_complete(buf, Ok(())))
        });
    }

    fn register(&'static self) {
        self.deferred_call.register(self);
    }
}
