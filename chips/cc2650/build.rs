//! The build script also sets the linker flags to tell it which link script to use.

use std::ffi::OsStr;
use std::fs::File;
use std::io::Write;
use std::iter::FromIterator;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

const LIB_ROM_ORIGINAL: &str = "libROM_driverlib.elf";
const LIB_ROM_FILTERED: &str = "libROM_driverlib_filtered.elf";

const LIB_NOROM_ORIGINAL: &str = "libNOROM_driverlib.a";
const LIB_NOROM_NOPREFIX: &str = "libdriverlib.a";

const LIB_FULL_O: &str = "libdriverlib_full.o";
const LIB_FULL: &str = "libdriverlib_full.a";

const EXTERN_O_NAME: &str = "extern.o";

fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    driverlib_config(&out);
}

fn driverlib_config(out: &PathBuf) {
    // let driverlib_path = env::var("DRIVERLIB_PATH").unwrap_or_else(|_| String::from("driverlib"));
    let driverlib_path = env::var("DRIVERLIB_PATH").unwrap_or_else(|_| {
        String::from("/home/xps15/Studia/Sem8/Tock/driverlib/cc26x0/driverlib")
    });
    let cc26x0_crate_root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let cc26x0_crate_driverlib = cc26x0_crate_root.join("src/driverlib");

    let extern_o_path = generate_bindings(out, &driverlib_path);

    // ROM symbols are fetched first, so that `-zmuldefs` option enabled will first use them
    // if available.
    strip_disabled_rom_fns(out, &driverlib_path, &cc26x0_crate_driverlib);
    // fetch_rom_symbols(cc26x0_crate_driverlib.join(LIB_ROM_FILTERED));

    // NOROM symbols are fetched next, so that `-zmuldefs` option enabled will use them
    // if ROM version is not available.
    transform_norom_symbols(out, &cc26x0_crate_driverlib);

    merge_lib(out, &extern_o_path);

    link_driverlib(out, &cc26x0_crate_root);
}

fn generate_bindings(out: &PathBuf, driverlib_path: &str) -> PathBuf {
    let extern_c_path = out.join("extern.c");

    println!(
        "cargo:rerun-if-changed={}/driverlib_full.h",
        &driverlib_path
    );

    // Create driverlib bindings
    let bindings = bindgen::Builder::default()
        // The input header we would like to generate
        // bindings for.
        .header(format!("{}/driverlib_full.h", driverlib_path))
        // This creates wrapper functions around "static inline" fns to make them available...
        .wrap_static_fns(true)
        // ...and this stores them in the provided path.
        .wrap_static_fns_path(&extern_c_path)
        // Instead of ::str::... qualification, use ::core::...
        .use_core()
        // Don't look for standard C types in ::std; instead, use cty crate.
        .ctypes_prefix("cty")
        // Required to get reasonable function signatures in driverlib headers.
        .clang_arg("-DDOXYGEN")
        // Required in rust-analyzer to succeed in building.
        .clang_arg("-D__GLIBC_USE(...)")
        // Add newlib headers. E.g. <string.h> is required.
        .clang_arg("-I/usr/arm-none-eabi/include")
        // Don't extract doc comments.
        .generate_comments(false)
        // Don't create layout tests - trust bindgen.
        .layout_tests(false)
        // Tell cargo to invalidate the built crate whenever any of the included header files changed.
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        // Finish the builder and generate the bindings.
        .generate()
        // Unwrap the Result and panic on failure.
        .unwrap_or_else(|err| panic!("Unable to generate bindings: {}", err));

    bindings
        .write_to_file("src/driverlib/bindings.rs")
        .expect("Couldn't write bindings!");

    compile_static_inline_extern_fns(out, &extern_c_path)
}

fn compile_static_inline_extern_fns(out: &PathBuf, extern_c_path: &PathBuf) -> PathBuf {
    // let extern_o_path = out.join("extern.o");
    // Compile extern.c containing (formerly) static inline functions
    let extern_bc_path = cc::Build::new()
        .compiler("clang")
        .target("arm-none-eabi")
        .file(extern_c_path)
        .warnings(false)
        .define("DOXYGEN", None)
        .include("/usr/arm-none-eabi/include")
        .flag("-flto=thin")
        .cargo_metadata(false) // We do not link to the chip crate lib, yet to the end board crate binary.
        .compile_intermediates()
        .into_iter()
        .next()
        .unwrap();

    let extern_o_path = out.join(EXTERN_O_NAME);

    // llc --filetype obj blahblah-extern.o -o extern.o
    let status = Command::new("llc")
        .arg("--filetype")
        .arg("obj")
        .arg(&extern_bc_path)
        .arg("-o")
        .arg(&extern_o_path)
        .spawn()
        .unwrap()
        .wait()
        .unwrap();
    assert!(status.success(), "extern.o llc failed");

    extern_o_path
}

fn merge_lib(out: &PathBuf, extern_o_path: &PathBuf) {
    let libdriverlib_o_full_path = out.join(LIB_FULL_O);

    const DRIVERLIB_O_PATH: &str = "driverlib";

    let driverlib_os_path = out.join(DRIVERLIB_O_PATH);

    // mkdir -p ${out}/driverlib
    fs::create_dir_all(&driverlib_os_path).unwrap();

    // ar x libdriverlib.a --output ${out}/driverlib
    let status = Command::new("ar")
        .arg("x")
        .arg(out.join(LIB_NOROM_NOPREFIX))
        .arg("--output")
        .arg(&driverlib_os_path)
        .status()
        .unwrap();
    assert!(status.success(), "ar extracting driverlib failed");

    let mut driverlib_os = fs::read_dir(driverlib_os_path).unwrap();
    let driverlib_os = std::iter::from_fn(|| {
        driverlib_os
            .next()
            .transpose()
            .unwrap()
            .map(|entry| entry.path())
    });

    // arm-none-eabi-ld --relocatable --just-symbols libROM_driverlib_filtered.elf -zmuldefs extern.o libdriverlib.a -o libdriverlib_full.o
    let status = Command::new("arm-none-eabi-ld")
        .arg("--relocatable")
        .arg("-zmuldefs")
        .arg("--just-symbols")
        .arg(out.join(LIB_ROM_FILTERED))
        .arg(&extern_o_path)
        .args(driverlib_os)
        .arg("-o")
        .arg(&libdriverlib_o_full_path)
        .status()
        .unwrap();
    assert!(status.success(), "ld merging lib failed");

    let status = Command::new("ar")
        .arg("crus")
        .arg(out.join(LIB_FULL))
        .arg(&libdriverlib_o_full_path)
        .status()
        .unwrap();
    assert!(status.success(), "libdriverlib_full.o ar failed");
}

// Strips those functions from ROM symbols ELF, which are disabled in rom.h.
fn strip_disabled_rom_fns(out: &PathBuf, driverlib_path: &str, driverlib_artifacts_path: &PathBuf) {
    let enabled_rom_fns = out.join("enabled_rom_fns.txt");
    get_enabled_rom_fns(&enabled_rom_fns, driverlib_path);

    let status = Command::new("arm-none-eabi-objcopy")
        .arg(format!(
            "--keep-global-symbols={}",
            enabled_rom_fns.to_str().unwrap()
        ))
        .arg(driverlib_artifacts_path.join(LIB_ROM_ORIGINAL)) // source file
        .arg(out.join(LIB_ROM_FILTERED)) // target file
        .spawn()
        .unwrap()
        .wait()
        .unwrap();
    assert!(
        status.success(),
        "objcopy strip disabled ROM symbols failed"
    );

    // Writes ROM symbols enabled in rom.h to a file with the given name.
    fn get_enabled_rom_fns(enabled_rom_fns: &PathBuf, driverlib_path: &str) {
        let rom_h = "rom.h";
        let status = Command::new("bash")
            .arg("-c")
            .arg("-f")
            .arg(format!(
                r#"sed -E -n -e '/^#define ROM_/s/^#define ROM_(.*) \\/\1/p' {} > {}"#,
                PathBuf::from(driverlib_path).join(rom_h).to_str().unwrap(),
                enabled_rom_fns.to_str().unwrap(),
            ))
            .spawn()
            .unwrap()
            .wait()
            .unwrap();
        assert!(status.success(), "getting enabled ROM fns failed")
    }
}

// Makes linker use symbols from the given ELF. Intended for usage with stripped ROM ELF.
fn fetch_rom_symbols(rom_elf: impl AsRef<Path>) {
    println!(
        "cargo:rustc-link-arg=--just-symbols={}",
        rom_elf.as_ref().to_str().unwrap()
    );
}

fn transform_norom_symbols(out: &PathBuf, driverlib_artifacts_path: &PathBuf) {
    let lib_norom_original_path = driverlib_artifacts_path.join(LIB_NOROM_ORIGINAL);
    let symbols = get_norom_symbols(&lib_norom_original_path);

    rename_symbols(
        out,
        &symbols,
        &lib_norom_original_path,
        out.join(LIB_NOROM_NOPREFIX),
    );

    // Returns all symbols contained in a given ELF.
    // Intended for NOROM symbols stored in libdriverlib.a.
    fn get_norom_symbols(lib_norom_original_path: &PathBuf) -> Vec<u8> {
        Command::new("nm")
            .arg("-f")
            .arg("just-symbols")
            .arg(lib_norom_original_path)
            .output()
            .unwrap()
            .stdout
    }

    // Creates a new ELF in `target` path that builds upon the ELF from `source` path
    // with NOROM_* symbols having their prefix deleted.
    // `symbols` are already fetched symbols from `source` ELF,
    // `out` is used as a location for text file with the remapping.
    fn rename_symbols(
        out: &PathBuf,
        symbols: &[u8],
        source: impl AsRef<OsStr>,
        target: impl AsRef<OsStr>,
    ) {
        let norom_symbols_remapping = out.join("norom_symbols_remapping.txt");
        let mut symbols = Vec::from_iter(symbols.split(|&c| c == b'\n'));
        symbols.retain(|sym| sym.starts_with(b"NOROM"));
        symbols.sort_unstable();
        symbols.dedup();

        let mut buf = Vec::new();
        for sym in symbols.into_iter() {
            buf.extend_from_slice(sym);
            buf.push(b' ');
            buf.extend_from_slice(sym.strip_prefix(b"NOROM_").unwrap());
            buf.push(b'\n');
        }
        File::create(&norom_symbols_remapping)
            .unwrap()
            .write_all(&buf)
            .unwrap();

        let status = Command::new("arm-none-eabi-objcopy")
            .arg(format!(
                "--redefine-syms={}",
                norom_symbols_remapping.as_os_str().to_str().unwrap()
            ))
            .arg(source)
            .arg(target)
            .spawn()
            .unwrap()
            .wait()
            .unwrap();
        assert!(status.success(), "objcopy redefine-syms failed")
    }
}

fn link_driverlib(out: &PathBuf, _root: &PathBuf) {
    let current_dir = std::env::current_dir().unwrap();
    println!("cargo:rustc-link-search={}", current_dir.to_str().unwrap());
    println!("cargo:rustc-link-lib=static=driverlib_full");
    println!("cargo:rustc-link-search=native={}", out.to_str().unwrap());
    println!("cargo:rustc-link-arg=-zmuldefs");
}
