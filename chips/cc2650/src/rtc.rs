//! REAL-TIME CLOCK

use kernel::{
    hil::time::{Alarm, AlarmClient, Freq32KHz, Ticks as _, Ticks32, Time},
    ErrorCode,
};
use tock_cells::optional_cell::OptionalCell;

use crate::driverlib;

pub struct Rtc<'a> {
    aon_rtc: cc2650::AON_RTC,
    client: OptionalCell<&'a dyn AlarmClient>,
}

impl<'a> Rtc<'a> {
    pub fn new(aon_rtc: cc2650::AON_RTC) -> Self {
        // Modelled after ContikiNG's soc_rtc_init().
        unsafe {
            let interrupts_disabled = driverlib::IntMasterDisable();
            driverlib::AONRTCDisable();
            driverlib::AONRTCEventClear(driverlib::AON_RTC_CH0);
            driverlib::AONRTCEventClear(driverlib::AON_RTC_CH1);
            driverlib::AONRTCEventClear(driverlib::AON_RTC_CH2);
            driverlib::AONEventMcuWakeUpSet(driverlib::AON_EVENT_MCU_WU0, driverlib::AON_RTC_CH0);
            driverlib::AONEventMcuWakeUpSet(driverlib::AON_EVENT_MCU_WU1, driverlib::AON_RTC_CH1);
            driverlib::AONEventMcuWakeUpSet(driverlib::AON_EVENT_MCU_WU2, driverlib::AON_RTC_CH2);
            driverlib::AONRTCCombinedEventConfig(
                driverlib::AON_RTC_CH0 | driverlib::AON_RTC_CH1 | driverlib::AON_RTC_CH2,
            );

            aon_rtc.sec.reset();

            if !interrupts_disabled {
                driverlib::IntMasterEnable();
            }
        }

        aon_rtc.chctl.write(|w| w.ch1_en().set_bit());

        aon_rtc
            .ctl
            .modify(|_r, w| w.en().set_bit().rtc_upd_en().set_bit());

        Self {
            aon_rtc,
            client: OptionalCell::empty(),
        }
    }

    fn sync(&self) {
        self.aon_rtc.sync.read().bits();
    }

    fn ticks_from_secs_and_subsecs(secs: u32, subsecs: u32) -> <Self as Time>::Ticks {
        ((secs << 16) | subsecs >> 16).into()
    }

    fn read_counter(&self) -> <Self as Time>::Ticks {
        /*
            SEC can change during the SUBSEC read, so we need to be certain
            that the SUBSEC we read belong to the correct SEC counterpart.
        */
        loop {
            let secs = self.aon_rtc.sec.read().value().bits();
            let subsecs = self.aon_rtc.subsec.read().value().bits();
            let secs_after_subsecs_read = self.aon_rtc.sec.read().value().bits();
            // kernel::debug!(
            //     "Read RTC: {} secs, {} subsecs -> {} ticks",
            //     secs,
            //     subsecs,
            //     Self::ticks_from_secs_and_subsecs(secs, subsecs).into_u32()
            // );

            // TRM: RTC Channel 1 compare value.
            // Bit 31 to 16 represents seconds and bits 15 to 0 represents
            // subseconds of the compare value.
            // The compare value is compared against SEC.VALUE (15:0) and
            // SUBSEC.VALUE (31:16) values of the Real Time Clock register.
            // A Channel 1 event is generated when
            // {SEC.VALUE(15:0),SUBSEC.VALUE (31:16)} is reaching or exceeding
            // the compare value.
            //
            // So, since only lowest 16 seconds bits and highest 16 subseconds bits
            // are taken into account, we read only them.
            if secs == secs_after_subsecs_read {
                return Self::ticks_from_secs_and_subsecs(secs, subsecs);
            }
        }
    }

    fn arm(&self, ticks: <Self as Time>::Ticks) {
        self.aon_rtc.ctl.modify(|_r, w| w.comb_ev_mask().ch1());
        self.aon_rtc
            .ch1cmp
            .write(|w| unsafe { w.value().bits(ticks.into_u32()) });
        self.aon_rtc.chctl.modify(|_r, w| w.ch1_en().set_bit());
    }

    pub fn handle_interrupt(&self) {
        // Clear event flags
        // kernel::debug!("RTC got an interrupt");
        self.aon_rtc.evflags.write(|w| w.ch1().set_bit());

        self.client.map(|client| {
            client.alarm();
        });
    }
}

impl<'a> Time for Rtc<'a> {
    type Frequency = Freq32KHz;
    type Ticks = Ticks32;

    fn now(&self) -> Self::Ticks {
        self.read_counter()
    }
}

impl<'a> Alarm<'a> for Rtc<'a> {
    fn set_alarm_client(&self, client: &'a dyn AlarmClient) {
        self.client.set(client);
    }

    fn set_alarm(&self, reference: Self::Ticks, dt: Self::Ticks) {
        let _ = self.disarm();

        const SYNC_TICS: u32 = 2;

        let mut expire = reference.wrapping_add(dt);

        let now = self.now();
        let earliest_possible = now.wrapping_add(Self::Ticks::from(SYNC_TICS));

        if !now.within_range(reference, expire) || expire.wrapping_sub(now).into_u32() <= SYNC_TICS
        {
            expire = earliest_possible;
        }

        // kernel::debug!(
        //     "At reference {} asked to set dt {}, setting expire={}",
        //     reference.into_u32(),
        //     dt.into_u32(),
        //     expire.into_u32()
        // );

        self.arm(expire);
    }

    fn get_alarm(&self) -> Self::Ticks {
        self.aon_rtc.ch1cmp.read().value().bits().into()
    }

    fn disarm(&self) -> Result<(), ErrorCode> {
        self.aon_rtc.ctl.modify(|_r, w| w.comb_ev_mask().none());
        self.aon_rtc.chctl.modify(|_r, w| w.ch0_en().clear_bit());
        self.sync();

        Ok(())
    }

    fn is_armed(&self) -> bool {
        self.aon_rtc.chctl.read().ch1_en().bit()
    }

    fn minimum_dt(&self) -> Self::Ticks {
        // As per TRM:
        // When setting a compare event, the compare time must be set at least four SCLK_LF cycles
        // into the future to avoid being delayed until the RTC free-running value wraps around and
        // matches the compare value again.
        const MINIMUM_SUBSECS: u32 = 0x80_000 * 4;

        Self::ticks_from_secs_and_subsecs(0, MINIMUM_SUBSECS)
    }
}
