// Licensed under the Apache License, Version 2.0 or the MIT License.
// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright Tock Contributors 2024.

//! This build script can be used by Tock board crates to ensure that they are
//! rebuilt when there are any changes to the `layout.ld` linker script or any
//! of its `INCLUDE`s.
//!
//! Board crates can use this script from their `Cargo.toml` files:
//!
//! ```toml
//! [package]
//! # ...
//! build = "../path/to/build.rs"
//! ```

use std::fs;
use std::path::{Path, PathBuf};

const LINKER_SCRIPT: &str = "layout.ld";

// Makes linker use symbols from the given ELF. Intended for usage with stripped ROM ELF.
fn fetch_rom_symbols(rom_elf: impl AsRef<Path>) {
    println!(
        "cargo:rustc-link-arg=--just-symbols={}",
        rom_elf.as_ref().to_str().unwrap()
    );
}

fn main() {
    if !Path::new(LINKER_SCRIPT).exists() {
        panic!("Boards must provide a `layout.ld` link script file");
    }

    let out = std::env::var("OUT_DIR").unwrap();
    println!("cargo:rustc-link-search=native={}", &out);
    let out = PathBuf::from(out);
    let shared_dir = out.join("../../../").canonicalize().unwrap();
    fetch_rom_symbols(shared_dir.join("libROM_driverlib_filtered.elf"));
    println!("cargo:rustc-link-arg=-ldriverlib");
    println!("cargo:rustc-link-arg=-lextern");
    println!("cargo:rustc-link-arg=-zmuldefs");

    track_linker_script(LINKER_SCRIPT);
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
