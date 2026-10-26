//! macOS native Keychain calls keep credential bytes outside process arguments.
use anyhow::{ensure, Result};
use std::{ffi::c_void, ptr};
#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    fn SecKeychainFindGenericPassword(
        keychain: *const c_void,
        service_len: u32,
        service: *const u8,
        account_len: u32,
        account: *const u8,
        password_len: *mut u32,
        password: *mut *mut c_void,
        item: *mut *mut c_void,
    ) -> i32;
    fn SecKeychainAddGenericPassword(
        keychain: *const c_void,
        service_len: u32,
        service: *const u8,
        account_len: u32,
        account: *const u8,
        password_len: u32,
        password: *const u8,
        item: *mut *mut c_void,
    ) -> i32;
    fn SecKeychainItemModifyAttributesAndData(
        item: *mut c_void,
        attrs: *const c_void,
        len: u32,
        data: *const u8,
    ) -> i32;
    fn SecKeychainItemFreeContent(attrs: *mut c_void, data: *mut c_void) -> i32;
    fn SecKeychainItemDelete(item: *mut c_void) -> i32;
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(item: *const c_void);
}
const SERVICE: &[u8] = b"gesac";
fn find(id: &str) -> Result<Option<*mut c_void>> {
    let mut item = ptr::null_mut();
    let code = unsafe {
        SecKeychainFindGenericPassword(
            ptr::null(),
            SERVICE.len() as u32,
            SERVICE.as_ptr(),
            id.len() as u32,
            id.as_ptr(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut item,
        )
    };
    if code == -25300 {
        return Ok(None);
    }
    ensure!(code == 0, "Keychain lookup failed ({code})");
    Ok(Some(item))
}
pub fn store(id: &str, bytes: &[u8]) -> Result<()> {
    let code = if let Some(item) = find(id)? {
        let c = unsafe {
            SecKeychainItemModifyAttributesAndData(
                item,
                ptr::null(),
                bytes.len() as u32,
                bytes.as_ptr(),
            )
        };
        unsafe { CFRelease(item) };
        c
    } else {
        unsafe {
            SecKeychainAddGenericPassword(
                ptr::null(),
                SERVICE.len() as u32,
                SERVICE.as_ptr(),
                id.len() as u32,
                id.as_ptr(),
                bytes.len() as u32,
                bytes.as_ptr(),
                ptr::null_mut(),
            )
        }
    };
    ensure!(code == 0, "Keychain store failed ({code})");
    Ok(())
}
pub fn read(id: &str) -> Result<Option<Vec<u8>>> {
    let mut len = 0;
    let mut data = ptr::null_mut();
    let code = unsafe {
        SecKeychainFindGenericPassword(
            ptr::null(),
            SERVICE.len() as u32,
            SERVICE.as_ptr(),
            id.len() as u32,
            id.as_ptr(),
            &mut len,
            &mut data,
            ptr::null_mut(),
        )
    };
    if code == -25300 {
        return Ok(None);
    }
    ensure!(code == 0, "Keychain read failed ({code})");
    if len > 65536 {
        unsafe { SecKeychainItemFreeContent(ptr::null_mut(), data) };
        anyhow::bail!("Keychain record too large");
    }
    let bytes = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), len as usize) }.to_vec();
    unsafe { SecKeychainItemFreeContent(ptr::null_mut(), data) };
    Ok(Some(bytes))
}
pub fn delete(id: &str) -> Result<()> {
    if let Some(item) = find(id)? {
        let code = unsafe { SecKeychainItemDelete(item) };
        unsafe { CFRelease(item) };
        ensure!(code == 0, "Keychain delete failed ({code})");
    }
    Ok(())
}
