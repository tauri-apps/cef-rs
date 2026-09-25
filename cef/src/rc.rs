//! Reference counted module
//!
//! Many cef types are reference counted, this module is the building block to create them. Users
//! typically don't need to uses these types, the `update-bindings` tool generates all the code
//! which should ever call them.

use std::{
    fmt::Debug,
    mem,
    ops::Deref,
    ptr::{self, NonNull},
    sync::atomic::{fence, AtomicUsize, Ordering},
};

use cef_dll_sys::cef_base_ref_counted_t;

/// Reference counted trait for types has [`cef_base_ref_counted_t`].
pub trait Rc {
    /// Increase the reference count by 1.
    ///
    /// # Safety
    ///
    /// Calling this method when you need to manually handle the reference count.
    /// Otherwise, these methods shouldn't be called externally in most cases.
    unsafe fn add_ref(&self) {
        self.as_base().add_ref();
    }

    /// Decrease reference count by 1 and release the value if the count meets 0.
    /// Reuturn `True` if it is released.
    ///
    /// # Safety
    ///
    /// Calling this method when you need to manually handle the reference count.
    /// Otherwise, these methods shouldn't be called externally in most cases.
    unsafe fn release(&self) -> bool {
        self.as_base().release()
    }

    /// `True` if the reference count is exactly 1.
    fn has_one_ref(&self) -> bool {
        self.as_base().has_one_ref()
    }

    /// `True` if the reference count is larger than 0.
    fn has_at_least_one_ref(&self) -> bool {
        self.as_base().has_at_least_one_ref()
    }

    /// Get the reference of [cef_base_ref_counted_t].
    fn as_base(&self) -> &cef_base_ref_counted_t;
}

/// Implementation detail used by generated `wrap_*` macros to read the wrapped CEF object pointer.
#[doc(hidden)]
pub trait WrapRcPtr {
    /// Returns the raw CEF object pointer without changing its reference count.
    fn as_rc_ptr(&self) -> *mut std::os::raw::c_void;
}

impl Rc for cef_base_ref_counted_t {
    unsafe fn add_ref(&self) {
        if let Some(add_ref) = self.add_ref {
            add_ref(ptr::from_ref(self) as *mut _);
        }
    }

    fn has_one_ref(&self) -> bool {
        if let Some(has_one_ref) = self.has_one_ref {
            let result = unsafe { has_one_ref(ptr::from_ref(self) as *mut _) };
            return result == 1;
        }

        false
    }

    fn has_at_least_one_ref(&self) -> bool {
        if let Some(has_at_least_one_ref) = self.has_at_least_one_ref {
            let result = unsafe { has_at_least_one_ref(ptr::from_ref(self) as *mut _) };
            return result == 1;
        }

        false
    }

    unsafe fn release(&self) -> bool {
        if let Some(release) = self.release {
            return release(ptr::from_ref(self) as *mut _) == 1;
        }

        false
    }

    fn as_base(&self) -> &Self {
        self
    }
}

pub trait ConvertParam<T: Sized> {
    fn into_raw(self) -> T;
}

impl<T, U> ConvertParam<U> for T
where
    T: Sized + Into<U>,
    U: Sized,
{
    fn into_raw(self) -> U {
        self.into()
    }
}

impl<T> ConvertParam<*mut T> for &RefGuard<T>
where
    T: Sized + Rc,
{
    /// Access the [RefGuard] and return the raw pointer without decreasing the reference count.
    ///
    /// # Safety
    ///
    /// This should be used when you need to pass wrapper type to the FFI function as **parameter**, and it is **not**
    /// the `self` type (usually the first parameter). This means we pass the ownership of the
    /// value to the function call. Using this method elsewehre may cause incorrect reference count
    /// and memory safety issues.
    fn into_raw(self) -> *mut T {
        unsafe { self.into_raw() }
    }
}

pub struct WrapParamRef<T, P>
where
    T: Sized + Into<P>,
    P: Sized + Copy + Into<T>,
{
    value: T,
    output: Option<NonNull<P>>,
}

impl<T, P> Drop for WrapParamRef<T, P>
where
    T: Sized + Into<P>,
    P: Sized + Copy + Into<T>,
{
    fn drop(&mut self) {
        if let Some(output) = &mut self.output {
            let output = unsafe { output.as_mut() };
            let mut value = unsafe { mem::zeroed() };
            mem::swap(&mut self.value, &mut value);
            *output = value.into();
        }
    }
}

impl<T, P> From<*mut P> for WrapParamRef<T, P>
where
    T: Sized + Into<P>,
    P: Sized + Copy + Into<T>,
{
    fn from(value: *mut P) -> Self {
        let mut output = NonNull::new(value);
        let value = output
            .as_mut()
            .map(|p| {
                let mut value = unsafe { mem::zeroed() };
                mem::swap(unsafe { p.as_mut() }, &mut value);
                value.into()
            })
            .unwrap_or_else(|| unsafe { mem::zeroed() });

        Self { value, output }
    }
}

impl<T, P> From<*const P> for WrapParamRef<T, P>
where
    T: Sized + Into<P>,
    P: Sized + Copy + Into<T>,
{
    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    fn from(value: *const P) -> Self {
        let value = unsafe { value.as_ref() }
            .map(|value| (*value).into())
            .unwrap_or_else(|| unsafe { mem::zeroed() });

        Self {
            value,
            output: None,
        }
    }
}

impl<T, P> AsMut<T> for WrapParamRef<T, P>
where
    T: Sized + Into<P>,
    P: Sized + Copy + Into<T>,
{
    fn as_mut(&mut self) -> &mut T {
        &mut self.value
    }
}

impl<T, P> AsRef<T> for WrapParamRef<T, P>
where
    T: Sized + Into<P>,
    P: Sized + Copy + Into<T>,
{
    fn as_ref(&self) -> &T {
        &self.value
    }
}

pub trait ConvertReturnValue<T: Sized> {
    fn wrap_result(self) -> T;
}

impl<T, U> ConvertReturnValue<U> for T
where
    T: Sized + Into<U>,
    U: Sized,
{
    fn wrap_result(self) -> U {
        self.into()
    }
}

/// A smart pointer for types from cef library.
pub struct RefGuard<T: Rc> {
    object: *mut T,
}

impl<T: Debug + Rc> Debug for RefGuard<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let object_ref = unsafe { self.object.as_ref() };
        write!(f, "RefGuard({object_ref:#?})")
    }
}

impl<T: Rc> RefGuard<T> {
    /// Create [RefGuard] from a raw C pointer.
    ///
    /// # Safety
    ///
    /// This should be used to get the **return value** of the FFI function. This means we get the
    /// ownership of the value. The reference count of the return value is already increased when
    /// you get it. So we don't need to increase it again manually. Using this method elsewhere may
    /// cause incorrect reference count and memory safety issues.
    pub unsafe fn from_raw(ptr: *mut T) -> RefGuard<T> {
        RefGuard { object: ptr }
    }

    /// Create [RefGuard] from a raw C pointer and increase a reference count. This should be used
    /// when you want to copy the value and create another wrapper type.
    ///
    /// # Safety
    ///
    /// THis should be used when you want to manually increase the reference count upon getting the
    /// raw pointer. Using this method elsewhere may cause incorrect reference count and memory
    /// safety issues.
    pub unsafe fn from_raw_add_ref(ptr: *mut T) -> RefGuard<T> {
        let guard = RefGuard { object: ptr };

        guard.add_ref();

        guard
    }

    // Get the raw pointer of [RefGuard].
    //
    /// # Safety
    ///
    /// This should be used when you need to pass wrapper type to the FFI function as **parameter**, and it **is**
    /// the `self` type (usually the first parameter). This means we pass the ownership of the
    /// value to the function call. Using this method elsewhere may cause incorrect reference count
    /// and memory safety issues.
    pub unsafe fn into_raw(&self) -> *mut T {
        self.object
    }

    /// Borrow the raw pointer without changing the reference count or implying an ownership transfer.
    ///
    /// Use this accessor when the pointer is only observed. Use [`RefGuard::into_raw`] for call
    /// sites that intentionally pass ownership at an FFI boundary.
    pub fn as_ptr(&self) -> *mut T {
        self.object
    }

    /// Convert the value to another value that is also reference counted.
    ///
    /// # Safety
    ///
    /// This should be used when the type has type `U` as its base type. Using this method
    /// elsewhere may cause memory safety issues.
    pub unsafe fn convert<U: Rc>(&self) -> RefGuard<U> {
        RefGuard::from_raw_add_ref(self.into_raw().cast())
    }
}

unsafe impl<T: Rc> Send for RefGuard<T> {}
unsafe impl<T: Rc> Sync for RefGuard<T> {}

impl<T: Rc> Clone for RefGuard<T> {
    fn clone(&self) -> RefGuard<T> {
        unsafe { self.add_ref() };

        RefGuard {
            object: self.object,
        }
    }
}

impl<T: Rc> Deref for RefGuard<T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.object }
    }
}

impl<T: Rc> Drop for RefGuard<T> {
    fn drop(&mut self) {
        unsafe { self.release() };
    }
}

/// There are some types require users to implement one their own in Rust and then create a raw type around it to
/// pass to sys level api. This is the wrapper type for it.
#[repr(C)]
pub struct RcImpl<T, I> {
    /// Raw cef types
    pub cef_object: T,
    /// Rust interface of such type
    pub interface: I,
    ref_count: AtomicUsize,
}

impl<T, I> RcImpl<T, I> {
    pub fn new(mut cef_object: T, interface: I) -> *mut RcImpl<T, I> {
        let base: &mut cef_base_ref_counted_t =
            unsafe { &mut *(ptr::from_mut(&mut cef_object).cast()) };

        base.size = std::mem::size_of::<T>();
        base.add_ref = Some(add_ref::<T, I>);
        base.has_one_ref = Some(has_one_ref::<T, I>);
        base.has_at_least_one_ref = Some(has_at_least_one_ref::<T, I>);
        base.release = Some(release::<T, I>);

        Box::into_raw(Box::new(RcImpl {
            cef_object,
            interface,
            ref_count: AtomicUsize::new(1),
        }))
    }

    pub fn get<'a>(ptr: *mut T) -> &'a mut RcImpl<T, I> {
        unsafe { &mut *(ptr.cast()) }
    }
}

extern "C" fn add_ref<T, I>(this: *mut cef_base_ref_counted_t) {
    let obj = RcImpl::<T, I>::get(this.cast());

    obj.ref_count.fetch_add(1, Ordering::Relaxed);
}

extern "C" fn has_one_ref<T, I>(this: *mut cef_base_ref_counted_t) -> i32 {
    let obj = RcImpl::<T, I>::get(this.cast());

    if obj.ref_count.load(Ordering::Relaxed) == 1 {
        1
    } else {
        0
    }
}

extern "C" fn has_at_least_one_ref<T, I>(this: *mut cef_base_ref_counted_t) -> i32 {
    let obj = RcImpl::<T, I>::get(this.cast());

    if obj.ref_count.load(Ordering::Relaxed) >= 1 {
        1
    } else {
        0
    }
}

pub extern "C" fn release<T, I>(this: *mut cef_base_ref_counted_t) -> i32 {
    let obj = RcImpl::<T, I>::get(this.cast());

    if obj.ref_count.fetch_sub(1, Ordering::Release) != 1 {
        0
    } else {
        fence(Ordering::Acquire);
        let _: Box<RcImpl<T, I>> = unsafe { Box::from_raw(this.cast()) };
        1
    }
}
