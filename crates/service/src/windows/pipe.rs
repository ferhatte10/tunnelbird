// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! The named pipe `\\.\pipe\tunnelbird`, the Windows channel to the interface.
//!
//! Its security descriptor lets SYSTEM and administrators do anything and the
//! logged-in (interactive) users read and write; remote clients are refused.
//! Reads and writes are overlapped, so the event thread can write to a client
//! while that client's thread waits for its next request.
//!
//! NOT YET RUN ON WINDOWS (docs/WINDOWS.md).

use std::io;
use std::sync::Arc;

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_IO_PENDING, ERROR_PIPE_CONNECTED, GetLastError, HANDLE,
    INVALID_HANDLE_VALUE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX, ReadFile, WriteFile,
};
use windows_sys::Win32::System::IO::{GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
    PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::CreateEventW;

use super::wide;
use crate::transport::TryClone;

pub const NAME: &str = r"\\.\pipe\tunnelbird";
/// SYSTEM and administrators: all; interactive users: read and write.
const SDDL: &str = "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)";

struct Handle(HANDLE);

// SAFETY: a kernel handle may be used from any thread; overlapped calls on it
// each carry their own OVERLAPPED.
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: owned handle, closed once.
        unsafe { CloseHandle(self.0) };
    }
}

/// One connected client.
#[derive(Clone)]
pub struct PipeStream(Arc<Handle>);

impl TryClone for PipeStream {
    fn try_clone_writer(&self) -> io::Result<Self> {
        Ok(self.clone())
    }
}

/// Waits for an overlapped call on `pipe` to finish; returns the byte count.
fn finish(pipe: HANDLE, started: i32, overlapped: &mut OVERLAPPED) -> io::Result<u32> {
    // SAFETY: `overlapped` belongs to the call just started on `pipe`.
    unsafe {
        if started == 0 && GetLastError() != ERROR_IO_PENDING {
            return Err(io::Error::last_os_error());
        }
        let mut done = 0u32;
        if GetOverlappedResult(pipe, overlapped, &mut done, 1) == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(done)
    }
}

fn event() -> io::Result<Handle> {
    // SAFETY: an unnamed manual-reset event.
    let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
    if event.is_null() {
        Err(io::Error::last_os_error())
    } else {
        Ok(Handle(event))
    }
}

impl io::Read for PipeStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let event = event()?;
        // SAFETY: zeroed OVERLAPPED with its event; buffer valid for the call.
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
        overlapped.hEvent = event.0;
        let len = u32::try_from(buf.len()).unwrap_or(u32::MAX);
        let started = unsafe {
            ReadFile(
                self.0.0,
                buf.as_mut_ptr(),
                len,
                std::ptr::null_mut(),
                &mut overlapped,
            )
        };
        match finish(self.0.0, started, &mut overlapped) {
            Ok(n) => Ok(n as usize),
            // The client went away: end of stream.
            Err(err) if err.raw_os_error() == Some(109) => Ok(0),
            Err(err) => Err(err),
        }
    }
}

impl io::Write for PipeStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let event = event()?;
        // SAFETY: as in `read`.
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
        overlapped.hEvent = event.0;
        let len = u32::try_from(buf.len()).unwrap_or(u32::MAX);
        let started = unsafe {
            WriteFile(
                self.0.0,
                buf.as_ptr(),
                len,
                std::ptr::null_mut(),
                &mut overlapped,
            )
        };
        finish(self.0.0, started, &mut overlapped).map(|n| n as usize)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Endless clients on the pipe, one instance each.
pub fn incoming() -> impl Iterator<Item = io::Result<PipeStream>> {
    std::iter::repeat_with(accept)
}

fn accept() -> io::Result<PipeStream> {
    let name = wide(NAME);
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
        return Err(io::Error::last_os_error());
    }
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    // SAFETY: valid name and attributes for the call.
    let pipe = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            64 * 1024,
            64 * 1024,
            0,
            &attributes,
        )
    };
    // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW.
    unsafe { LocalFree(descriptor) };
    if pipe == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let pipe = Handle(pipe);
    let event = event()?;
    // SAFETY: as in `read`.
    let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
    overlapped.hEvent = event.0;
    let started = unsafe { ConnectNamedPipe(pipe.0, &mut overlapped) };
    // A client that connected between creation and this call is fine too.
    if started == 0 && unsafe { GetLastError() } == ERROR_PIPE_CONNECTED {
        return Ok(PipeStream(Arc::new(pipe)));
    }
    finish(pipe.0, started, &mut overlapped)?;
    Ok(PipeStream(Arc::new(pipe)))
}
