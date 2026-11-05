//! `tls!`: `thread_local!` that costs one pthread key in total on Android.
//!
//! On armv7 Android std backs every `thread_local!` with its own pthread key, created on first use
//! and never freed, and bionic gives a process 128 keys shared with every system library. The app
//! has over a hundred such statics, so a session could run out and std aborted ("out of TLS
//! keys"). On Android `tls!` keeps them all in one per-thread table under a single key; everywhere
//! else it is `std::thread_local!` itself. Use `tls!` for every thread-local in this tree.

/// `std::thread_local!` with the same syntax and the same `LocalKey` methods.
#[cfg(not(target_os = "android"))]
#[macro_export]
macro_rules! tls {
    ($($body:tt)*) => { ::std::thread_local! { $($body)* } };
}

/// `std::thread_local!` with the same syntax, backed by [`tls::LocalKey`](crate::tls::LocalKey).
#[cfg(target_os = "android")]
#[macro_export]
macro_rules! tls {
    () => {};
    ($(#[$attr:meta])* $vis:vis static $name:ident: $t:ty = const $init:block $(; $($rest:tt)*)?) => {
        $(#[$attr])* $vis static $name: $crate::tls::LocalKey<$t> = $crate::tls::LocalKey::new(|| $init);
        $($crate::tls! { $($rest)* })?
    };
    ($(#[$attr:meta])* $vis:vis static $name:ident: $t:ty = $init:expr $(; $($rest:tt)*)?) => {
        $(#[$attr])* $vis static $name: $crate::tls::LocalKey<$t> = $crate::tls::LocalKey::new(|| $init);
        $($crate::tls! { $($rest)* })?
    };
}

#[cfg(any(target_os = "android", all(test, unix)))]
pub use imp::{AccessError, LocalKey};

#[cfg(any(target_os = "android", all(test, unix)))]
mod imp {
    use std::any::Any;
    use std::cell::{Cell, RefCell};
    use std::marker::PhantomData;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::OnceLock;

    type Slots = Vec<Option<Box<dyn Any>>>;

    static NEXT_SLOT: AtomicUsize = AtomicUsize::new(0);

    /// A thread-local value, created per thread by `init` on first access and dropped at that
    /// thread's exit.
    pub struct LocalKey<T: 'static> {
        /// Index into every thread's table, plus one; 0 until the first access assigns it.
        slot: AtomicUsize,
        init: fn() -> T,
        _value: PhantomData<fn() -> T>,
    }

    /// Never returned: a value whose thread is exiting is created again, as pthread does.
    #[derive(Debug)]
    pub struct AccessError;

    impl<T: 'static> LocalKey<T> {
        pub const fn new(init: fn() -> T) -> Self {
            Self { slot: AtomicUsize::new(0), init, _value: PhantomData }
        }

        pub fn with<F: FnOnce(&T) -> R, R>(&'static self, f: F) -> R {
            let value = self.get_or_insert(None);
            f(unsafe { &*value })
        }

        pub fn try_with<F: FnOnce(&T) -> R, R>(&'static self, f: F) -> Result<R, AccessError> {
            Ok(self.with(f))
        }

        /// `f` gets `Some(value)` back when the key was already initialized; otherwise `value`
        /// becomes this thread's value without running `init`, as in std.
        fn initialize_with<F: FnOnce(Option<T>, &T) -> R, R>(&'static self, value: T, f: F) -> R {
            let mut value = Some(value);
            let current = self.get_or_insert(Some(&mut value));
            f(value, unsafe { &*current })
        }

        fn index(&self) -> usize {
            let slot = self.slot.load(Ordering::Acquire);
            if slot != 0 {
                return slot - 1;
            }
            let fresh = NEXT_SLOT.fetch_add(1, Ordering::Relaxed) + 1;
            match self.slot.compare_exchange(0, fresh, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => fresh - 1,
                Err(won) => won - 1,
            }
        }

        /// The pointer stays valid until this thread exits: a slot's box is never replaced.
        fn get_or_insert(&'static self, value: Option<&mut Option<T>>) -> *const T {
            let index = self.index();
            let table = table();
            if let Some(Some(current)) = unsafe { &*table }.get(index) {
                return current.downcast_ref::<T>().expect("one type per slot");
            }
            // `init` may use other keys, which can grow the table, so no borrow of it is held here.
            let fresh: Box<dyn Any> = Box::new(match value.and_then(Option::take) {
                Some(v) => v,
                None => (self.init)(),
            });
            let slots = unsafe { &mut *table };
            if slots.len() <= index {
                slots.resize_with(index + 1, || None);
            }
            // An `init` that used this same key first keeps its value; the spare drops with no borrow held.
            let spare = match slots[index] {
                Some(_) => Some(fresh),
                None => {
                    slots[index] = Some(fresh);
                    None
                }
            };
            let current: *const T = slots[index].as_ref().unwrap().downcast_ref::<T>().expect("one type per slot");
            drop(spare);
            current
        }
    }

    impl<T: 'static> LocalKey<Cell<T>> {
        pub fn set(&'static self, value: T) {
            self.initialize_with(Cell::new(value), |value, cell| {
                if let Some(value) = value {
                    cell.set(value.into_inner());
                }
            });
        }

        pub fn get(&'static self) -> T
        where
            T: Copy,
        {
            self.with(Cell::get)
        }

        pub fn take(&'static self) -> T
        where
            T: Default,
        {
            self.with(Cell::take)
        }

        pub fn replace(&'static self, value: T) -> T {
            self.with(|cell| cell.replace(value))
        }
    }

    impl<T: 'static> LocalKey<RefCell<T>> {
        pub fn with_borrow<F: FnOnce(&T) -> R, R>(&'static self, f: F) -> R {
            self.with(|cell| f(&cell.borrow()))
        }

        pub fn with_borrow_mut<F: FnOnce(&mut T) -> R, R>(&'static self, f: F) -> R {
            self.with(|cell| f(&mut cell.borrow_mut()))
        }

        pub fn set(&'static self, value: T) {
            self.initialize_with(RefCell::new(value), |value, cell| {
                if let Some(value) = value {
                    *cell.borrow_mut() = value.into_inner();
                }
            });
        }

        pub fn take(&'static self) -> T
        where
            T: Default,
        {
            self.with(RefCell::take)
        }

        pub fn replace(&'static self, value: T) -> T {
            self.with(|cell| cell.replace(value))
        }
    }

    fn key() -> libc::pthread_key_t {
        static KEY: OnceLock<libc::pthread_key_t> = OnceLock::new();
        *KEY.get_or_init(|| {
            let mut key = 0;
            let rc = unsafe { libc::pthread_key_create(&mut key, Some(destroy)) };
            assert_eq!(rc, 0, "pthread_key_create for the thread-local table");
            key
        })
    }

    fn table() -> *mut Slots {
        let key = key();
        let table = unsafe { libc::pthread_getspecific(key) }.cast::<Slots>();
        if !table.is_null() {
            return table;
        }
        let table = Box::into_raw(Box::<Slots>::default());
        let rc = unsafe { libc::pthread_setspecific(key, table.cast()) };
        assert_eq!(rc, 0, "pthread_setspecific for the thread-local table");
        table
    }

    /// Drops the values last-created first. The table stays installed meanwhile, because a
    /// value's `Drop` may use another key; one used again after its drop is created anew and
    /// dropped on a later pass, up to pthread's own limit of four.
    unsafe extern "C" fn destroy(table: *mut libc::c_void) {
        let table = table.cast::<Slots>();
        unsafe { libc::pthread_setspecific(key(), table.cast()) };
        for _ in 0..4 {
            let mut dropped = false;
            loop {
                let next = unsafe { &mut *table }.iter_mut().rev().find_map(Option::take);
                let Some(value) = next else { break };
                dropped = true;
                drop(value);
            }
            if !dropped {
                break;
            }
        }
        unsafe { libc::pthread_setspecific(key(), std::ptr::null()) };
        drop(unsafe { Box::from_raw(table) });
    }

    #[cfg(test)]
    mod tests {
        use super::LocalKey;
        use std::cell::{Cell, RefCell};
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        fn leak<T>(init: fn() -> T) -> &'static LocalKey<T> {
            Box::leak(Box::new(LocalKey::new(init)))
        }

        #[test]
        fn more_keys_than_pthread_allows_all_work_on_one_key() {
            let keys: Vec<&'static LocalKey<Cell<usize>>> = (0..300).map(|_| leak(|| Cell::new(0))).collect();
            std::thread::spawn(move || {
                for (i, key) in keys.iter().enumerate() {
                    key.set(i + 1);
                }
                for (i, key) in keys.iter().enumerate() {
                    assert_eq!(key.get(), i + 1);
                }
            })
            .join()
            .unwrap();
        }

        #[test]
        fn each_thread_sees_its_own_value() {
            let key = leak(|| Cell::new(7));
            key.set(1);
            std::thread::spawn(move || assert_eq!(key.get(), 7)).join().unwrap();
            assert_eq!(key.get(), 1);
        }

        #[test]
        fn set_before_first_use_skips_init() {
            let key: &'static LocalKey<RefCell<u32>> = leak(|| panic!("init ran"));
            std::thread::spawn(move || {
                key.set(5);
                assert_eq!(key.with_borrow(|v| *v), 5);
            })
            .join()
            .unwrap();
        }

        static INNER: LocalKey<Cell<u32>> = LocalKey::new(|| Cell::new(40));
        static OUTER: LocalKey<Cell<u32>> = LocalKey::new(|| Cell::new(INNER.get() + 2));

        #[test]
        fn an_init_may_use_another_key() {
            std::thread::spawn(|| assert_eq!(OUTER.get(), 42)).join().unwrap();
        }

        struct Witness(Arc<AtomicUsize>);
        impl Drop for Witness {
            fn drop(&mut self) {
                INNER.set(INNER.get() + 1);
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        #[test]
        fn values_drop_at_thread_exit_even_when_a_drop_uses_another_key() {
            let drops = Arc::new(AtomicUsize::new(0));
            let key: &'static LocalKey<RefCell<Option<Witness>>> = leak(|| RefCell::new(None));
            let witness = Witness(drops.clone());
            std::thread::spawn(move || {
                INNER.get();
                key.set(Some(witness));
            })
            .join()
            .unwrap();
            assert_eq!(drops.load(Ordering::SeqCst), 1);
        }
    }
}
