fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rerun-if-changed=src/macos/bridge.m");
        cc::Build::new()
            .file("src/macos/bridge.m")
            .flag("-fobjc-arc")
            .compile("testbar_mac_bridge");
        for framework in [
            "Cocoa",
            "AppKit",
            "Foundation",
            "CoreGraphics",
            "CoreFoundation",
            "ApplicationServices",
            "QuickLookThumbnailing",
        ] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
    }

    tauri_build::build()
}
