//! UDMA support.
//!
//! Notki z TRM:
//! - każdy kanał ma 2 priorytety: normalny i wysoki
//! - `arbitration size` oznacza ile elementów zostanie wysłanych, zanim nastąpi ponowny wybór kanału
//! - `burst` vs `single` transfer: burst wysyła wiele naraz, nie da się go przerwać
//!     - w przypadku UART należy skonfigurować burst threshold (np. 1/2 * 32)  taki sam jak arbitration size (16)
//!     - da się wyłączyć `single` za pomocą UDMA:SETBURST.
//! - oprócz włączania zasilania i zegarów, trzeba włączać kontroler
//! - DMA generuje przerwania konkretnych peryferiów, dlatego należy wyłączyć
//!   ich własne źródła przerwań, jeśli używa się DMA.
//! -

use core::{ffi::c_void, marker::PhantomData, ptr::addr_of};

use crate::driverlib;

pub struct Udma {
    udma: cc2650::UDMA0,
}

impl Udma {
    pub(crate) const fn new(udma: cc2650::UDMA0) -> Self {
        Self { udma }
    }

    #[inline(never)]
    pub fn enable(&self) {
        // Set the pointer to the channel control map.
        let map_addr = unsafe { addr_of!(CHANNEL_CONTROL_MAP) } as u32;

        // `w.baseptr()` is BUGGED!!! Don't use it!
        // It performs shift left 10 bits on your argument!
        self.udma.ctrl.write(|w| unsafe { w.bits(map_addr) });

        self.udma.cfg.write(|w| w.masterenable().set_bit());
    }

    #[inline]
    pub fn disable(&self) {
        self.udma.cfg.write(|w| w.masterenable().clear_bit());
    }

    // No separate `uart_enable_{tx,rx}` functions because enabling is done
    // only in `uart_transfer_{tx,rx}`.

    #[inline]
    pub fn uart_disable_tx(&self) {
        unsafe {
            CHANNEL_CONTROL_MAP.primary_channel_2.disable(&self.udma); // TX
        }
    }

    #[inline]
    pub fn uart_disable_rx(&self) {
        unsafe {
            CHANNEL_CONTROL_MAP.primary_channel_1.disable(&self.udma); // RX
        }
    }

    #[inline]
    pub fn uart_channels_configure(&self) {
        let data_size = DataSize::Size8;
        let arbitration_size = ArbitrationSize::Arb32;

        let control_rx = ControlWord {
            data_size,
            src_addr_inc: SrcAddrIncrement::IncNone, // We are reading from UART:DR all the time
            dst_addr_inc: DstAddrIncrement::Inc8,    // We are reading bytes
            arbitration_size,
        };
        let control_tx = ControlWord {
            data_size,
            src_addr_inc: SrcAddrIncrement::Inc8, // We are writing bytes
            dst_addr_inc: DstAddrIncrement::IncNone, // We are writing to UART:DR all the time
            arbitration_size,
        };

        unsafe {
            CHANNEL_CONTROL_MAP
                .primary_channel_1 // RX
                .set_control(control_rx);
            CHANNEL_CONTROL_MAP
                .primary_channel_2 // TX
                .set_control(control_tx);
        }
    }

    #[inline]
    pub fn uart_transfer_rx(&self, mem: &mut [u8]) {
        unsafe {
            CHANNEL_CONTROL_MAP.primary_channel_1.set_transfer(
                &(*cc2650::UART0::ptr()).dr as *const cc2650::uart0::DR as *mut (),
                mem.as_mut_ptr() as *mut (),
                mem.len() as u32,
            );
            CHANNEL_CONTROL_MAP.primary_channel_1.enable(&self.udma);
        }
    }

    #[inline]
    pub fn uart_transfer_tx(&self, mem: &[u8]) {
        unsafe {
            CHANNEL_CONTROL_MAP.primary_channel_2.set_transfer(
                mem.as_ptr() as *mut (),
                &(*cc2650::UART0::ptr()).dr as *const cc2650::uart0::DR as *mut (),
                mem.len() as u32,
            );
            CHANNEL_CONTROL_MAP.primary_channel_2.enable(&self.udma);
        }
    }

    #[inline]
    pub fn uart_is_enabled_rx(&self) -> bool {
        unsafe { CHANNEL_CONTROL_MAP.primary_channel_1.is_enabled(&self.udma) }
    }

    #[inline]
    pub fn uart_is_enabled_tx(&self) -> bool {
        unsafe { CHANNEL_CONTROL_MAP.primary_channel_2.is_enabled(&self.udma) }
    }

    #[inline]
    pub fn uart_request_done_rx(&self) -> bool {
        unsafe {
            CHANNEL_CONTROL_MAP
                .primary_channel_1
                .request_done(&self.udma)
        }
    }

    #[inline]
    pub fn uart_request_done_tx(&self) -> bool {
        unsafe {
            CHANNEL_CONTROL_MAP
                .primary_channel_2
                .request_done(&self.udma)
        }
    }

    #[inline]
    pub fn uart_request_done_rx_clear(&self) {
        unsafe {
            CHANNEL_CONTROL_MAP
                .primary_channel_1
                .request_done_clear(&self.udma)
        }
    }

    #[inline]
    pub fn uart_request_done_tx_clear(&self) {
        unsafe {
            CHANNEL_CONTROL_MAP
                .primary_channel_2
                .request_done_clear(&self.udma)
        }
    }
}

mod channel_control_entry_kind {
    pub trait Sealed {}
    pub trait ChannelControlEntryKind: Sealed {}

    pub struct Primary;
    impl Sealed for Primary {}
    impl ChannelControlEntryKind for Primary {}
    pub struct Alternate;
    impl Sealed for Alternate {}
    impl ChannelControlEntryKind for Alternate {}
}
use channel_control_entry_kind::{Alternate, ChannelControlEntryKind, Primary};

pub mod control_word {
    use crate::driverlib;

    #[derive(Clone, Copy)]
    pub struct ControlWord {
        pub data_size: DataSize,
        pub src_addr_inc: SrcAddrIncrement,
        pub dst_addr_inc: DstAddrIncrement,
        pub arbitration_size: ArbitrationSize,
    }

    impl ControlWord {
        #[inline]
        pub fn as_u32(&self) -> u32 {
            self.data_size as u32
                | self.src_addr_inc as u32
                | self.dst_addr_inc as u32
                | self.arbitration_size as u32
        }
    }

    #[derive(Clone, Copy)]
    #[repr(u32)]
    pub enum DataSize {
        Size8 = driverlib::UDMA_SIZE_8,
        Size16 = driverlib::UDMA_SIZE_16,
        Size32 = driverlib::UDMA_SIZE_32,
    }

    #[derive(Clone, Copy)]
    #[repr(u32)]
    pub enum SrcAddrIncrement {
        Inc8 = driverlib::UDMA_SRC_INC_8,
        Inc16 = driverlib::UDMA_SRC_INC_16,
        Inc32 = driverlib::UDMA_SRC_INC_32,
        IncNone = driverlib::UDMA_SRC_INC_NONE,
    }

    #[derive(Clone, Copy)]
    #[repr(u32)]
    pub enum DstAddrIncrement {
        Inc8 = driverlib::UDMA_DST_INC_8,
        Inc16 = driverlib::UDMA_DST_INC_16,
        Inc32 = driverlib::UDMA_DST_INC_32,
        IncNone = driverlib::UDMA_DST_INC_NONE,
    }

    #[derive(Clone, Copy)]
    #[repr(u32)]
    pub enum ArbitrationSize {
        Arb1 = driverlib::UDMA_ARB_1,
        Arb2 = driverlib::UDMA_ARB_2,
        Arb4 = driverlib::UDMA_ARB_4,
        Arb8 = driverlib::UDMA_ARB_8,
        Arb16 = driverlib::UDMA_ARB_16,
        Arb32 = driverlib::UDMA_ARB_32,
        Arb64 = driverlib::UDMA_ARB_64,
        Arb128 = driverlib::UDMA_ARB_128,
        Arb256 = driverlib::UDMA_ARB_256,
        Arb512 = driverlib::UDMA_ARB_512,
        Arb1024 = driverlib::UDMA_ARB_1024,
    }
}
pub use control_word::ControlWord;
use control_word::{ArbitrationSize, DataSize, DstAddrIncrement, SrcAddrIncrement};

#[repr(C, align(16))]
struct ChannelControlEntry<KIND: ChannelControlEntryKind, const INDEX: u32> {
    src_end_ptr: u32,
    dest_end_ptr: u32,
    control_word: u32,
    _unused: u32,

    _phantom: PhantomData<KIND>,
}

impl<const INDEX: u32> ChannelControlEntry<Primary, INDEX> {
    fn enable(&self, udma: &cc2650::UDMA0) {
        // We do not use any of these; TRM suggests clearing them explicitly,
        // but at the same time says it's not needed.
        // unsafe {
        //     driverlib::uDMAChannelAttributeDisable(
        //         driverlib::UDMA0_BASE,
        //         INDEX,
        //         driverlib::UDMA_ATTR_USEBURST
        //             | driverlib::UDMA_ATTR_ALTSELECT
        //             | driverlib::UDMA_ATTR_HIGH_PRIORITY
        //             | driverlib::UDMA_ATTR_REQMASK,
        //     )
        // };

        udma.setchannelen
            .write(|w| unsafe { w.chnls().bits(1 << INDEX) })
    }

    fn disable(&self, udma: &cc2650::UDMA0) {
        udma.clearchannelen
            .write(|w| unsafe { w.chnls().bits(1 << INDEX) })
    }

    fn is_enabled(&self, udma: &cc2650::UDMA0) -> bool {
        udma.setchannelen.read().chnls().bits() & (1 << INDEX) != 0
    }

    fn software_request(&self, udma: &cc2650::UDMA0) {
        udma.softreq
            .write(|w| unsafe { w.chnls().bits(1 << INDEX) })
    }

    fn request_done(&self, udma: &cc2650::UDMA0) -> bool {
        udma.reqdone.read().chnls().bits() & (1 << INDEX) != 0
    }

    fn request_done_clear(&self, udma: &cc2650::UDMA0) {
        udma.reqdone
            .write(|w| unsafe { w.chnls().bits(1 << INDEX) })
    }
}

impl<KIND: ChannelControlEntryKind, const INDEX: u32> ChannelControlEntry<KIND, INDEX> {
    const fn new() -> Self {
        Self {
            src_end_ptr: 0,
            dest_end_ptr: 0,
            control_word: 0,
            _unused: 0,
            _phantom: PhantomData,
        }
    }

    fn set_control(&self, control: ControlWord) {
        unsafe { driverlib::uDMAChannelControlSet(driverlib::UDMA0_BASE, INDEX, control.as_u32()) }
    }

    fn set_transfer(&self, src: *mut (), dest: *mut (), len: u32) {
        unsafe {
            driverlib::uDMAChannelTransferSet(
                driverlib::UDMA0_BASE,
                INDEX,
                driverlib::UDMA_MODE_BASIC,
                src as *mut c_void,
                dest as *mut c_void,
                len,
            )
        }
    }
}

#[repr(C, align(1024))]
struct ChannelControlMap {
    primary_channel_0: ChannelControlEntry<Primary, 0>, // Software 0
    primary_channel_1: ChannelControlEntry<Primary, 1>, // UART0_RX
    primary_channel_2: ChannelControlEntry<Primary, 2>, // UART0_TX
    primary_channel_3: ChannelControlEntry<Primary, 3>, // SSP0_RX
    primary_channel_4: ChannelControlEntry<Primary, 4>, // SSP0_TX
    primary_channel_5: ChannelControlEntry<Primary, 5>, // Reserved
    primary_channel_6: ChannelControlEntry<Primary, 6>, // Reserved
    primary_channel_7: ChannelControlEntry<Primary, 7>, // AUX_ADC
    primary_channel_8: ChannelControlEntry<Primary, 8>, // AUX_SW
    primary_channel_9: ChannelControlEntry<Primary, 9>, // GPT0_A
    primary_channel_10: ChannelControlEntry<Primary, 10>, // GPT0_B
    primary_channel_11: ChannelControlEntry<Primary, 11>, // GPT1_A
    primary_channel_12: ChannelControlEntry<Primary, 12>, // GPT1_B
    primary_channel_13: ChannelControlEntry<Primary, 13>, // AON_PROG2
    primary_channel_14: ChannelControlEntry<Primary, 14>, // DMA_PROG
    primary_channel_15: ChannelControlEntry<Primary, 15>, // AON_RTC
    primary_channel_16: ChannelControlEntry<Primary, 16>, // SSP1_RX
    primary_channel_17: ChannelControlEntry<Primary, 17>, // SSP1_TX
    primary_channel_18: ChannelControlEntry<Primary, 18>, // Software 1
    primary_channel_19: ChannelControlEntry<Primary, 19>, // Software 2
    primary_channel_20: ChannelControlEntry<Primary, 20>, // Software 3
    primary_channel_21: ChannelControlEntry<Primary, 21>, // Reserved
    primary_channel_22: ChannelControlEntry<Primary, 22>, // Reserved
    primary_channel_23: ChannelControlEntry<Primary, 23>, // Reserved
    primary_channel_24: ChannelControlEntry<Primary, 24>, // Reserved
    primary_channel_25: ChannelControlEntry<Primary, 25>, // Reserved
    primary_channel_26: ChannelControlEntry<Primary, 26>, // Reserved
    primary_channel_27: ChannelControlEntry<Primary, 27>, // Reserved
    primary_channel_28: ChannelControlEntry<Primary, 28>, // Reserved
    primary_channel_29: ChannelControlEntry<Primary, 29>, // Reserved
    primary_channel_30: ChannelControlEntry<Primary, 30>, // Reserved
    primary_channel_31: ChannelControlEntry<Primary, 31>, // Reserved
    alternate_channel_0: ChannelControlEntry<Alternate, 32>, // Software 0
    alternate_channel_1: ChannelControlEntry<Alternate, 33>, // UART0_RX
    alternate_channel_2: ChannelControlEntry<Alternate, 34>, // UART0_TX
    alternate_channel_3: ChannelControlEntry<Alternate, 35>, // SSP0_RX
    alternate_channel_4: ChannelControlEntry<Alternate, 36>, // SSP0_TX
    alternate_channel_5: ChannelControlEntry<Alternate, 37>, // Reserved
    alternate_channel_6: ChannelControlEntry<Alternate, 38>, // Reserved
    alternate_channel_7: ChannelControlEntry<Alternate, 39>, // AUX_ADC
    alternate_channel_8: ChannelControlEntry<Alternate, 40>, // AUX_SW
    alternate_channel_9: ChannelControlEntry<Alternate, 41>, // GPT0_A
    alternate_channel_10: ChannelControlEntry<Alternate, 42>, // GPT0_B
    alternate_channel_11: ChannelControlEntry<Alternate, 43>, // GPT1_A
    alternate_channel_12: ChannelControlEntry<Alternate, 44>, // GPT1_B
    alternate_channel_13: ChannelControlEntry<Alternate, 45>, // AON_PROG2
    alternate_channel_14: ChannelControlEntry<Alternate, 46>, // DMA_PROG
    alternate_channel_15: ChannelControlEntry<Alternate, 47>, // AON_RTC
    alternate_channel_16: ChannelControlEntry<Alternate, 48>, // SSP1_RX
    alternate_channel_17: ChannelControlEntry<Alternate, 49>, // SSP1_TX
    alternate_channel_18: ChannelControlEntry<Alternate, 50>, // Software 1
    alternate_channel_19: ChannelControlEntry<Alternate, 51>, // Software 2
    alternate_channel_20: ChannelControlEntry<Alternate, 52>, // Software 3
    alternate_channel_21: ChannelControlEntry<Alternate, 53>, // Reserved
    alternate_channel_22: ChannelControlEntry<Alternate, 54>, // Reserved
    alternate_channel_23: ChannelControlEntry<Alternate, 55>, // Reserved
    alternate_channel_24: ChannelControlEntry<Alternate, 56>, // Reserved
    alternate_channel_25: ChannelControlEntry<Alternate, 57>, // Reserved
    alternate_channel_26: ChannelControlEntry<Alternate, 58>, // Reserved
    alternate_channel_27: ChannelControlEntry<Alternate, 59>, // Reserved
    alternate_channel_28: ChannelControlEntry<Alternate, 60>, // Reserved
    alternate_channel_29: ChannelControlEntry<Alternate, 61>, // Reserved
    alternate_channel_30: ChannelControlEntry<Alternate, 62>, // Reserved
    alternate_channel_31: ChannelControlEntry<Alternate, 63>, // Reserved
}

impl ChannelControlMap {}

static mut CHANNEL_CONTROL_MAP: ChannelControlMap = ChannelControlMap {
    primary_channel_0: ChannelControlEntry::new(),
    primary_channel_1: ChannelControlEntry::new(),
    primary_channel_2: ChannelControlEntry::new(),
    primary_channel_3: ChannelControlEntry::new(),
    primary_channel_4: ChannelControlEntry::new(),
    primary_channel_5: ChannelControlEntry::new(),
    primary_channel_6: ChannelControlEntry::new(),
    primary_channel_7: ChannelControlEntry::new(),
    primary_channel_8: ChannelControlEntry::new(),
    primary_channel_9: ChannelControlEntry::new(),
    primary_channel_10: ChannelControlEntry::new(),
    primary_channel_11: ChannelControlEntry::new(),
    primary_channel_12: ChannelControlEntry::new(),
    primary_channel_13: ChannelControlEntry::new(),
    primary_channel_14: ChannelControlEntry::new(),
    primary_channel_15: ChannelControlEntry::new(),
    primary_channel_16: ChannelControlEntry::new(),
    primary_channel_17: ChannelControlEntry::new(),
    primary_channel_18: ChannelControlEntry::new(),
    primary_channel_19: ChannelControlEntry::new(),
    primary_channel_20: ChannelControlEntry::new(),
    primary_channel_21: ChannelControlEntry::new(),
    primary_channel_22: ChannelControlEntry::new(),
    primary_channel_23: ChannelControlEntry::new(),
    primary_channel_24: ChannelControlEntry::new(),
    primary_channel_25: ChannelControlEntry::new(),
    primary_channel_26: ChannelControlEntry::new(),
    primary_channel_27: ChannelControlEntry::new(),
    primary_channel_28: ChannelControlEntry::new(),
    primary_channel_29: ChannelControlEntry::new(),
    primary_channel_30: ChannelControlEntry::new(),
    primary_channel_31: ChannelControlEntry::new(),
    alternate_channel_0: ChannelControlEntry::new(),
    alternate_channel_1: ChannelControlEntry::new(),
    alternate_channel_2: ChannelControlEntry::new(),
    alternate_channel_3: ChannelControlEntry::new(),
    alternate_channel_4: ChannelControlEntry::new(),
    alternate_channel_5: ChannelControlEntry::new(),
    alternate_channel_6: ChannelControlEntry::new(),
    alternate_channel_7: ChannelControlEntry::new(),
    alternate_channel_8: ChannelControlEntry::new(),
    alternate_channel_9: ChannelControlEntry::new(),
    alternate_channel_10: ChannelControlEntry::new(),
    alternate_channel_11: ChannelControlEntry::new(),
    alternate_channel_12: ChannelControlEntry::new(),
    alternate_channel_13: ChannelControlEntry::new(),
    alternate_channel_14: ChannelControlEntry::new(),
    alternate_channel_15: ChannelControlEntry::new(),
    alternate_channel_16: ChannelControlEntry::new(),
    alternate_channel_17: ChannelControlEntry::new(),
    alternate_channel_18: ChannelControlEntry::new(),
    alternate_channel_19: ChannelControlEntry::new(),
    alternate_channel_20: ChannelControlEntry::new(),
    alternate_channel_21: ChannelControlEntry::new(),
    alternate_channel_22: ChannelControlEntry::new(),
    alternate_channel_23: ChannelControlEntry::new(),
    alternate_channel_24: ChannelControlEntry::new(),
    alternate_channel_25: ChannelControlEntry::new(),
    alternate_channel_26: ChannelControlEntry::new(),
    alternate_channel_27: ChannelControlEntry::new(),
    alternate_channel_28: ChannelControlEntry::new(),
    alternate_channel_29: ChannelControlEntry::new(),
    alternate_channel_30: ChannelControlEntry::new(),
    alternate_channel_31: ChannelControlEntry::new(),
};
