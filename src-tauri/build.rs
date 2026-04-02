fn main() {
    // Link Sparkle.framework on macOS
    #[cfg(target_os = "macos")]
    {
        println!("cargo:rustc-link-search=framework=./");
        println!("cargo:rustc-link-lib=framework=Sparkle");
    }
    
    tauri_build::build()
}
