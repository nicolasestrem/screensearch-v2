#![allow(unsafe_code)]

//! Safe wrapper over the Windows low-memory resource notification.
//!
//! The daemon uses this to honour the spec §11 / ADR 0003 requirement that a resident
//! generation model unloads on an *idle timeout or a memory-pressure signal*. All `unsafe`
//! FFI is confined to this module; callers see only `Result<bool, PortError>`.

use screensearch_ports::PortError;
use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::Memory::{
            CreateMemoryResourceNotification, LowMemoryResourceNotification,
            QueryMemoryResourceNotification,
        },
    },
    core::BOOL,
};

/// Reports whether Windows is currently signalling low available physical memory.
///
/// Backed by `CreateMemoryResourceNotification(LowMemoryResourceNotification)`. Querying the
/// handle reads the current state without blocking, so it is cheap to call from a periodic loop.
pub struct MemoryPressureMonitor {
    handle: HANDLE,
}

impl MemoryPressureMonitor {
    /// Creates a monitor backed by the OS low-memory resource notification object.
    ///
    /// Returns [`PortError::Unavailable`] if the notification object cannot be created.
    pub fn new() -> Result<Self, PortError> {
        // SAFETY: CreateMemoryResourceNotification only reads the notification-type enum and
        // returns a freshly owned handle; ownership is taken by the returned guard.
        let handle = unsafe { CreateMemoryResourceNotification(LowMemoryResourceNotification) }
            .map_err(|error| {
                PortError::Unavailable(format!("create memory resource notification: {error}"))
            })?;
        Ok(Self { handle })
    }

    /// Returns `true` when the OS currently reports low available physical memory.
    ///
    /// Returns [`PortError::Internal`] if the notification state cannot be queried.
    pub fn is_low_memory(&self) -> Result<bool, PortError> {
        let mut state = BOOL(0);
        // SAFETY: `self.handle` is a live notification handle from CreateMemoryResourceNotification;
        // QueryMemoryResourceNotification writes the current low-memory state into `state`.
        unsafe { QueryMemoryResourceNotification(self.handle, &raw mut state) }.map_err(
            |error| PortError::Internal(format!("query memory resource notification: {error}")),
        )?;
        Ok(state.as_bool())
    }
}

impl Drop for MemoryPressureMonitor {
    fn drop(&mut self) {
        // SAFETY: The monitor owns a successful CreateMemoryResourceNotification handle and
        // closes it exactly once here.
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

// SAFETY: The handle names a process-global memory-resource-notification object. Querying it is a
// thread-safe read of OS state and closing it is thread-safe; the monitor owns the handle
// exclusively (no aliasing), so transferring ownership between threads is sound. This lets the
// monitor be held across `.await` points inside the daemon's `Send` idle-unload task.
unsafe impl Send for MemoryPressureMonitor {}

#[cfg(test)]
mod tests {
    use super::MemoryPressureMonitor;

    #[test]
    #[ignore = "requires Windows; queries the live low-memory resource notification"]
    fn monitor_constructs_and_queries() {
        let monitor = MemoryPressureMonitor::new().expect("create memory pressure monitor");
        // The live state is environment-dependent; we only assert the query path succeeds.
        let _ = monitor
            .is_low_memory()
            .expect("query memory pressure state");
    }
}
