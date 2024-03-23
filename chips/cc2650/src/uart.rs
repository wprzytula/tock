pub fn init_uart() {
    let peripherals = unsafe { cc2650::Peripherals::steal() };
    // 2. Configure the IOC module to map UART signals to the correct GPIO pins.
    // RF1.7_UART_RX EM -> DIO_2
    peripherals
        .IOC
        .iocfg3
        .modify(|_r, w| w.port_id().uart0_tx().ie().clear_bit());
    // RF1.9_UART_TX EM -> DIO_3
    peripherals
        .IOC
        .iocfg2
        .modify(|_r, w| w.port_id().uart0_rx().ie().set_bit());

    // For this example, the UART clock is assumed to be 24 MHz, and the desired UART configuration is:
    // • Baud rate: 115 200
    // • Data length of 8 bits
    // • One stop bit
    // • No parity
    // • FIFOs disabled
    // • No interrupts
    //
    // The first thing to consider when programming the UART is the BRD because the UART:IBRD and
    // UART:FBRD registers must be written before the UART:LCRH register.
    // The BRD can be calculated using the equation:
    //      BRD = 24 000 000 / (16 × 115 200) = 13.0208
    // The result of Equation 3 indicates that the UART:IBRD DIVINT field must be set to 13 decimal or 0xD.
    //
    // Equation 4 calculates the value to be loaded into the UART:FBRD register.
    //      UART:FBRD.DIVFRAC = integer (0.0208 × 64 + 0.5) = 1
    //
    // With the BRD values available, the UART configuration is written to the module in the following order:
    let uart = &peripherals.UART0;

    // 1. Disable the UART by clearing the UART:CTL UARTEN bit.
    uart.ctl.modify(|_r, w| w.uarten().dis());

    // 2. Write the integer portion of the BRD to the UART:IBRD register.
    // uart.ibrd.modify(|_r, w| unsafe { w.divint().bits(13) });
    uart.ibrd.modify(|_r, w| unsafe { w.divint().bits(26) }); // for 48 MHz

    // 3. Write the fractional portion of the BRD to the UART:FBRD register.
    // uart.fbrd.modify(|_r, w| unsafe { w.divfrac().bits(1) });
    uart.fbrd.modify(|_r, w| unsafe { w.divfrac().bits(3) }); // for 48 MHz

    // 4. Write the desired serial parameters to the UART:LCRH register (in this case, a value of 0x0000 0060).
    uart.lcrh.modify(|_r, w| w.pen().dis().wlen()._8());

    // 5. Enable the UART by setting the UART:CTL UARTEN bit.
    uart.ctl
        .modify(|_r, w| w.uarten().en().txe().en().rxe().en());
}
