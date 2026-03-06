fn main() {
    #[cfg(target_os = "macos")]
    {
        println!("cargo:rustc-link-search=framework=/System/Library/PrivateFrameworks");
        println!("cargo:rustc-link-lib=framework=ScreenCaptureKit");

        cc::Build::new()
            .file("src/capture.m")
            .flag("-fobjc-arc")
            .flag("-fmodules")
            .compile("capture");
    }

    tauri_build::build()
}
