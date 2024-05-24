// Licensed under the Apache License, Version 2.0 or the MIT License.
// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright Tock Contributors 2022.

//! Interface for sending and receiving IEEE 802.15.4 packets.
//!
//! Hardware independent interface for an 802.15.4 radio. Note that
//! configuration commands are asynchronous and must be committed with a call to
//! config_commit. For example, calling set_address will change the source
//! address of packets but does not change the address stored in hardware used
//! for address recognition. This must be committed to hardware with a call to
//! config_commit. Please see the relevant TRD for more details.

use crate::ErrorCode;

pub trait TxClient {
    fn send_done(&self, buf: &'static mut [u8], acked: bool, result: Result<(), ErrorCode>);
}

pub trait RxClient {
    fn receive(
        &self,
        buf: &'static mut [u8],
        frame_len: usize,
        lqi: u8,
        crc_valid: bool,
        result: Result<(), ErrorCode>,
    );
}

pub trait ConfigClient {
    fn config_done(&self, result: Result<(), ErrorCode>);
}

pub trait PowerClient {
    fn changed(&self, on: bool);
}

/// These constants are used for interacting with the SPI buffer, which contains
/// a 1-byte SPI command, a 1-byte PHY header, and then the 802.15.4 frame. In
/// theory, the number of extra bytes in front of the frame can depend on the
/// particular method used to communicate with the radio, but we leave this as a
/// constant in this generic trait for now.
///
/// Furthermore, the minimum MHR size assumes that
///
/// - The source PAN ID is omitted
/// - There is no auxiliary security header
/// - There are no IEs
///
/// ```text
/// +---------+-----+-----+-------------+-----+
/// | SPI com | PHR | MHR | MAC payload | MFR |
/// +---------+-----+-----+-------------+-----+
/// \______ Static buffer rx/txed to SPI _____/
///                 \__ PSDU / frame length __/
/// \___ 2 bytes ___/
/// ```

pub const MIN_MHR_SIZE: usize = 9;
pub const MFR_SIZE: usize = 2;
pub const MAX_MTU: usize = 127;
pub const MIN_FRAME_SIZE: usize = MIN_MHR_SIZE + MFR_SIZE;
pub const MAX_FRAME_SIZE: usize = MAX_MTU;

pub const PSDU_OFFSET: usize = 2;
pub const LQI_SIZE: usize = 1;
pub const MAX_BUF_SIZE: usize = PSDU_OFFSET + MAX_MTU + LQI_SIZE;
pub const MIN_PAYLOAD_OFFSET: usize = PSDU_OFFSET + MIN_MHR_SIZE;

pub trait Radio<'a>: RadioConfig<'a> + RadioData<'a> {}
// Provide blanket implementations for trait group
impl<'a, T: RadioConfig<'a> + RadioData<'a>> Radio<'a> for T {}

/// Configure the 802.15.4 radio.
pub trait RadioConfig<'a> {
    /// buf must be at least MAX_BUF_SIZE in length, and
    /// reg_read and reg_write must be 2 bytes.
    fn initialize(
        &self,
        spi_buf: &'static mut [u8],
        reg_write: &'static mut [u8],
        reg_read: &'static mut [u8],
    ) -> Result<(), ErrorCode>;
    fn reset(&self) -> Result<(), ErrorCode>;
    fn start(&self) -> Result<(), ErrorCode>;
    fn stop(&self) -> Result<(), ErrorCode>;
    fn is_on(&self) -> bool;
    fn busy(&self) -> bool;

    fn set_power_client(&self, client: &'a dyn PowerClient);

    /// Commit the config calls to hardware, changing the address,
    /// PAN ID, TX power, and channel to the specified values, issues
    /// a callback to the config client when done.
    fn config_commit(&self);
    fn set_config_client(&self, client: &'a dyn ConfigClient);

    fn get_address(&self) -> u16; //....... The local 16-bit address
    fn get_address_long(&self) -> [u8; 8]; // 64-bit address
    fn get_pan(&self) -> u16; //........... The 16-bit PAN ID
    fn get_tx_power(&self) -> i8; //....... The transmit power, in dBm
    fn get_channel(&self) -> u8; // ....... The 802.15.4 channel

    fn set_address(&self, addr: u16);
    fn set_address_long(&self, addr: [u8; 8]);
    fn set_pan(&self, id: u16);
    fn set_tx_power(&self, power: i8) -> Result<(), ErrorCode>;
    fn set_channel(&self, chan: RadioChannel);
}

pub trait RadioData<'a> {
    fn set_transmit_client(&self, client: &'a dyn TxClient);
    fn set_receive_client(&self, client: &'a dyn RxClient);

    fn set_receive_buffer(&self, receive_buffer: &'static mut [u8]);

    fn transmit(
        &self,
        spi_buf: &'static mut [u8],
        frame_len: usize,
    ) -> Result<(), (ErrorCode, &'static mut [u8])>;
}

#[derive(PartialEq, Debug, Copy, Clone)]
pub enum RadioChannel {
    Channel11 = 5,
    Channel12 = 10,
    Channel13 = 15,
    Channel14 = 20,
    Channel15 = 25,
    Channel16 = 30,
    Channel17 = 35,
    Channel18 = 40,
    Channel19 = 45,
    Channel20 = 50,
    Channel21 = 55,
    Channel22 = 60,
    Channel23 = 65,
    Channel24 = 70,
    Channel25 = 75,
    Channel26 = 80,
}

impl RadioChannel {
    /// Returns the u8 value of the channel, which is the IEEE 802.15.4 channel
    pub fn get_channel_number(&self) -> u8 {
        match *self {
            RadioChannel::Channel11 => 11,
            RadioChannel::Channel12 => 12,
            RadioChannel::Channel13 => 13,
            RadioChannel::Channel14 => 14,
            RadioChannel::Channel15 => 15,
            RadioChannel::Channel16 => 16,
            RadioChannel::Channel17 => 17,
            RadioChannel::Channel18 => 18,
            RadioChannel::Channel19 => 19,
            RadioChannel::Channel20 => 20,
            RadioChannel::Channel21 => 21,
            RadioChannel::Channel22 => 22,
            RadioChannel::Channel23 => 23,
            RadioChannel::Channel24 => 24,
            RadioChannel::Channel25 => 25,
            RadioChannel::Channel26 => 26,
        }
    }
}

impl TryFrom<u8> for RadioChannel {
    type Error = ();
    /// Returns the RadioChannel for the given u8 value, which is the IEEE 802.15.4 channel
    fn try_from(val: u8) -> Result<RadioChannel, ()> {
        match val {
            11 => Ok(RadioChannel::Channel11),
            12 => Ok(RadioChannel::Channel12),
            13 => Ok(RadioChannel::Channel13),
            14 => Ok(RadioChannel::Channel14),
            15 => Ok(RadioChannel::Channel15),
            16 => Ok(RadioChannel::Channel16),
            17 => Ok(RadioChannel::Channel17),
            18 => Ok(RadioChannel::Channel18),
            19 => Ok(RadioChannel::Channel19),
            20 => Ok(RadioChannel::Channel20),
            21 => Ok(RadioChannel::Channel21),
            22 => Ok(RadioChannel::Channel22),
            23 => Ok(RadioChannel::Channel23),
            24 => Ok(RadioChannel::Channel24),
            25 => Ok(RadioChannel::Channel25),
            26 => Ok(RadioChannel::Channel26),
            _ => Err(()),
        }
    }
}
