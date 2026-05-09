//! Opaque SDK facade handles.
//!
//! These handles are inert configuration facades. Constructing them records
//! caller intent but does not resolve endpoints, open sockets, or contact a
//! daemon.

mod builder;
mod rmux;

pub use builder::RmuxBuilder;
pub use rmux::Rmux;

fn assert_static_facade_contract<T: Send + Sync + 'static>() {
    let _ = std::marker::PhantomData::<T>;
}

const _: fn() = assert_static_facade_contract::<Rmux>;
const _: fn() = assert_static_facade_contract::<RmuxBuilder>;
