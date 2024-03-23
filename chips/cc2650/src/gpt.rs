//! GENERAL PURPOSE TIMER

use kernel::{
    hil::time::{Frequency as FrequencyTrait, Ticks as _, *},
    ErrorCode,
};
use tock_cells::optional_cell::OptionalCell;

/// 48MHz `Frequency`
#[derive(Debug)]
pub enum Freq48MHz {}
impl FrequencyTrait for Freq48MHz {
    fn frequency() -> u32 {
        48_000_000
    }
}

type Frequency = Freq48MHz;
type Ticks = Ticks32;

pub struct Gpt<'a> {
    /* TMP pub */ pub client: OptionalCell<&'a dyn AlarmClient>,
    gpt: cc2650::GPT0,
}

impl<'a> Gpt<'a> {
    pub fn new() -> Self {
        // Safety: this is the only object that ever accesses GPT0.
        let gpt = unsafe { cc2650::Peripherals::steal().GPT0 };

        // Use 32-bit mode.
        gpt.cfg.write(|w| w.cfg()._32bit_timer());

        gpt.tamr.modify(|_r, w| {
            w.tacintd()
                .dis_to_intr() // Disable time-out event interrupts.
                .tamie()
                .en() // Enable match interrupts.
                .tacdir()
                .up() // Count up.
                .tamr()
                .periodic() // Run in periodic mode.
        });

        Self {
            client: OptionalCell::empty(),
            gpt,
        }
    }

    fn start_counting(&self) {
        self.gpt.ctl.modify(|_r, w| w.taen().en());
    }

    fn stop_counting(&self) {
        self.gpt.ctl.modify(|_r, w| w.taen().dis());
    }

    fn interrupts_enabled(&self) -> bool {
        // We only use match interrupts.
        self.gpt.imr.read().tamim().bit_is_set()
    }

    fn enable_interrupts(&self) {
        self.gpt.imr.modify(|_r, w| w.tamim().en());
    }

    fn disable_interrupts(&self) {
        self.gpt.imr.modify(|_r, w| w.tamim().dis());
    }

    fn get_match_value(&self) -> <Self as Time>::Ticks {
        self.gpt.tamatchr.read().bits().into()
    }

    fn set_match_value(&self, value: <Self as Time>::Ticks) {
        self.gpt
            .tamatchr
            // Safety: any u32 value is valid for the TAMATCHR.
            .write(|w| unsafe { w.bits(value.into_u32()) })
    }

    fn clear_alarm(&self) {
        self.disable_interrupts();
        // Clear interrupt flag
        self.gpt.iclr.modify(|_r, w| w.tamcint().set_bit());
    }

    pub fn handle_interrupt(&self) {
        self.clear_alarm();
        self.client.map(|client| {
            client.alarm();
        });
    }
}

impl<'a> Time for Gpt<'a> {
    type Frequency = Frequency;

    type Ticks = Ticks;

    fn now(&self) -> Self::Ticks {
        self.gpt.tar.read().bits().into()
    }
}

impl<'a> Alarm<'a> for Gpt<'a> {
    fn set_alarm_client(&self, client: &'a dyn AlarmClient) {
        self.client.set(client);
    }

    fn set_alarm(&self, reference: Self::Ticks, dt: Self::Ticks) {
        self.disable_interrupts();

        const SYNC_TICS: u32 = 2;

        let mut expire = reference.wrapping_add(dt);

        let now = self.now();
        let earliest_possible = now.wrapping_add(Self::Ticks::from(SYNC_TICS));

        if !now.within_range(reference, expire) || expire.wrapping_sub(now).into_u32() <= SYNC_TICS
        {
            expire = earliest_possible;
        }

        self.stop_counting();
        self.set_match_value(expire);
        self.enable_interrupts();
        self.start_counting();
    }

    fn get_alarm(&self) -> Self::Ticks {
        self.get_match_value()
    }

    fn disarm(&self) -> Result<(), ErrorCode> {
        self.disable_interrupts();
        Ok(())
    }

    fn is_armed(&self) -> bool {
        self.interrupts_enabled()
    }

    fn minimum_dt(&self) -> Self::Ticks {
        // TODO: not tested, arbitrary value
        Self::Ticks::from(10)
    }
}
