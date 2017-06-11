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

use bytes::{Buf, IntoBuf};
use std::marker::PhantomData;
use std::str;
use super::{path, wire};
use super::error::{Error, Result};

enum MsgData {
    Path(path::RelativePath),
    PathWatch(path::RelativePath, path::RelativePath),
    PathRest(path::RelativePath, Vec<String>),
    Bool(bool),
    NoArg,
}

pub struct Request<M> {
    msg_type: M,
    inner: MsgData,
    phantom: PhantomData<M>,
    /*
    pub fn parse(msg_type: wire::Something, body: wire::Body) -> Result<Self> {
        match msg_type {
            wire::XS_DIRECTORY => Directory::parse(body),
            wire::XS_READ => Read::parse(body),
            wire::XS_WRITE => Write::parse(body),
            wire::XS_GET_PERMS => GetPerms::parse(body),
            wire::XS_SET_PERMS => SetPerms::parse(body),
            wire::XS_MKDIR => Mkdir::parse(body),
            wire::XS_RM => Remove::parse(body),
            wire::XS_WATCH => Watch::parse(body),
            wire::XS_UNWATCH => Unwatch::parse(body),
            wire::XS_TRANSACTION_START => TransactionStart::parse(body),
            wire::XS_TRANSACTION_END => TransactionEnd::parse(body),
            wire::XS_RELEASE => Release::parse(body),
            wire::XS_GET_DOMAIN_PATH => GetDomainPath::parse(body),
            wire::XS_RESUME => Resume::parse(body),
            wire::XS_RESTRICT => Restrict::parse(body),
            _ => Err(Error::EINVAL(format!("bad msg id: {}", msg_type))),
        }
    }
    */
}

enum IngressPath {
    Directory = wire::XS_DIRECTORY as isize,
    Read = wire::XS_READ as isize,
    GetPerms = wire::XS_GET_PERMS as isize,
    Mkdir = wire::XS_MKDIR as isize,
    Remove = wire::XS_RM as isize,
}

enum IngressWPath {
    Watch = wire::XS_WATCH as isize,
    Unwatch = wire::XS_UNWATCH as isize,
}

enum IngressPathRest {
    Write = wire::XS_WRITE as isize,
    SetPerms = wire::XS_SET_PERMS as isize,
}

enum IngressBool {
    TransactionEnd = wire::XS_TRANSACTION_END as isize,
}

enum IngressNoArg {
    TransactionStart = wire::XS_TRANSACTION_START as isize,
    Release = wire::XS_RELEASE as isize,
    GetDomainPath = wire::XS_GET_DOMAIN_PATH as isize,
    Resume = wire::XS_RESUME as isize,
    Restrict = wire::XS_RESTRICT as isize,
}

macro_rules! ingress_path {
    ($fnname:ident, $id:ident) => {
        pub fn $fnname(path: path::RelativePath) -> Self {
            Request {
                msg_type: IngressPath::$id,
                inner: MsgData::Path(path),
                phantom: PhantomData,
            }
        }
    }
}

impl Request<IngressPath> {
    ingress_path!(dir, Directory);
    ingress_path!(read, Read);
    ingress_path!(get_perms, GetPerms);
    ingress_path!(mkdir, Mkdir);
    ingress_path!(rm, Remove);

    fn to_bytes(self) -> wire::Body {
        let mut body = match self.inner {
            MsgData::Path(x) => x.into_buf().collect::<Vec<u8>>(),
            _ => unreachable!(),
        };
        // toss the NULL on the end and build our final vector of bytes
        body.push(b'\0');
        wire::Body(vec![body])
    }
}

macro_rules! ingress_wpath {
    ($fnname:ident, $id:ident) => {
        pub fn $fnname(node: path::RelativePath, token: path::RelativePath) -> Self {
            Request {
                msg_type: IngressWPath::$id,
                inner: MsgData::PathWatch(node, token),
                phantom: PhantomData,
            }
        }
    }
}

impl Request<IngressWPath> {
    ingress_wpath!(watch, Watch);
    ingress_wpath!(unwatch, Unwatch);
}

macro_rules! ingress_path_rest {
    ($fnname:ident, $id:ident) => {
        pub fn $fnname(path: path::RelativePath, data: Vec<String>) -> Self {
            Request {
                msg_type: IngressPathRest::$id,
                inner: MsgData::PathRest(path, data),
                phantom: PhantomData,
            }
        }
    }
}

impl Request<IngressPathRest> {
    ingress_path_rest!(write, Write);
    ingress_path_rest!(set_perms, SetPerms);
}

macro_rules! ingress_bool {
    ($fnname:ident, $id:ident) => {
        pub fn $fnname(data: bool) -> Self {
            Request {
                msg_type: IngressBool::$id,
                inner: MsgData::Bool(data),
                phantom: PhantomData,
            }
        }
    }
}

impl Request<IngressBool> {
    ingress_bool!(transaction_end, TransactionEnd);
}

macro_rules! ingress_no_arg {
    ($fnname:ident, $id:ident) => {
        pub fn $fnname() -> Self {
            Request {
                msg_type: IngressNoArg::$id,
                inner: MsgData::NoArg,
                phantom: PhantomData,
            }
        }
    }
}

impl Request<IngressNoArg> {
    ingress_no_arg!(transaction_start, TransactionStart);
    ingress_no_arg!(release, Release);
    ingress_no_arg!(get_domain_path, GetDomainPath);
    ingress_no_arg!(resume, Resume);
    ingress_no_arg!(restrict, Restrict);
}

#[cfg(test)]
mod tests {
    use super::*;
    use path;

    #[test]
    fn dir() {
        let path = path::RelativePath::new("something");
        let req = Request::dir(path);
    }
}
