fn main() {
    println!("cargo:rerun-if-changed=src/native_passkey.m");
    println!("cargo:rerun-if-changed=src/apple_app_attest.m");
    #[cfg(target_os = "macos")]
    {
        cc::Build::new()
            .file("src/apple_app_attest.m")
            .file("src/native_passkey.m")
            .flag("-fobjc-arc")
            .flag("-fblocks")
            .compile("gate_apple_app_attest");
        println!("cargo:rustc-link-lib=framework=AppKit");
        println!("cargo:rustc-link-lib=framework=AuthenticationServices");
        println!("cargo:rustc-link-lib=framework=DeviceCheck");
        println!("cargo:rustc-link-lib=framework=Foundation");
        println!("cargo:rustc-link-lib=framework=Security");
    }
    #[cfg(feature = "desktop")]
    tauri_build::build();
}
