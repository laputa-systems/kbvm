use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "linux" {
        return;
    }
    println!("cargo:rustc-cdylib-link-arg=-Wl,-soname,libxkbcommon.so.0");
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let features = env::var("CARGO_CFG_TARGET_FEATURE").unwrap_or_default();
    if target_env == "musl" && !features.split(',').any(|f| f == "crt-static") {
        link_unwinder_statically();
    }
}

fn link_unwinder_statically() {
    let rustc = env::var("RUSTC").expect("cargo sets RUSTC");
    let target = env::var("TARGET").expect("cargo sets TARGET");
    let output = Command::new(&rustc)
        .args(["--print", "sysroot"])
        .output()
        .expect("rustc --print sysroot runs");
    assert!(output.status.success(), "rustc --print sysroot failed");
    let sysroot = String::from_utf8(output.stdout).expect("sysroot is UTF-8");
    let libunwind = PathBuf::from(sysroot.trim())
        .join("lib/rustlib")
        .join(&target)
        .join("lib/self-contained/libunwind.a");
    assert!(
        libunwind.is_file(),
        "{} is missing; the {target} rust-std component ships it",
        libunwind.display()
    );
    let shim = PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR")).join("static-gcc_s");
    std::fs::create_dir_all(&shim).expect("create the static gcc_s directory");
    let archive = shim.join("libgcc_s.a");
    std::fs::copy(&libunwind, &archive).expect("copy libunwind.a as libgcc_s.a");
    println!("cargo:rustc-link-search=native={}", shim.display());
}
