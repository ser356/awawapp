//! Sparkle OTA updates integration for macOS.
//!
//! This module initializes the Sparkle updater framework for automatic
//! and manual update checks on macOS.

#[cfg(target_os = "macos")]
pub mod updater {
    use objc::runtime::{Class, Object};
    use objc::{msg_send, sel, sel_impl};
    
    /// Initialize Sparkle updater on app startup.
    /// This creates an SPUStandardUpdaterController which will:
    /// - Check for updates automatically based on user preferences
    /// - Handle the update UI and installation process
    pub fn init_sparkle() {
        unsafe {
            // Get the SPUStandardUpdaterController class from Sparkle.framework
            let class = Class::get("SPUStandardUpdaterController");
            
            if let Some(controller_class) = class {
                // Allocate and initialize the controller
                // SPUStandardUpdaterController automatically starts checking for updates
                let controller: *mut Object = msg_send![controller_class, alloc];
                let _controller: *mut Object = msg_send![
                    controller,
                    initWithStartingUpdater: true as objc::runtime::BOOL
                    updaterDelegate: std::ptr::null::<Object>()
                    userDriverDelegate: std::ptr::null::<Object>()
                ];
                
                tracing::info!("Sparkle updater initialized successfully");
                
                // Keep the controller alive - it's a singleton that manages itself
                // We intentionally don't drop it as it needs to persist for the app lifetime
                std::mem::forget(_controller);
            } else {
                tracing::warn!("SPUStandardUpdaterController class not found - Sparkle may not be bundled");
            }
        }
    }
    
    /// Manually trigger an update check.
    /// This opens the Sparkle update UI if an update is available.
    pub fn check_for_updates() {
        unsafe {
            let class = Class::get("SPUStandardUpdaterController");
            
            if let Some(controller_class) = class {
                // Get shared instance approach - create new controller
                let controller: *mut Object = msg_send![controller_class, alloc];
                let controller: *mut Object = msg_send![
                    controller,
                    initWithStartingUpdater: false as objc::runtime::BOOL
                    updaterDelegate: std::ptr::null::<Object>()
                    userDriverDelegate: std::ptr::null::<Object>()
                ];
                
                // Get the updater and check for updates
                let updater: *mut Object = msg_send![controller, updater];
                let _: () = msg_send![updater, checkForUpdates];
                
                tracing::info!("Manual update check triggered");
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub mod updater {
    /// No-op on non-macOS platforms
    pub fn init_sparkle() {
        tracing::debug!("Sparkle updates are only available on macOS");
    }
    
    /// No-op on non-macOS platforms
    pub fn check_for_updates() {
        tracing::debug!("Sparkle updates are only available on macOS");
    }
}

pub use updater::{init_sparkle, check_for_updates};
