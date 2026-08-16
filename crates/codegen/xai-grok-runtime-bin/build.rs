fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let native_build = std::env::var("HOST").ok() == std::env::var("TARGET").ok();
    let optimize_for_size = std::env::var("OPT_LEVEL").is_ok_and(|level| level == "z");

    if native_build && target_os == "linux" && target_env == "gnu" && optimize_for_size {
        println!("cargo:rustc-link-arg-bin=grok-runtime=-fuse-ld=lld");
        println!("cargo:rustc-link-arg-bin=grok-runtime=-Wl,--icf=all");
        println!("cargo:rustc-link-arg-bin=grok-runtime=-Wl,-z,pack-relative-relocs");
        println!("cargo:rustc-link-arg-bin=grok-runtime=-Wl,--no-eh-frame-hdr");
        println!("cargo:rustc-link-arg-bin=grok-runtime=-Wl,--build-id=none");
    }
}
