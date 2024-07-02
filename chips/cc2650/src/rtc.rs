pub struct Rtc {
    aon_rtc: cc2650::AON_RTC,
}

impl Rtc {
    pub fn new(aon_rtc: cc2650::AON_RTC) -> Self {
        aon_rtc
            .ctl
            .modify(|_r, w| w.en().set_bit().rtc_upd_en().set_bit());

        Self { aon_rtc }
    }
}
