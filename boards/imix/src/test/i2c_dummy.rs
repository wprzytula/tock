// Licensed under the Apache License, Version 2.0 or the MIT License.
// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright Tock Contributors 2022.

//! A dummy I2C client

use core::cell::Cell;
use core::ptr::addr_of_mut;
use kernel::debug;
use kernel::hil;
use kernel::hil::i2c::{Error, I2CMaster};

// ===========================================
// Scan for I2C Slaves
// ===========================================

struct ScanClient {
    dev_id: Cell<u8>,
    i2c_master: &'static dyn I2CMaster<'static>,
}

impl ScanClient {
    pub fn new(i2c_master: &'static dyn I2CMaster<'static>) -> Self {
        Self {
            dev_id: Cell::new(1),
            i2c_master,
        }
    }
}

impl hil::i2c::I2CHwMasterClient for ScanClient {
    fn command_complete(&self, buffer: &'static mut [u8], status: Result<(), Error>) {
        let mut dev_id = self.dev_id.get();

        if status == Ok(()) {
            debug!("{:#x}", dev_id);
        }

        let dev: &dyn I2CMaster<'static> = self.i2c_master;
        if dev_id < 0x7F {
            dev_id += 1;
            self.dev_id.set(dev_id);
            dev.write(dev_id, buffer, 2).unwrap();
        } else {
            debug!(
                "Done scanning for I2C devices. Buffer len: {}",
                buffer.len()
            );
        }
    }
}

/// This test should be called with I2C2, specifically
pub fn i2c_scan_slaves(i2c_master: &'static dyn I2CMaster<'static>) {
    static mut DATA: [u8; 255] = [0; 255];

    let dev = i2c_master;

    let i2c_client = unsafe { kernel::static_init!(ScanClient, ScanClient::new(dev)) };
    dev.set_master_client(i2c_client);

    dev.enable();

    debug!("Scanning for I2C devices...");
    dev.write(i2c_client.dev_id.get(), unsafe { &mut *addr_of_mut!(DATA) }, 2)
        .unwrap();
}

// ===========================================
// Test FXOS8700CQ
// ===========================================

#[derive(Copy, Clone)]
enum AccelClientState {
    ReadingWhoami,
    Activating,
    Deactivating,
    ReadingAccelData,
}

struct AccelClient {
    state: Cell<AccelClientState>,
    i2c_master: &'static dyn I2CMaster<'static>,
}

impl AccelClient {
    pub fn new(i2c_master: &'static dyn I2CMaster<'static>) -> Self {
        Self {
            state: Cell::new(AccelClientState::ReadingWhoami),
            i2c_master,
        }
    }
}

impl hil::i2c::I2CHwMasterClient for AccelClient {
    fn command_complete(&self, buffer: &'static mut [u8], status: Result<(), Error>) {
        let dev = self.i2c_master;

        match self.state.get() {
            AccelClientState::ReadingWhoami => {
                debug!("WHOAMI Register 0x{:x} ({:?})", buffer[0], status);
                debug!("Activating Sensor...");
                buffer[0] = 0x2A_u8; // CTRL_REG1
                buffer[1] = 1; // Bit 1 sets `active`
                dev.write(0x1e, buffer, 2).unwrap();
                self.state.set(AccelClientState::Activating);
            }
            AccelClientState::Activating => {
                debug!("Sensor Activated ({:?})", status);
                buffer[0] = 0x01_u8; // X-MSB register
                                     // Reading 6 bytes will increment the register pointer through
                                     // X-MSB, X-LSB, Y-MSB, Y-LSB, Z-MSB, Z-LSB
                dev.write_read(0x1e, buffer, 1, 6).unwrap();
                self.state.set(AccelClientState::ReadingAccelData);
            }
            AccelClientState::ReadingAccelData => {
                let x = (((buffer[0] as u16) << 8) | buffer[1] as u16) as usize;
                let y = (((buffer[2] as u16) << 8) | buffer[3] as u16) as usize;
                let z = (((buffer[4] as u16) << 8) | buffer[5] as u16) as usize;

                let x = ((x >> 2) * 976) / 1000;
                let y = ((y >> 2) * 976) / 1000;
                let z = ((z >> 2) * 976) / 1000;

                debug!(
                    "Accel data ready x: {}, y: {}, z: {} ({:?})",
                    x >> 2,
                    y >> 2,
                    z >> 2,
                    status
                );

                buffer[0] = 0x01_u8; // X-MSB register
                                     // Reading 6 bytes will increment the register pointer through
                                     // X-MSB, X-LSB, Y-MSB, Y-LSB, Z-MSB, Z-LSB
                dev.write_read(0x1e, buffer, 1, 6).unwrap();
                self.state.set(AccelClientState::ReadingAccelData);
            }
            AccelClientState::Deactivating => {
                debug!("Sensor deactivated ({:?})", status);
                debug!("Reading Accel's WHOAMI...");
                buffer[0] = 0x0D_u8; // 0x0D == WHOAMI register
                dev.write_read(0x1e, buffer, 1, 1).unwrap();
                self.state.set(AccelClientState::ReadingWhoami);
            }
        }
    }
}

/// This test should be called with I2C2, specifically
pub fn i2c_accel_test(i2c_master: &'static dyn I2CMaster<'static>) {
    static mut DATA: [u8; 255] = [0; 255];

    let dev = i2c_master;

    let i2c_client = unsafe { kernel::static_init!(AccelClient, AccelClient::new(dev)) };
    dev.set_master_client(i2c_client);
    dev.enable();

    let buf = unsafe { &mut *addr_of_mut!(DATA) };
    debug!("Reading Accel's WHOAMI...");
    buf[0] = 0x0D_u8; // 0x0D == WHOAMI register
    dev.write_read(0x1e, buf, 1, 1).unwrap();
    i2c_client.state.set(AccelClientState::ReadingWhoami);
}

// ===========================================
// Test LI
// ===========================================

#[derive(Copy, Clone)]
enum LiClientState {
    Enabling,
    ReadingLI,
}

struct LiClient {
    state: Cell<LiClientState>,
    i2c_master: &'static dyn I2CMaster<'static>,
}

impl LiClient {
    pub fn new(i2c_master: &'static dyn I2CMaster<'static>) -> Self {
        Self {
            state: Cell::new(LiClientState::Enabling),
            i2c_master,
        }
    }
}

impl hil::i2c::I2CHwMasterClient for LiClient {
    fn command_complete(&self, buffer: &'static mut [u8], status: Result<(), Error>) {
        let dev = self.i2c_master;

        match self.state.get() {
            LiClientState::Enabling => {
                debug!("Reading luminance Registers ({:?})", status);
                buffer[0] = 0x02_u8;
                buffer[0] = 0;
                dev.write_read(0x44, buffer, 1, 2).unwrap();
                self.state.set(LiClientState::ReadingLI);
            }
            LiClientState::ReadingLI => {
                let intensity = ((buffer[1] as usize) << 8) | buffer[0] as usize;
                debug!(
                    "Light Intensity: {}% ({:?})",
                    (intensity * 100) >> 16,
                    status
                );
                buffer[0] = 0x02_u8;
                dev.write_read(0x44, buffer, 1, 2).unwrap();
                self.state.set(LiClientState::ReadingLI);
            }
        }
    }
}

/// This test should be called with I2C2, specifically
pub fn i2c_li_test(i2c_master: &'static dyn I2CMaster<'static>) {
    static mut DATA: [u8; 255] = [0; 255];

    let pin = sam4l::gpio::GPIOPin::new(sam4l::gpio::Pin::PA16);
    pin.enable_output();
    pin.set();

    let dev = i2c_master;

    let i2c_client = unsafe { kernel::static_init!(LiClient, LiClient::new(dev)) };
    dev.set_master_client(i2c_client);
    dev.enable();

    let buf = unsafe { &mut *addr_of_mut!(DATA) };
    debug!("Enabling LI...");
    buf[0] = 0;
    buf[1] = 0b10100000;
    buf[2] = 0b00000000;
    dev.write(0x44, buf, 3).unwrap();
    i2c_client.state.set(LiClientState::Enabling);
}
