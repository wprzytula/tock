// Licensed under the Apache License, Version 2.0 or the MIT License.
// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright Tock Contributors 2022.

//! Analog-Digital Converter (ADC)

use crate::{dma, ref_module, timer};
use core::cell::Cell;
use core::{mem, slice};
use kernel::hil;
use kernel::utilities::cells::{OptionalCell, TakeCell};
use kernel::utilities::registers::interfaces::{ReadWriteable, Readable, Writeable};
use kernel::utilities::registers::{
    register_bitfields, register_structs, ReadOnly, ReadWrite, WriteOnly,
};
use kernel::utilities::StaticRef;
use kernel::ErrorCode;

const ADC_BASE: StaticRef<AdcRegisters> =
    unsafe { StaticRef::new(0x4001_2000 as *const AdcRegisters) };

const AVAILABLE_ADC_CHANNELS: usize = 24;
const DEFAULT_ADC_RESOLUTION: AdcResolution = AdcResolution::Bits14;
// Maximum sampling frequency is 1Msps, but due to the timer, limit it to 150kHz
const MAX_SAMPLE_FREQ_HZ: u32 = 150_000;

register_structs! {
    /// ADC14
    AdcRegisters {
        /// ADC control 0 register
        (0x000 => ctl0: ReadWrite<u32, CTL0::Register>),
        /// ADC control 1 register
        (0x004 => ctl1: ReadWrite<u32, CTL1::Register>),
        /// Window comparator low threshold 0 register
        (0x008 => lo0: ReadWrite<u32>),
        /// Window comparator high threshold 1
        (0x00C => hi0: ReadWrite<u32>),
        /// Window comparator low threshold 1 register
        (0x010 => lo1: ReadWrite<u32>),
        /// Window comparator high threshold 1 register
        (0x014 => hi1: ReadWrite<u32>),
        /// Memory control register 0-31
        (0x018 => mctl: [ReadWrite<u32, MCTLx::Register>; 32]),
        /// Memory register 0-31
        (0x098 => mem: [ReadWrite<u32>; 32]),
        (0x118 => _reserved),
        /// Interrupt enable 0 register
        (0x13C => ie0: ReadWrite<u32>),
        /// Interrupt enable 1 register
        (0x140 => ie1: ReadWrite<u32, IER1::Register>),
        /// Interrupt flag 0 register
        (0x144 => ifg0: ReadOnly<u32>),
        /// Interrupt flag 1 register
        (0x148 => ifg1: ReadOnly<u32, IFGR1::Register>),
        /// Clear interrupt flag 0 register
        (0x14C => clrifg0: WriteOnly<u32>),
        /// Clear interrupt flag 1 register
        (0x150 => clrifg1: WriteOnly<u32, CLRIFGR1::Register>),
        /// Interrupt vector register
        (0x154 => iv: ReadOnly<u32, IV::Register>),
        (0x158 => @END),
    }
}

register_bitfields![u32,
    /// ADC Control 0 register
    CTL0 [
        /// ADC Start conversion
        SC OFFSET(0) NUMBITS(1) [],
        /// ADC Enable conversion
        ENC OFFSET(1) NUMBITS(1) [],
        // ADC on
        ON OFFSET(4) NUMBITS(1) [],
        /// ADC multiple sample an conversion
        MSC OFFSET(7) NUMBITS(1) [],
        /// ADC sample-and-hold time for pulse sample mode.
        /// Valid for ADCMEM0 to ADCMEM7 and ADCMEM24 to ADCMEM31.
        SHTOx OFFSET(8) NUMBITS(4) [
            /// 4 clock cycles sample-and-hold time
            Cycles4 = 0,
            /// 8 clock cycles sample-and-hold time
            Cycles8 = 1,
            /// 16 clock cycles sample-and-hold time
            Cycles16 = 2,
            /// 32 clock cycles sample-and-hold time
            Cycles32 = 3,
            /// 64 clock cycles sample-and-hold time
            Cycles64 = 4,
            /// 96 clock cycles sample-and-hold time
            Cycles96 = 5,
            /// 128 clock cycles sample-and-hold time
            Cycles128 = 6,
            /// 192 clock cycles sample-and-hold time
            Cycles192 = 7
        ],
        /// ADC sample-and-hold time for pulse sample mode.
        /// Valid for ADCMEM8 to ADCMEM23.
        SHT1x OFFSET(12) NUMBITS(4) [
            /// 4 clock cycles sample-and-hold time
            Cycles4 = 0,
            /// 8 clock cycles sample-and-hold time
            Cycles8 = 1,
            /// 16 clock cycles sample-and-hold time
            Cycles16 = 2,
            /// 32 clock cycles sample-and-hold time
            Cycles32 = 3,
            /// 64 clock cycles sample-and-hold time
            Cycles64 = 4,
            /// 96 clock cycles sample-and-hold time
            Cycles96 = 5,
            /// 128 clock cycles sample-and-hold time
            Cycles128 = 6,
            /// 192 clock cycles sample-and-hold time
            Cycles192 = 7
        ],
        /// ADC Busy
        BUSY OFFSET(16) NUMBITS(1) [],
        /// ADC conversion sequence mode select
        CONSEQx OFFSET(17) NUMBITS(2) [
            /// Single channel, single conversion
            SingleChannelSingleConversion = 0,
            /// Sequence of channels
            SingleChannelSequence = 1,
            /// Repeat single channel
            RepeatSingleChannel = 2,
            /// Repeat sequence of channels
            RepeatChannelSequence = 3
        ],
        /// ADC clock source select
        SSELx OFFSET(19) NUMBITS(3) [
            /// MODCLK
            MODCLK = 0,
            /// SYSCLK
            SYSCLK = 1,
            /// ACLK
            ACLK = 2,
            /// MCLK
            MCLK = 3,
            /// SMCLK
            SMCLK = 4,
            /// HSMCLK
            HSMCLK = 5
        ],
        /// ADC clock divider
        DIVx OFFSET(22) NUMBITS(3) [
            /// Divide clock by 1
            DivideBy1 = 0,
            /// Divide clock by 2
            DivideBy2 = 1,
            /// Divide clock by 3
            DivideBy3 = 2,
            /// Divide clock by 4
            DivideBy4 = 3,
            /// Divide clock by 5
            DivideBy5 = 4,
            /// Divide clock by 6
            DivideBy6 = 5,
            /// Divide clock by 7
            DivideBy7 = 6,
            /// Divide clock by 8
            DivideBy8 = 7
        ],
        /// ADC invert signal sample-and-hold
        ISSH OFFSET(25) NUMBITS(1) [],
        /// ADC sample-and-hold pulse-mode select
        SHP OFFSET(26) NUMBITS(1) [],
        /// ADC sample-and-hold souce select
        SHSx OFFSET(27) NUMBITS(3) [
            /// ADC14SC bit
            SCBit = 0,
            /// Source 1, see device-specific datasheet
            Source1 = 1,
            /// Source 2, see device-specific datasheet
            Source2 = 2,
            /// Source 3, see device-specific datasheet
            Source3 = 3,
            /// Source 4, see device-specific datasheet
            Source4 = 4,
            /// Source 5, see device-specific datasheet
            Source5 = 5,
            /// Source 6, see device-specific datasheet
            Source6 = 6,
            /// Source 7, see device-specific datasheet
            Source7 = 7
        ],
        /// ADC pre-divider
        PDIV OFFSET(30) NUMBITS(2) [
            /// Pre-divde by 1
            PreDivideBy1 = 0,
            /// Pre-divde by 4
            PreDivideBy4 = 1,
            /// Pre-divde by 32
            PreDivideBy32 = 2,
            /// Pre-divde by 64
            PreDivideBy64 = 3
        ]
    ],
    /// ADC control 1 register
    CTL1 [
        /// ADC power modes
        PWRMD OFFSET(0) NUMBITS(2) [
            /// Regular power mode with any resolution setting. Sample-rate up to 1Msps.
            Regular = 0,
            /// Low-power mode for 12-, 10-, and 8-bit resolutions. Sample-rate up to 200ksps.
            LowPower = 1
        ],
        /// ADC reference buffer burst
        REFBURST OFFSET(2) NUMBITS(1) [
            /// ADC reference buffer on continuously
            Continuously = 0,
            /// ADC reference buffer on only during sample-and-conversion
            DuringSampleAndConversion = 1
        ],
        /// ADC data read-back format. Data is always stored in the binary unsigned format.
        DF OFFSET(3) NUMBITS(1) [
            /// Binary unsigned, at 14bit: -Vref = 0, +Vref = 0x3FFF
            Unsigned = 0,
            /// Binary signed, at 14bit: -Vref = 0x8000, +Vref = 0x7FFC
            Signed = 1
        ],
        /// ADC resolution
        RES OFFSET(4) NUMBITS(2) [
            /// 8bit (9 clock cycles conversion time)
            Resolution8Bit = 0,
            /// 10bit (11 clock cycles conversion time)
            Resolution11Bit = 1,
            /// 12bit (14 clock cycles conversion time)
            Resolution14Bit = 2,
            /// 14bit (16 clock cycles conversion time)
            Resolution16Bit = 3
        ],
        /// ADC conversion start address, select ADC14MEM0 to ADC14MEM31
        STARTADDx OFFSET(16) NUMBITS(5) [],
        /// Controls 1/2 AVCC ADC input channel selection
        BATMAP OFFSET(22) NUMBITS(1) [
            /// ADC internal 1/2 x AVCC channel is not selected for ADC
            NotSelected = 0,
            /// ADC internal 1/2 x AVCC channel is selected for ADC input channel MAX
            Selected = 1
        ],
        /// Controls temperature sensor ADC input channel selection
        TCMAP OFFSET(23) NUMBITS(1) [
            /// ADC internal temperature sensor is not selected
            NotSelected = 0,
            /// ADC internal temperature sensor is selected
            Selected = 1
        ],
        /// Controls internal channel 0 selection to ADC input channel MAX - 2
        CH0MAP OFFSET(24) NUMBITS(1) [],
        /// Controls internal channel 1 selection to ADC input channel MAX - 3
        CH1MAP OFFSET(25) NUMBITS(1) [],
        /// Controls internal channel 2 selection to ADC input channel MAX - 4
        CH2MAP OFFSET(26) NUMBITS(1) [],
        /// Controls internal channel 3 selection to ADC input channel MAX - 5
        CH3MAP OFFSET(27) NUMBITS(1) []
    ],
    /// ADC conversion memory control x register
    MCTLx [
        /// Input channel select. If even channels are set as differential then odd channel configuration is ignored.
        INCHx OFFSET(0) NUMBITS(5) [
            ///  If ADC14DIF = 0: A0; If ADC14DIF = 1: Ain+ = A0, Ain- = A1
            A0A1Even = 0,
            /// If ADC14DIF = 0: A1; If ADC14DIF = 1: Ain+ = A0, Ain- = A1
            A0A1Odd = 1,
            /// If ADC14DIF = 0: A2; If ADC14DIF = 1: Ain+ = A2, Ain- = A3
            A2A3Even = 2,
            /// If ADC14DIF = 0: A3; If ADC14DIF = 1: Ain+ = A2, Ain- = A3
            A2A3Odd = 3,
            /// If ADC14DIF = 0: A4; If ADC14DIF = 1: Ain+ = A4, Ain- = A5
            A4A5Even = 4,
            /// If ADC14DIF = 0: A5; If ADC14DIF = 1: Ain+ = A4, Ain- = A5
            A4A5Odd = 5,
            /// If ADC14DIF = 0: A6; If ADC14DIF = 1: Ain+ = A6, Ain- = A7
            A6A7Even = 6,
            /// If ADC14DIF = 0: A7; If ADC14DIF = 1: Ain+ = A6, Ain- = A7
            A6A7Odd = 7,
            /// If ADC14DIF = 0: A8; If ADC14DIF = 1: Ain+ = A8, Ain- = A9
            A8A9Even = 8,
            /// If ADC14DIF = 0: A9; If ADC14DIF = 1: Ain+ = A8, Ain- = A9
            A8A9Odd = 9,
            /// If ADC14DIF = 0: A10; If ADC14DIF = 1: Ain+ = A10, Ain- = A11
            A10A11Even = 10,
            /// If ADC14DIF = 0: A11; If ADC14DIF = 1: Ain+ = A10, Ain- = A11
            A10A11Odd = 11,
            /// If ADC14DIF = 0: A12; If ADC14DIF = 1: Ain+ = A12, Ain- = A13
            A12A13Even = 12,
            /// If ADC14DIF = 0: A13; If ADC14DIF = 1: Ain+ = A12, Ain- = A13
            A12A13Odd = 13,
            /// If ADC14DIF = 0: A14; If ADC14DIF = 1: Ain+ = A14, Ain- = A15
            A14A15Even = 14,
            /// If ADC14DIF = 0: A15; If ADC14DIF = 1: Ain+ = A14, Ain- = A15
            A14A15Odd = 15,
            /// If ADC14DIF = 0: A16; If ADC14DIF = 1: Ain+ = A16, Ain- = A17
            A16A17Even = 16,
            /// If ADC14DIF = 0: A17; If ADC14DIF = 1: Ain+ = A16, Ain- = A17
            A16A17Odd = 17,
            /// If ADC14DIF = 0: A18; If ADC14DIF = 1: Ain+ = A18, Ain- = A19
            A18A19Even = 18,
            /// If ADC14DIF = 0: A19; If ADC14DIF = 1: Ain+ = A18, Ain- = A19
            A18A19Odd = 19,
            /// If ADC14DIF = 0: A20; If ADC14DIF = 1: Ain+ = A20, Ain- = A21
            A20A21Even = 20,
            /// If ADC14DIF = 0: A21; If ADC14DIF = 1: Ain+ = A20, Ain- = A21
            A20A21Odd = 21,
            /// If ADC14DIF = 0: A22; If ADC14DIF = 1: Ain+ = A22, Ain- = A23
            A22A23Even = 22,
            /// If ADC14DIF = 0: A23; If ADC14DIF = 1: Ain+ = A22, Ain- = A23
            A22A23Odd = 23,
            /// If ADC14DIF = 0: A24; If ADC14DIF = 1: Ain+ = A24, Ain- = A25
            A24A25Even = 24,
            /// If ADC14DIF = 0: A25; If ADC14DIF = 1: Ain+ = A24, Ain- = A25
            A24A25Odd = 25,
            /// If ADC14DIF = 0: A26; If ADC14DIF = 1: Ain+ = A26, Ain- = A27
            A26A27Even = 26,
            /// If ADC14DIF = 0: A27; If ADC14DIF = 1: Ain+ = A26, Ain- = A27
            A26A27Odd = 27,
            /// If ADC14DIF = 0: A28; If ADC14DIF = 1: Ain+ = A28, Ain- = A29
            A28A29Even = 28,
            /// If ADC14DIF = 0: A29; If ADC14DIF = 1: Ain+ = A28, Ain- = A29
            A28A29Odd = 29,
            /// If ADC14DIF = 0: A30; If ADC14DIF = 1: Ain+ = A30, Ain- = A31
            A30A31Even = 30,
            /// If ADC14DIF = 0: A31; If ADC14DIF = 1: Ain+ = A30, Ain- = A31
            A30A31Odd = 31
        ],
        /// End of sequence. Indicates the last conversion in a sequence.
        EOS OFFSET(7) NUMBITS(1) [],
        /// Selects combinations of +Vref and -Vref sources as well as the buffer selection and buffer on or off.
        VRSEL OFFSET(8) NUMBITS(4) [
            /// +Vref = AVCC, -Vref = AVSS
            AvccAvss = 0,
            /// +Vref = VREF buffered, -Vref = AVSS
            VRefBufferedAvss = 1,
            /// +Vref = VeREF+, -Vref = VeRE-
            VeRef = 14,
            /// +Vref = VeREF+ buffered, -Vref = VeREF-
            VeRefBuffered = 15
        ],
        /// Differential mode
        DIF OFFSET(13) NUMBITS(1) [
            /// Single-ended mode enabled
            SingleEnded = 0,
            /// Differential mode enabled
            Differential = 1
        ],
        /// Comparator window enable
        WINC OFFSET(14) NUMBITS(1) [],
        /// Window comparator threshold register selection
        WINCTH OFFSET(15) NUMBITS(1) [
            /// Use window comparator thresholds 0, ADC14LO0 and ADC14HI0
            Threshold0 = 0,
            /// Use window comparator thresholds 1, ADC14LO1 and ADC14HI1
            Threshold1 = 1
        ]
    ],
    /// ADC interrupt enable 1 register
    IER1 [
        /// Interrupt enable for the ADC14MEMx result register being greater than the ADC14LO
        /// threshold and below the ADC14HI threshold
        INIE OFFSET(1) NUMBITS(1) [],
        /// Interrupt enable for the falling short of the lower limit interrupt of the window
        /// comparator for the ADC14MEMx result registers.
        LOIE OFFSET(2) NUMBITS(1) [],
        /// Interrupt enable for the exceeding the upper limit interrupt of the window
        /// comparator for ADC14MEMx result register.
        HIIE OFFSET(3) NUMBITS(1) [],
        /// ADC14MEMx overflow interrupt enable
        OVIE OFFSET(4) NUMBITS(1) [],
        /// ADC14 conversion-time-overflow interrupt enable
        TOVIE OFFSET(5) NUMBITS(1) [],
        /// ADC14 local buffered reference ready interrupt enable
        RDYIE OFFSET(6) NUMBITS(1) []
    ],
    /// ADC interrupt flag 1 register
    IFGR1 [
        /// Interrupt flag for the ADC14MEMx result register being greater than the ADC14LO
        /// threshold and below the ADC14HI threshold
        INIFG OFFSET(1) NUMBITS(1) [],
        /// Interrupt flag for the falling short of the lower limit interrupt of the window
        /// comparator for the ADC14MEMx result registers.
        LOIFG OFFSET(2) NUMBITS(1) [],
        /// Interrupt flag for the exceeding the upper limit interrupt of the window
        /// comparator for ADC14MEMx result register.
        HIIFG OFFSET(3) NUMBITS(1) [],
        /// ADC14MEMx overflow interrupt flag
        OVIFG OFFSET(4) NUMBITS(1) [],
        /// ADC14 conversion-time-overflow interrupt flag
        TOVIFG OFFSET(5) NUMBITS(1) [],
        /// ADC14 local buffered reference ready interrupt flag
        RDYIFG OFFSET(6) NUMBITS(1) []
    ],
    /// ADC clear interrupt flag 1 register
    CLRIFGR1 [
        /// Clear INIFG
        CLRINIFG OFFSET(1) NUMBITS(1) [],
        /// Clear LOIFG
        CLRLOIFG OFFSET(2) NUMBITS(1) [],
        /// Clear HIIFG
        CLRHIIFG OFFSET(3) NUMBITS(1) [],
        /// Clear OVIFG
        CLROVIFG OFFSET(4) NUMBITS(1) [],
        /// Clear TOIFG
        CLRTOVIFG OFFSET(5) NUMBITS(1) [],
        /// Clear RDYIFG
        CLRRDYIFG OFFSET(6) NUMBITS(1) []
    ],
    /// ADC interrupt vector register
    IV [
        /// ADC interrupt vector value
        IVx OFFSET(0) NUMBITS(32) [
            /// No interrupt pending
            NoInterrupt = 0x00,
            /// ADC14MEMx overflow, highest priority
            MemOverflow = 0x02,
            /// Conversion time overflow
            ConversionTimeOverflow = 0x04,
            /// ADC window high interrupt flag
            WindowHigh = 0x06,
            /// ADC window low interrupt flag
            WindowLow = 0x08,
            /// ADC in-window interrupt flag
            WindowIn = 0x0A,
            /// MEM0 interrupt flag
            Mem0 = 0x0C,
            /// MEM1 interrupt flag
            Mem1 = 0x0E,
            /// MEM2 interrupt flag
            Mem2 = 0x10,
            /// MEM3 interrupt flag
            Mem3 = 0x12,
            /// MEM4 interrupt flag
            Mem4 = 0x14,
            /// MEM5 interrupt flag
            Mem5 = 0x16,
            /// MEM6 interrupt flag
            Mem6 = 0x18,
            /// MEM7 interrupt flag
            Mem7 = 0x1A,
            /// MEM8 interrupt flag
            Mem8 = 0x1C,
            /// MEM9 interrupt flag
            Mem9 = 0x1E,
            /// MEM10 interrupt flag
            Mem10 = 0x20,
            /// MEM11 interrupt flag
            Mem11 = 0x22,
            /// MEM12 interrupt flag
            Mem12 = 0x24,
            /// MEM13 interrupt flag
            Mem13 = 0x26,
            /// MEM14 interrupt flag
            Mem14 = 0x28,
            /// MEM15 interrupt flag
            Mem15 = 0x2A,
            /// MEM16 interrupt flag
            Mem16 = 0x2C,
            /// MEM17 interrupt flag
            Mem17 = 0x2E,
            /// MEM18 interrupt flag
            Mem18 = 0x30,
            /// MEM19 interrupt flag
            Mem19 = 0x32,
            /// MEM20 interrupt flag
            Mem20 = 0x34,
            /// MEM21 interrupt flag
            Mem21 = 0x36,
            /// MEM22 interrupt flag
            Mem22 = 0x38,
            /// MEM23 interrupt flag
            Mem23 = 0x3A,
            /// MEM24 interrupt flag
            Mem24 = 0x3C,
            /// MEM25 interrupt flag
            Mem25 = 0x3E,
            /// MEM26 interrupt flag
            Mem26 = 0x40,
            /// MEM27 interrupt flag
            Mem27 = 0x42,
            /// MEM28 interrupt flag
            Mem28 = 0x44,
            /// MEM29 interrupt flag
            Mem29 = 0x46,
            /// MEM30 interrupt flag
            Mem30 = 0x48,
            /// MEM31 interrupt flag
            Mem31 = 0x4A,
            /// RDYIFG interrupt flag
            Ready = 0x4C
        ]
    ]
];

pub struct Adc<'a> {
    registers: StaticRef<AdcRegisters>,
    resolution: AdcResolution,
    mode: Cell<AdcMode>,
    active_channel: Cell<Channel>,
    ref_module: OptionalCell<&'a dyn ref_module::AnalogReference>,
    timer: OptionalCell<&'a dyn timer::InternalTimer>,
    dma: OptionalCell<&'a dma::DmaChannel<'a>>,
    pub(crate) dma_chan: usize,
    dma_src: u8,
    buffer1: TakeCell<'static, [u16]>,
    buffer2: TakeCell<'static, [u16]>,
    client: OptionalCell<&'a dyn hil::adc::Client>,
    highspeed_client: OptionalCell<&'a dyn hil::adc::HighSpeedClient>,
}

impl Adc<'_> {
    pub fn new() -> Self {
        Self {
            registers: ADC_BASE,
            resolution: DEFAULT_ADC_RESOLUTION,
            mode: Cell::new(AdcMode::Disabled),
            active_channel: Cell::new(Channel::Channel0),
            ref_module: OptionalCell::empty(),
            timer: OptionalCell::empty(), // must be TIMER_A3!
            dma: OptionalCell::empty(),
            dma_chan: 7,
            dma_src: 7,
            buffer1: TakeCell::empty(),
            buffer2: TakeCell::empty(),
            client: OptionalCell::empty(),
            highspeed_client: OptionalCell::empty(),
        }
    }
}

#[repr(u32)]
#[derive(Copy, Clone, PartialEq)]
pub enum Channel {
    Channel0 = 0,
    Channel1 = 1,
    Channel2 = 2,
    Channel3 = 3,
    Channel4 = 4,
    Channel5 = 5,
    Channel6 = 6,
    Channel7 = 7,
    Channel8 = 8,
    Channel9 = 9,
    Channel10 = 10,
    Channel11 = 11,
    Channel12 = 12,
    Channel13 = 13,
    Channel14 = 14,
    Channel15 = 15,
    Channel16 = 16,
    Channel17 = 17,
    Channel18 = 18,
    Channel19 = 19,
    Channel20 = 20,
    Channel21 = 21,
    Channel22 = 22,
    Channel23 = 23,
}

#[allow(dead_code)]
#[repr(u32)]
#[derive(Copy, Clone, PartialEq)]
enum AdcResolution {
    Bits8 = 0,
    Bits10 = 1,
    Bits12 = 2,
    Bits14 = 3,
}

#[allow(dead_code)]
#[derive(Copy, Clone, PartialEq)]
enum AdcMode {
    Single,
    Repeated,
    Highspeed,
    Disabled,
}

/// This function converts a `&'static mut [u8]` slice reference to a
/// `&'static mut [u16]` slice.
///
/// ## Safety
///
/// It is a necessary condition for the passed buffer to have an even
/// length, otherwise the function will panic because the conversion
/// from `u8` to `u16` would end up in undefined behavior if the last
/// word of the buffer converted is accessed!
///
/// Furthermore, this function assumes that the passed buffer is well
/// aligned, to `core::mem::align_of::<u16>()`. Improper alignment of
/// the passed buffer will result in undefined behavior.
///
/// This function is necessary since the DMA returns only `u8`-buffers
/// whereas the ADC-traits only work with `u16`-buffers.
unsafe fn buf_u8_to_buf_u16(buf: &'static mut [u8]) -> &'static mut [u16] {
    if (buf.len() % 2) > 0 {
        panic!("ADC: cannot convert an u8 array with an odd length to an u16 array");
    }

    let buf_ptr = mem::transmute::<*mut u8, *mut u16>(buf.as_mut_ptr());
    slice::from_raw_parts_mut(buf_ptr, buf.len() / 2)
}

/// This function converts a `&'static mut [u16]` slice reference to a
/// `&'static mut [u8]` slice reference.
///
/// Since the DMA only accepts only `u8`-buffers and the ADC-traits
/// use `u16`-buffers, they have to be converted.
unsafe fn buf_u16_to_buf_u8(buf: &'static mut [u16]) -> &'static mut [u8] {
    let buf_ptr = mem::transmute::<*mut u16, *mut u8>(buf.as_mut_ptr());
    slice::from_raw_parts_mut(buf_ptr, buf.len() * 2)
}

impl<'a> Adc<'a> {
    fn is_enabled(&self) -> bool {
        self.registers.ctl0.is_set(CTL0::ON)
    }

    fn stop(&self) {
        // This is the recommended way to stop a conversation in any mode.
        // See datasheet p. 855 section 22.2.8.6.
        self.registers
            .ctl0
            .modify(CTL0::ENC::CLEAR + CTL0::CONSEQx::SingleChannelSequence);

        // Disable all interrupts
        self.registers.ie0.set(0);

        // Clear all pending interrupts
        self.registers.clrifg0.set(u32::MAX);
        self.registers.clrifg1.set(u32::MAX);
    }

    fn setup(&self) {
        self.stop();

        for i in 0..AVAILABLE_ADC_CHANNELS {
            // Set the input for the channel
            // Set Reference voltage to Internal AVCC for Vref+ and AVSS (GND) for Vref-
            // Configure the channel for single-ended mode
            // Disable comparator window
            self.registers.mctl[i].modify(
                MCTLx::INCHx.val(i as u32)
                    + MCTLx::VRSEL::AvccAvss
                    + MCTLx::DIF::SingleEnded
                    + MCTLx::WINC::CLEAR,
            );
        }

        // Set predivider of the ADC-clock to 1
        // Set divider of the ADC-clock to 1
        // Set ADC-clock source to HSMCLK
        // Set the sample-and-hold time to 4 clock-cyles for channel 0-7 and 24-31
        // Set the sample-and-hold time to 4 clock-cyles for channel 8-23
        self.registers.ctl0.modify(
            CTL0::PDIV::PreDivideBy1
                + CTL0::DIVx::DivideBy1
                + CTL0::SSELx::HSMCLK
                + CTL0::SHTOx::Cycles4
                + CTL0::SHT1x::Cycles4,
        );

        // Enable the battery monitor on channel 23 (measures 1/2 * AVCC)
        // Enable the internal temperature sensor on channel 22
        // Set the ADC resolution
        self.registers.ctl1.modify(
            CTL1::BATMAP::Selected + CTL1::TCMAP::Selected + CTL1::RES.val(self.resolution as u32),
        );

        let dma_conf = dma::DmaConfig {
            src_chan: self.dma_src,
            mode: dma::DmaMode::PingPong,
            width: dma::DmaDataWidth::Width16Bit,
            src_incr: dma::DmaPtrIncrement::NoIncr,
            dst_incr: dma::DmaPtrIncrement::Incr16Bit,
        };

        // Setup DMA
        self.dma.map(|dma| dma.initialize(&dma_conf));

        // Enable ADC
        self.registers.ctl0.modify(CTL0::ON::SET);
    }

    fn get_sample(&self, chan: Channel) -> u16 {
        // calculate the number of shifts which are necessary to align the sample to u16
        let shift = 8 - 2 * (self.resolution as usize);

        // Align the sample
        (self.registers.mem[chan as usize].get() << shift) as u16
    }

    fn enable_interrupt(&self, chan: Channel) {
        self.registers
            .ie0
            .set(self.registers.ie0.get() | (1 << (chan as u32)));
    }

    fn disable_interrupt(&self, chan: Channel) {
        self.registers
            .ie0
            .set(self.registers.ie0.get() & !(1 << (chan as u32)));
    }

    pub fn set_modules(
        &self,
        ref_module: &'a dyn ref_module::AnalogReference,
        timer: &'a dyn timer::InternalTimer,
        dma: &'a dma::DmaChannel<'a>,
    ) {
        self.ref_module.set(ref_module);
        self.timer.set(timer);
        self.dma.set(dma);
    }

    pub fn handle_interrupt(&self) {
        let chan = self.active_channel.get();
        let chan_nr = chan as usize;
        let int_bit = 1 << (chan as u32);
        let mode = self.mode.get();

        if (self.registers.ifg0.get() & int_bit) > 0 {
            // Clear interrupt flag
            self.registers.clrifg0.set(int_bit);

            if mode == AdcMode::Single {
                self.mode.set(AdcMode::Disabled);

                self.disable_interrupt(chan);

                // Stop sampling
                self.registers.ctl0.modify(CTL0::ENC::CLEAR);
            }

            // Throw callback
            self.client.map(|client| {
                client.sample_ready(self.get_sample(chan));
            });
        } else {
            panic!("ADC: unhandled interrupt: channel {}", chan_nr);
        }
    }
}

impl<'a> dma::DmaClient for Adc<'a> {
    fn transfer_done(
        &self,
        _tx_buf: Option<&'static mut [u8]>,
        rx_buf: Option<&'static mut [u8]>,
        transmitted_bytes: usize,
    ) {
        if let Some(buffer) = rx_buf {
            // Cast the [u8] buffer back to [u16]
            let buf = unsafe { buf_u8_to_buf_u16(buffer) };

            // Align the received data to 16bit
            let samples = transmitted_bytes / 2;
            let shift = 8 - 2 * (self.resolution as usize);
            for i in 0..samples {
                buf[i] <<= shift;
            }

            self.highspeed_client.map(|client| {
                client.samples_ready(buf, samples);
            });
        }
    }
}

impl<'a> hil::adc::Adc<'a> for Adc<'a> {
    type Channel = Channel;

    fn sample(&self, channel: &Self::Channel) -> Result<(), ErrorCode> {
        if !self.is_enabled() {
            self.setup();
        }

        if self.mode.get() != AdcMode::Disabled {
            return Err(ErrorCode::BUSY);
        }

        self.mode.set(AdcMode::Repeated);
        self.active_channel.set(*channel);

        // Set the channel-number where to start sampling
        self.registers
            .ctl1
            .modify(CTL1::STARTADDx.val(*channel as u32));

        self.enable_interrupt(*channel);

        // Set ADC to mode where a single channel gets sampled once
        // Set the sample-and-hold source select to software-based
        // Set the sampling-timer for generating the sample-period
        // Enable conversation
        // Start conversation
        self.registers.ctl0.modify(
            CTL0::CONSEQx::SingleChannelSingleConversion
                + CTL0::SHSx::SCBit
                + CTL0::SHP::SET
                + CTL0::ENC::SET
                + CTL0::SC::SET,
        );

        Ok(())
    }

    fn sample_continuous(&self, channel: &Self::Channel, frequency: u32) -> Result<(), ErrorCode> {
        if !self.is_enabled() {
            self.setup();
        }

        if self.mode.get() != AdcMode::Disabled {
            return Err(ErrorCode::BUSY);
        }

        if frequency == 0 || frequency > 5000 {
            // Limit the frequency to 5kHz since the overhead of the callback and the interrupt
            // handler is too big otherwise and the timing starts becoming incorrect
            return Err(ErrorCode::INVAL);
        }

        self.mode.set(AdcMode::Repeated);
        self.active_channel.set(*channel);

        // Setup timer
        self.timer
            .map(|timer| timer.start(frequency, timer::InternalTrigger::CaptureCompare1));

        // Set the channel-number where to start sampling
        self.registers
            .ctl1
            .modify(CTL1::STARTADDx.val(*channel as u32));

        self.enable_interrupt(*channel);

        // Set ADC to mode where a single channel gets sampled continuously
        // Set the sample-and-hold source select to timer-triggered
        // Use TIMER_A3 to generate the SAMPCON signal
        // Enable multiple sample and conversions
        // Enable conversation
        self.registers.ctl0.modify(
            CTL0::CONSEQx::RepeatSingleChannel
                + CTL0::SHSx::Source7
                + CTL0::SHP::CLEAR
                + CTL0::MSC::SET
                + CTL0::ENC::SET,
        );

        Ok(())
    }

    fn stop_sampling(&self) -> Result<(), ErrorCode> {
        let mode = self.mode.get();

        if mode == AdcMode::Disabled {
            return Err(ErrorCode::OFF);
        }

        self.timer.map(|timer| timer.stop());
        self.stop();
        if mode == AdcMode::Highspeed {
            self.dma.map(|dma| {
                let (_nr_bytes, _tx1, rx1, _tx2, rx2) = dma.stop();

                if let Some(buffer) = rx1 {
                    let buf = unsafe { buf_u8_to_buf_u16(buffer) };
                    self.buffer1.replace(buf);
                }

                if let Some(buffer) = rx2 {
                    let buf = unsafe { buf_u8_to_buf_u16(buffer) };
                    self.buffer2.replace(buf);
                }
            });
        }

        self.mode.set(AdcMode::Disabled);
        Ok(())
    }

    fn get_resolution_bits(&self) -> usize {
        match self.resolution {
            AdcResolution::Bits8 => 8,
            AdcResolution::Bits10 => 10,
            AdcResolution::Bits12 => 12,
            AdcResolution::Bits14 => 14,
        }
    }

    fn get_voltage_reference_mv(&self) -> Option<usize> {
        self.ref_module.map(|ref_mod| ref_mod.ref_voltage_mv())
    }

    fn set_client(&self, client: &'a dyn hil::adc::Client) {
        self.client.set(client);
    }
}

impl<'a> hil::adc::AdcHighSpeed<'a> for Adc<'a> {
    fn sample_highspeed(
        &self,
        channel: &Self::Channel,
        frequency: u32,
        buffer1: &'static mut [u16],
        length1: usize,
        buffer2: &'static mut [u16],
        length2: usize,
    ) -> Result<(), (ErrorCode, &'static mut [u16], &'static mut [u16])> {
        if !self.is_enabled() {
            self.setup();
        }
        if self.mode.get() != AdcMode::Disabled {
            return Err((ErrorCode::BUSY, buffer1, buffer2));
        }
        if frequency == 0 || frequency > MAX_SAMPLE_FREQ_HZ {
            return Err((ErrorCode::INVAL, buffer1, buffer2));
        }
        if length1 == 0 {
            return Err((ErrorCode::INVAL, buffer1, buffer2));
        }

        self.mode.set(AdcMode::Highspeed);
        self.active_channel.set(*channel);

        // Set the channel-number where to start sampling
        self.registers
            .ctl1
            .modify(CTL1::STARTADDx.val(*channel as u32));

        // Set ADC to mode where a single channel gets sampled continuously
        // Set the sample-and-hold source select to timer-triggered
        // Use TIMER_A3 to generate the SAMPCON signal
        // Enable multiple sample and conversions
        // Enable conversation
        self.registers.ctl0.modify(
            CTL0::CONSEQx::RepeatSingleChannel
                + CTL0::SHSx::Source7
                + CTL0::SHP::CLEAR
                + CTL0::MSC::SET
                + CTL0::ENC::SET,
        );

        let adc_reg =
            (core::ptr::from_ref::<ReadWrite<u32>>(&self.registers.mem[*channel as usize]))
                .cast::<()>();

        // Convert the [u16] into an [u8] since the DMA works only with [u8]
        let buf1 = unsafe { buf_u16_to_buf_u8(buffer1) };
        let buf2 = unsafe { buf_u16_to_buf_u8(buffer2) };

        // Setup the DMA transfer
        if length2 == 0 {
            // If only 1 buffer is provided, we do a simple DMA transfer without toggling
            // the buffers
            self.dma
                .map(move |dma| dma.transfer_periph_to_mem(adc_reg, buf1, length1 * 2));
        } else {
            // If two buffers are provided, we use the pingpong mode of the DMA which can toggle
            // between 2 buffers
            self.dma.map(move |dma| {
                dma.transfer_periph_to_mem_pingpong(adc_reg, buf1, length1 * 2, buf2, length2 * 2)
            });
        }

        // Setup the timer
        self.timer
            .map(|timer| timer.start(frequency, timer::InternalTrigger::CaptureCompare1));

        Ok(())
    }

    fn provide_buffer(
        &self,
        buffer: &'static mut [u16],
        length: usize,
    ) -> Result<(), (ErrorCode, &'static mut [u16])> {
        if self.mode.get() != AdcMode::Highspeed {
            panic!("ADC: cannot provide buffers in a different mode than Highspeed!");
        }

        let buf = unsafe { buf_u16_to_buf_u8(buffer) };
        self.dma
            .map(move |dma| dma.provide_new_buffer(buf, length * 2));
        Ok(())
    }

    fn retrieve_buffers(
        &self,
    ) -> Result<(Option<&'static mut [u16]>, Option<&'static mut [u16]>), ErrorCode> {
        if self.mode.get() != AdcMode::Disabled {
            // When the device is active, the buffers cannot be returned
            Err(ErrorCode::INVAL)
        } else {
            Ok((self.buffer1.take(), self.buffer2.take()))
        }
    }

    fn set_highspeed_client(&self, client: &'a dyn hil::adc::HighSpeedClient) {
        self.highspeed_client.set(client);
    }
}
