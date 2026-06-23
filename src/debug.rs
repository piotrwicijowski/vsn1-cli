use std::sync::atomic::{AtomicBool, Ordering};

static DEBUG_ENABLED: AtomicBool = AtomicBool::new(false);

pub fn set_debug_enabled(enabled: bool) {
    DEBUG_ENABLED.store(enabled, Ordering::SeqCst);
}

pub fn is_debug_enabled() -> bool {
    DEBUG_ENABLED.load(Ordering::SeqCst)
}

pub fn log(component: &str, message: impl AsRef<str>) {
    if is_debug_enabled() {
        eprintln!("[debug][{component}] {}", message.as_ref());
    }
}
