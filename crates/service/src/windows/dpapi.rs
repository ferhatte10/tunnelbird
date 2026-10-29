// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! Private storage on Windows: a folder only SYSTEM and administrators can
//! open (`%ProgramData%\Tunnelbird`), and each file encrypted with DPAPI for
//! this machine, like WireGuard for Windows stores its tunnels.
//!
//! NOT YET RUN ON WINDOWS (docs/WINDOWS.md).

use std::path::Path;

use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::Cryptography::{
    CRYPT_INTEGER_BLOB, CRYPTPROTECT_LOCAL_MACHINE, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData,
    CryptUnprotectData,
};
use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows_sys::Win32::Storage::FileSystem::CreateDirectoryW;

use super::wide;

/// SYSTEM and administrators only, inherited by the files inside.
const SDDL: &str = "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)";

pub fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let path = wide(&dir.to_string_lossy());
    let sddl = wide(SDDL);
    let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    // SAFETY: the API allocates the descriptor; freed below.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    // SAFETY: valid path and attributes.
    let created = unsafe { CreateDirectoryW(path.as_ptr(), &attributes) };
    let error = unsafe { GetLastError() };
    unsafe { LocalFree(descriptor) };
    if created == 0 && error != ERROR_ALREADY_EXISTS {
        return Err(std::io::Error::from_raw_os_error(error as i32));
    }
    Ok(())
}

pub fn protect(data: &[u8]) -> std::io::Result<Vec<u8>> {
    crypt(data, true)
}

pub fn unprotect(data: &[u8]) -> std::io::Result<Vec<u8>> {
    crypt(data, false)
}

fn crypt(data: &[u8], encrypt: bool) -> std::io::Result<Vec<u8>> {
    let input = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(data.len()).map_err(|_| std::io::Error::other("too large"))?,
        pbData: data.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    let flags = CRYPTPROTECT_LOCAL_MACHINE | CRYPTPROTECT_UI_FORBIDDEN;
    // SAFETY: input points to `data`; the API allocates the output, freed below.
    let done = unsafe {
        if encrypt {
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                flags,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                flags,
                &mut output,
            )
        }
    };
    if done == 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: the API returned `cbData` bytes at `pbData`.
    let bytes =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    unsafe { LocalFree(output.pbData.cast()) };
    Ok(bytes)
}
