//! Global debug-logging flag, controlled by the `--debug` CLI flag.
//!
//! The emulator's internal diagnostics (`[emulator::sysreg]`,
//! `[emulator::mem]`, `[unicorn::mem_map]`, ...) are printed through the
//! [`debug_log!`] macro and are therefore only shown when `--debug` is passed.
//! Boot-progress output from the firmware/plugin is unaffected.

use std::sync::atomic::{AtomicBool, Ordering};

static DEBUG_ENABLED: AtomicBool = AtomicBool::new(false);

/// Enables or disables debug logging (called once from `main`).
pub fn set_debug(enabled: bool) {
    DEBUG_ENABLED.store(enabled, Ordering::Relaxed);
}

/// Returns whether debug logging is currently enabled.
pub fn debug_enabled() -> bool {
    DEBUG_ENABLED.load(Ordering::Relaxed)
}

/// Prints the message only when debug logging is enabled.
#[macro_export]
macro_rules! debug_log {
    ($($arg:tt)*) => {
        if $crate::logging::debug_enabled() {
            println!($($arg)*);
        }
    };
}
