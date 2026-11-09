fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "linux" {
        println!("cargo:rustc-cdylib-link-arg=-Wl,-soname,libxkbcommon.so.0");
    }
}
