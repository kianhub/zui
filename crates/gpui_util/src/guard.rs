//! Contain Rust panics at native callback and capture-disposal boundaries.

use std::{
    mem::ManuallyDrop,
    ops::{Deref, DerefMut},
    panic::{AssertUnwindSafe, catch_unwind},
};

/// Run a native callback, returning its safe default if Rust code panics.
///
/// The default must have a panic-free destructor. The application's panic
/// hook still observes the original panic; this helper does not log or perform
/// recovery I/O. Callers must restore any state removed before invoking a
/// callback, because catching an unwind does not roll back partial work.
pub fn guarded_callback<T>(default: T, body: impl FnOnce() -> T) -> T {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(value) => value,
        Err(payload) => {
            // A panic_any payload may itself panic in Drop. Contain that second
            // unwind too, forgetting only its payload to prevent recursion.
            if let Err(secondary) = catch_unwind(AssertUnwindSafe(|| drop(payload))) {
                std::mem::forget(secondary);
            }
            default
        }
    }
}

/// An owned native callback capture whose destruction cannot unwind into FFI.
///
/// Channel senders can invoke a receiver's waker when released. Native block
/// disposal and Objective-C deallocation must contain a panicking waker too.
pub struct GuardedDrop<T>(ManuallyDrop<T>);

impl<T> GuardedDrop<T> {
    /// Own a value whose release may run Rust code from a native disposer.
    pub fn new(value: T) -> Self {
        Self(ManuallyDrop::new(value))
    }
}

impl<T> Deref for GuardedDrop<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> DerefMut for GuardedDrop<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T> Drop for GuardedDrop<T> {
    fn drop(&mut self) {
        // SAFETY: Drop runs once and no other operation extracts this value.
        let value = unsafe { ManuallyDrop::take(&mut self.0) };
        guarded_callback((), || drop(value));
    }
}
