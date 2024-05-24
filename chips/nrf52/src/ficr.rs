// Licensed under the Apache License, Version 2.0 or the MIT License.
// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright Tock Contributors 2022.

//! Factory Information Configuration Registers (FICR)
//!
//! Factory information configuration registers (FICR) are pre-programmed in
//! factory and cannot be erased by the user. These registers contain
//! chip-specific information and configuration.
//!
//! - Author: Pat Pannuto <ppannuto@berkeley.edu>
//! - Date: November 27, 2017

use core::fmt;
use kernel::utilities::registers::interfaces::Readable;
use kernel::utilities::registers::{register_bitfields, ReadOnly};
use kernel::utilities::StaticRef;

const FICR_BASE: StaticRef<FicrRegisters> =
    unsafe { StaticRef::new(0x10000000 as *const FicrRegisters) };

/// Struct of the FICR registers
///
/// Section 13.1 of <https://infocenter.nordicsemi.com/pdf/nRF52832_PS_v1.0.pdf>
/// Section  4.4 of <https://infocenter.nordicsemi.com/pdf/nRF52840_PS_v1.1.pdf>
/// The structure is identical for the data mapped here, differences start
/// at address 0x350.
#[repr(C)]
struct FicrRegisters {
    /// Reserved
    _reserved0: [u32; 4],
    /// Code memory page size
    /// - Address: 0x010 - 0x014
    codepagesize: ReadOnly<u32, CodePageSize::Register>,
    /// Code memory size
    /// - Address: 0x014 - 0x018
    codesize: ReadOnly<u32, CodeSize::Register>,
    /// Reserved
    _reserved1: [u32; 18],
    /// Device identifier
    /// - Address: 0x060 - 0x064
    deviceid0: ReadOnly<u32, DeviceId0::Register>,
    /// Device identifier
    /// - Address: 0x064 - 0x068
    deviceid1: ReadOnly<u32, DeviceId1::Register>,
    /// Reserved
    _reserved2: [u32; 6],
    /// Encryption Root
    /// - Address: 0x080 - 0x090
    er: [ReadOnly<u32, EncryptionRoot::Register>; 4],
    /// Identity Root
    /// - Address: 0x090 - 0x0A0
    ir: [ReadOnly<u32, IdentityRoot::Register>; 4],
    /// Device address type
    /// - Address: 0x0A0 - 0x0A4
    deviceaddrtype: ReadOnly<u32, DeviceAddressType::Register>,
    /// Device address
    /// - Address: 0x0A4 - 0x0A8
    deviceaddr0: ReadOnly<u32, DeviceAddress0::Register>,
    /// Device address
    /// - Address: 0x0A8 - 0x0AC
    deviceaddr1: ReadOnly<u32, DeviceAddress1::Register>,
    /// Reserved
    _reserved3: [u32; 21],
    /// Part code
    /// - Address: 0x100 - 0x104
    info_part: ReadOnly<u32, InfoPart::Register>,
    /// Part Variant, Hardware version and Production configuration
    /// - Address: 0x104 - 0x108
    info_variant: ReadOnly<u32, InfoVariant::Register>,
    /// Package option
    /// - Address: 0x108 - 0x10C
    info_package: ReadOnly<u32, InfoPackage::Register>,
    /// RAM variant
    /// - Address: 0x10C - 0x110
    info_ram: ReadOnly<u32, InfoRam::Register>,
    /// Flash variant
    /// - Address: 0x110 - 0x114
    info_flash: ReadOnly<u32, InfoFlash::Register>,
}

register_bitfields! [u32,
    /// Code memory page size
    CodePageSize [
        /// Code memory page size
        CODEPAGESIZE OFFSET(0) NUMBITS(32)
    ],
    /// Code memory size
    CodeSize [
        /// Code memory size in number of pages
        CODESIZE OFFSET(0) NUMBITS(32)
    ],
    /// Device Identifier
    DeviceId0 [
        /// 32 LSB of 64 bit unique device identifier
        DEVICEID OFFSET(0) NUMBITS(32)
    ],
    /// Device Identifier
    DeviceId1 [
        /// 32 MSB of 64 bit unique device identifier
        DEVICEID OFFSET(0) NUMBITS(32)
    ],
    /// Encryption Root
    EncryptionRoot [
        /// Encryption Root, word n
        ER OFFSET(0) NUMBITS(32)
    ],
    /// Identity Root
    IdentityRoot [
        /// Identity Root, word n
        IR OFFSET(0) NUMBITS(32)
    ],
    /// Device address type
    DeviceAddressType [
        /// Device address type
        DEVICEADDRESSTYPE OFFSET(0) NUMBITS(1) [
            /// Public
            PUBLIC = 0,
            /// Random
            RANDOM = 1
        ]
    ],
    /// Device address 1
    DeviceAddress0 [
        /// 32 LSB of 48 bit device address
        DEVICEADDRESS OFFSET(0) NUMBITS(32)
    ],
    /// Device address 2
    DeviceAddress1 [
        /// 16 MSB of 48 bit device address
        DEVICEADDRESS OFFSET(0) NUMBITS(16)
    ],
    /// Part code
    InfoPart [
        PART OFFSET(0) NUMBITS(32) [
            /// nRF52832
            N52832 = 0x52832,
            /// nRF52833
            N52833 = 0x52833,
            /// nRF52840
            N52840 = 0x52840,
            /// Unspecified
            Unspecified = 0xffffffff
        ]
    ],
    /// Part Variant, Hardware version and Production configuration
    InfoVariant [
        /// Part Variant, Hardware version and Production configuration, encoded as ASCII
        // Note, some of these are not present in datasheets
        // but are in nrf52.svd or are observed in the wild
        VARIANT OFFSET(0) NUMBITS(32) [
            /// AAA0
            AAA0 = 0x41414130,
            /// AAAA
            AAAA = 0x41414141,
            /// AAAB
            AAAB = 0x41414142,
            /// AAB0
            AAB0 = 0x41414230,
            /// AABA
            AABA = 0x41414241,
            /// AABB
            AABB = 0x41414242,
            /// AAC0
            AAC0 = 0x41414330,
            /// AACA
            AACA = 0x41414341,
            /// AACB
            AACB = 0x41414342,
            /// ABBA
            ABBA = 0x41424241,
            /// AAD0
            AAD0 = 0x41414430,
            /// AAD1
            AAD1 = 0x41414431,
            /// AADA
            AADA = 0x41414441,
            /// AAE0
            AAE0 = 0x41414530,
            /// AAEA
            AAEA = 0x41414541,
            /// AAF0
            AAF0 = 0x41414630,
            /// BAAA
            BAAA = 0x42414141,
            /// CAAA
            CAAA = 0x43414141,
            /// Unspecified
            Unspecified = 0xffffffff
        ]
    ],
    /// Package option
    // Note, some of these are not present in datasheet but is in nrf52.svd
    InfoPackage [
        PACKAGE OFFSET(0) NUMBITS(32) [
            /// QFxx - 48-pin QFN
            QF = 0x2000,
            /// CHxx - 7x8 WLCSP 56 balls
            CH = 0x2001,
            /// CIxx - 7x8 WLCSP 56 balls<
            CI = 0x2002,
            /// QIxx - 73-pin aQFN
            QI = 0x2004,
            /// CKxx - 7x8 WLCSP 56 balls with backside coating for light protection
            CK = 0x2005,
            /// Unspecified
            Unspecified = 0xffffffff
        ]
    ],
    /// RAM variant
    InfoRam [
        RAM OFFSET(0) NUMBITS(32) [
            /// 16 kByte RAM
            K16 = 0x10,
            /// 32 kByte RAM
            K32 = 0x20,
            /// 64 kByte RAM
            K64 = 0x40,
            /// 128 kByte RAM
            K128 = 0x80,
            /// 256 kByte RAM
            K256 = 0x100,
            Unspecified = 0xffffffff

        ]
    ],
    /// Flash
    InfoFlash [
        FLASH OFFSET(0) NUMBITS(32) [
            /// 128 kByte FLASH
            K128 = 0x80,
            /// 256 kByte FLASH
            K256 = 0x100,
            /// 512 kByte FLASH
            K512 = 0x200,
            /// 1024 kByte FLASH
            K1024 = 0x400,
            /// 2048 kByte FLASH
            K2048 = 0x800,
            /// Unspecified
            Unspecified = 0xffffffff
        ]
    ]
];

/// Variant describes part variant, hardware version, and production configuration.
#[derive(PartialEq, Debug)]
#[repr(u32)]
pub(crate) enum Variant {
    AAA0 = 0x41414130,
    AAAA = 0x41414141,
    AAAB = 0x41414142,
    AAB0 = 0x41414230,
    AABA = 0x41414241,
    AABB = 0x41414242,
    AAC0 = 0x41414330,
    AACA = 0x41414341,
    AACB = 0x41414342,
    AAD0 = 0x41414430,
    AAD1 = 0x41414431,
    AADA = 0x41414441,
    AAE0 = 0x41414530,
    AAEA = 0x41414541,
    AAF0 = 0x41414630,
    ABBA = 0x41424241,
    BAAA = 0x42414141,
    CAAA = 0x43414141,
    Unspecified = 0xffffffff,
}

#[derive(PartialEq, Debug)]
#[repr(u32)]
enum Part {
    N52832 = 0x52832,
    N52833 = 0x52833,
    N52840 = 0x52840,
    Unspecified = 0xffffffff,
}

#[derive(PartialEq, Debug)]
#[repr(u32)]
enum Package {
    QF = 0x2000,
    CH = 0x2001,
    CI = 0x2002,
    QI = 0x2004,
    CK = 0x2005,
    Unspecified = 0xffffffff,
}

#[derive(PartialEq, Debug)]
#[repr(u32)]
enum Ram {
    K16 = 0x10,
    K32 = 0x20,
    K64 = 0x40,
    K128 = 0x80,
    K256 = 0x100,
    Unspecified = 0xffffffff,
}

#[derive(Debug)]
#[repr(u32)]
enum Flash {
    K128 = 0x80,
    K256 = 0x100,
    K512 = 0x200,
    K1024 = 0x400,
    K2048 = 0x800,
    Unspecified = 0xffffffff,
}

#[derive(Debug)]
#[repr(u32)]
pub enum AddressType {
    Public = 0x0,
    Random = 0x1,
}

pub struct Ficr {
    registers: StaticRef<FicrRegisters>,
}

impl Ficr {
    pub(crate) const fn new() -> Ficr {
        Ficr {
            registers: FICR_BASE,
        }
    }

    fn part(&self) -> Part {
        match self.registers.info_part.get() {
            0x52832 => Part::N52832,
            0x52833 => Part::N52833,
            0x52840 => Part::N52840,
            _ => Part::Unspecified,
        }
    }

    pub(crate) fn variant(&self) -> Variant {
        // If you update this, make sure to update
        // `has_updated_approtect_logic()` as well.
        match self.registers.info_variant.get() {
            0x41414130 => Variant::AAA0,
            0x41414141 => Variant::AAAA,
            0x41414142 => Variant::AAAB,
            0x41414230 => Variant::AAB0,
            0x41414241 => Variant::AABA,
            0x41414242 => Variant::AABB,
            0x41414330 => Variant::AAC0,
            0x41414341 => Variant::AACA,
            0x41414342 => Variant::AACB,
            0x41424241 => Variant::ABBA,
            0x41414430 => Variant::AAD0,
            0x41414431 => Variant::AAD1,
            0x41414441 => Variant::AADA,
            0x41414530 => Variant::AAE0,
            0x41414541 => Variant::AAEA,
            0x41414630 => Variant::AAF0,
            0x42414141 => Variant::BAAA,
            0x43414141 => Variant::CAAA,
            _ => Variant::Unspecified,
        }
    }

    /// Returns if this variant of the nRF52 has the updated APPROTECT logic.
    /// This changed occurred towards the end of 2021 with chips becoming widely
    /// available/used in 2023.
    ///
    /// See https://devzone.nordicsemi.com/nordic/nordic-blog/b/blog/posts/working-with-the-nrf52-series-improved-approtect
    /// for more information.
    pub(crate) fn has_updated_approtect_logic(&self) -> bool {
        // We assume that an unspecified version means that it is new and this
        // module hasn't been updated to recognize it.
        match self.variant() {
            Variant::AAF0 | Variant::Unspecified => true,
            _ => false,
        }
    }

    fn package(&self) -> Package {
        match self.registers.info_package.get() {
            0x2000 => Package::QF,
            0x2001 => Package::CH,
            0x2002 => Package::CI,
            0x2004 => Package::QI,
            0x2005 => Package::CK,
            _ => Package::Unspecified,
        }
    }

    fn ram(&self) -> Ram {
        match self.registers.info_ram.get() {
            0x10 => Ram::K16,
            0x20 => Ram::K32,
            0x40 => Ram::K64,
            0x80 => Ram::K128,
            0x100 => Ram::K256,
            _ => Ram::Unspecified,
        }
    }

    fn flash(&self) -> Flash {
        match self.registers.info_flash.get() {
            0x80 => Flash::K128,
            0x100 => Flash::K256,
            0x200 => Flash::K512,
            0x400 => Flash::K1024,
            0x800 => Flash::K2048,
            _ => Flash::Unspecified,
        }
    }

    pub fn id(&self) -> [u8; 8] {
        let lo = self.registers.deviceid0.read(DeviceId0::DEVICEID);
        let hi = self.registers.deviceid1.read(DeviceId1::DEVICEID);
        let mut addr = [0; 8];
        addr[..4].copy_from_slice(&lo.to_le_bytes());
        addr[4..].copy_from_slice(&hi.to_le_bytes());
        addr
    }

    pub fn address(&self) -> [u8; 6] {
        let lo = self
            .registers
            .deviceaddr0
            .read(DeviceAddress0::DEVICEADDRESS);
        let hi = self
            .registers
            .deviceaddr1
            .read(DeviceAddress1::DEVICEADDRESS) as u16;
        let mut addr = [0; 6];
        addr[..4].copy_from_slice(&lo.to_le_bytes());
        addr[4..].copy_from_slice(&hi.to_le_bytes());
        addr
    }

    pub fn address_type(&self) -> AddressType {
        match self
            .registers
            .deviceaddrtype
            .read(DeviceAddressType::DEVICEADDRESSTYPE)
        {
            0x0 => AddressType::Public,
            _ => AddressType::Random,
        }
    }

    /// Return a MAC address string that has been hardcoded on this chip in the
    /// format `46:db:52:cd:93:9e`.
    pub fn address_str(&self, buf: &'static mut [u8; 17]) -> &'static str {
        let lo = self
            .registers
            .deviceaddr0
            .read(DeviceAddress0::DEVICEADDRESS);
        let hi = self
            .registers
            .deviceaddr1
            .read(DeviceAddress1::DEVICEADDRESS);

        let h: [u8; 16] = *b"0123456789abcdef";

        buf[0] = h[((hi >> 12) & 0xf) as usize];
        buf[1] = h[((hi >> 8) & 0xf) as usize];
        buf[2] = b':';
        buf[3] = h[((hi >> 4) & 0xf) as usize];
        buf[4] = h[((hi >> 0) & 0xf) as usize];
        buf[5] = b':';
        buf[6] = h[((lo >> 28) & 0xf) as usize];
        buf[7] = h[((lo >> 24) & 0xf) as usize];
        buf[8] = b':';
        buf[9] = h[((lo >> 20) & 0xf) as usize];
        buf[10] = h[((lo >> 16) & 0xf) as usize];
        buf[11] = b':';
        buf[12] = h[((lo >> 12) & 0xf) as usize];
        buf[13] = h[((lo >> 8) & 0xf) as usize];
        buf[14] = b':';
        buf[15] = h[((lo >> 4) & 0xf) as usize];
        buf[16] = h[((lo >> 0) & 0xf) as usize];

        // Safe because we use only ascii characters in this buffer.
        unsafe { &*(core::ptr::from_ref::<[u8]>(buf) as *const str) }
    }
}

impl fmt::Display for Ficr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "NRF52 HW INFO: Variant: {:?}, Part: {:?}, Package: {:?}, Ram: {:?}, Flash: {:?}",
            self.variant(),
            self.part(),
            self.package(),
            self.ram(),
            self.flash()
        )
    }
}

/// Static instance for the board. Only one (read-only) set of factory registers.
pub static mut FICR_INSTANCE: Ficr = Ficr::new();
