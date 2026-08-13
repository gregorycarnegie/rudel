//! What the `arc` Koto feature buys, asserted so it cannot be flipped back by
//! accident.
//!
//! Koto defaults to `rc`, where every value is `Rc`-backed and nothing can
//! cross a thread. Rudel needs more than that: a script's own function can end
//! up *inside* a pattern (`apply(pickRestart({a: x => …}))`), and a pattern is
//! queried by the scheduler thread and the UI thread at the same time — so its
//! query closure is `Send + Sync`.
//!
//! Under `arc` the values are `Arc`-backed and the VM is `Send`, which is
//! exactly enough: a VM behind a mutex is `Send + Sync` and can be captured by
//! a query closure. Switching the feature back would break that with a type
//! error a long way from the Cargo.toml line that caused it.

fn assert_send_sync<T: Send + Sync>() {}
fn assert_send<T: Send>() {}

#[test]
fn koto_values_cross_threads_and_the_vm_can_be_moved_to_one() {
    assert_send_sync::<koto::runtime::KValue>();
    assert_send_sync::<koto::runtime::KNativeFunction>();
    // The VM is `Send` but not `Sync`; sharing one means a mutex.
    assert_send::<koto::runtime::KotoVm>();
}
