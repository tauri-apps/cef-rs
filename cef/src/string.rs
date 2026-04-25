//! String module

use cef_dll_sys::{
    _cef_string_list_t, _cef_string_map_t, _cef_string_multimap_t, _cef_string_utf16_t,
    _cef_string_utf8_t, _cef_string_wide_t,
};
use std::{
    collections::BTreeSet,
    fmt::{self, Debug, Display, Formatter},
    mem,
    ptr::{self, NonNull},
    slice,
};

use crate::CefString;

struct UserFreeData<T>(Option<NonNull<T>>);

impl<T> Default for UserFreeData<T> {
    fn default() -> Self {
        Self(None)
    }
}

impl<T> From<*mut T> for UserFreeData<T> {
    fn from(value: *mut T) -> Self {
        Self(NonNull::new(value))
    }
}

impl<T> From<UserFreeData<T>> for *mut T {
    fn from(value: UserFreeData<T>) -> Self {
        let mut value = value;
        mem::take(&mut value.0)
            .map(NonNull::as_ptr)
            .unwrap_or(ptr::null_mut())
    }
}

impl Clone for UserFreeData<_cef_string_utf8_t> {
    fn clone(&self) -> Self {
        Self(self.0.as_ref().and_then(|value| unsafe {
            let data = NonNull::new(cef_dll_sys::cef_string_userfree_utf8_alloc())?;
            if cef_dll_sys::cef_string_utf8_set(
                value.as_ref().str_,
                value.as_ref().length,
                data.as_ptr(),
                1,
            ) == 0
            {
                cef_dll_sys::cef_string_userfree_utf8_free(data.as_ptr());
                None
            } else {
                Some(data)
            }
        }))
    }
}

impl Clone for UserFreeData<_cef_string_utf16_t> {
    fn clone(&self) -> Self {
        Self(self.0.as_ref().and_then(|value| unsafe {
            let data = NonNull::new(cef_dll_sys::cef_string_userfree_utf16_alloc())?;
            if cef_dll_sys::cef_string_utf16_set(
                value.as_ref().str_,
                value.as_ref().length,
                data.as_ptr(),
                1,
            ) == 0
            {
                cef_dll_sys::cef_string_userfree_utf16_free(data.as_ptr());
                None
            } else {
                Some(data)
            }
        }))
    }
}

impl Clone for UserFreeData<_cef_string_wide_t> {
    fn clone(&self) -> Self {
        Self(self.0.as_ref().and_then(|value| unsafe {
            let data = NonNull::new(cef_dll_sys::cef_string_userfree_wide_alloc())?;
            if cef_dll_sys::cef_string_wide_set(
                value.as_ref().str_,
                value.as_ref().length,
                data.as_ptr(),
                1,
            ) == 0
            {
                cef_dll_sys::cef_string_userfree_wide_free(data.as_ptr());
                None
            } else {
                Some(data)
            }
        }))
    }
}

#[derive(Clone, Default)]
pub struct CefStringUserfreeUtf8(UserFreeData<_cef_string_utf8_t>);

impl From<*mut _cef_string_utf8_t> for CefStringUserfreeUtf8 {
    fn from(value: *mut _cef_string_utf8_t) -> Self {
        Self(value.into())
    }
}

impl From<CefStringUserfreeUtf8> for *mut _cef_string_utf8_t {
    fn from(value: CefStringUserfreeUtf8) -> Self {
        let mut value = value;
        mem::take(&mut value.0).into()
    }
}

impl From<&CefStringUserfreeUtf8> for Option<&_cef_string_utf8_t> {
    fn from(value: &CefStringUserfreeUtf8) -> Self {
        value.0 .0.as_ref().map(|value| unsafe { value.as_ref() })
    }
}

impl Drop for CefStringUserfreeUtf8 {
    fn drop(&mut self) {
        let value: *mut _cef_string_utf8_t = mem::take(&mut self.0).into();
        if !value.is_null() {
            unsafe {
                cef_dll_sys::cef_string_userfree_utf8_free(value);
            }
        }
    }
}

#[derive(Clone, Default)]
pub struct CefStringUserfreeUtf16(UserFreeData<_cef_string_utf16_t>);

impl From<*mut _cef_string_utf16_t> for CefStringUserfreeUtf16 {
    fn from(value: *mut _cef_string_utf16_t) -> Self {
        Self(value.into())
    }
}

impl From<CefStringUserfreeUtf16> for *mut _cef_string_utf16_t {
    fn from(value: CefStringUserfreeUtf16) -> Self {
        let mut value = value;
        mem::take(&mut value.0).into()
    }
}

impl From<&CefStringUserfreeUtf16> for Option<&_cef_string_utf16_t> {
    fn from(value: &CefStringUserfreeUtf16) -> Self {
        value.0 .0.as_ref().map(|value| unsafe { value.as_ref() })
    }
}

impl Drop for CefStringUserfreeUtf16 {
    fn drop(&mut self) {
        let value: *mut _cef_string_utf16_t = mem::take(&mut self.0).into();
        if !value.is_null() {
            unsafe {
                cef_dll_sys::cef_string_userfree_utf16_free(value);
            }
        }
    }
}

impl Debug for CefStringUtf16 {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", CefStringUtf8::from(self))
    }
}

#[derive(Clone, Default)]
pub struct CefStringUserfreeWide(UserFreeData<_cef_string_wide_t>);

impl From<*mut _cef_string_wide_t> for CefStringUserfreeWide {
    fn from(value: *mut _cef_string_wide_t) -> Self {
        Self(value.into())
    }
}

impl From<CefStringUserfreeWide> for *mut _cef_string_wide_t {
    fn from(value: CefStringUserfreeWide) -> Self {
        let mut value = value;
        mem::take(&mut value.0).into()
    }
}

impl From<&CefStringUserfreeWide> for Option<&_cef_string_wide_t> {
    fn from(value: &CefStringUserfreeWide) -> Self {
        value.0 .0.as_ref().map(|value| unsafe { value.as_ref() })
    }
}

impl From<*const _cef_string_utf16_t> for CefStringUserfreeWide {
    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    fn from(value: *const _cef_string_utf16_t) -> Self {
        Self(UserFreeData(unsafe {
            value.as_ref().and_then(|value| {
                if value.str_.is_null() || value.length == 0 {
                    return None;
                }
                let slice = slice::from_raw_parts(value.str_, value.length);
                NonNull::new(cef_dll_sys::cef_string_userfree_wide_alloc()).and_then(|data| {
                    if cef_dll_sys::cef_string_utf16_to_wide(
                        slice.as_ptr().cast(),
                        slice.len(),
                        data.as_ptr(),
                    ) == 0
                    {
                        cef_dll_sys::cef_string_userfree_wide_free(data.as_ptr());
                        None
                    } else {
                        Some(data)
                    }
                })
            })
        }))
    }
}

impl From<&CefStringUtf16> for CefStringUserfreeWide {
    fn from(value: &CefStringUtf16) -> Self {
        let value: *const _cef_string_utf16_t = value.into();
        Self::from(value)
    }
}

impl Drop for CefStringUserfreeWide {
    fn drop(&mut self) {
        let value: *mut _cef_string_wide_t = mem::take(&mut self.0).into();
        if !value.is_null() {
            unsafe {
                cef_dll_sys::cef_string_userfree_wide_free(value);
            }
        }
    }
}

enum CefStringData<T> {
    Borrowed(Option<T>),
    BorrowedMut(Option<NonNull<T>>),
    Clear(Option<T>),
}

impl<T> Clone for CefStringData<T>
where
    T: Copy,
{
    fn clone(&self) -> Self {
        let data: Option<&T> = self.into();
        let data = data.map(ptr::from_ref).unwrap_or(ptr::null());
        data.into()
    }
}

impl<T> Default for CefStringData<T> {
    fn default() -> Self {
        Self::Borrowed(None)
    }
}

impl<T> From<*const T> for CefStringData<T>
where
    T: Copy,
{
    fn from(value: *const T) -> Self {
        Self::Borrowed(unsafe { value.as_ref() }.copied())
    }
}

impl<T> From<*mut T> for CefStringData<T> {
    fn from(value: *mut T) -> Self {
        Self::BorrowedMut(NonNull::new(value))
    }
}

impl<'a, T> From<&'a CefStringData<T>> for Option<&'a T> {
    fn from(value: &'a CefStringData<T>) -> Self {
        match value {
            CefStringData::Borrowed(value) | CefStringData::Clear(value) => value.as_ref(),
            CefStringData::BorrowedMut(value) => {
                value.as_ref().map(|value| unsafe { value.as_ref() })
            }
        }
    }
}

impl<'a, T> From<&'a mut CefStringData<T>> for Option<&'a mut T> {
    fn from(value: &'a mut CefStringData<T>) -> Self {
        match value {
            CefStringData::BorrowedMut(value) => {
                value.as_mut().map(|value| unsafe { value.as_mut() })
            }
            CefStringData::Clear(value) => value.as_mut(),
            _ => None,
        }
    }
}

/// See [_cef_string_utf8_t] for more documentation.
#[derive(Clone)]
pub struct CefStringUtf8(CefStringData<_cef_string_utf8_t>);

impl Drop for CefStringUtf8 {
    fn drop(&mut self) {
        if let CefStringData::Clear(mut value) = &mut self.0 {
            if let Some(mut value) = mem::take(&mut value) {
                unsafe {
                    cef_dll_sys::cef_string_utf8_clear(&mut value);
                }
            }
        }
    }
}

impl From<&str> for CefStringUtf8 {
    fn from(value: &str) -> Self {
        Self(CefStringData::Clear(unsafe {
            let mut data = mem::zeroed();
            if cef_dll_sys::cef_string_utf8_set(value.as_ptr().cast(), value.len(), &mut data, 1)
                == 0
            {
                None
            } else {
                Some(data)
            }
        }))
    }
}

impl From<&CefStringUserfreeUtf8> for CefStringUtf8 {
    fn from(value: &CefStringUserfreeUtf8) -> Self {
        let value: Option<&_cef_string_utf8_t> = value.into();
        Self(CefStringData::Clear(value.and_then(|value| unsafe {
            let mut data = mem::zeroed();
            if cef_dll_sys::cef_string_utf8_set(value.str_, value.length, &mut data, 1) == 0 {
                None
            } else {
                Some(data)
            }
        })))
    }
}

impl From<*const _cef_string_utf8_t> for CefStringUtf8 {
    fn from(value: *const _cef_string_utf8_t) -> Self {
        Self(value.into())
    }
}

impl From<*mut _cef_string_utf8_t> for CefStringUtf8 {
    fn from(value: *mut _cef_string_utf8_t) -> Self {
        Self(value.into())
    }
}

impl From<&CefStringUtf8> for *const _cef_string_utf8_t {
    fn from(value: &CefStringUtf8) -> Self {
        let data: Option<&_cef_string_utf8_t> = (&value.0).into();
        data.map(ptr::from_ref).unwrap_or(ptr::null())
    }
}

impl From<&mut CefStringUtf8> for *mut _cef_string_utf8_t {
    fn from(value: &mut CefStringUtf8) -> Self {
        let data: Option<&mut _cef_string_utf8_t> = (&mut value.0).into();
        data.map(ptr::from_mut).unwrap_or(ptr::null_mut())
    }
}

impl From<_cef_string_utf8_t> for CefStringUtf8 {
    fn from(value: _cef_string_utf8_t) -> Self {
        Self(CefStringData::Borrowed(Some(value)))
    }
}

impl From<CefStringUtf8> for _cef_string_utf8_t {
    fn from(value: CefStringUtf8) -> Self {
        match value.0 {
            CefStringData::Borrowed(value) => value,
            _ => None,
        }
        .unwrap_or(unsafe { mem::zeroed() })
    }
}

impl CefStringUtf8 {
    pub fn as_str(&self) -> Option<&str> {
        let data: Option<&_cef_string_utf8_t> = (&self.0).into();
        let (str_, length) = data.map(|value| (value.str_, value.length))?;
        if str_.is_null() || length == 0 {
            return None;
        }
        Some(unsafe {
            let slice = slice::from_raw_parts(str_.cast(), length);
            std::str::from_utf8_unchecked(slice)
        })
    }

    pub fn as_slice(&self) -> Option<&[u8]> {
        let data: Option<&_cef_string_utf8_t> = (&self.0).into();
        let (str_, length) = data.map(|value| (value.str_, value.length))?;
        if str_.is_null() || length == 0 {
            return None;
        }
        Some(unsafe { slice::from_raw_parts(str_.cast(), length) })
    }

    pub fn try_set(&mut self, value: &str) -> bool {
        let CefStringData::BorrowedMut(Some(data)) = &mut self.0 else {
            return false;
        };

        unsafe {
            assert_ne!(value.as_ptr(), data.as_ref().str_.cast());
            cef_dll_sys::cef_string_utf8_clear(data.as_ptr());
            cef_dll_sys::cef_string_utf8_set(value.as_ptr().cast(), value.len(), data.as_ptr(), 1)
                != 0
        }
    }
}

impl From<&CefStringUtf16> for CefStringUtf8 {
    fn from(value: &CefStringUtf16) -> Self {
        Self(CefStringData::Clear(unsafe {
            value.as_slice().and_then(|value| {
                let mut data = mem::zeroed();
                if cef_dll_sys::cef_string_utf16_to_utf8(
                    value.as_ptr().cast(),
                    value.len(),
                    &mut data,
                ) == 0
                {
                    None
                } else {
                    Some(data)
                }
            })
        }))
    }
}

impl From<&CefStringWide> for CefStringUtf8 {
    fn from(value: &CefStringWide) -> Self {
        Self(CefStringData::Clear(unsafe {
            value.as_slice().and_then(|value| {
                let mut data = mem::zeroed();
                if cef_dll_sys::cef_string_wide_to_utf8(
                    value.as_ptr().cast(),
                    value.len(),
                    &mut data,
                ) == 0
                {
                    None
                } else {
                    Some(data)
                }
            })
        }))
    }
}

impl Display for CefStringUtf8 {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        if let Some(value) = self.as_str() {
            write!(f, "{value}")
        } else {
            Ok(())
        }
    }
}

/// See [_cef_string_utf16_t] for more documentation.
#[derive(Clone, Default)]
pub struct CefStringUtf16(CefStringData<_cef_string_utf16_t>);

impl Drop for CefStringUtf16 {
    fn drop(&mut self) {
        if let CefStringData::Clear(mut value) = &mut self.0 {
            if let Some(mut value) = mem::take(&mut value) {
                unsafe {
                    cef_dll_sys::cef_string_utf16_clear(&mut value);
                }
            }
        }
    }
}

impl From<&str> for CefStringUtf16 {
    fn from(value: &str) -> Self {
        Self(CefStringData::Clear(unsafe {
            let mut data = mem::zeroed();
            if cef_dll_sys::cef_string_utf8_to_utf16(value.as_ptr().cast(), value.len(), &mut data)
                == 0
            {
                None
            } else {
                Some(data)
            }
        }))
    }
}

impl From<&CefStringUserfreeUtf16> for CefStringUtf16 {
    fn from(value: &CefStringUserfreeUtf16) -> Self {
        let value: Option<&_cef_string_utf16_t> = value.into();
        Self(CefStringData::Clear(value.and_then(|value| unsafe {
            let mut data = mem::zeroed();
            if cef_dll_sys::cef_string_utf16_set(value.str_, value.length, &mut data, 1) == 0 {
                eprintln!("Invalid UTF-16 string");
                None
            } else {
                Some(data)
            }
        })))
    }
}

impl From<*const _cef_string_utf16_t> for CefStringUtf16 {
    fn from(value: *const _cef_string_utf16_t) -> Self {
        Self(value.into())
    }
}

impl From<*mut _cef_string_utf16_t> for CefStringUtf16 {
    fn from(value: *mut _cef_string_utf16_t) -> Self {
        Self(value.into())
    }
}

impl From<&CefStringUtf16> for *const _cef_string_utf16_t {
    fn from(value: &CefStringUtf16) -> Self {
        let data: Option<&_cef_string_utf16_t> = (&value.0).into();
        data.map(ptr::from_ref).unwrap_or(ptr::null())
    }
}

impl From<&mut CefStringUtf16> for *mut _cef_string_utf16_t {
    fn from(value: &mut CefStringUtf16) -> Self {
        let data: Option<&mut _cef_string_utf16_t> = (&mut value.0).into();
        data.map(ptr::from_mut).unwrap_or(ptr::null_mut())
    }
}

impl From<_cef_string_utf16_t> for CefStringUtf16 {
    fn from(value: _cef_string_utf16_t) -> Self {
        Self(CefStringData::Borrowed(Some(value)))
    }
}

impl From<CefStringUtf16> for _cef_string_utf16_t {
    fn from(value: CefStringUtf16) -> Self {
        match value.0 {
            CefStringData::Borrowed(value) => value,
            _ => None,
        }
        .unwrap_or(unsafe { mem::zeroed() })
    }
}

impl CefStringUtf16 {
    pub fn as_slice(&self) -> Option<&[u16]> {
        let data: Option<&_cef_string_utf16_t> = (&self.0).into();
        let (str_, length) = data.map(|value| (value.str_, value.length))?;
        if str_.is_null() || length == 0 {
            return None;
        }
        Some(unsafe { slice::from_raw_parts(str_.cast(), length) })
    }

    pub fn try_set(&mut self, value: &str) -> bool {
        let CefStringData::BorrowedMut(Some(data)) = &mut self.0 else {
            return false;
        };

        unsafe {
            cef_dll_sys::cef_string_utf16_clear(data.as_ptr());
            cef_dll_sys::cef_string_utf8_to_utf16(value.as_ptr().cast(), value.len(), data.as_ptr())
                != 0
        }
    }
}

impl From<&CefStringUtf8> for CefStringUtf16 {
    fn from(value: &CefStringUtf8) -> Self {
        Self(CefStringData::Clear(unsafe {
            value.as_str().and_then(|value| {
                let mut data = mem::zeroed();
                if cef_dll_sys::cef_string_utf8_to_utf16(
                    value.as_ptr().cast(),
                    value.len(),
                    &mut data,
                ) == 0
                {
                    None
                } else {
                    Some(data)
                }
            })
        }))
    }
}

impl From<&CefStringWide> for CefStringUtf16 {
    fn from(value: &CefStringWide) -> Self {
        Self(CefStringData::Clear(unsafe {
            value.as_slice().and_then(|value| {
                let mut data = mem::zeroed();
                if cef_dll_sys::cef_string_wide_to_utf16(
                    value.as_ptr().cast(),
                    value.len(),
                    &mut data,
                ) == 0
                {
                    None
                } else {
                    Some(data)
                }
            })
        }))
    }
}

impl Display for CefStringUtf16 {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let value = CefStringUtf8::from(self);
        if let Some(value) = value.as_str() {
            write!(f, "{value}")
        } else {
            Ok(())
        }
    }
}

/// See [_cef_string_wide_t] for more documentation.
#[derive(Clone, Default)]
pub struct CefStringWide(CefStringData<_cef_string_wide_t>);

impl Drop for CefStringWide {
    fn drop(&mut self) {
        if let CefStringData::Clear(mut value) = &mut self.0 {
            if let Some(mut value) = mem::take(&mut value) {
                unsafe {
                    cef_dll_sys::cef_string_wide_clear(&mut value);
                }
            }
        }
    }
}

impl From<&str> for CefStringWide {
    fn from(value: &str) -> Self {
        Self(CefStringData::Clear(unsafe {
            let mut data = mem::zeroed();
            if cef_dll_sys::cef_string_utf8_to_wide(value.as_ptr().cast(), value.len(), &mut data)
                == 0
            {
                None
            } else {
                Some(data)
            }
        }))
    }
}

impl From<&CefStringUserfreeWide> for CefStringWide {
    fn from(value: &CefStringUserfreeWide) -> Self {
        let value: Option<&_cef_string_wide_t> = value.into();
        Self(CefStringData::Clear(value.and_then(|value| unsafe {
            let mut data = mem::zeroed();
            if cef_dll_sys::cef_string_wide_set(value.str_, value.length, &mut data, 1) == 0 {
                None
            } else {
                Some(data)
            }
        })))
    }
}

impl From<*const _cef_string_wide_t> for CefStringWide {
    fn from(value: *const _cef_string_wide_t) -> Self {
        Self(value.into())
    }
}

impl From<*mut _cef_string_wide_t> for CefStringWide {
    fn from(value: *mut _cef_string_wide_t) -> Self {
        Self(value.into())
    }
}

impl From<&CefStringWide> for *const _cef_string_wide_t {
    fn from(value: &CefStringWide) -> Self {
        let data: Option<&_cef_string_wide_t> = (&value.0).into();
        data.map(ptr::from_ref).unwrap_or(ptr::null())
    }
}

impl From<&mut CefStringWide> for *mut _cef_string_wide_t {
    fn from(value: &mut CefStringWide) -> Self {
        let data: Option<&mut _cef_string_wide_t> = (&mut value.0).into();
        data.map(ptr::from_mut).unwrap_or(ptr::null_mut())
    }
}

impl From<_cef_string_wide_t> for CefStringWide {
    fn from(value: _cef_string_wide_t) -> Self {
        Self(CefStringData::Borrowed(Some(value)))
    }
}

impl From<CefStringWide> for _cef_string_wide_t {
    fn from(value: CefStringWide) -> Self {
        match value.0 {
            CefStringData::Borrowed(value) => value,
            _ => None,
        }
        .unwrap_or(unsafe { mem::zeroed() })
    }
}

impl CefStringWide {
    pub fn as_slice(&self) -> Option<&[i32]> {
        let data: Option<&_cef_string_wide_t> = (&self.0).into();
        let (str_, length) = data.map(|value| (value.str_, value.length))?;
        if str_.is_null() || length == 0 {
            return None;
        }
        Some(unsafe { slice::from_raw_parts(str_.cast(), length) })
    }

    pub fn try_set(&mut self, value: &str) -> bool {
        let CefStringData::BorrowedMut(Some(data)) = &mut self.0 else {
            return false;
        };

        unsafe {
            cef_dll_sys::cef_string_wide_clear(data.as_ptr());
            cef_dll_sys::cef_string_utf8_to_wide(value.as_ptr().cast(), value.len(), data.as_ptr())
                != 0
        }
    }
}

impl From<&CefStringUtf8> for CefStringWide {
    fn from(value: &CefStringUtf8) -> Self {
        Self(CefStringData::Clear(unsafe {
            value.as_str().and_then(|value| {
                let mut data = mem::zeroed();
                if cef_dll_sys::cef_string_utf8_to_wide(
                    value.as_ptr().cast(),
                    value.len(),
                    &mut data,
                ) == 0
                {
                    None
                } else {
                    Some(data)
                }
            })
        }))
    }
}

impl From<&CefStringUtf16> for CefStringWide {
    fn from(value: &CefStringUtf16) -> Self {
        Self(CefStringData::Clear(unsafe {
            value.as_slice().and_then(|value| {
                let mut data = mem::zeroed();
                if cef_dll_sys::cef_string_utf16_to_wide(
                    value.as_ptr().cast(),
                    value.len(),
                    &mut data,
                ) == 0
                {
                    None
                } else {
                    Some(data)
                }
            })
        }))
    }
}

impl Display for CefStringWide {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let value = CefStringUtf8::from(self);
        if let Some(value) = value.as_str() {
            write!(f, "{value}")
        } else {
            Ok(())
        }
    }
}

enum CefStringCollection<T> {
    Borrowed(Option<T>),
    BorrowedMut(Option<NonNull<T>>),
    Free(Option<NonNull<T>>),
}

impl<T> Clone for CefStringCollection<T>
where
    T: Copy,
{
    fn clone(&self) -> Self {
        let data: Option<&T> = self.into();
        let data = data.map(ptr::from_ref).unwrap_or(ptr::null());
        data.into()
    }
}

impl<T> Default for CefStringCollection<T> {
    fn default() -> Self {
        Self::Borrowed(None)
    }
}

impl<T> From<*const T> for CefStringCollection<T>
where
    T: Copy,
{
    fn from(value: *const T) -> Self {
        Self::Borrowed(unsafe { value.as_ref() }.copied())
    }
}

impl<T> From<*mut T> for CefStringCollection<T> {
    fn from(value: *mut T) -> Self {
        Self::BorrowedMut(NonNull::new(value))
    }
}

impl<'a, T> From<&'a CefStringCollection<T>> for Option<&'a T> {
    fn from(value: &'a CefStringCollection<T>) -> Self {
        match value {
            CefStringCollection::Borrowed(value) => value.as_ref(),
            CefStringCollection::BorrowedMut(value) | CefStringCollection::Free(value) => {
                value.as_ref().map(|value| unsafe { value.as_ref() })
            }
        }
    }
}

impl<'a, T> From<&'a mut CefStringCollection<T>> for Option<&'a mut T> {
    fn from(value: &'a mut CefStringCollection<T>) -> Self {
        match value {
            CefStringCollection::BorrowedMut(value) | CefStringCollection::Free(value) => {
                value.as_mut().map(|value| unsafe { value.as_mut() })
            }
            _ => None,
        }
    }
}

/// See [_cef_string_list_t] for more documentation.
#[derive(Clone)]
pub struct CefStringList(CefStringCollection<_cef_string_list_t>);

impl CefStringList {
    pub fn new() -> Self {
        Self(CefStringCollection::Free(NonNull::new(unsafe {
            cef_dll_sys::cef_string_list_alloc()
        })))
    }

    pub fn append(&mut self, value: &str) -> bool {
        let list: Option<&mut _cef_string_list_t> = (&mut self.0).into();
        let Some(list) = list else {
            return false;
        };

        let value = CefString::from(value);
        unsafe { cef_dll_sys::cef_string_list_append(list, (&value).into()) };
        true
    }
}

impl Default for CefStringList {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for CefStringList {
    fn drop(&mut self) {
        unsafe {
            if let CefStringCollection::Free(Some(list)) = &mut self.0 {
                cef_dll_sys::cef_string_list_free(list.as_ptr());
            }
        }
    }
}

impl From<*const _cef_string_list_t> for CefStringList {
    fn from(value: *const _cef_string_list_t) -> Self {
        Self(value.into())
    }
}

impl From<*mut _cef_string_list_t> for CefStringList {
    fn from(value: *mut _cef_string_list_t) -> Self {
        Self(value.into())
    }
}

impl From<&CefStringList> for *const _cef_string_list_t {
    fn from(value: &CefStringList) -> Self {
        let data: Option<&_cef_string_list_t> = (&value.0).into();
        data.map(ptr::from_ref).unwrap_or(ptr::null())
    }
}

impl From<&mut CefStringList> for *mut _cef_string_list_t {
    fn from(value: &mut CefStringList) -> Self {
        let data: Option<&mut _cef_string_list_t> = (&mut value.0).into();
        data.map(ptr::from_mut).unwrap_or(ptr::null_mut())
    }
}

impl From<_cef_string_list_t> for CefStringList {
    fn from(value: _cef_string_list_t) -> Self {
        Self(CefStringCollection::Borrowed(Some(value)))
    }
}

impl From<CefStringList> for _cef_string_list_t {
    fn from(value: CefStringList) -> Self {
        match value.0 {
            CefStringCollection::Borrowed(value) => value,
            _ => None,
        }
        .unwrap_or(unsafe { mem::zeroed() })
    }
}

impl IntoIterator for CefStringList {
    type Item = String;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        let mut list = self;
        let list: *mut _cef_string_list_t = (&mut list).into();
        let list = unsafe { list.as_mut() };
        list.map(|list| {
            let count = unsafe { cef_dll_sys::cef_string_list_size(list) };
            (0..count)
                .filter_map(|i| unsafe {
                    let mut value = mem::zeroed();
                    (cef_dll_sys::cef_string_list_value(list, i, &mut value) > 0).then_some(value)
                })
                .map(|value| CefString::from(ptr::from_ref(&value)).to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
        .into_iter()
    }
}

impl Debug for CefStringList {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let list: *const _cef_string_list_t = self.into();
        let list = unsafe { list.as_ref() };
        unsafe {
            let Some(list) = list else {
                return write!(f, "null");
            };
            let list = ptr::from_ref(list) as *mut _;

            write!(f, "CefStringList [")?;

            let count = cef_dll_sys::cef_string_list_size(list);
            for i in 0..count {
                let separator = if i > 0 { ", " } else { "" };
                let mut value = mem::zeroed();
                if cef_dll_sys::cef_string_list_value(list, i, &mut value) != 0 {
                    let value = CefString::from(ptr::from_ref(&value)).to_string();
                    write!(f, "{separator}{value:?}")?;
                } else {
                    write!(f, "{separator}null")?;
                }
            }

            write!(f, "]")
        }
    }
}

/// See [_cef_string_map_t] for more documentation.
#[derive(Clone)]
pub struct CefStringMap(CefStringCollection<_cef_string_map_t>);

impl CefStringMap {
    pub fn new() -> Self {
        Self(CefStringCollection::Free(NonNull::new(unsafe {
            cef_dll_sys::cef_string_map_alloc()
        })))
    }

    pub fn append(&mut self, key: &str, value: &str) -> bool {
        let map: Option<&mut _cef_string_map_t> = (&mut self.0).into();
        let Some(map) = map else {
            return false;
        };

        let key = CefString::from(key);
        let value = CefString::from(value);
        unsafe { cef_dll_sys::cef_string_map_append(map, (&key).into(), (&value).into()) != 0 }
    }
}

impl Default for CefStringMap {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for CefStringMap {
    fn drop(&mut self) {
        unsafe {
            if let CefStringCollection::Free(Some(map)) = &mut self.0 {
                cef_dll_sys::cef_string_map_free(map.as_ptr());
            }
        }
    }
}

impl From<*const _cef_string_map_t> for CefStringMap {
    fn from(value: *const _cef_string_map_t) -> Self {
        Self(value.into())
    }
}

impl From<*mut _cef_string_map_t> for CefStringMap {
    fn from(value: *mut _cef_string_map_t) -> Self {
        Self(value.into())
    }
}

impl From<&CefStringMap> for *const _cef_string_map_t {
    fn from(value: &CefStringMap) -> Self {
        let data: Option<&_cef_string_map_t> = (&value.0).into();
        data.map(ptr::from_ref).unwrap_or(ptr::null())
    }
}

impl From<&mut CefStringMap> for *mut _cef_string_map_t {
    fn from(value: &mut CefStringMap) -> Self {
        let data: Option<&mut _cef_string_map_t> = (&mut value.0).into();
        data.map(ptr::from_mut).unwrap_or(ptr::null_mut())
    }
}

impl From<_cef_string_map_t> for CefStringMap {
    fn from(value: _cef_string_map_t) -> Self {
        Self(CefStringCollection::Borrowed(Some(value)))
    }
}

impl From<CefStringMap> for _cef_string_map_t {
    fn from(value: CefStringMap) -> Self {
        match value.0 {
            CefStringCollection::Borrowed(value) => value,
            _ => None,
        }
        .unwrap_or(unsafe { mem::zeroed() })
    }
}

impl IntoIterator for CefStringMap {
    type Item = (String, String);
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        let mut map = self;
        let map: *mut _cef_string_map_t = (&mut map).into();
        let map = unsafe { map.as_mut() };
        map.map(|map| {
            let count = unsafe { cef_dll_sys::cef_string_map_size(map) };
            (0..count)
                .filter_map(|i| unsafe {
                    let mut key = mem::zeroed();
                    let mut value = mem::zeroed();
                    (cef_dll_sys::cef_string_map_key(map, i, &mut key) > 0
                        && cef_dll_sys::cef_string_map_value(map, i, &mut value) > 0)
                        .then_some((key, value))
                })
                .map(|(key, value)| {
                    (
                        CefString::from(ptr::from_ref(&key)).to_string(),
                        CefString::from(ptr::from_ref(&value)).to_string(),
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
        .into_iter()
    }
}

impl Debug for CefStringMap {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let map: *const _cef_string_map_t = self.into();
        let map = unsafe { map.as_ref() };
        unsafe {
            let Some(map) = map else {
                return write!(f, "null");
            };
            let map = ptr::from_ref(map) as *mut _;

            write!(f, "CefStringMultimap {{")?;

            let count = cef_dll_sys::cef_string_map_size(map);
            for i in 0..count {
                let mut key = mem::zeroed();
                if cef_dll_sys::cef_string_map_key(map, i, &mut key) != 0 {
                    let separator = if i > 0 { ", " } else { "" };
                    let key = CefString::from(ptr::from_ref(&key));
                    write!(f, "{separator}{key}: ")?;

                    let mut value = mem::zeroed();
                    if cef_dll_sys::cef_string_map_value(map, i, &mut value) != 0 {
                        let value = CefString::from(ptr::from_ref(&value)).to_string();
                        write!(f, "{value:?}")?;
                    } else {
                        write!(f, "null")?;
                    }
                }
            }

            write!(f, "}}")
        }
    }
}

/// See [_cef_string_multimap_t] for more documentation.
#[derive(Clone)]
pub struct CefStringMultimap(CefStringCollection<_cef_string_multimap_t>);

impl CefStringMultimap {
    pub fn new() -> Self {
        Self(CefStringCollection::Free(NonNull::new(unsafe {
            cef_dll_sys::cef_string_multimap_alloc()
        })))
    }

    pub fn append(&mut self, key: &str, value: &str) -> bool {
        let map: Option<&mut _cef_string_multimap_t> = (&mut self.0).into();
        let Some(map) = map else {
            return false;
        };

        let key = CefString::from(key);
        let value = CefString::from(value);
        unsafe { cef_dll_sys::cef_string_multimap_append(map, (&key).into(), (&value).into()) != 0 }
    }
}

impl Default for CefStringMultimap {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for CefStringMultimap {
    fn drop(&mut self) {
        unsafe {
            if let CefStringCollection::Free(Some(map)) = &mut self.0 {
                cef_dll_sys::cef_string_multimap_clear(map.as_ptr());
            }
        }
    }
}

impl From<*const _cef_string_multimap_t> for CefStringMultimap {
    fn from(value: *const _cef_string_multimap_t) -> Self {
        Self(value.into())
    }
}

impl From<*mut _cef_string_multimap_t> for CefStringMultimap {
    fn from(value: *mut _cef_string_multimap_t) -> Self {
        Self(value.into())
    }
}

impl From<&CefStringMultimap> for *const _cef_string_multimap_t {
    fn from(value: &CefStringMultimap) -> Self {
        let data: Option<&_cef_string_multimap_t> = (&value.0).into();
        data.map(ptr::from_ref).unwrap_or(ptr::null())
    }
}

impl From<&mut CefStringMultimap> for *mut _cef_string_multimap_t {
    fn from(value: &mut CefStringMultimap) -> Self {
        let data: Option<&mut _cef_string_multimap_t> = (&mut value.0).into();
        data.map(ptr::from_mut).unwrap_or(ptr::null_mut())
    }
}

impl From<_cef_string_multimap_t> for CefStringMultimap {
    fn from(value: _cef_string_multimap_t) -> Self {
        Self(CefStringCollection::Borrowed(Some(value)))
    }
}

impl From<CefStringMultimap> for _cef_string_multimap_t {
    fn from(value: CefStringMultimap) -> Self {
        match value.0 {
            CefStringCollection::Borrowed(value) => value,
            _ => None,
        }
        .unwrap_or(unsafe { mem::zeroed() })
    }
}

impl IntoIterator for CefStringMultimap {
    type Item = (String, Vec<String>);
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        let mut multimap = self;
        let multimap: *mut _cef_string_multimap_t = (&mut multimap).into();
        let multimap = unsafe { multimap.as_mut() };
        let mut entries = vec![];
        multimap
            .map(|multimap| {
                unsafe {
                    let count = cef_dll_sys::cef_string_multimap_size(multimap);
                    let mut visited: BTreeSet<String> = Default::default();
                    for i in 0..count {
                        let mut key = mem::zeroed();
                        if cef_dll_sys::cef_string_multimap_key(multimap, i, &mut key) != 0 {
                            let key = CefString::from(ptr::from_ref(&key));
                            let key_string = key.to_string();
                            if visited.contains(&key_string) {
                                continue;
                            }

                            let count = cef_dll_sys::cef_string_multimap_find_count(
                                multimap,
                                (&key).into(),
                            );
                            let mut values = vec![];
                            for i in 0..count {
                                let mut value = mem::zeroed();
                                if cef_dll_sys::cef_string_multimap_enumerate(
                                    multimap,
                                    (&key).into(),
                                    i,
                                    &mut value,
                                ) != 0
                                {
                                    values.push(CefString::from(ptr::from_ref(&value)).to_string());
                                }
                            }

                            visited.insert(key_string.clone());
                            entries.push((key_string, values));
                        }
                    }
                }
                entries
            })
            .unwrap_or_default()
            .into_iter()
    }
}

impl Debug for CefStringMultimap {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let multimap: *const _cef_string_multimap_t = self.into();
        let multimap = unsafe { multimap.as_ref() };
        unsafe {
            let Some(multimap) = multimap else {
                return write!(f, "null");
            };
            let multimap = ptr::from_ref(multimap) as *mut _;

            write!(f, "CefStringMultimap {{")?;

            let count = cef_dll_sys::cef_string_multimap_size(multimap);
            let mut visited: BTreeSet<String> = Default::default();
            for i in 0..count {
                let mut key = mem::zeroed();
                if cef_dll_sys::cef_string_multimap_key(multimap, i, &mut key) != 0 {
                    let key = CefString::from(ptr::from_ref(&key));
                    let key_string = key.to_string();
                    if visited.contains(&key_string) {
                        continue;
                    }

                    let separator = if i > 0 { ", " } else { "" };
                    write!(f, "{separator}{key_string}: [")?;

                    let count =
                        cef_dll_sys::cef_string_multimap_find_count(multimap, (&key).into());
                    for i in 0..count {
                        let separator = if i > 0 { ", " } else { "" };
                        let mut value = mem::zeroed();
                        if cef_dll_sys::cef_string_multimap_enumerate(
                            multimap,
                            (&key).into(),
                            i,
                            &mut value,
                        ) != 0
                        {
                            let value = CefString::from(ptr::from_ref(&value)).to_string();
                            write!(f, "{separator}{value:?}")?;
                        } else {
                            write!(f, "{separator}null")?;
                        }
                    }

                    write!(f, "]")?;

                    visited.insert(key_string);
                }
            }

            write!(f, "}}")
        }
    }
}

#[cfg(test)]
mod test {
    use crate::*;

    #[cfg(target_os = "macos")]
    fn ensure_dll_loaded() {
        use std::sync::Once;

        static LOAD_DLL: Once = Once::new();

        LOAD_DLL.call_once(|| {
            use std::os::unix::ffi::OsStrExt;

            let cef_dir = sys::get_cef_dir().expect("CEF not found");
            let framework_dir = cef_dir
                .join(sys::FRAMEWORK_PATH)
                .canonicalize()
                .expect("failed to get framework path");
            let framework_dir =
                std::ffi::CString::new(framework_dir.as_os_str().as_bytes()).expect("invalid path");

            assert_eq!(
                unsafe { sys::cef_load_library(framework_dir.as_ptr().cast()) },
                1
            );
        })
    }

    #[test]
    fn test_string_list() {
        #[cfg(target_os = "macos")]
        ensure_dll_loaded();

        let mut list = CefStringList::new();
        list.append("foo");
        list.append("bar");
        list.append("baz");

        let values: Vec<_> = list.into_iter().collect();
        assert_eq!(values, vec!["foo", "bar", "baz"]);
    }

    #[test]
    fn test_string_map() {
        #[cfg(target_os = "macos")]
        ensure_dll_loaded();

        let mut map = CefStringMap::new();
        map.append("foo", "value1");
        map.append("bar", "value2");
        map.append("baz", "value3");

        let values: Vec<_> = map.into_iter().collect();
        assert_eq!(
            values,
            vec![
                ("foo".to_string(), "value1".to_string()),
                ("bar".to_string(), "value2".to_string()),
                ("baz".to_string(), "value3".to_string())
            ]
        );
    }

    #[test]
    fn test_string_multimap() {
        #[cfg(target_os = "macos")]
        ensure_dll_loaded();

        let mut map = CefStringMultimap::new();
        map.append("foo", "value1a");
        map.append("bar", "value2a");
        map.append("bar", "value2b");
        map.append("baz", "value3a");
        map.append("baz", "value3b");
        map.append("baz", "value3c");

        let values: Vec<_> = map.into_iter().collect();
        assert_eq!(
            values,
            vec![
                ("foo".to_string(), vec!["value1a".to_string()]),
                (
                    "bar".to_string(),
                    vec!["value2a".to_string(), "value2b".to_string()]
                ),
                (
                    "baz".to_string(),
                    vec![
                        "value3a".to_string(),
                        "value3b".to_string(),
                        "value3c".to_string()
                    ]
                )
            ]
        );
    }
}
