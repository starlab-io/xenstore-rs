/**
    xenstore-rs provides a Rust based xenstore implementation.
    Copyright (C) 2016 Star Lab Corp.

    This program is free software; you can redistribute it and/or modify
    it under the terms of the GNU General Public License as published by
    the Free Software Foundation; either version 2 of the License, or
    (at your option) any later version.

    This program is distributed in the hope that it will be useful,
    but WITHOUT ANY WARRANTY; without even the implied warranty of
    MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
    GNU General Public License for more details.

    You should have received a copy of the GNU General Public License along
    with this program; if not, see <http://www.gnu.org/licenses/>.
**/

/// XenStore message types
pub const XS_DEBUG: u16 = 0;
pub const XS_DIRECTORY: u16 = 1;
pub const XS_READ: u16 = 2;
pub const XS_GET_PERMS: u16 = 3;
pub const XS_WATCH: u16 = 4;
pub const XS_UNWATCH: u16 = 5;
pub const XS_TRANSACTION_START: u16 = 6;
pub const XS_TRANSACTION_END: u16 = 7;
pub const XS_INTRODUCE: u16 = 8;
pub const XS_RELEASE: u16 = 9;
pub const XS_GET_DOMAIN_PATH: u16 = 10;
pub const XS_WRITE: u16 = 11;
pub const XS_MKDIR: u16 = 12;
pub const XS_RM: u16 = 13;
pub const XS_SET_PERMS: u16 = 14;
pub const XS_WATCH_EVENT: u16 = 15;
pub const XS_ERROR: u16 = 16;
pub const XS_IS_DOMAIN_INTRODUCED: u16 = 17;
pub const XS_RESUME: u16 = 18;
pub const XS_SET_TARGET: u16 = 19;
pub const XS_RESTRICT: u16 = 20;
pub const XS_RESET_WATCHES: u16 = 21;
pub const XS_INVALID: u16 = 0xffff;

/// XenStore error types
pub const XSE_EINVAL: &'static str = "EINVAL";
pub const XSE_EACCES: &'static str = "EACCES";
pub const XSE_EEXIST: &'static str = "EEXIST";
pub const XSE_EISDIR: &'static str = "EISDIR";
pub const XSE_ENOENT: &'static str = "ENOENT";
pub const XSE_ENOMEM: &'static str = "ENOMEM";
pub const XSE_ENOSPC: &'static str = "ENOSPC";
pub const XSE_EIO: &'static str = "EIO";
pub const XSE_ENOTEMPTY: &'static str = "ENOTEMPTY";
pub const XSE_ENOSYS: &'static str = "ENOSYS";
pub const XSE_EROFS: &'static str = "EROFS";
pub const XSE_EBUSY: &'static str = "EBUSY";
pub const XSE_EAGAIN: &'static str = "EAGAIN";
pub const XSE_EISCONN: &'static str = "EISCONN";
pub const XSE_E2BIG: &'static str = "E2BIG";

/// XenStore watch types
pub const XS_WATCH_PATH: u32 = 0;
pub const XS_WATCH_TOKEN: u32 = 1;

/// Miscellaneous protocol values
pub const XENSTORE_PAYLOAD_MAX: usize = 4096;
pub const XENSTORE_ABS_PATH_MAX: usize = 3072;
pub const XENSTORE_REL_PATH_MAX: usize = 2048;
pub const XENSTORE_SERVER_FEATURE_RECONNECTION: usize = 1;
pub const XENSTORE_CONNECTED: usize = 0;
pub const XENSTORE_RECONNECT: usize = 1;
