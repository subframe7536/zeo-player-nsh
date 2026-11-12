pub fn start() {}
pub fn trim_memory() {}
#[cfg(any(debug_assertions, test))]
pub fn memory_usage() -> (u64, u64) {
    (0, 0)
}
