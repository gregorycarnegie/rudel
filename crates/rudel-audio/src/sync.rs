//! Poison-recovering lock helpers shared by the audio and scheduler threads.

use std::sync::{Mutex, RwLock};

/// A panic on one thread must not cascade into the audio/scheduler threads.
fn recover_poison<G>(error: std::sync::PoisonError<G>) -> G {
    eprintln!(
        "[rudel-audio] recovered poisoned {} (another thread panicked)",
        std::any::type_name::<G>()
    );
    error.into_inner()
}

pub(crate) fn read_lock<T>(lock: &RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(recover_poison)
}

pub(crate) fn write_lock<T>(lock: &RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(recover_poison)
}

pub(crate) fn lock_mutex<T>(lock: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(recover_poison)
}
