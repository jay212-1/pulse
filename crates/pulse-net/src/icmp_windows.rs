//! ICMP echo on Windows through `IcmpSendEcho` (IP helper API), which needs
//! no administrator rights.
//!
//! The only `unsafe` code in Pulse (agreed on 2026-11-21): three calls into
//! the Windows API with buffers we own and sizes we pass along.
#![allow(unsafe_code)]

use std::net::Ipv4Addr;
use std::time::Duration;
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::NetworkManagement::IpHelper::{
    IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho, ICMP_ECHO_REPLY,
};

/// Closes the ICMP handle when dropped.
struct Handle(windows_sys::Win32::Foundation::HANDLE);

impl Handle {
    fn open() -> std::io::Result<Handle> {
        // SAFETY: no arguments; the result is checked before use.
        let h = unsafe { IcmpCreateFile() };
        if h == INVALID_HANDLE_VALUE || h.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Handle(h))
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle came from IcmpCreateFile and is closed once.
        unsafe {
            IcmpCloseHandle(self.0);
        }
    }
}

pub fn available() -> Result<(), String> {
    Handle::open().map(|_| ()).map_err(|e| e.to_string())
}

pub fn echo(addr: Ipv4Addr, _seq: u16, timeout: Duration) -> std::io::Result<Option<f64>> {
    let h = Handle::open()?;
    let payload = crate::icmp::token();
    // Room for one reply, its data and an ICMP error (as the API asks).
    let size = std::mem::size_of::<ICMP_ECHO_REPLY>() + payload.len() + 8 + 64;
    let mut reply = vec![0u8; size];
    let ms = u32::try_from(timeout.as_millis())
        .unwrap_or(u32::MAX)
        .max(1);
    let start = std::time::Instant::now();
    // SAFETY: the handle is valid; the request and reply buffers live for the
    // call and their sizes are the ones passed.
    let n = unsafe {
        IcmpSendEcho(
            h.0,
            u32::from_ne_bytes(addr.octets()),
            payload.as_ptr().cast(),
            payload.len() as u16,
            std::ptr::null(),
            reply.as_mut_ptr().cast(),
            size as u32,
            ms,
        )
    };
    if n == 0 {
        // Time out and unreachable both end up here: no answer.
        return Ok(None);
    }
    // Status is the second u32 of ICMP_ECHO_REPLY (0 = IP_SUCCESS).
    let status = u32::from_ne_bytes([reply[4], reply[5], reply[6], reply[7]]);
    if status != 0 {
        return Ok(None);
    }
    // RoundTripTime has a resolution of a millisecond; measure ourselves.
    Ok(Some(start.elapsed().as_secs_f64() * 1000.0))
}
