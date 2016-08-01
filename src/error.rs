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

use std::error;
use std::fmt;
use std::result;
use wire;

#[derive(Debug)]
pub enum Error {
    EINVAL(String),
    EACCES(String),
    EEXIST(String),
    EISDIR(String),
    ENOENT(String),
    ENOMEM(String),
    ENOSPC(String),
    EIO(String),
    ENOTEMPTY(String),
    ENOSYS(String),
    EROFS(String),
    EBUSY(String),
    EAGAIN(String),
    EISCONN(String),
    E2BIG(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Error::EINVAL(ref msg) => write!(f, "{}: {}", wire::XSE_EINVAL, msg),
            Error::EACCES(ref msg) => write!(f, "{}: {}", wire::XSE_EACCES, msg),
            Error::EEXIST(ref msg) => write!(f, "{}: {}", wire::XSE_EEXIST, msg),
            Error::EISDIR(ref msg) => write!(f, "{}: {}", wire::XSE_EISDIR, msg),
            Error::ENOENT(ref msg) => write!(f, "{}: {}", wire::XSE_ENOENT, msg),
            Error::ENOMEM(ref msg) => write!(f, "{}: {}", wire::XSE_ENOMEM, msg),
            Error::ENOSPC(ref msg) => write!(f, "{}: {}", wire::XSE_ENOSPC, msg),
            Error::EIO(ref msg) => write!(f, "{}: {}", wire::XSE_EIO, msg),
            Error::ENOTEMPTY(ref msg) => write!(f, "{}: {}", wire::XSE_ENOTEMPTY, msg),
            Error::ENOSYS(ref msg) => write!(f, "{}: {}", wire::XSE_ENOSYS, msg),
            Error::EROFS(ref msg) => write!(f, "{}: {}", wire::XSE_EROFS, msg),
            Error::EBUSY(ref msg) => write!(f, "{}: {}", wire::XSE_EBUSY, msg),
            Error::EAGAIN(ref msg) => write!(f, "{}: {}", wire::XSE_EAGAIN, msg),
            Error::EISCONN(ref msg) => write!(f, "{}: {}", wire::XSE_EISCONN, msg),
            Error::E2BIG(ref msg) => write!(f, "{}: {}", wire::XSE_E2BIG, msg),
        }
    }
}

impl error::Error for Error {
    fn description(&self) -> &str {
        match *self {
            Error::EINVAL(_) => wire::XSE_EINVAL,
            Error::EACCES(_) => wire::XSE_EACCES,
            Error::EEXIST(_) => wire::XSE_EEXIST,
            Error::EISDIR(_) => wire::XSE_EISDIR,
            Error::ENOENT(_) => wire::XSE_ENOENT,
            Error::ENOMEM(_) => wire::XSE_ENOMEM,
            Error::ENOSPC(_) => wire::XSE_ENOSPC,
            Error::EIO(_) => wire::XSE_EIO,
            Error::ENOTEMPTY(_) => wire::XSE_ENOTEMPTY,
            Error::ENOSYS(_) => wire::XSE_ENOSYS,
            Error::EROFS(_) => wire::XSE_EROFS,
            Error::EBUSY(_) => wire::XSE_EBUSY,
            Error::EAGAIN(_) => wire::XSE_EAGAIN,
            Error::EISCONN(_) => wire::XSE_EISCONN,
            Error::E2BIG(_) => wire::XSE_E2BIG,
        }
    }
}

pub type Result<T> = result::Result<T, Error>;
