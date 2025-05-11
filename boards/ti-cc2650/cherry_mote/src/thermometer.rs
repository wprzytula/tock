use cc2650_chip::{gpio::GPIOPin, i2c::I2C};
use kernel::hil::gpio::{Configure, Output};
use kernel::hil::i2c::{Error, I2CClient, I2CDevice, I2CHwMasterClient, I2CMaster, SMBusDevice};
use kernel::utilities::cells::OptionalCell;

use ti_cc2650_common::SetThermometerClient;

pub struct Tmp431<'a> {
    i2c: &'a I2C<'a>,
    dio: &'a GPIOPin,
    thermometer_client: OptionalCell<&'a dyn I2CClient>,
}

impl<'a> Tmp431<'a> {
    pub fn new(i2c: &'a I2C<'a>, dio: &'a GPIOPin) -> Self {
        Self {
            i2c,
            dio,
            thermometer_client: OptionalCell::empty(),
        }
    }
}

impl<'a> SetThermometerClient<'a> for Tmp431<'a> {
    fn set_client(&self, thermometer_client: &'a dyn I2CClient) {
        self.thermometer_client.replace(thermometer_client);
    }
}

// Taken from whip6, which tries the first address, and if an error occurs,
// tries switching to the second one. Apparently, both are possible in CherryMotes.
// Experiments showed that at least CherryMote 116 and 677 have their thermometers
// at DEV_ADDRESS1.
const DEV_ADDRESS1: u8 = 0x4C;
#[allow(dead_code)]
const DEV_ADDRESS2: u8 = 0x4D;

const TMP431_ADDR: u8 = DEV_ADDRESS1;

impl<'a> I2CDevice for Tmp431<'a> {
    fn enable(&self) {
        self.dio.make_output();
        self.dio.set();
    }

    fn disable(&self) {
        self.dio.clear();
        self.dio.deactivate_to_low_power();
    }

    fn write_read(
        &self,
        data: &'static mut [u8],
        write_len: usize,
        read_len: usize,
    ) -> Result<(), (Error, &'static mut [u8])> {
        self.i2c.write_read(TMP431_ADDR, data, write_len, read_len)
    }

    fn write(&self, data: &'static mut [u8], len: usize) -> Result<(), (Error, &'static mut [u8])> {
        self.i2c.write(TMP431_ADDR, data, len)
    }

    fn read(
        &self,
        buffer: &'static mut [u8],
        len: usize,
    ) -> Result<(), (Error, &'static mut [u8])> {
        self.i2c.read(TMP431_ADDR, buffer, len)
    }
}

impl<'a> SMBusDevice for Tmp431<'a> {
    fn smbus_write_read(
        &self,
        data: &'static mut [u8],
        write_len: usize,
        read_len: usize,
    ) -> Result<(), (Error, &'static mut [u8])> {
        self.write_read(data, write_len, read_len)
    }

    fn smbus_write(
        &self,
        data: &'static mut [u8],
        len: usize,
    ) -> Result<(), (Error, &'static mut [u8])> {
        self.write(data, len)
    }

    fn smbus_read(
        &self,
        buffer: &'static mut [u8],
        len: usize,
    ) -> Result<(), (Error, &'static mut [u8])> {
        self.read(buffer, len)
    }
}

impl<'a> I2CHwMasterClient for Tmp431<'a> {
    fn command_complete(&self, buffer: &'static mut [u8], status: Result<(), Error>) {
        self.thermometer_client
            .map(|client| client.command_complete(buffer, status));
    }
}
