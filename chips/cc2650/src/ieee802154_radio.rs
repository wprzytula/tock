use crate::driverlib;
use crate::gpt::Gpt;
use core::cell::Cell;
use kernel::hil::radio::{self, PowerClient, RadioChannel, RadioConfig as _, RadioData};
use kernel::hil::time::{Alarm, AlarmClient, Time};
use kernel::utilities::cells::{OptionalCell, TakeCell};
use kernel::ErrorCode;

mod cmd {
    use crate::driverlib;

    #[must_use]
    #[repr(u32)]
    #[derive(Debug)]
    pub(super) enum RadioCmdStatus {
        /// The command has not been parsed.
        Pending = 0x00,
        /// Immediate command: The command finished successfully. Radio operation command: The command was successfully submitted for execution.
        Done = 0x01,
        /// The pointer signaled in CMDR is not valid.
        IllegalPointer = 0x81,
        /// The command ID number in the command structure is unknown.
        UnknownCommand = 0x82,
        /// The command number for a direct command is unknown, or the command is not a direct command.
        UnknownDirCommand = 0x83,
        /// An immediate or direct command was issued in a context where it is not supported.
        ContextError = 0x85,
        /// A radio operation command was attempted to be scheduled while another operation was already running in the RF core. The new command is rejected, while the command already running is not impacted.
        SchedulingError = 0x86,
        /// There were errors in the command parameters that are parsed on submission. For radio operation commands, errors in parameters parsed after start of the command are signaled by the command ending, and an error is indicated in the status field of that command structure.
        ParError = 0x87,
        /// An operation on a data entry queue was attempted, but the operation was not supported by the queue in its current state.
        QueueError = 0x88,
        /// An operation on a data entry was attempted while that entry was busy.
        QueueBusy = 0x89,
    }

    pub(super) type RadioCmdResult<T> = Result<T, RadioCmdStatus>;

    pub(super) trait RadioCommand {
        fn send(&mut self) -> RadioCmdResult<()> {
            // Contiki-NG impl:
            /*     uint_fast8_t
            rf_core_send_cmd(uint32_t cmd, uint32_t *status)
            {
              uint32_t timeout_count = 0;
              bool interrupts_disabled;
              bool is_radio_op = false;

              /* reset the status variables to invalid values */
              last_cmd_status = (uint32_t)-1;
              *status = last_cmd_status;

              /*
               * If cmd is 4-byte aligned, then it's either a radio OP or an immediate
               * command. Clear the status field if it's a radio OP
               */
              if((cmd & 0x03) == 0) {
                uint32_t cmd_type;
                cmd_type = ((rfc_command_t *)cmd)->commandNo & RF_CORE_COMMAND_TYPE_MASK;
                if(cmd_type == RF_CORE_COMMAND_TYPE_IEEE_FG_RADIO_OP ||
                   cmd_type == RF_CORE_COMMAND_TYPE_RADIO_OP) {
                  is_radio_op = true;
                  ((rfc_radioOp_t *)cmd)->status = RF_CORE_RADIO_OP_STATUS_IDLE;
                }
              }

              /*
               * Make sure ContikiMAC doesn't turn us off from within an interrupt while
               * we are accessing RF Core registers
               */
              interrupts_disabled = ti_lib_int_master_disable();

              if(!rf_core_is_accessible()) {
                PRINTF("rf_core_send_cmd: RF was off\n");
                if(!interrupts_disabled) {
                  ti_lib_int_master_enable();
                }
                return RF_CORE_CMD_ERROR;
              }

              if(is_radio_op) {
                uint16_t command_no = ((rfc_radioOp_t *)cmd)->commandNo;
                if((command_no & RF_CORE_COMMAND_PROTOCOL_MASK) != RF_CORE_COMMAND_PROTOCOL_COMMON &&
                   (command_no & RF_CORE_COMMAND_TYPE_MASK) == RF_CORE_COMMAND_TYPE_RADIO_OP) {
                  last_radio_op = (rfc_radioOp_t *)cmd;
                }
              }

              HWREG(RFC_DBELL_BASE + RFC_DBELL_O_CMDR) = cmd;
              do {
                last_cmd_status = HWREG(RFC_DBELL_BASE + RFC_DBELL_O_CMDSTA);
                if(++timeout_count > 50000) {
                  PRINTF("rf_core_send_cmd: 0x%08lx Timeout\n", cmd);
                  if(!interrupts_disabled) {
                    ti_lib_int_master_enable();
                  }
                  *status = last_cmd_status;
                  return RF_CORE_CMD_ERROR;
                }
              } while((last_cmd_status & RF_CORE_CMDSTA_RESULT_MASK) == RF_CORE_CMDSTA_PENDING);

              if(!interrupts_disabled) {
                ti_lib_int_master_enable();
              }

              /*
               * If we reach here the command is no longer pending. It is either completed
               * successfully or with error
               */
              *status = last_cmd_status;
              return (last_cmd_status & RF_CORE_CMDSTA_RESULT_MASK) == RF_CORE_CMDSTA_DONE;
            } */

            unsafe {
                let status: RadioCmdStatus = core::mem::transmute(driverlib::RFCDoorbellSendTo(
                    self as *mut Self as *mut () as u32,
                ));
                match status {
                    RadioCmdStatus::Pending => unreachable!(),
                    RadioCmdStatus::Done => RadioCmdResult::Ok(()),
                    err => Err(err),
                }
            }
        }
    }

    pub(crate) use driverlib::rfc_CMD_RADIO_SETUP_s as RfcRadioSetup;
    impl RadioCommand for RfcRadioSetup {}

    pub(crate) use driverlib::rfc_CMD_FS_POWERUP_s as RfcFsPowerup;
    impl RadioCommand for RfcFsPowerup {}

    pub(crate) use driverlib::rfc_CMD_IEEE_RX_s as RfcIeeeRx;
    impl RadioCommand for RfcIeeeRx {}

    pub(crate) use driverlib::rfc_CMD_IEEE_CCA_REQ_s as RfcIeeeCcaReq;
    impl RadioCommand for RfcIeeeCcaReq {}

    pub(crate) use driverlib::rfc_CMD_IEEE_TX_s as RfcIeeeTx;
    impl RadioCommand for RfcIeeeTx {}

    pub(crate) const RF_CORE_CMD_CCA_REQ_RSSI_UNKNOWN: i8 = -128;

    pub(crate) const RF_CORE_CMD_CCA_REQ_CCA_STATE_IDLE     : u8 = 0 /* 00 */;
    pub(crate) const RF_CORE_CMD_CCA_REQ_CCA_STATE_BUSY     : u8 = 1 /* 01 */;
    pub(crate) const RF_CORE_CMD_CCA_REQ_CCA_STATE_INVALID  : u8 = 2 /* 10 */;
}

use driverlib::dataQueue_t as RfcQueue;
use driverlib::rfc_ieeeRxOutput_s as RfcRxOutput;
use tock_cells::volatile_cell::VolatileCell;

use self::cmd::{RadioCmdStatus, RadioCommand};

/*---------------------------------------------------------------------------*/
/* TX Power dBm lookup table - values from SmartRF Studio */
#[derive(Clone, Copy)]
struct PowerOutputConfig {
    dbm: i8,
    tx_power: u16, /* Value for the CMD_RADIO_SETUP.txPower field */
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
static OUTPUT_POWER_MIN: PowerOutputConfig = OUTPUT_POWER[OUTPUT_CONFIG_COUNT - 1];
static OUTPUT_POWER_MAX: PowerOutputConfig = OUTPUT_POWER[0];
const OUTPUT_POWER_UNKNOWN: i8 = 0xFFu8 as i8;

#[derive(Debug, Clone, Copy)]
enum RadioState {
    OFF,
    TX,
    RX,
    ACK,
}

// static mut RX_RESULT: RfcRxOutput = Default::default();
// static mut RX_QUEUE: RfcRxOutput = Default::default();

pub struct Radio<'a> {
    rfc_pwr: cc2650::RFC_PWR,
    rfc_dbell: cc2650::RFC_DBELL,

    tx_power: Cell<PowerOutputConfig>,
    rx_client: OptionalCell<&'a dyn radio::RxClient>,
    tx_client: OptionalCell<&'a dyn radio::TxClient>,
    tx_buf: TakeCell<'static, [u8]>,
    rx_buf: TakeCell<'static, [u8]>,
    // ack_buf: TakeCell<'static, [u8]>,
    addr: Cell<u16>,
    addr_long: Cell<[u8; 8]>,
    pan: Cell<u16>,
    cca_count: Cell<u8>,
    cca_be: Cell<u8>,
    random_nonce: Cell<u32>,
    channel: Cell<RadioChannel>,
    timer0: OptionalCell<&'a Gpt<'a>>,
    state: Cell<RadioState>,

    rx_result: VolatileCell<RfcRxOutput>,
    rx_queue: VolatileCell<RfcRxOutput>,
}

impl<'a> Radio<'a> {
    pub fn new(rfc_pwr: cc2650::RFC_PWR, rfc_dbell: cc2650::RFC_DBELL) -> Self {
        Self {
            rfc_pwr,
            rfc_dbell,

            tx_power: Cell::new(OUTPUT_POWER_MAX),
            rx_client: OptionalCell::empty(),
            tx_client: OptionalCell::empty(),
            tx_buf: TakeCell::empty(),
            rx_buf: TakeCell::empty(),
            addr: Cell::new(0),
            addr_long: Cell::new([0x00; 8]),
            pan: Cell::new(0),
            cca_count: Cell::new(0),
            cca_be: Cell::new(0),
            random_nonce: Cell::new(0xDEADBEEF),
            channel: Cell::new(RadioChannel::Channel26),
            timer0: OptionalCell::empty(),
            state: Cell::new(RadioState::OFF),
            rx_result: Default::default(),
            rx_queue: Default::default(),
        }
    }

    // Contiki-NG power change routines
    /* fn rf_core_power_up() {
        uint32_t cmd_status;
        bool interrupts_disabled = ti_lib_int_master_disable();

        ti_lib_int_pend_clear(INT_RFC_CPE_0);
        ti_lib_int_pend_clear(INT_RFC_CPE_1);
        ti_lib_int_disable(INT_RFC_CPE_0);
        ti_lib_int_disable(INT_RFC_CPE_1);

        /* Enable RF Core power domain */
        ti_lib_prcm_power_domain_on(PRCM_DOMAIN_RFCORE);
        while(ti_lib_prcm_power_domain_status(PRCM_DOMAIN_RFCORE)
                != PRCM_DOMAIN_POWER_ON);

        ti_lib_prcm_domain_enable(PRCM_DOMAIN_RFCORE);
        ti_lib_prcm_load_set();
        while(!ti_lib_prcm_load_get());

        HWREG(RFC_DBELL_NONBUF_BASE + RFC_DBELL_O_RFCPEIFG) = 0x0;
        HWREG(RFC_DBELL_NONBUF_BASE + RFC_DBELL_O_RFCPEIEN) = 0x0;
        ti_lib_int_enable(INT_RFC_CPE_0);
        ti_lib_int_enable(INT_RFC_CPE_1);

        if(!interrupts_disabled) {
            ti_lib_int_master_enable();
        }

        rf_switch_power_up();

        /* Let CPE boot */
        HWREG(RFC_PWR_NONBUF_BASE + RFC_PWR_O_PWMCLKEN) = RF_CORE_CLOCKS_MASK;

        /* Turn on additional clocks on boot */
        HWREG(RFC_DBELL_BASE + RFC_DBELL_O_RFACKIFG) = 0;
        HWREG(RFC_DBELL_BASE+RFC_DBELL_O_CMDR) =
            CMDR_DIR_CMD_2BYTE(RF_CMD0,
                            RFC_PWR_PWMCLKEN_MDMRAM | RFC_PWR_PWMCLKEN_RFERAM);

        /* Send ping (to verify RFCore is ready and alive) */
        if(rf_core_send_cmd(CMDR_DIR_CMD(CMD_PING), &cmd_status) != RF_CORE_CMD_OK) {
            PRINTF("rf_core_power_up: CMD_PING fail, CMDSTA=0x%08lx\n", cmd_status);
            return RF_CORE_CMD_ERROR;
        }

        return RF_CORE_CMD_OK;
    } */
    /*---------------------------------------------------------------------------*/
    /* uint8_t
    rf_core_start_rat(void)
    {
    uint32_t cmd_status;
    rfc_CMD_SYNC_START_RAT_t cmd_start;

    /* Start radio timer (RAT) */
    rf_core_init_radio_op((rfc_radioOp_t *)&cmd_start, sizeof(cmd_start), CMD_SYNC_START_RAT);

    /* copy the value and send back */
    cmd_start.rat0 = rat_offset;

    if(rf_core_send_cmd((uint32_t)&cmd_start, &cmd_status) != RF_CORE_CMD_OK) {
        PRINTF("rf_core_get_rat_rtc_offset: SYNC_START_RAT fail, CMDSTA=0x%08lx\n",
            cmd_status);
        return RF_CORE_CMD_ERROR;
    }

    /* Wait until done (?) */
    if(rf_core_wait_cmd_done(&cmd_start) != RF_CORE_CMD_OK) {
        PRINTF("rf_core_cmd_ok: SYNC_START_RAT wait, CMDSTA=0x%08lx, status=0x%04x\n",
            cmd_status, cmd_start.status);
        return RF_CORE_CMD_ERROR;
    }

    return RF_CORE_CMD_OK;
    } */
    /*---------------------------------------------------------------------------*/
    /* uint8_t
    rf_core_stop_rat(void)
    {
    rfc_CMD_SYNC_STOP_RAT_t cmd_stop;
    uint32_t cmd_status;

    rf_core_init_radio_op((rfc_radioOp_t *)&cmd_stop, sizeof(cmd_stop), CMD_SYNC_STOP_RAT);

    int ret = rf_core_send_cmd((uint32_t)&cmd_stop, &cmd_status);
    if(ret != RF_CORE_CMD_OK) {
        PRINTF("rf_core_get_rat_rtc_offset: SYNC_STOP_RAT fail, ret %d CMDSTA=0x%08lx\n",
            ret, cmd_status);
        return ret;
    }

    /* Wait until done */
    ret = rf_core_wait_cmd_done(&cmd_stop);
    if(ret != RF_CORE_CMD_OK) {
        PRINTF("rf_core_cmd_ok: SYNC_STOP_RAT wait, CMDSTA=0x%08lx, status=0x%04x\n",
            cmd_status, cmd_stop.status);
        return ret;
    }

    if(!rat_offset_known) {
        /* save the offset, but only if this is the first time */
        rat_offset_known = true;
        rat_offset = cmd_stop.rat0;
    }

    return RF_CORE_CMD_OK;
    } */
    /*---------------------------------------------------------------------------*/
    /* void
    rf_core_power_down()
    {
    bool interrupts_disabled = ti_lib_int_master_disable();
    ti_lib_int_disable(INT_RFC_CPE_0);
    ti_lib_int_disable(INT_RFC_CPE_1);

    if(rf_core_is_accessible()) {
        HWREG(RFC_DBELL_NONBUF_BASE + RFC_DBELL_O_RFCPEIFG) = 0x0;
        HWREG(RFC_DBELL_NONBUF_BASE + RFC_DBELL_O_RFCPEIEN) = 0x0;

        /* need to send FS_POWERDOWN or analog components will use power */
        fs_powerdown();
    }

    rf_core_stop_rat();

    /* Shut down the RFCORE clock domain in the MCU VD */
    ti_lib_prcm_domain_disable(PRCM_DOMAIN_RFCORE);
    ti_lib_prcm_load_set();
    while(!ti_lib_prcm_load_get());

    /* Turn off RFCORE PD */
    ti_lib_prcm_power_domain_off(PRCM_DOMAIN_RFCORE);
    while(ti_lib_prcm_power_domain_status(PRCM_DOMAIN_RFCORE)
            != PRCM_DOMAIN_POWER_OFF);

    rf_switch_power_down();

    ti_lib_int_pend_clear(INT_RFC_CPE_0);
    ti_lib_int_pend_clear(INT_RFC_CPE_1);
    ti_lib_int_enable(INT_RFC_CPE_0);
    ti_lib_int_enable(INT_RFC_CPE_1);
    if(!interrupts_disabled) {
        ti_lib_int_master_enable();
    }
    } */

    fn setup(&self) -> cmd::RadioCmdResult<()> {
        let mut cmd = cmd::RfcRadioSetup {
            commandNo: driverlib::CMD_RADIO_SETUP as u16,
            status: 0,
            pNextOp: core::ptr::null_mut(),
            startTime: 0,
            startTrigger: driverlib::rfc_CMD_RADIO_SETUP_s__bindgen_ty_1 {
                _bitfield_1: driverlib::rfc_CMD_RADIO_SETUP_s__bindgen_ty_1::new_bitfield_1(
                    0, 0, 0, 0,
                ),
                ..Default::default()
            },
            condition: driverlib::rfc_CMD_RADIO_SETUP_s__bindgen_ty_2 {
                _bitfield_1: driverlib::rfc_CMD_RADIO_SETUP_s__bindgen_ty_2::new_bitfield_1(0, 0),
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
            txPower: self.tx_power.get().tx_power,
            pRegOverride: core::ptr::null_mut(),
        };
        cmd.send()
    }

    fn start_synthesizer(&self) -> cmd::RadioCmdResult<()> {
        let mut cmd = cmd::RfcFsPowerup {
            commandNo: driverlib::CMD_FS_POWERUP as u16,
            status: 0,
            pNextOp: core::ptr::null_mut(),
            startTime: 0,
            startTrigger: driverlib::rfc_CMD_FS_POWERUP_s__bindgen_ty_1 {
                _bitfield_1: driverlib::rfc_CMD_FS_POWERUP_s__bindgen_ty_1::new_bitfield_1(
                    0, 0, 0, 0,
                ),
                ..Default::default()
            },
            condition: driverlib::rfc_CMD_FS_POWERUP_s__bindgen_ty_2 {
                _bitfield_1: driverlib::rfc_CMD_FS_POWERUP_s__bindgen_ty_2::new_bitfield_1(0, 0),
                ..Default::default()
            },
            __dummy0: 0,
            pRegOverride: core::ptr::null_mut(),
        };
        cmd.send()
    }

    fn enable_interrupts(&self) {
        self.rfc_dbell
            .rfcpeisl
            .modify(|_r, w| w.rx_data_written().cpe0());

        self.change_interrupts_state::<true>()
    }

    fn disable_interrupts(&self) {
        self.change_interrupts_state::<false>()
    }

    fn change_interrupts_state<const ON: bool>(&self) {
        self.rfc_dbell
            .rfcpeien
            .modify(|_r, w| w.rx_data_written().bit(ON).last_command_done().bit(ON))
    }

    pub(crate) fn handle_interrupt_cpe0(&self) {}

    pub(crate) fn handle_interrupt_cpe1(&self) {
        unreachable!("interrupt cpe1 is disabled");
    }

    fn rx(&self) -> cmd::RadioCmdResult<()> {
        let mut cmd = cmd::RfcIeeeRx {
            commandNo: driverlib::CMD_RADIO_SETUP as u16,
            status: 0,
            pNextOp: core::ptr::null_mut(),
            startTime: 0,
            startTrigger: driverlib::rfc_CMD_IEEE_RX_s__bindgen_ty_1 {
                _bitfield_1: driverlib::rfc_CMD_IEEE_RX_s__bindgen_ty_1::new_bitfield_1(0, 0, 0, 0),
                ..Default::default()
            },
            condition: driverlib::rfc_CMD_IEEE_RX_s__bindgen_ty_2 {
                _bitfield_1: driverlib::rfc_CMD_IEEE_RX_s__bindgen_ty_2::new_bitfield_1(0, 0),
                ..Default::default()
            },
            channel: self.get_channel(),
            rxConfig: driverlib::rfc_CMD_IEEE_RX_s__bindgen_ty_3 {
                _bitfield_1: driverlib::rfc_CMD_IEEE_RX_s__bindgen_ty_3::new_bitfield_1(
                    0, 0, 0, 0, 0, 0, 0, 0,
                ),
                ..Default::default()
            },
            pRxQ: unsafe { core::mem::transmute(&self.rx_queue) },
            pOutput: unsafe { core::mem::transmute(&self.rx_result) },
            frameFiltOpt: driverlib::rfc_CMD_IEEE_RX_s__bindgen_ty_4 {
                _bitfield_1: driverlib::rfc_CMD_IEEE_RX_s__bindgen_ty_4::new_bitfield_1(
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                ),
                ..Default::default()
            },
            frameTypes: driverlib::rfc_CMD_IEEE_RX_s__bindgen_ty_5 {
                _bitfield_1: driverlib::rfc_CMD_IEEE_RX_s__bindgen_ty_5::new_bitfield_1(
                    0, 0, 0, 0, 0, 0, 0, 0,
                ),
                ..Default::default()
            },
            ccaOpt: driverlib::rfc_CMD_IEEE_RX_s__bindgen_ty_6 {
                _bitfield_1: driverlib::rfc_CMD_IEEE_RX_s__bindgen_ty_6::new_bitfield_1(
                    0, 0, 0, 0, 0, 0,
                ),
                ..Default::default()
            },
            ccaRssiThr: 0, // TODO: is it correct? Then ccaEnergy is always BUSY.
            __dummy0: 0,
            numExtEntries: 0,
            numShortEntries: 0,
            pExtEntryList: core::ptr::null_mut(),
            pShortEntryList: core::ptr::null_mut(),
            localExtAddr: u64::from_ne_bytes(self.get_address_long()),
            localShortAddr: self.get_address(),
            localPanID: self.get_pan(),
            __dummy1: 0,
            __dummy2: 0,
            endTrigger: driverlib::rfc_CMD_IEEE_RX_s__bindgen_ty_7 {
                _bitfield_1: driverlib::rfc_CMD_IEEE_RX_s__bindgen_ty_7::new_bitfield_1(0, 0, 0, 0),
                ..Default::default()
            },
            endTime: 0,
        };
        cmd.send()
    }

    fn cca_req(&self) -> cmd::RadioCmdResult<cmd::RfcIeeeCcaReq> {
        let mut cmd = cmd::RfcIeeeCcaReq {
            commandNo: driverlib::CMD_IEEE_CCA_REQ as u16,
            ..Default::default() // Other fields are read-only.
        };
        cmd.send()?;
        Ok(cmd)
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
        if cca_info.ccaInfo.ccaSync() != 0 {
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

impl<'a> kernel::hil::radio::RadioConfig<'a> for Radio<'a> {
    /// Initialize the radio.
    ///
    /// This should perform any needed initialization, but should not turn the
    /// radio on.
    ///
    /// ## Return
    ///
    /// `Ok(())` on success. On `Err()`, valid errors are:
    ///
    /// - `ErrorCode::FAIL`: Internal error occurred.
    fn initialize(&self) -> Result<(), ErrorCode> {
        unsafe { driverlib::RFCClockEnable() }
        // self.rfc_pwr.pwmclken.write(|w| w.cpe().set_bit())
        self.setup().unwrap();

        Ok(())
    }

    /// Reset the radio.
    ///
    /// Perform a radio reset which may reset the internal state of the radio.
    ///
    /// ## Return
    ///
    /// `Ok(())` on success. On `Err()`, valid errors are:
    ///
    /// - `ErrorCode::FAIL`: Internal error occurred.
    fn reset(&self) -> Result<(), ErrorCode> {
        self.stop()?;
        self.start()?;
        Ok(())
    }

    /// Start the radio.
    ///
    /// This should put the radio into receive mode.
    ///
    /// ## Return
    ///
    /// `Ok(())` on success. On `Err()`, valid errors are:
    ///
    /// - `ErrorCode::FAIL`: Internal error occurred.
    fn start(&self) -> Result<(), ErrorCode> {
        // Begin receiving procedure.
        self.enable_interrupts();
        self.start_synthesizer().unwrap();
        self.rx().unwrap();

        Ok(())
    }

    /// Stop the radio.
    ///
    /// This should turn the radio off, disabling receive mode, and put the
    /// radio into a low power state.
    ///
    /// ## Return
    ///
    /// `Ok(())` on success. On `Err()`, valid errors are:
    ///
    /// - `ErrorCode::FAIL`: Internal error occurred.
    fn stop(&self) -> Result<(), ErrorCode> {
        unsafe { driverlib::RFCSynthPowerDown() }
        self.disable_interrupts();
        unsafe { driverlib::RFCClockDisable() }

        Ok(())
    }

    /// Check if the radio is currently on.
    ///
    /// ## Return
    ///
    /// True if the radio is on, false otherwise.
    fn is_on(&self) -> bool {
        unsafe { driverlib::PRCMRfReady() }
    }

    /// Check if the radio is currently busy transmitting or receiving a packet.
    ///
    /// If this returns `true`, the radio is unable to start another operation.
    ///
    /// ## Return
    ///
    /// True if the radio is busy, false otherwise.
    fn busy(&self) -> bool {
        self.is_transmitting().unwrap() || self.is_receiving().unwrap()
    }

    /// Set the client that is called when the radio changes power states.
    fn set_power_client(&self, _client: &'a dyn PowerClient) {
        // happily ignored :)
    }

    /// Commit the configuration calls to the radio.
    ///
    /// This will set the address, PAN ID, TX power, and channel to the
    /// specified values within the radio hardware. When this finishes, this
    /// will issue a callback to the config client when done.
    fn config_commit(&self) {
        self.stop().unwrap();
        self.initialize().unwrap();
    }

    /// Set the client that is called when configuration finishes.
    fn set_config_client(&self, _client: &'a dyn radio::ConfigClient) {
        // happily ignored :)
    }

    //#################################################
    /// Accessors
    //#################################################

    /// Get the 802.15.4 short (16-bit) address for the radio.
    ///
    /// ## Return
    ///
    /// The radio's short address.
    fn get_address(&self) -> u16 {
        self.addr.get()
    }

    /// Get the 802.15.4 extended (64-bit) address for the radio.
    ///
    /// ## Return
    ///
    /// The radio's extended address.
    fn get_address_long(&self) -> [u8; 8] {
        self.addr_long.get()
    }

    /// Get the 802.15.4 16-bit PAN ID for the radio.
    ///
    /// ## Return
    ///
    /// The radio's PAN ID.
    fn get_pan(&self) -> u16 {
        self.pan.get()
    }

    /// Get the radio's transmit power.
    ///
    /// ## Return
    ///
    /// The transmit power setting used by the radio, in dBm.
    fn get_tx_power(&self) -> i8 {
        self.tx_power.get().dbm
    }

    /// Get the 802.15.4 channel the radio is currently using.
    ///
    /// ## Return
    ///
    /// The channel number.
    fn get_channel(&self) -> u8 {
        self.channel.get().get_channel_number()
    }

    //#################################################
    /// Mutators
    //#################################################

    /// Set the 802.15.4 short (16-bit) address for the radio.
    ///
    /// Note, calling this function configures the software driver, but does not
    /// take effect in the radio hardware. Call `RadioConfig::config_commit()`
    /// to set the configuration settings in the radio hardware.
    ///
    /// ## Argument
    ///
    /// - `addr`: The short address.
    fn set_address(&self, addr: u16) {
        self.addr.set(addr);
    }

    /// Set the 802.15.4 extended (64-bit) address for the radio.
    ///
    /// Note, calling this function configures the software driver, but does not
    /// take effect in the radio hardware. Call `RadioConfig::config_commit()`
    /// to set the configuration settings in the radio hardware.
    ///
    /// ## Argument
    ///
    /// - `addr`: The extended address.
    fn set_address_long(&self, addr: [u8; 8]) {
        self.addr_long.set(addr);
    }

    /// Set the 802.15.4 PAN ID (16-bit) for the radio.
    ///
    /// Note, calling this function configures the software driver, but does not
    /// take effect in the radio hardware. Call `RadioConfig::config_commit()`
    /// to set the configuration settings in the radio hardware.
    ///
    /// ## Argument
    ///
    /// - `id`: The PAN ID.
    fn set_pan(&self, id: u16) {
        self.pan.set(id);
    }

    /// Set the radio's transmit power.
    ///
    /// Note, calling this function configures the software driver, but does not
    /// take effect in the radio hardware. Call `RadioConfig::config_commit()`
    /// to set the configuration settings in the radio hardware.
    ///
    /// ## Argument
    ///
    /// - `power`: The transmit power in dBm.
    ///
    /// ## Return
    ///
    /// `Ok(())` on success. On `Err()`, valid errors are:
    ///
    /// - `ErrorCode::INVAL`: The transmit power is above acceptable limits.
    /// - `ErrorCode::NOSUPPORT`: The transmit power is not supported by the
    ///   radio.
    /// - `ErrorCode::FAIL`: Internal error occurred.
    fn set_tx_power(&self, power: i8) -> Result<(), ErrorCode> {
        let new_cfg = OUTPUT_POWER
            .iter()
            .copied()
            .find(|cfg| cfg.dbm == power)
            .ok_or(ErrorCode::INVAL)?;

        self.tx_power.set(new_cfg);
        Ok(())
    }

    /// Set the 802.15.4 channel for the radio.
    ///
    /// Note, calling this function configures the software driver, but does not
    /// take effect in the radio hardware. Call `RadioConfig::config_commit()`
    /// to set the configuration settings in the radio hardware.
    ///
    /// ## Argument
    ///
    /// - `chan`: The 802.15.4 channel.
    fn set_channel(&self, chan: RadioChannel) {
        self.channel.set(chan);
    }
}

/// Send and receive packets with the 802.15.4 radio.
impl<'a> kernel::hil::radio::RadioData<'a> for Radio<'a> {
    /// Set the client that will be called when packets are transmitted.
    fn set_transmit_client(&self, client: &'a dyn radio::TxClient) {
        self.tx_client.set(client);
    }

    /// Set the client that will be called when packets are received.
    fn set_receive_client(&self, client: &'a dyn radio::RxClient) {
        self.rx_client.set(client);
    }

    /// Set the buffer to receive packets into.
    ///
    /// ## Argument
    ///
    /// - `receive_buffer`: The buffer to receive into. Must be at least
    ///   `MAX_BUF_SIZE` bytes long.
    fn set_receive_buffer(&self, buffer: &'static mut [u8]) {
        self.rx_buf.replace(buffer);
    }

    /// Transmit a packet.
    ///
    /// The radio will create and insert the PHR (Frame length) field.
    ///
    /// ## Argument
    ///
    /// - `buf`: Buffer with the MAC layer 802.15.4 frame to be transmitted.
    ///   The buffer must conform to the buffer formatted documented in the HIL.
    ///   That is, the MAC payload (PSDU) must start at the third byte.
    ///   The first byte must be reserved for the radio driver (i.e.
    ///   for a SPI transaction) and the second byte is reserved for the PHR.
    ///   The buffer must be at least `frame_len` + 2 + MFR_SIZE` bytes long.
    /// - `frame_len`: The length of the MAC payload, not including the MFR.
    ///
    /// ## Return
    ///
    /// `Ok(())` on success. On `Err()`, valid errors are:
    ///
    /// - `ErrorCode::OFF`: The radio is off and cannot transmit.
    /// - `ErrorCode::BUSY`: The radio is busy. This is likely to occur because
    ///   the radio is already transmitting a packet.
    /// - `ErrorCode::SIZE`: The buffer does not have room for the MFR (CRC).
    /// - `ErrorCode::FAIL`: Internal error occurred.
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
        }

        /*
         * We are certainly not TXing a frame as a result of CMD_IEEE_TX, but we may
         * be in the process of TXing an ACK. In that case, wait for the TX to finish
         * or return after approx TX_WAIT_TIMEOUT.
         */
        // t0 = RTIMER_NOW();
        while self.is_transmitting().unwrap()
        // && (RTIMER_CLOCK_LT(RTIMER_NOW(), t0 + RF_CORE_TX_TIMEOUT))
        {}

        let mut cmd = cmd::RfcIeeeTx {
            commandNo: driverlib::CMD_IEEE_TX as u16,
            status: 0,
            pNextOp: core::ptr::null_mut(),
            startTime: 0,
            startTrigger: driverlib::rfc_CMD_IEEE_TX_s__bindgen_ty_1 {
                _bitfield_1: driverlib::rfc_CMD_IEEE_TX_s__bindgen_ty_1::new_bitfield_1(0, 0, 0, 0),
                ..Default::default()
            },
            condition: driverlib::rfc_CMD_IEEE_TX_s__bindgen_ty_2 {
                _bitfield_1: driverlib::rfc_CMD_IEEE_TX_s__bindgen_ty_2::new_bitfield_1(0, 0),
                ..Default::default()
            },
            txOpt: driverlib::rfc_CMD_IEEE_TX_s__bindgen_ty_3 {
                _bitfield_1: driverlib::rfc_CMD_IEEE_TX_s__bindgen_ty_3::new_bitfield_1(0, 0, 0),
                ..Default::default()
            },
            payloadLen: frame_len as u8,
            pPayload: buf[radio::PSDU_OFFSET..].as_mut_ptr(),
            timeStamp: 0,
        };
        cmd.send().unwrap();

        Ok(())
    }
}
