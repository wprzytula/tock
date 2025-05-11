// Licensed under the Apache License, Version 2.0 or the MIT License.
// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright Tock Contributors 2024.

//! This build script can be used by Tock board crates based on TI CC2650 chip
//! to ensure that they are rebuilt when there are any changes to the `layout.ld`
//! linker script or any of its `INCLUDE`s. Also, it provides the linker scripts
//! with KERNEL_LEN definition, enabling compile-time configuration
//! of kernel/apps FLASH division.
//!
//! Board crates can use this script from their `Cargo.toml` files:
//!
//! ```toml
//! [package]
//! # ...
//! build = "../path/to/build.rs"
//! ```

use std::fmt::Display;
use std::fs;
use std::path::Path;

const LINKER_SCRIPT: &str = "layout.ld";

const KERNEL_FLASH_LEN_VAR: &str = "KERNEL_FLASH_LEN";
const DEFAULT_KERNEL_FLASH_LEN: usize = 0x12000;

fn main() {
    if !Path::new(LINKER_SCRIPT).exists() {
        panic!("Boards must provide a `layout.ld` link script file");
    }

    track_linker_script(LINKER_SCRIPT);

    // Instruct linker about kernel/apps FLASH division.
    define_kernel_apps_flash_division();
}

/// Track the given linker script and all of its `INCLUDE`s so that the build
/// is rerun when any of them change.
fn track_linker_script<P: AsRef<Path>>(path: P) {
    let path = path.as_ref();

    assert!(path.is_file(), "expected path {path:?} to be a file");

    println!("cargo:rerun-if-changed={}", path.display());

    // Find all the `INCLUDE <relative path>` lines in the linker script.
    let link_script = fs::read_to_string(path).expect("failed to read {path:?}");
    let includes = link_script
        .lines()
        .filter_map(|line| line.strip_prefix("INCLUDE").map(str::trim));

    // Recursively track included linker scripts.
    for include in includes {
        track_linker_script(include);
    }
}

fn define_kernel_apps_flash_division() {
    fn define_linker_symbol(symbol: impl Display, value: impl Display) {
        println!("cargo::rustc-link-arg=--defsym={}={}", symbol, value)
    }

    /// The first byte of the flash.
    const FLASH_BEG: usize = 0x0;
    /// The first byte **behind** the flash.
    const FLASH_END: usize = 0x0001FFA9;
    /// Total capacity of the flash.
    const FLASH_LEN: usize = FLASH_END - FLASH_BEG;

    let kernel_len = std::env::var(KERNEL_FLASH_LEN_VAR)
        .as_deref()
        .map(|kernel_len_str| {
            str::parse::<usize>(kernel_len_str)
                .ok()
                .or_else(|| {
                    let without_prefix = kernel_len_str.trim_start_matches("0x");
                    usize::from_str_radix(without_prefix, 16).ok()
                })
                .expect("Couldn't parse KERNEL_LEN as usize")
        })
        .unwrap_or(DEFAULT_KERNEL_FLASH_LEN);

    let kernel_beg = FLASH_BEG;
    let kernel_end = kernel_beg + kernel_len;

    let apps_len = FLASH_LEN - kernel_len;
    let apps_beg = kernel_end;
    let _apps_end = FLASH_END;

    define_linker_symbol("ROM_BEG", kernel_beg);
    define_linker_symbol("ROM_LEN", kernel_len);
    define_linker_symbol("PROG_BEG", apps_beg);
    define_linker_symbol("PROG_LEN", apps_len);
}
