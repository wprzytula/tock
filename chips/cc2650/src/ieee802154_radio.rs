use crate::driverlib;
use crate::prcm::Clock;
use crate::prcm::Clocks;
use crate::prcm::Prcm;
use core::borrow::Borrow;
use core::cell::Cell;
use core::cell::RefCell;
use core::cell::UnsafeCell;
use cortexm3::nvic::Nvic;
use driverlib::dataQueue_t as RfcQueue;
use driverlib::rfc_dataEntryPointer_s as RfcDataEntryPointer;
use driverlib::rfc_ieeeRxOutput_s as RfcRxOutput;

use kernel::deferred_call::{DeferredCall, DeferredCallClient};
use kernel::hil::radio::{self, PowerClient, RadioChannel, RadioConfig, RadioData};
use kernel::static_init;
use kernel::utilities::cells::{OptionalCell, TakeCell};
use kernel::ErrorCode;

/**    23.2.2.3 RF Core Command Acknowledge Interrupt
 * The system-level interrupt RF_CMD_ACK is produced when an RF core command is acknowledged (that
 * is, when the status becomes available in CMDSTA [see Section 23.8.2.2]). When the status becomes
 * available, the RFACKIFG.ACKFLAG register bit is set to 1. Whenever this bit is 1, the RF_CMD_ACK
 * interrupt is raised, which means that the ISR must clear RFACKIFG.ACKFLAG when processing the
 * RF_CMD_ACK interrupt.
 */
pub(crate) unsafe extern "C" fn rfc_cmd_ack_handler() {
    let rfc_dbell = cc2650::RFC_DBELL::ptr().as_ref().unwrap_unchecked();
    rfc_dbell.rfackifg.write(|w| w.ackflag().clear_bit());
}

mod cmd {
    use super::{driverlib, RfcDataEntryPointer, RfcQueue};
    use core::cell::Cell;

    use kernel::ErrorCode;

    /* RF Radio Op status constants. Field 'status' in Radio Op command struct */
    pub(super) const RADIO_OP_STATUS_IDLE: u16 = 0x0000;
    pub(super) const RADIO_OP_STATUS_PENDING: u16 = 0x0001;
    pub(super) const RADIO_OP_STATUS_ACTIVE: u16 = 0x0002;
    pub(super) const RADIO_OP_STATUS_SKIPPED: u16 = 0x0003;
    pub(super) const RADIO_OP_STATUS_DONE_OK: u16 = 0x0400;
    pub(super) const RADIO_OP_STATUS_DONE_COUNTDOWN: u16 = 0x0401;
    pub(super) const RADIO_OP_STATUS_DONE_RXERR: u16 = 0x0402;
    pub(super) const RADIO_OP_STATUS_DONE_TIMEOUT: u16 = 0x0403;
    pub(super) const RADIO_OP_STATUS_DONE_STOPPED: u16 = 0x0404;
    pub(super) const RADIO_OP_STATUS_DONE_ABORT: u16 = 0x0405;
    pub(super) const RADIO_OP_STATUS_ERROR_PAST_START: u16 = 0x0800;
    pub(super) const RADIO_OP_STATUS_ERROR_START_TRIG: u16 = 0x0801;
    pub(super) const RADIO_OP_STATUS_ERROR_CONDITION: u16 = 0x0802;
    pub(super) const RADIO_OP_STATUS_ERROR_PAR: u16 = 0x0803;
    pub(super) const RADIO_OP_STATUS_ERROR_POINTER: u16 = 0x0804;
    pub(super) const RADIO_OP_STATUS_ERROR_CMDID: u16 = 0x0805;
    pub(super) const RADIO_OP_STATUS_ERROR_NO_SETUP: u16 = 0x0807;
    pub(super) const RADIO_OP_STATUS_ERROR_NO_FS: u16 = 0x0808;
    pub(super) const RADIO_OP_STATUS_ERROR_SYNTH_PROG: u16 = 0x0809;

    /* Additional Op status values for IEEE mode */
    pub(super) const RADIO_OP_STATUS_IEEE_SUSPENDED: u16 = 0x2001;
    pub(super) const RADIO_OP_STATUS_IEEE_DONE_OK: u16 = 0x2400;
    pub(super) const RADIO_OP_STATUS_IEEE_DONE_BUSY: u16 = 0x2401;
    pub(super) const RADIO_OP_STATUS_IEEE_DONE_STOPPED: u16 = 0x2402;
    pub(super) const RADIO_OP_STATUS_IEEE_DONE_ACK: u16 = 0x2403;
    pub(super) const RADIO_OP_STATUS_IEEE_DONE_ACKPEND: u16 = 0x2404;
    pub(super) const RADIO_OP_STATUS_IEEE_DONE_TIMEOUT: u16 = 0x2405;
    pub(super) const RADIO_OP_STATUS_IEEE_DONE_BGEND: u16 = 0x2406;
    pub(super) const RADIO_OP_STATUS_IEEE_DONE_ABORT: u16 = 0x2407;
    pub(super) const RADIO_OP_STATUS_ERROR_WRONG_BG: u16 = 0x0806;
    pub(super) const RADIO_OP_STATUS_IEEE_ERROR_PAR: u16 = 0x2800;
    pub(super) const RADIO_OP_STATUS_IEEE_ERROR_NO_SETUP: u16 = 0x2801;
    pub(super) const RADIO_OP_STATUS_IEEE_ERROR_NO_FS: u16 = 0x2802;
    pub(super) const RADIO_OP_STATUS_IEEE_ERROR_SYNTH_PROG: u16 = 0x2803;
    pub(super) const RADIO_OP_STATUS_IEEE_ERROR_RXOVF: u16 = 0x2804;
    pub(super) const RADIO_OP_STATUS_IEEE_ERROR_TXUNF: u16 = 0x2805;

    #[must_use]
    #[allow(unused)]
    #[repr(u32)]
    #[derive(Debug)]
    pub(super) enum RadioCmdStatus {
        /// The command has not been parsed.
        Pending = 0x00,
        /// Immediate command: The command finished successfully.
        /// Radio operation command: The command was successfully submitted for execution.
        Done = 0x01,
        /// The pointer signaled in CMDR is not valid.
        IllegalPointer = 0x81,
        /// The command ID number in the command structure is unknown.
        UnknownCommand = 0x82,
        /// The command number for a direct command is unknown, or the command is not a direct command.
        UnknownDirCommand = 0x83,
        /// An immediate or direct command was issued in a context where it is not supported.
        ContextError = 0x85,
        /// A radio operation command was attempted to be scheduled while another operation
        /// was already running in the RF core. The new command is rejected, while the command
        /// already running is not impacted.
        SchedulingError = 0x86,
        /// There were errors in the command parameters that are parsed on submission.
        /// For radio operation commands,  errors in parameters parsed after start
        /// of the command are signaled by the command ending, and an error is indicated
        /// in the status field of that command structure.
        ParError = 0x87,
        /// An operation on a data entry queue was attempted, but the operation was not supported
        ///  by the queue in its current state.
        QueueError = 0x88,
        /// An operation on a data entry was attempted while that entry was busy.
        QueueBusy = 0x89,
    }

    impl From<RadioCmdStatus> for ErrorCode {
        fn from(value: RadioCmdStatus) -> Self {
            match value {
                RadioCmdStatus::Pending | RadioCmdStatus::Done => unreachable!(),
                RadioCmdStatus::IllegalPointer
                | RadioCmdStatus::UnknownCommand
                | RadioCmdStatus::UnknownDirCommand
                | RadioCmdStatus::ContextError
                | RadioCmdStatus::SchedulingError
                | RadioCmdStatus::ParError
                | RadioCmdStatus::QueueError
                | RadioCmdStatus::QueueBusy => ErrorCode::FAIL,
            }
        }
    }

    pub(super) type RadioCmdResult<T> = Result<T, RadioCmdStatus>;

    #[derive(Debug, Clone, Copy)]
    #[repr(u16)]
    pub(super) enum RadioOpStatus {
        /* Operation Not Finished */
        IDLE = 0x0000,    // Operation has not started.
        PENDING = 0x0001, // Waiting for a start trigger.
        ACTIVE = 0x0002,  // Running an operation.
        SKIPPED = 0x0003, // Operation skipped due to condition in another command.
        /* Operation Finished Normally */
        DONE_OK = 0x0400,        // Operation ended normally.
        DONE_COUNTDOWN = 0x0401, // Counter reached zero.
        DONE_RXERR = 0x0402,     // Operation ended with CRC error.
        DONE_TIMEOUT = 0x0403,   // Operation ended with time-out.
        DONE_STOPPED = 0x0404,   // Operation stopped after CMD_STOP command.
        DONE_ABORT = 0x0405,     // Operation aborted by CMD_ABORT command.
        /* Operation Finished With Error */
        ERROR_PAST_START = 0x0800, // The start trigger occurred in the past.
        ERROR_START_TRIG = 0x0801, // Illegal start trigger parameter.
        ERROR_CONDITION = 0x0802,  // Illegal condition for next operation.
        ERROR_PAR = 0x0803,        // Error in a command specific parameter.
        ERROR_POINTER = 0x0804,    // Invalid pointer to next operation.
        ERROR_CMDID = 0x0805, // The next operation has a command ID that is undefined or not a radio operation command.
        ERROR_NO_SETUP = 0x0807, // Operation using RX, TX, or synthesizer attempted without CMD_RADIO_SETUP.
        ERROR_NO_FS = 0x0808, // Operation using RX or TX attempted without the synthesizer being programmed or powered on.
        ERROR_SYNTH_PROG = 0x0809, // Synthesizer programming failed.
        ERROR_TXUNF = 0x080A, // Modem TX underflow observed.
        ERROR_RXOVF = 0x080B, // Modem RX overflow observed.
        ERROR_NO_RX = 0x080C, // Data requested from last RX when no such data exists.
    }

    pub(super) type RadioOpResult<T> = Result<T, RadioOpStatus>;

    impl RadioOpStatus {
        pub(super) fn finished(&self) -> bool {
            match self {
                RadioOpStatus::IDLE | RadioOpStatus::PENDING | RadioOpStatus::ACTIVE => false,
                RadioOpStatus::SKIPPED
                | RadioOpStatus::DONE_OK
                | RadioOpStatus::DONE_COUNTDOWN
                | RadioOpStatus::DONE_RXERR
                | RadioOpStatus::DONE_TIMEOUT
                | RadioOpStatus::DONE_STOPPED
                | RadioOpStatus::DONE_ABORT
                | RadioOpStatus::ERROR_PAST_START
                | RadioOpStatus::ERROR_START_TRIG
                | RadioOpStatus::ERROR_CONDITION
                | RadioOpStatus::ERROR_PAR
                | RadioOpStatus::ERROR_POINTER
                | RadioOpStatus::ERROR_CMDID
                | RadioOpStatus::ERROR_NO_SETUP
                | RadioOpStatus::ERROR_NO_FS
                | RadioOpStatus::ERROR_SYNTH_PROG
                | RadioOpStatus::ERROR_TXUNF
                | RadioOpStatus::ERROR_RXOVF
                | RadioOpStatus::ERROR_NO_RX => true,
            }
        }

        pub(super) fn to_result(self) -> RadioOpResult<()> {
            match self {
                RadioOpStatus::IDLE | RadioOpStatus::PENDING | RadioOpStatus::ACTIVE => {
                    unreachable!()
                }
                RadioOpStatus::DONE_OK => Ok(()),
                RadioOpStatus::SKIPPED
                | RadioOpStatus::DONE_COUNTDOWN
                | RadioOpStatus::DONE_RXERR
                | RadioOpStatus::DONE_TIMEOUT
                | RadioOpStatus::DONE_STOPPED
                | RadioOpStatus::DONE_ABORT
                | RadioOpStatus::ERROR_PAST_START
                | RadioOpStatus::ERROR_START_TRIG
                | RadioOpStatus::ERROR_CONDITION
                | RadioOpStatus::ERROR_PAR
                | RadioOpStatus::ERROR_POINTER
                | RadioOpStatus::ERROR_CMDID
                | RadioOpStatus::ERROR_NO_SETUP
                | RadioOpStatus::ERROR_NO_FS
                | RadioOpStatus::ERROR_SYNTH_PROG
                | RadioOpStatus::ERROR_TXUNF
                | RadioOpStatus::ERROR_RXOVF
                | RadioOpStatus::ERROR_NO_RX => Err(self),
            }
        }
    }

    pub(super) trait RadioCommand {
        const COMMAND_NO: u16;

        fn send(&mut self) -> RadioCmdResult<()> {
            let status: RadioCmdStatus = unsafe {
                core::mem::transmute(driverlib::RFCDoorbellSendTo(
                    self as *mut Self as *mut () as u32,
                ))
            };
            match status {
                RadioCmdStatus::Pending => unreachable!(),
                RadioCmdStatus::Done => RadioCmdResult::Ok(()),
                err => Err(err),
            }
        }
    }

    pub(crate) use driverlib::rfc_CMD_PING_s as Ping;
    impl RadioCommand for Ping {
        const COMMAND_NO: u16 = driverlib::CMD_PING as u16;
    }
    impl Ping {
        pub(super) fn new() -> Self {
            Self {
                commandNo: Self::COMMAND_NO,
            }
        }
    }

    pub(crate) use driverlib::rfc_CMD_RADIO_SETUP_s as RadioSetup;
    impl RadioCommand for RadioSetup {
        const COMMAND_NO: u16 = driverlib::CMD_RADIO_SETUP as u16;
    }
    impl RadioSetup {
        pub(super) fn new(tx_power: u16) -> Self {
            Self {
                commandNo: Self::COMMAND_NO,
                status: RadioOpStatus::IDLE as u16,
                pNextOp: core::ptr::null_mut(),
                startTime: 0,
                startTrigger: driverlib::rfc_CMD_RADIO_SETUP_s__bindgen_ty_1 {
                    _bitfield_1: driverlib::rfc_CMD_RADIO_SETUP_s__bindgen_ty_1::new_bitfield_1(
                        driverlib::TRIG_NOW as u8,
                        0,
                        0,
                        0,
                    ),
                    ..Default::default()
                },
                condition: driverlib::rfc_CMD_RADIO_SETUP_s__bindgen_ty_2 {
                    _bitfield_1: driverlib::rfc_CMD_RADIO_SETUP_s__bindgen_ty_2::new_bitfield_1(
                        driverlib::COND_NEVER as u8,
                        0,
                    ),
                    ..Default::default()
                },
                mode: 0x01, // IEEE 802.15.4
                __dummy0: 0,
                config: driverlib::rfc_CMD_RADIO_SETUP_s__bindgen_ty_3 {
                    _bitfield_1: driverlib::rfc_CMD_RADIO_SETUP_s__bindgen_ty_3::new_bitfield_1(
                        0x0, // differential mode
                        0,   // internal bias
                        0x0, // Always write analog config - won't hurt, sometimes redundant.
                        0x0, // Power up frequency synthesizer.
                    ),
                    ..Default::default()
                },
                txPower: tx_power,
                pRegOverride: core::ptr::null_mut(),
            }
        }
    }

    pub(crate) use driverlib::rfc_CMD_START_RAT_s as StartRat;
    impl RadioCommand for StartRat {
        const COMMAND_NO: u16 = driverlib::CMD_START_RAT as u16;
    }
    impl StartRat {
        pub(super) fn new() -> Self {
            Self {
                commandNo: Self::COMMAND_NO,
            }
        }
    }

    pub(crate) use driverlib::rfc_CMD_SYNC_STOP_RAT_s as SyncStopRat;
    impl RadioCommand for SyncStopRat {
        const COMMAND_NO: u16 = driverlib::CMD_SYNC_STOP_RAT as u16;
    }
    impl SyncStopRat {
        pub(super) fn new() -> Self {
            Self {
                commandNo: Self::COMMAND_NO,
                status: RadioOpStatus::IDLE as u16,
                pNextOp: core::ptr::null_mut(),
                startTime: 0,
                startTrigger: driverlib::rfc_CMD_SYNC_STOP_RAT_s__bindgen_ty_1 {
                    _bitfield_1: driverlib::rfc_CMD_SYNC_STOP_RAT_s__bindgen_ty_1::new_bitfield_1(
                        driverlib::TRIG_NOW as u8,
                        0,
                        0,
                        0,
                    ),
                    ..Default::default()
                },
                condition: driverlib::rfc_CMD_SYNC_STOP_RAT_s__bindgen_ty_2 {
                    _bitfield_1: driverlib::rfc_CMD_SYNC_STOP_RAT_s__bindgen_ty_2::new_bitfield_1(
                        driverlib::COND_NEVER as u8,
                        0,
                    ),
                    ..Default::default()
                },
                __dummy0: 0,
                rat0: 0, // FIXME: actually sync RAT
            }
        }
    }

    pub(crate) use driverlib::rfc_CMD_FS_POWERUP_s as FsPowerup;
    impl RadioCommand for FsPowerup {
        const COMMAND_NO: u16 = driverlib::CMD_FS_POWERUP as u16;
    }
    impl FsPowerup {
        pub(super) fn new() -> Self {
            Self {
                commandNo: Self::COMMAND_NO,
                status: RadioOpStatus::IDLE as u16,
                pNextOp: core::ptr::null_mut(),
                startTime: 0,
                startTrigger: driverlib::rfc_CMD_FS_POWERUP_s__bindgen_ty_1 {
                    _bitfield_1: driverlib::rfc_CMD_FS_POWERUP_s__bindgen_ty_1::new_bitfield_1(
                        driverlib::TRIG_NOW as u8,
                        0,
                        0,
                        0,
                    ),
                    ..Default::default()
                },
                condition: driverlib::rfc_CMD_FS_POWERUP_s__bindgen_ty_2 {
                    _bitfield_1: driverlib::rfc_CMD_FS_POWERUP_s__bindgen_ty_2::new_bitfield_1(
                        driverlib::COND_NEVER as u8,
                        0,
                    ),
                    ..Default::default()
                },
                __dummy0: 0,
                pRegOverride: core::ptr::null_mut(),
            }
        }
    }

    pub(crate) use driverlib::rfc_CMD_FS_POWERDOWN_s as FsPowerdown;
    impl RadioCommand for FsPowerdown {
        const COMMAND_NO: u16 = driverlib::CMD_FS_POWERDOWN as u16;
    }
    impl FsPowerdown {
        pub(super) fn new() -> Self {
            Self {
                commandNo: Self::COMMAND_NO,
                status: RadioOpStatus::IDLE as u16,
                pNextOp: core::ptr::null_mut(),
                startTime: 0,
                startTrigger: driverlib::rfc_CMD_FS_POWERDOWN_s__bindgen_ty_1 {
                    _bitfield_1: driverlib::rfc_CMD_FS_POWERDOWN_s__bindgen_ty_1::new_bitfield_1(
                        driverlib::TRIG_NOW as u8,
                        0,
                        0,
                        0,
                    ),
                    ..Default::default()
                },
                condition: driverlib::rfc_CMD_FS_POWERDOWN_s__bindgen_ty_2 {
                    _bitfield_1: driverlib::rfc_CMD_FS_POWERDOWN_s__bindgen_ty_2::new_bitfield_1(
                        driverlib::COND_NEVER as u8,
                        0,
                    ),
                    ..Default::default()
                },
            }
        }
    }

    pub(crate) use driverlib::rfc_CMD_IEEE_RX_s as IeeeRx;
    impl RadioCommand for IeeeRx {
        const COMMAND_NO: u16 = driverlib::CMD_IEEE_RX as u16;
    }
    impl IeeeRx {
        pub(super) fn new(
            channel: u8,
            pan: u16,
            addr: u16,
            addr_long: [u8; 8],
            rx_queue: &Cell<super::RfcQueue>,
            rx_result: &Cell<super::RfcRxOutput>,
        ) -> Self {
            Self {
                commandNo: Self::COMMAND_NO,
                status: RadioOpStatus::IDLE as u16,
                pNextOp: core::ptr::null_mut(),
                startTime: 0,
                startTrigger: driverlib::rfc_CMD_IEEE_RX_s__bindgen_ty_1 {
                    _bitfield_1: driverlib::rfc_CMD_IEEE_RX_s__bindgen_ty_1::new_bitfield_1(
                        driverlib::TRIG_NOW as u8,
                        0,
                        0,
                        0,
                    ),
                    ..Default::default()
                },
                condition: driverlib::rfc_CMD_IEEE_RX_s__bindgen_ty_2 {
                    _bitfield_1: driverlib::rfc_CMD_IEEE_RX_s__bindgen_ty_2::new_bitfield_1(
                        driverlib::COND_NEVER as u8,
                        0,
                    ),
                    ..Default::default()
                },
                channel,
                rxConfig: driverlib::rfc_CMD_IEEE_RX_s__bindgen_ty_3 {
                    _bitfield_1: driverlib::rfc_CMD_IEEE_RX_s__bindgen_ty_3::new_bitfield_1(
                        1, 0, 0, 0, 0, 0, 0, 0,
                    ),
                    ..Default::default()
                },
                pRxQ: unsafe { core::mem::transmute(rx_queue) },
                pOutput: unsafe { core::mem::transmute(rx_result) },
                frameFiltOpt: driverlib::rfc_CMD_IEEE_RX_s__bindgen_ty_4 {
                    _bitfield_1: driverlib::rfc_CMD_IEEE_RX_s__bindgen_ty_4::new_bitfield_1(
                        0, 0, 1, 0, 0, 0, 0, 0, 2, 0, 0, 0,
                    ),
                    ..Default::default()
                },
                frameTypes: driverlib::rfc_CMD_IEEE_RX_s__bindgen_ty_5 {
                    _bitfield_1: driverlib::rfc_CMD_IEEE_RX_s__bindgen_ty_5::new_bitfield_1(
                        1, 1, 1, 1, 1, 1, 1, 1,
                    ),
                    ..Default::default()
                },
                ccaOpt: driverlib::rfc_CMD_IEEE_RX_s__bindgen_ty_6 {
                    _bitfield_1: driverlib::rfc_CMD_IEEE_RX_s__bindgen_ty_6::new_bitfield_1(
                        1, 1, 1, 1, 0, 3,
                    ),
                    ..Default::default()
                },
                ccaRssiThr: 0xA6_u8 as i8,
                __dummy0: 0,
                numExtEntries: 0,
                numShortEntries: 0,
                pExtEntryList: core::ptr::null_mut(),
                pShortEntryList: core::ptr::null_mut(),
                localExtAddr: u64::from_ne_bytes(addr_long),
                localShortAddr: addr,
                localPanID: pan,
                __dummy1: 0,
                __dummy2: 0,
                endTrigger: driverlib::rfc_CMD_IEEE_RX_s__bindgen_ty_7 {
                    _bitfield_1: driverlib::rfc_CMD_IEEE_RX_s__bindgen_ty_7::new_bitfield_1(
                        0, 0, 0, 0,
                    ),
                    ..Default::default()
                },
                endTime: 0,
            }
        }
    }

    pub(crate) use driverlib::rfc_CMD_IEEE_CCA_REQ_s as IeeeCcaReq;
    impl RadioCommand for IeeeCcaReq {
        const COMMAND_NO: u16 = driverlib::CMD_IEEE_CCA_REQ as u16;
    }
    impl IeeeCcaReq {
        pub(super) fn new() -> Self {
            Self {
                commandNo: Self::COMMAND_NO,
                ..Default::default() // Other fields are read-only.
            }
        }
    }

    pub(crate) use driverlib::rfc_CMD_IEEE_TX_s as IeeeTx;
    impl RadioCommand for IeeeTx {
        const COMMAND_NO: u16 = driverlib::CMD_IEEE_TX as u16;
    }
    impl IeeeTx {
        pub(super) fn new(payload: *mut u8, payload_len: u8) -> Self {
            Self {
                commandNo: Self::COMMAND_NO,
                status: RadioOpStatus::IDLE as u16,
                pNextOp: core::ptr::null_mut(),
                startTime: 0,
                startTrigger: driverlib::rfc_CMD_IEEE_TX_s__bindgen_ty_1 {
                    _bitfield_1: driverlib::rfc_CMD_IEEE_TX_s__bindgen_ty_1::new_bitfield_1(
                        driverlib::TRIG_NOW as u8,
                        0,
                        0,
                        0,
                    ),
                    ..Default::default()
                },
                condition: driverlib::rfc_CMD_IEEE_TX_s__bindgen_ty_2 {
                    _bitfield_1: driverlib::rfc_CMD_IEEE_TX_s__bindgen_ty_2::new_bitfield_1(
                        driverlib::COND_NEVER as u8,
                        0,
                    ),
                    ..Default::default()
                },
                txOpt: driverlib::rfc_CMD_IEEE_TX_s__bindgen_ty_3 {
                    _bitfield_1: driverlib::rfc_CMD_IEEE_TX_s__bindgen_ty_3::new_bitfield_1(
                        0, 0, 0,
                    ),
                    ..Default::default()
                },
                payloadLen: payload_len,
                pPayload: payload,
                timeStamp: 0,
            }
        }
    }

    /// On reception, the radio CPU appends the provided data entry to the queue indicated. The radio CPU
    /// performs the following operations:
    /// ```
    /// Set pQueue-> pLastEntry-> pNextEntry = pEntry
    /// Set pQueue-> pLastEntry = pEntry
    /// ```
    /// If either of the pointers pQueue or pEntry are invalid (that is, in an address range that is not memory or
    /// without 32-bit word alignment), the command fails, and the radio CPU sets the result byte of CMDSTA to
    /// ParError. If the queue specified in pQueue is set up not to allow entries to be appended (see
    /// Section 23.3.2.7.1), the command fails, and the radio CPU sets the result byte of CMDSTA to QueueError.
    pub(crate) use driverlib::rfc_CMD_ADD_DATA_ENTRY_s as AddDataEntry;
    impl RadioCommand for AddDataEntry {
        const COMMAND_NO: u16 = driverlib::CMD_ADD_DATA_ENTRY as u16;
    }
    impl AddDataEntry {
        pub(super) fn new(queue: *mut RfcQueue, entry: &mut RfcDataEntryPointer) -> Self {
            Self {
                commandNo: Self::COMMAND_NO,
                __dummy0: Default::default(),
                pQueue: queue,
                pEntry: entry as *mut RfcDataEntryPointer as *mut u8,
            }
        }
    }

    /// On reception, the radio CPU removes the first data entry from the queue indicated. The command returns
    /// a pointer to the entry that was removed. The radio CPU performs the following operations:
    /// ```
    /// Set pEntry = pQueue->pCurrEntry
    /// Set pQueue->pCurrEntry = pEntry->pNextEntry
    /// Set pEntry->status = Finished
    /// ```
    /// If the pointer pQueue is invalid, the command fails, and the radio CPU sets the result byte of CMDSTA to
    /// ParError. If the queue specified in pQueue is empty, the command fails, and the radio CPU sets the result
    /// byte of CMDSTA to QueueError. If the entry to be removed is in the BUSY state, the command fails, and
    /// the radio CPU sets the result byte of CMDSTA to QueueBusy.
    pub(crate) use driverlib::rfc_CMD_REMOVE_DATA_ENTRY_s as RemoveDataEntry;
    impl RadioCommand for RemoveDataEntry {
        const COMMAND_NO: u16 = driverlib::CMD_REMOVE_DATA_ENTRY as u16;
    }
    impl RemoveDataEntry {
        pub(super) fn new(queue: &mut RfcQueue) -> Self {
            Self {
                commandNo: Self::COMMAND_NO,
                __dummy0: Default::default(),
                pQueue: queue,
                pEntry: core::ptr::null_mut(), // R parameter
            }
        }
    }

    pub(crate) const RF_CORE_CMD_CCA_REQ_RSSI_UNKNOWN: i8 = -128;

    pub(crate) const RF_CORE_CMD_CCA_REQ_CCA_STATE_IDLE: u8 = 0; /* 00 */
    pub(crate) const RF_CORE_CMD_CCA_REQ_CCA_STATE_BUSY: u8 = 1; /* 01 */
    pub(crate) const RF_CORE_CMD_CCA_REQ_CCA_STATE_INVALID: u8 = 2; /* 10 */
}
use cmd::RadioCommand;

mod power {
    /*---------------------------------------------------------------------------*/
    /* TX Power dBm lookup table - values from SmartRF Studio */
    #[derive(Clone, Copy)]
    pub(super) struct PowerOutputConfig {
        pub(super) dbm: i8,
        pub(super) tx_power: u16, /* Value for the CMD_RADIO_SETUP.txPower field */
    }

    const OUTPUT_CONFIG_COUNT: usize = 13;

    static OUTPUT_POWER: [PowerOutputConfig; OUTPUT_CONFIG_COUNT] = [
        PowerOutputConfig {
            dbm: 5,
            tx_power: 0x9330,
        },
        PowerOutputConfig {
            dbm: 4,
            tx_power: 0x9324,
        },
        PowerOutputConfig {
            dbm: 3,
            tx_power: 0x5a1c,
        },
        PowerOutputConfig {
            dbm: 2,
            tx_power: 0x4e18,
        },
        PowerOutputConfig {
            dbm: 1,
            tx_power: 0x4214,
        },
        PowerOutputConfig {
            dbm: 0,
            tx_power: 0x3161,
        },
        PowerOutputConfig {
            dbm: -3,
            tx_power: 0x2558,
        },
        PowerOutputConfig {
            dbm: -6,
            tx_power: 0x1d52,
        },
        PowerOutputConfig {
            dbm: -9,
            tx_power: 0x194e,
        },
        PowerOutputConfig {
            dbm: -12,
            tx_power: 0x144b,
        },
        PowerOutputConfig {
            dbm: -15,
            tx_power: 0x0ccb,
        },
        PowerOutputConfig {
            dbm: -18,
            tx_power: 0x0cc9,
        },
        PowerOutputConfig {
            dbm: -21,
            tx_power: 0x0cc7,
        },
    ];

    /* Max and Min Output Power in dBm */
    #[allow(unused)]
    pub(super) static OUTPUT_POWER_MIN: PowerOutputConfig = OUTPUT_POWER[OUTPUT_CONFIG_COUNT - 1];
    pub(super) static OUTPUT_POWER_MAX: PowerOutputConfig = OUTPUT_POWER[0];

    pub(super) fn get_power_cfg(power: i8) -> Option<PowerOutputConfig> {
        OUTPUT_POWER.iter().copied().find(|cfg| cfg.dbm == power)
    }
}
use power::{get_power_cfg, PowerOutputConfig, OUTPUT_POWER_MAX};
use tock_cells::volatile_cell::VolatileCell;

/// We use a single deferred call for two operations: triggering config clients
/// and power change clients. This allows us to track which operation we need to
/// perform when we get the deferred call callback.
#[derive(Debug, Clone, Copy)]
enum DeferredOperation {
    /// Waiting to notify that the configuration operation is complete.
    ConfigClientCallback,
    /// Waiting to notify that the power state of the radio changed
    /// (i.e. it turned on or off).
    PowerClientCallback,
}

impl RfcDataEntryPointer {
    const STATUS_PENDING: u8 = 0x00; /* Not in use by the Radio CPU */
    const STATUS_ACTIVE: u8 = 0x01; /* Open for r/w by the radio CPU */
    const STATUS_BUSY: u8 = 0x02; /* Ongoing r/w */
    const STATUS_FINISHED: u8 = 0x03; /* Free to use and to free */
    const STATUS_UNFINISHED: u8 = 0x04; /* Partial RX entry */

    const POINTER_ENTRY_TYPE: u8 = 2;

    fn new(data: *mut RxBuf, length: u16, next_entry: *mut RfcDataEntryPointer) -> Self {
        Self {
            pNextEntry: next_entry as *mut u8,
            status: Self::STATUS_PENDING,
            config: driverlib::rfc_dataEntryPointer_s__bindgen_ty_1 {
                _bitfield_align_1: Default::default(),
                _bitfield_1: driverlib::rfc_dataEntryPointer_s__bindgen_ty_1::new_bitfield_1(
                    Self::POINTER_ENTRY_TYPE,
                    // u16 is the type of the field, but little endian allows us
                    // to ignore the more significant byte.
                    1,
                    0,
                ),
            },
            length,
            pData: data as *mut u8,
        }
    }
}

impl RfcQueue {
    /// Set pQueue-> pLastEntry-> pNextEntry = pEntry
    /// Set pQueue-> pLastEntry = pEntry
    fn append_entry(&mut self, entry: &RfcDataEntryPointer) {
        let last_entry = unsafe {
            (self.pLastEntry as *mut RfcDataEntryPointer)
                .as_mut()
                .unwrap()
        };
        last_entry.pNextEntry = entry as *const RfcDataEntryPointer as *mut u8;
        self.pLastEntry = entry as *const RfcDataEntryPointer as *mut u8;
    }
}

type RxBuf = [u8; radio::MAX_BUF_SIZE];

struct RxMachinery {
    stats: Cell<RfcRxOutput>,
    queue: Cell<RfcQueue>,
    bufs: [(RefCell<RfcDataEntryPointer>, TakeCell<'static, [u8]>); Self::N_BUFS],
    next_finished: Cell<usize>,
}

const _N_BUFS_ASSERTION: () = assert!(RxMachinery::N_BUFS >= 2, "At least 2 RX bufs are needed");

impl RxMachinery {
    const N_BUFS: usize = 2;

    fn new() -> Self {
        // const CELL: VolatileCell<u8> = VolatileCell::new(0);
        fn make_buf() -> &'static mut RxBuf {
            unsafe { static_init!(RxBuf, [0_u8; radio::MAX_BUF_SIZE]) }
        }
        fn make_entry() -> RefCell<RfcDataEntryPointer> {
            RefCell::new(RfcDataEntryPointer::new(
                core::ptr::null_mut(),
                radio::MAX_BUF_SIZE as u16,
                core::ptr::null_mut(),
            ))
        }

        Self {
            stats: Default::default(),
            queue: Default::default(),
            bufs: core::array::from_fn(|idx| {
                (
                    make_entry(),
                    // The last buffer is going to be given from the layer above HIL,
                    // by `RadioData::set_receive_buffer()`.
                    if idx + 1 < Self::N_BUFS {
                        TakeCell::new(make_buf().as_mut_slice())
                    } else {
                        TakeCell::empty()
                    },
                )
            }),
            next_finished: Cell::new(0),
        }
    }

    fn link_entries(&'static mut self) -> &'static mut Self {
        use core::ops::DerefMut as _;

        // Link entries without cycles
        for window in self.bufs.windows(2) {
            let (entry1, _buf1) = &window[0];
            let (entry2, _buf2) = &window[1];

            entry1.borrow_mut().pNextEntry =
                entry2.borrow_mut().deref_mut() as *mut RfcDataEntryPointer as *mut u8;
        }

        for (entry, buf) in self.bufs[..Self::N_BUFS - 1].iter_mut() {
            entry.get_mut().pData = buf.get_mut().unwrap() as *mut [u8] as *mut u8;
        }

        // The last entry is going to be linked to the buffer received eventually from upper layer,
        // when `RadioData::set_receive_buf()` is called.

        // Setup queue.
        self.queue.set(RfcQueue {
            pCurrEntry: self.bufs.first_mut().as_mut().unwrap().0.get_mut()
                as *mut RfcDataEntryPointer as *mut u8,
            pLastEntry: self.bufs.last_mut().as_mut().unwrap().0.get_mut()
                as *mut RfcDataEntryPointer as *mut u8,
        });

        self
    }

    fn poweroff_cleanup(&self) {
        /*
         * Just in case there was an ongoing RX (which started after we began the
         * shutdown sequence), we don't want to leave the buffer in state == ongoing
         */
        for (entry, _buf) in self.bufs.iter() {
            let status = &mut entry.borrow_mut().status;
            if *status == RfcDataEntryPointer::STATUS_BUSY {
                *status = RfcDataEntryPointer::STATUS_PENDING;
            }
        }
    }

    fn set_higher_layer_buffer(&self, buf: &'static mut [u8], radio_is_on: bool) {
        use core::ops::{Deref as _, DerefMut as _};

        if let Some((entry, buf_slot)) = self.bufs.iter().find(|(_entry, buf)| buf.is_none()) {
            if radio_is_on {
                // Radio is on, so to prevent races we employ itself to add the entry to the queue.
                let mut cmd = cmd::AddDataEntry::new(
                    &self.queue as *const Cell<RfcQueue> as *mut RfcQueue,
                    entry.borrow_mut().deref_mut(),
                );
                cmd.send().unwrap();
            } else {
                // Radio is off, so we cannot use cmds.
                // Instead, let's add the entry to the queue manually.
                let mut queue = self.queue.get();
                queue.append_entry(entry.borrow().deref());
                self.queue.set(queue);
            }
            buf_slot.replace(buf);
        } else {
            let last_entry_ptr = self.queue.get().pLastEntry as *const RfcDataEntryPointer;
            let (_entry, buf_slot) = self
                .bufs
                .iter()
                .find(|(entry, _buf)| {
                    entry.borrow().deref() as *const RfcDataEntryPointer == last_entry_ptr
                })
                .unwrap();
            buf_slot.replace(buf);
        }
    }

    fn take_finished(&self) -> Option<&'static mut [u8]> {
        let next_finished = self.next_finished.get();
        let (entry, buf_slot) = &self.bufs[next_finished];
        (entry.borrow().status == RfcDataEntryPointer::STATUS_FINISHED).then(|| {
            self.next_finished.set(next_finished + 1);
            buf_slot.take().unwrap()
        })
    }
}

pub struct Radio<'a> {
    #[allow(unused)]
    rfc_pwr: cc2650::RFC_PWR,
    rfc_dbell: cc2650::RFC_DBELL,
    #[allow(unused)]
    rfc_rat: cc2650::RFC_RAT,

    // interrupts
    cpe0: Nvic,
    cpe1: Nvic,

    // clients
    config_client: OptionalCell<&'a dyn radio::ConfigClient>,
    power_client: OptionalCell<&'a dyn radio::PowerClient>,
    rx_client: OptionalCell<&'a dyn radio::RxClient>,
    tx_client: OptionalCell<&'a dyn radio::TxClient>,

    // config
    addr: Cell<u16>,
    addr_long: Cell<[u8; 8]>,
    pan: Cell<u16>,
    channel: Cell<RadioChannel>,
    tx_power: Cell<PowerOutputConfig>,

    // tx helpers
    tx_buf: TakeCell<'static, [u8]>,
    tx_cmd: RefCell<cmd::IeeeTx>,

    // rx helpers
    rx_cmd: RefCell<cmd::IeeeRx>,
    rx_machinery: &'static mut RxMachinery,

    // deferred call machinery
    deferred_call: DeferredCall,
    deferred_call_operation: OptionalCell<DeferredOperation>,
}

impl<'a> Radio<'a> {
    pub fn new(
        rfc_pwr: cc2650::RFC_PWR,
        rfc_dbell: cc2650::RFC_DBELL,
        rfc_rat: cc2650::RFC_RAT,
    ) -> Self {
        let rx_machinery = unsafe { static_init!(RxMachinery, RxMachinery::new()) };
        let rx_machinery = rx_machinery.link_entries();
        let rx_cmd = RefCell::new(cmd::IeeeRx::new(
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            &rx_machinery.queue,
            &rx_machinery.stats,
        ));

        let tx_cmd = RefCell::new(cmd::IeeeTx::new(core::ptr::null_mut(), Default::default()));

        Self {
            rfc_pwr,
            rfc_dbell,
            rfc_rat,

            cpe0: unsafe { Nvic::new(crate::peripheral_interrupts::RF_CPE0) },
            cpe1: unsafe { Nvic::new(crate::peripheral_interrupts::RF_CPE1) },

            config_client: OptionalCell::empty(),
            power_client: OptionalCell::empty(),
            rx_client: OptionalCell::empty(),
            tx_client: OptionalCell::empty(),

            tx_buf: TakeCell::empty(),
            tx_cmd,

            addr: Cell::new(0),
            addr_long: Cell::new([0x00; 8]),
            pan: Cell::new(0),
            channel: Cell::new(RadioChannel::Channel26),
            tx_power: Cell::new(OUTPUT_POWER_MAX),

            rx_cmd,
            rx_machinery,

            deferred_call: DeferredCall::new(),
            deferred_call_operation: OptionalCell::empty(),
        }
    }

    /* CMD convenience wrappers */

    /// Send ping to verify that CPE works.
    fn ping(&self) -> cmd::RadioCmdResult<()> {
        let mut cmd = cmd::Ping::new();
        cmd.send()
    }

    fn setup(&self) -> cmd::RadioCmdResult<()> {
        let mut cmd = UnsafeCell::new(cmd::RadioSetup::new(self.tx_power.get().tx_power));
        cmd.get_mut().send()?;

        // Synchronously wait until radio setup finishes.
        let status = loop {
            let status: cmd::RadioOpStatus =
                unsafe { core::mem::transmute((cmd.get().read_volatile()).status) };
            if status.finished() {
                break status;
            }
        };
        status.to_result().unwrap();

        Ok(())
    }

    fn start_rat(&self) -> cmd::RadioCmdResult<()> {
        let mut cmd = cmd::StartRat::new();
        cmd.send()
    }

    fn stop_rat(&self) -> cmd::RadioCmdResult<()> {
        let mut cmd = cmd::SyncStopRat::new();
        cmd.send()
    }

    fn start_synthesizer(&self) -> cmd::RadioCmdResult<()> {
        let mut cmd = cmd::FsPowerup::new();
        cmd.send()
    }

    fn stop_synthesizer(&self) -> cmd::RadioCmdResult<()> {
        let mut cmd = cmd::FsPowerdown::new();
        cmd.send()
    }

    fn tx(&self, buf: &'static mut [u8], frame_len: u8) -> cmd::RadioCmdResult<()> {
        /*
         * We are certainly not TXing a frame as a result of CMD_IEEE_TX, but we may
         * be in the process of TXing an ACK. In that case, wait for the TX to finish
         * or return after approx TX_WAIT_TIMEOUT.
         */
        // TODO: add timeout
        // t0 = RTIMER_NOW();
        // FIXME: bring this back
        // while self.is_transmitting().unwrap()
        // && (RTIMER_CLOCK_LT(RTIMER_NOW(), t0 + RF_CORE_TX_TIMEOUT))
        // {}

        self.clear_pending_interrupts();
        self.clear_and_enable_tx_interrupt();

        let mut cmd = self.tx_cmd.borrow_mut();
        *cmd = cmd::IeeeTx::new(buf[radio::PSDU_OFFSET..].as_mut_ptr(), frame_len);

        // Save buf before sending the CMD to prevent races.
        self.tx_buf.put(Some(buf));

        cmd.send().unwrap();
        // core::mem::drop(cmd);

        // // Synchronously wait until TX finishes.
        // let status = loop {
        //     let status: cmd::RadioOpStatus =
        //         unsafe { core::mem::transmute(self.tx_cmd.as_ptr().read_volatile().status) };
        //     if status.finished() {
        //         break status;
        //     }
        // };
        // status
        //     .to_result()
        //     .unwrap_or_else(|err| panic!("Got TX result: {} = {:?}", status as u16, err));

        Ok(())
    }

    fn rx(&self) -> cmd::RadioCmdResult<()> {
        let mut cmd = self.rx_cmd.borrow_mut();
        *cmd = cmd::IeeeRx::new(
            self.get_channel(),
            self.get_pan(),
            self.get_address(),
            self.get_address_long(),
            &self.rx_machinery.queue,
            &self.rx_machinery.stats,
        );
        cmd.send().unwrap();

        Ok(())
    }

    fn cca_req(&self) -> cmd::RadioCmdResult<cmd::IeeeCcaReq> {
        let mut cmd = cmd::IeeeCcaReq::new();
        cmd.send()?;
        Ok(cmd)
    }

    /* Interrupt management */

    fn configure_interrupts(&self) {
        self.rfc_dbell.rfcpeisl.modify(|_r, w| {
            w.rx_data_written()
                .cpe0()
                .tx_done()
                .cpe0()
                .tx_entry_done()
                .cpe0()
                .last_fg_command_done()
                .cpe0()
                .fg_command_done()
                .cpe0()
                .last_command_done()
                .cpe0()
                .command_done()
                .cpe0()
                .internal_error()
                .cpe1()
                .rx_buf_full()
                .cpe0()
                .rx_nok()
                .cpe0()
                .rx_ok()
                .cpe0()
                .modules_unlocked()
                .cpe1()
                .rx_ignored()
                .cpe0()
                .boot_done()
                .cpe0()
                .synth_no_lock()
                .cpe1()
                .irq27()
                .cpe1()
                .rx_n_data_written()
                .cpe0()
                .rx_data_written()
                .cpe0()
                .rx_entry_done()
                .cpe0()
                .rx_ctrl_ack()
                .cpe0()
                .rx_ctrl()
                .cpe0()
                .rx_empty()
                .cpe0()
                .rx_aborted()
                .cpe0()
        });

        self.rfc_dbell.rfcpeien.write(|w| {
            w.rx_entry_done()
                .set_bit()
                // .tx_done()
                // .set_bit()
                // .tx_entry_done()
                // .set_bit()
                .internal_error()
                .set_bit()
                .rx_buf_full()
                .set_bit()
                .fg_command_done()
                .clear_bit()
                .command_done()
                .clear_bit()
                .last_command_done()
                .clear_bit()
                .last_fg_command_done()
                .clear_bit()
        });

        unsafe {
            // We make no use of this interrupt.
            let cmd_ack_interrupt =
                cortexm3::nvic::Nvic::new(crate::peripheral_interrupts::RF_CMD_ACK);
            cmd_ack_interrupt.disable();
            cmd_ack_interrupt.clear_pending();

            let rf_cpe0_interrupt =
                cortexm3::nvic::Nvic::new(crate::peripheral_interrupts::RF_CPE0);
            rf_cpe0_interrupt.clear_pending();

            let rf_cpe1_interrupt =
                cortexm3::nvic::Nvic::new(crate::peripheral_interrupts::RF_CPE1);
            rf_cpe1_interrupt.clear_pending();
        }
    }

    fn enable_interrupts(&self) {
        self.rfc_dbell.rfcpeifg.write(|w| unsafe { w.bits(0) });

        self.cpe0.enable();
        self.cpe1.enable();
    }

    fn disable_interrupts(&self) {
        self.cpe0.disable();
        self.cpe1.disable();
    }

    fn clear_pending_interrupts(&self) {
        self.cpe0.clear_pending();
        self.cpe1.clear_pending();
    }

    fn clear_and_enable_tx_interrupt(&self) {
        self.rfc_dbell.rfcpeifg.write(|w| {
            unsafe { w.bits(-1_i32 as u32) }
                .last_fg_command_done()
                .clear_bit()
        });

        self.rfc_dbell
            .rfcpeien
            .modify(|_r, w| w.last_fg_command_done().set_bit());
    }

    fn disable_tx_interrupt(&self) {
        self.rfc_dbell
            .rfcpeien
            .modify(|_r, w| w.last_fg_command_done().clear_bit());
    }

    pub(crate) fn handle_interrupt_cpe0(&self) {
        // FIXME: disable interrupts
        self.disable_interrupts();
        kernel::debug!("handling interrupt cpe0");

        let interrupts = self.rfc_dbell.rfcpeifg.read();
        let tx_done = interrupts.tx_done().bit_is_set();
        let last_fg_command_done = interrupts.last_fg_command_done().bit_is_set();
        let rx_entry_done = interrupts.rx_entry_done().bit_is_set();
        kernel::debug!(
            "interrupts: last_fg_command_done={}, tx_done={}, rx_entry_done={}",
            last_fg_command_done,
            tx_done,
            rx_entry_done
        );

        self.disable_tx_interrupt();

        self.rfc_dbell.rfcpeifg.write(|w| {
            unsafe { w.bits(-1_i32 as u32) }
                .tx_done()
                .clear_bit()
                .last_fg_command_done()
                .clear_bit()
                .rx_entry_done()
                .clear_bit()
        });

        // The interrupt means that we received or transmitted a frame. Let's determine
        // whether it's RX or TX that has triggered the interrupt.

        if let Some(tx_buf) = self.tx_buf.take() {
            assert!(last_fg_command_done);
            let status: cmd::RadioOpStatus =
                unsafe { core::mem::transmute(self.tx_cmd.borrow().status) };
            assert!(status.finished());
            status.to_result().unwrap();

            // TX completed
            self.tx_client.map(|client| {
                client.send_done(
                    tx_buf,
                    false /* FIXME: consider if we should set it to true, as automatic ACK is turned on */,
                    Ok(())
                )
            });
        } else {
            assert!(rx_entry_done);
            // RX completed
            self.rx_machinery.take_finished().map(|rx_buf| {
                let data_len = (rx_buf[radio::PHR_OFFSET] & 0x7F) as usize;

                // LQI is found just after the data received.
                let lqi = rx_buf[data_len];

                // We drop the CRC bytes (the MFR) from our frame.
                let frame_len = data_len - radio::MFR_SIZE;

                // RX completed
                self.rx_client
                    .map(|client| client.receive(rx_buf, frame_len, lqi, true, Ok(())));
            });
        };
        //  FIXME: enable interrupts
        self.enable_interrupts();
    }

    pub(crate) fn handle_interrupt_cpe1(&self) {
        let interrupts = self.rfc_dbell.rfcpeifg.read();

        let internal_error = interrupts.internal_error().bit_is_set();
        let boot_done = interrupts.boot_done().bit_is_set();
        let modules_unlocked = interrupts.modules_unlocked().bit_is_set();
        let synth_no_lock = interrupts.synth_no_lock().bit_is_set();
        let irq27 = interrupts.irq27().bit_is_set();
        let rx_aborted = interrupts.rx_aborted().bit_is_set();
        let rx_n_data_written = interrupts.rx_n_data_written().bit_is_set();
        let rx_data_written = interrupts.rx_data_written().bit_is_set();
        let rx_entry_done = interrupts.rx_entry_done().bit_is_set();
        let rx_buf_full = interrupts.rx_buf_full().bit_is_set();
        let rx_ctrl_ack = interrupts.rx_ctrl_ack().bit_is_set();
        let rx_ctrl = interrupts.rx_ctrl().bit_is_set();
        let rx_empty = interrupts.rx_empty().bit_is_set();
        let rx_ignored = interrupts.rx_ignored().bit_is_set();
        let rx_nok = interrupts.rx_nok().bit_is_set();
        let rx_ok = interrupts.rx_ok().bit_is_set();
        let irq15 = interrupts.irq15().bit_is_set();
        let irq14 = interrupts.irq14().bit_is_set();
        let irq13 = interrupts.irq13().bit_is_set();
        let irq12 = interrupts.irq12().bit_is_set();
        let tx_buffer_changed = interrupts.tx_buffer_changed().bit_is_set();
        let tx_entry_done = interrupts.tx_entry_done().bit_is_set();
        let tx_retrans = interrupts.tx_retrans().bit_is_set();
        let tx_ctrl_ack_ack = interrupts.tx_ctrl_ack_ack().bit_is_set();
        let tx_ctrl_ack = interrupts.tx_ctrl_ack().bit_is_set();
        let tx_ctrl = interrupts.tx_ctrl().bit_is_set();
        let tx_ack = interrupts.tx_ack().bit_is_set();
        let tx_done = interrupts.tx_done().bit_is_set();
        let last_fg_command_done = interrupts.last_fg_command_done().bit_is_set();
        let fg_command_done = interrupts.fg_command_done().bit_is_set();
        let last_command_done = interrupts.last_command_done().bit_is_set();
        let command_done = interrupts.command_done().bit_is_set();

        let bits = interrupts.bits();

        panic!(
            "Raised interrupt cpe1 - RFC error! bits={bits},

            internal_error  ={internal_error},
            modules_unlocked={modules_unlocked},
            synth_no_lock={synth_no_lock},
            irq27={irq27},
            rx_aborted={rx_aborted},
            rx_n_data_written={rx_n_data_written},
            rx_data_written={rx_data_written},
            rx_entry_done={rx_entry_done},
            rx_buf_full={rx_buf_full},
            rx_ctrl_ack={rx_ctrl_ack},
            rx_ctrl={rx_ctrl},
            rx_empty={rx_empty},
            rx_ignored={rx_ignored},
            rx_nok={rx_nok},
            rx_ok={rx_ok},
            irq15={irq15},
            irq14={irq14},
            irq13={irq13},
            irq12={irq12},
            tx_buffer_changed={tx_buffer_changed},
            tx_entry_done={tx_entry_done},
            tx_retrans={tx_retrans},
            tx_ctrl_ack_ack={tx_ctrl_ack_ack},
            tx_ctrl_ack={tx_ctrl_ack},
            tx_ctrl={tx_ctrl},
            tx_ack={tx_ack},
            tx_done={tx_done},
            last_fg_command_done={last_fg_command_done},
            fg_command_done={fg_command_done},
            last_command_done={last_command_done},
            command_done={command_done},
            ",
        );
    }

    /* Radio management logic */

    fn rx_on(&self) -> bool {
        if !self.is_on() {
            return false;
        }

        self.rx_cmd.borrow().status == cmd::RADIO_OP_STATUS_ACTIVE
    }

    /**
     * \brief Check the RF's TX status
     * \return true RF is transmitting
     * \return false RF is not transmitting
     *
     * TX mode may be triggered either by a CMD_IEEE_TX or by the automatic
     * transmission of an ACK frame.
     */
    fn is_transmitting(&self) -> cmd::RadioCmdResult<bool> {
        /* If we are off, we are not in TX */
        if !self.is_on() {
            return Ok(false);
        }

        let cmd = self.cca_req()?;

        if (cmd.currentRssi == cmd::RF_CORE_CMD_CCA_REQ_RSSI_UNKNOWN)
            && (cmd.ccaInfo.ccaEnergy() == cmd::RF_CORE_CMD_CCA_REQ_CCA_STATE_BUSY)
        {
            return Ok(true);
        }

        Ok(false)
    }

    fn is_receiving(&self) -> cmd::RadioCmdResult<bool> {
        /* If we are off, we are not receiving */
        if !self.is_on() {
            return Ok(false);
        }

        /* If we are transmitting (can only be an ACK here), we are not receiving */
        if self.is_transmitting()? {
            return Ok(false);
        }

        let cca_info = self.cca_req()?;

        // /* If we can't read CCA info, return "not receiving" */
        // if cca_info == cmd::RF_CORE_GET_CCA_INFO_ERROR {
        //     return Ok(false);
        // }

        /* If sync has been seen, return true (receiving) */
        Ok(cca_info.ccaInfo.ccaSync() != 0)
    }

    fn radio_on(&self) -> Result<(), ErrorCode> {
        unsafe {
            driverlib::OSCHF_TurnOnXosc();
        }

        let prcm = unsafe { &*cc2650::PRCM::ptr() };

        // Power domain
        let domains = crate::prcm::PowerDomains::empty().rfc();
        unsafe { driverlib::PRCMPowerDomainOn(domains.into()) };
        while !Prcm::are_enabled(domains) {}

        // Clock gating
        Clock::enable_clocks(prcm, Clocks::empty().rfc());

        assert!(self.is_on());
        self.configure_interrupts();

        // self.rfc_pwr
        //     .pwmclken
        //     .write(|w| w.cpe().set_bit().cperam().set_bit());

        // self.rfc_pwr.pwmclken.write(|w| unsafe {
        //     w.bits(
        //         driverlib::RFC_PWR_PWMCLKEN_RFCTRC
        //             | driverlib::RFC_PWR_PWMCLKEN_FSCA
        //             | driverlib::RFC_PWR_PWMCLKEN_PHA
        //             | driverlib::RFC_PWR_PWMCLKEN_RAT
        //             | driverlib::RFC_PWR_PWMCLKEN_RFERAM
        //             | driverlib::RFC_PWR_PWMCLKEN_RFE
        //             | driverlib::RFC_PWR_PWMCLKEN_MDMRAM
        //             | driverlib::RFC_PWR_PWMCLKEN_MDM
        //             | driverlib::RFC_PWR_PWMCLKEN_CPERAM
        //             | driverlib::RFC_PWR_PWMCLKEN_CPE
        //             | driverlib::RFC_PWR_PWMCLKEN_RFC,
        //     )
        // });
        unsafe { driverlib::RFCClockEnable() }

        self.ping().unwrap();

        self.setup().unwrap();

        // Switch to OSC from RC is needed before starting RAT.
        while unsafe { !driverlib::OSCHF_AttemptToSwitchToXosc() } {}
        self.start_rat().unwrap();

        // Not to catch interrupts from before
        self.clear_pending_interrupts();

        // Begin receiving procedure.
        self.enable_interrupts();
        // self.start_synthesizer().unwrap();
        self.rx().unwrap();

        Ok(())
    }

    fn radio_off(&self) -> Result<(), ErrorCode> {
        self.disable_interrupts();
        // kernel::debug!("interrupts disabled");
        if self.is_on() {
            unsafe { driverlib::RFCSynthPowerDown() }
            // self.stop_synthesizer().unwrap();
            // kernel::debug!("synth powered down");
            self.stop_rat().unwrap();
            // kernel::debug!("RAT stopped");
        }
        unsafe { driverlib::RFCClockDisable() }
        // kernel::debug!("clocks disabled");

        /* We pulled the plug, so we need to restore the status manually */
        self.rx_cmd.borrow_mut().status = cmd::RADIO_OP_STATUS_IDLE;

        self.rx_machinery.poweroff_cleanup();

        let prcm = unsafe { &*cc2650::PRCM::ptr() };

        // Clock gating
        Clock::disable_clocks(prcm, Clocks::empty().rfc());

        // Power domain
        let domains = crate::prcm::PowerDomains::empty().rfc();
        unsafe { driverlib::PRCMPowerDomainOff(domains.into()) };

        Ok(())
    }

    fn radio_initialize(&self) {}
}

impl<'a> RadioConfig<'a> for Radio<'a> {
    fn initialize(&self) -> Result<(), ErrorCode> {
        self.radio_initialize();
        Ok(())
    }

    fn reset(&self) -> Result<(), ErrorCode> {
        self.radio_off()?;

        Ok(())
    }

    fn start(&self) -> Result<(), ErrorCode> {
        self.radio_on()?;

        // Configure deferred call to trigger callback.
        self.deferred_call_operation
            .set(DeferredOperation::PowerClientCallback);
        self.deferred_call.set();

        Ok(())
    }

    fn stop(&self) -> Result<(), ErrorCode> {
        self.radio_off()?;

        // Configure deferred call to trigger callback.
        self.deferred_call_operation
            .set(DeferredOperation::PowerClientCallback);
        self.deferred_call.set();

        Ok(())
    }

    fn is_on(&self) -> bool {
        unsafe { driverlib::PRCMRfReady() }
    }

    fn busy(&self) -> bool {
        self.is_transmitting().unwrap() || self.is_receiving().unwrap()
    }

    fn set_power_client(&self, client: &'a dyn PowerClient) {
        self.power_client.set(client);
    }

    fn config_commit(&self) {
        // self.radio_initialize();

        // Enable deferred call so we can generate a `ConfigClient` callback.
        self.deferred_call_operation
            .set(DeferredOperation::ConfigClientCallback);
        self.deferred_call.set();
    }

    /// Set the client that is called when configuration finishes.
    fn set_config_client(&self, client: &'a dyn radio::ConfigClient) {
        self.config_client.set(client)
    }

    //#################################################
    /// Accessors
    //#################################################

    fn get_address(&self) -> u16 {
        self.addr.get()
    }

    fn get_address_long(&self) -> [u8; 8] {
        self.addr_long.get()
    }

    fn get_pan(&self) -> u16 {
        self.pan.get()
    }

    fn get_tx_power(&self) -> i8 {
        self.tx_power.get().dbm
    }

    fn get_channel(&self) -> u8 {
        self.channel.get().get_channel_number()
    }

    //#################################################
    /// Mutators
    //#################################################

    fn set_address(&self, addr: u16) {
        self.addr.set(addr);
    }

    fn set_address_long(&self, addr: [u8; 8]) {
        self.addr_long.set(addr);
    }

    fn set_pan(&self, id: u16) {
        self.pan.set(id);
    }

    fn set_tx_power(&self, power: i8) -> Result<(), ErrorCode> {
        let new_cfg = get_power_cfg(power).ok_or(ErrorCode::NOSUPPORT)?;

        self.tx_power.set(new_cfg);
        Ok(())
    }

    fn set_channel(&self, chan: RadioChannel) {
        self.channel.set(chan);
    }
}

/// Send and receive packets with the 802.15.4 radio.
impl<'a> RadioData<'a> for Radio<'a> {
    fn set_transmit_client(&self, client: &'a dyn radio::TxClient) {
        self.tx_client.set(client);
    }

    fn set_receive_client(&self, client: &'a dyn radio::RxClient) {
        self.rx_client.set(client);
    }

    fn set_receive_buffer(&self, buffer: &'static mut [u8]) {
        self.rx_machinery
            .set_higher_layer_buffer(buffer, self.is_on());
    }

    fn transmit(
        &self,
        buf: &'static mut [u8],
        frame_len: usize,
    ) -> Result<(), (ErrorCode, &'static mut [u8])> {
        if self.tx_buf.is_some() {
            // tx_buf TakeCell is only occupied when a transmission is underway. This
            // check ensures we do not interrupt an ungoing transmission
            return Err((ErrorCode::BUSY, buf));
        } else if radio::PSDU_OFFSET + frame_len >= buf.len() {
            // Not enough room for CRC
            return Err((ErrorCode::SIZE, buf));
        } else if !self.is_on() {
            return Err((ErrorCode::OFF, buf));
        }
        let frame_len = if let Ok(len) = u8::try_from(frame_len) {
            len
        } else {
            return Err((ErrorCode::INVAL, buf));
        };

        self.tx(buf, frame_len).unwrap();

        Ok(())
    }
}

impl DeferredCallClient for Radio<'_> {
    fn handle_deferred_call(&self) {
        // On deferred call we trigger the config or power callbacks. The
        // `.take()` ensures we clear what is pending.
        kernel::debug!("RADIO: Handling deferred call");
        self.deferred_call_operation.take().map(|op| match op {
            DeferredOperation::ConfigClientCallback => {
                self.config_client.map(|client| {
                    client.config_done(Ok(()));
                });
            }
            DeferredOperation::PowerClientCallback => {
                self.power_client.map(|client| {
                    client.changed(self.is_on());
                });
            }
        });
    }

    fn register(&'static self) {
        self.deferred_call.register(self);
    }
}