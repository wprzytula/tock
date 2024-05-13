pub struct Fcfg {
    fcfg: cc2650::FCFG1,
}

impl Fcfg {
    pub(crate) fn new(fcfg: cc2650::FCFG1) -> Self {
        Self { fcfg }
    }

}
