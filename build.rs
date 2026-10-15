use std::env;

fn main() {
    let target_os = env::var("CARGO_CFG_TARGET_OS").expect("Cargo did not set CARGO_CFG_TARGET_OS");
    let target_arch =
        env::var("CARGO_CFG_TARGET_ARCH").expect("Cargo did not set CARGO_CFG_TARGET_ARCH");

    if target_os == "linux" && target_arch == "x86_64" {
        println!("cargo:rustc-link-lib=atomic");
    }
}
