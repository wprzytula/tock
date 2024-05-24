// Licensed under the Apache License, Version 2.0 or the MIT License.
// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright Tock Contributors 2022.

pub use nrf52::gpio::{GPIOPin, Pin, Port};

pub const NUM_PINS: usize = 48;

pub const fn nrf52840_gpio_create<'a>() -> Port<'a, NUM_PINS> {
    Port::new([
        GPIOPin::new(Pin::P0_00),
        GPIOPin::new(Pin::P0_01),
        GPIOPin::new(Pin::P0_02),
        GPIOPin::new(Pin::P0_03),
        GPIOPin::new(Pin::P0_04),
        GPIOPin::new(Pin::P0_05),
        GPIOPin::new(Pin::P0_06),
        GPIOPin::new(Pin::P0_07),
        GPIOPin::new(Pin::P0_08),
        GPIOPin::new(Pin::P0_09),
        GPIOPin::new(Pin::P0_10),
        GPIOPin::new(Pin::P0_11),
        GPIOPin::new(Pin::P0_12),
        GPIOPin::new(Pin::P0_13),
        GPIOPin::new(Pin::P0_14),
        GPIOPin::new(Pin::P0_15),
        GPIOPin::new(Pin::P0_16),
        GPIOPin::new(Pin::P0_17),
        GPIOPin::new(Pin::P0_18),
        GPIOPin::new(Pin::P0_19),
        GPIOPin::new(Pin::P0_20),
        GPIOPin::new(Pin::P0_21),
        GPIOPin::new(Pin::P0_22),
        GPIOPin::new(Pin::P0_23),
        GPIOPin::new(Pin::P0_24),
        GPIOPin::new(Pin::P0_25),
        GPIOPin::new(Pin::P0_26),
        GPIOPin::new(Pin::P0_27),
        GPIOPin::new(Pin::P0_28),
        GPIOPin::new(Pin::P0_29),
        GPIOPin::new(Pin::P0_30),
        GPIOPin::new(Pin::P0_31),
        GPIOPin::new(Pin::P1_00),
        GPIOPin::new(Pin::P1_01),
        GPIOPin::new(Pin::P1_02),
        GPIOPin::new(Pin::P1_03),
        GPIOPin::new(Pin::P1_04),
        GPIOPin::new(Pin::P1_05),
        GPIOPin::new(Pin::P1_06),
        GPIOPin::new(Pin::P1_07),
        GPIOPin::new(Pin::P1_08),
        GPIOPin::new(Pin::P1_09),
        GPIOPin::new(Pin::P1_10),
        GPIOPin::new(Pin::P1_11),
        GPIOPin::new(Pin::P1_12),
        GPIOPin::new(Pin::P1_13),
        GPIOPin::new(Pin::P1_14),
        GPIOPin::new(Pin::P1_15),
    ])
}
