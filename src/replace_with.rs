//! Based on [replace_with](https://github.com/alecmocatta/replace_with/)

#![allow(unused)]

use core::{future::Future, mem::ManuallyDrop, ptr};

struct OnDrop<F: FnOnce()>(ManuallyDrop<F>);
impl<F: FnOnce()> Drop for OnDrop<F> {
    #[inline(always)]
    fn drop(&mut self) {
        (unsafe { ptr::read(&*self.0) })();
    }
}

#[doc(hidden)]
#[inline(always)]
pub async fn on_unwind_async<F: Future<Output = T>, T, P: FnOnce()>(
    f: impl FnOnce() -> F,
    p: P,
) -> T {
    let x = OnDrop(ManuallyDrop::new(p));
    let t = f().await;
    let mut x = ManuallyDrop::new(x);
    unsafe { ManuallyDrop::drop(&mut x.0) };
    t
}

#[inline]
pub async fn replace_with_and_return_async<T, U, D: FnOnce() -> T, F: Future<Output = (U, T)>>(
    dest: &mut T,
    default: D,
    f: impl FnOnce(T) -> F,
) -> U {
    unsafe {
        let old = ptr::read(dest);
        let (res, new) = on_unwind_async(move || f(old), || ptr::write(dest, default())).await;
        ptr::write(dest, new);
        res
    }
}

#[inline]
pub async fn replace_with_async<T, D: FnOnce() -> T, F: Future<Output = T>>(
    dest: &mut T,
    default: D,
    f: impl FnOnce(T) -> F,
) {
    unsafe {
        let old = ptr::read(dest);
        let new = on_unwind_async(move || f(old), || ptr::write(dest, default())).await;
        ptr::write(dest, new);
    }
}

#[inline]
pub async fn replace_with_or_abort_and_return_async<T, U, F: Future<Output = (U, T)>>(
    dest: &mut T,
    f: impl FnOnce(T) -> F,
) -> U {
    replace_with_and_return_async(dest, || panic!(), f).await
}

#[inline]
pub async fn replace_with_or_abort_async<T, F: Future<Output = T>>(
    dest: &mut T,
    f: impl FnOnce(T) -> F,
) {
    replace_with_async(dest, || panic!(), f).await
}
