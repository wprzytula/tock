/// GPIO Int handler
pub(crate) const GPIO: u32 = 0;

/// I2C
pub(crate) const I2C: u32 = 1;

/// RF Core Command & Packet Engine 1
pub(crate) const RF_CPE1: u32 = 2;

// 3 is unassigned

/// AON RTC
pub(crate) const AON_RTC: u32 = 4;

/// UART0 Rx and Tx
pub(crate) const UART0: u32 = 5;

/// AUX Software Event 0
pub(crate) const AUX_SWEV0: u32 = 6;

/// SSI0 Rx and Tx
pub(crate) const SSI0: u32 = 7;

/// SSI1 Rx and Tx
pub(crate) const SSI1: u32 = 8;

/// RF Core & Packet Engine 2
pub(crate) const RF_CPE0: u32 = 9;

/// RF Core Hardware
pub(crate) const RF_CORE_HW: u32 = 10;

/// RF Core Command Acknowledge
pub(crate) const RF_CMD_ACK: u32 = 11;

/// I2S
pub(crate) const I2S: u32 = 12;

// 13 is AUX Software Event 1

/// Watchdog timer
pub(crate) const WATCHDOG: u32 = 14;

/// Timer 0 subtimer A
pub(crate) const GPT0A: u32 = 15;

/// Timer 0 subtimer B
pub(crate) const GPT0B: u32 = 16;

/// Timer 1 subtimer A
pub(crate) const GPT1A: u32 = 17;

/// Timer 1 subtimer B
pub(crate) const GPT1B: u32 = 18;

/// Timer 2 subtimer A
pub(crate) const GPT2A: u32 = 19;

/// Timer 2 subtimer B
pub(crate) const GPT2B: u32 = 20;

/// Timer 3 subtimer A
pub(crate) const GPT3A: u32 = 21;

/// Timer 3 subtimer B
pub(crate) const GPT3B: u32 = 22;

/// Crypto Core Result available
pub(crate) const CRYPTO: u32 = 23;

/// uDMA Software
pub(crate) const DMA_SD: u32 = 24;

/// uDMA Error
pub(crate) const DMA_ERROR: u32 = 25;

/// Flash controller
pub(crate) const FLASH: u32 = 26;

/// Software Event 0
pub(crate) const SW_EVENT_0: u32 = 27;

/// AUX combined event
pub(crate) const AUX_COMBINED: u32 = 28;

/// AON programmable 0
pub(crate) const AON_PROG: u32 = 29;

/// Dynamic Programmable interrupt
pub(crate) const DYNAMIC_PROG: u32 = 30;

// AUX Comparator A
pub(crate) const AUX_COMP_A: u32 = 31;

// AUX ADC new sample or ADC DMA done, ADC underflow, ADC overflow
pub(crate) const AUX_ADC: u32 = 32;

// TRNG event
pub(crate) const TRNG: u32 = 33;
