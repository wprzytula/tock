pub struct Fcfg {
    fcfg: cc2650::FCFG1,
}

impl Fcfg {
    pub(crate) fn new(fcfg: cc2650::FCFG1) -> Self {
        Self { fcfg }
    }

    pub fn ieee_mac(&self) -> u64 {
        ((self.fcfg.mac_15_4_1.read().addr_32_63().bits() as u64) << 32)
            + self.fcfg.mac_15_4_0.read().addr_0_31().bits() as u64
    }
}
