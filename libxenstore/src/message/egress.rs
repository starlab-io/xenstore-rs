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

use std::error::Error;
use super::*;
use super::super::{error, path, store, watch, wire};

pub trait Egress {
    fn msg_type(&self) -> u32;
    fn md(&self) -> &Metadata;

    fn encode(&self) -> (wire::Header, wire::Body) {
        let body: Vec<Vec<u8>> = Vec::with_capacity(0);

        let header = wire::Header {
            msg_type: self.msg_type(),
            req_id: self.md().req_id,
            tx_id: self.md().tx_id,
            len: body.len() as u32,
        };

        (header, wire::Body(body))
    }
}

macro_rules! egress_no_arg {
    ($id:ident, $val:expr) => {
        pub struct $id {
            pub md: Metadata,
        }

        impl Egress for $id {
            fn msg_type(&self) -> u32 { $val }

            fn md(&self) -> &Metadata { &self.md }
        }
    }
}

egress_no_arg!(Debug, wire::XS_DEBUG);
egress_no_arg!(Watch, wire::XS_WATCH);
egress_no_arg!(Unwatch, wire::XS_UNWATCH);
egress_no_arg!(TransactionEnd, wire::XS_TRANSACTION_END);
egress_no_arg!(Introduce, wire::XS_INTRODUCE);
egress_no_arg!(Release, wire::XS_RELEASE);
egress_no_arg!(Write, wire::XS_WRITE);
egress_no_arg!(Mkdir, wire::XS_MKDIR);
egress_no_arg!(Remove, wire::XS_RM);
egress_no_arg!(SetPerms, wire::XS_SET_PERMS);
egress_no_arg!(Resume, wire::XS_RESUME);
egress_no_arg!(SetTarget, wire::XS_SET_TARGET);
egress_no_arg!(Restrict, wire::XS_RESTRICT);
egress_no_arg!(ResetWatches, wire::XS_RESET_WATCHES);

pub struct Directory {
    pub md: Metadata,
    pub paths: Vec<store::Basename>,
}

impl Egress for Directory {
    fn msg_type(&self) -> u32 {
        wire::XS_DIRECTORY
    }

    fn md(&self) -> &Metadata {
        &self.md
    }

    fn encode(&self) -> (wire::Header, wire::Body) {
        // a build a vector of vectors of u8
        let body = self.paths
            .iter()
            .map(|p| {
                     let mut p = p.as_bytes().to_owned();
                     p.push(b'\0');
                     p
                 })
            .collect();

        // covert to wire::Body
        let body = wire::Body(body);

        let header = wire::Header {
            msg_type: self.msg_type(),
            req_id: self.md().req_id,
            tx_id: self.md().tx_id,
            len: body.len() as u32,
        };

        (header, body)
    }
}

pub struct Read {
    pub md: Metadata,
    pub value: store::Value,
}

impl Egress for Read {
    fn msg_type(&self) -> u32 {
        wire::XS_READ
    }

    fn md(&self) -> &Metadata {
        &self.md
    }

    fn encode(&self) -> (wire::Header, wire::Body) {
        // a build a vector of u8s
        let value = self.value.as_bytes().to_owned();

        // convert to wire::Body
        let body = wire::Body(vec![value]);

        let header = wire::Header {
            msg_type: self.msg_type(),
            req_id: self.md().req_id,
            tx_id: self.md().tx_id,
            len: body.len() as u32,
        };

        (header, body)
    }
}

pub struct GetPerms {
    pub md: Metadata,
    pub perms: Vec<store::Permission>,
}

impl Egress for GetPerms {
    fn msg_type(&self) -> u32 {
        wire::XS_GET_PERMS
    }

    fn md(&self) -> &Metadata {
        &self.md
    }

    fn encode(&self) -> (wire::Header, wire::Body) {
        let perms = self.perms
            .iter()
            .map(|p| {
                let pstr = match p.perm {
                    store::Perm::Read => "r",
                    store::Perm::Write => "w",
                    store::Perm::Both => "b",
                    _ => "n",
                };
                let string = format!("{}{}", pstr, p.id);
                let mut bytes = string.as_bytes().to_owned();
                bytes.push(b'\0');
                bytes
            })
            .collect();

        // convert to wire::Body
        let body = wire::Body(perms);

        let header = wire::Header {
            msg_type: self.msg_type(),
            req_id: self.md().req_id,
            tx_id: self.md().tx_id,
            len: body.len() as u32,
        };

        (header, body)
    }
}

pub struct TransactionStart {
    pub md: Metadata,
    pub tx_id: wire::TxId,
}

impl Egress for TransactionStart {
    fn msg_type(&self) -> u32 {
        wire::XS_TRANSACTION_START
    }

    fn md(&self) -> &Metadata {
        &self.md
    }

    fn encode(&self) -> (wire::Header, wire::Body) {
        let value = format!("{}", self.tx_id).as_bytes().to_owned();

        // convert to wire::Body
        let body = wire::Body(vec![value]);

        let header = wire::Header {
            msg_type: self.msg_type(),
            req_id: self.md().req_id,
            tx_id: self.md().tx_id,
            len: body.len() as u32,
        };

        (header, body)
    }
}

pub struct GetDomainPath {
    pub md: Metadata,
    pub path: path::Path,
}

impl Egress for GetDomainPath {
    fn msg_type(&self) -> u32 {
        wire::XS_GET_DOMAIN_PATH
    }

    fn md(&self) -> &Metadata {
        &self.md
    }
}

pub struct IsDomainIntroduced {
    pub md: Metadata,
    pub introduced: bool,
}

impl Egress for IsDomainIntroduced {
    fn msg_type(&self) -> u32 {
        wire::XS_IS_DOMAIN_INTRODUCED
    }

    fn md(&self) -> &Metadata {
        &self.md
    }
}

pub struct ErrorMsg {
    pub md: Metadata,
    pub err: String,
}

impl ErrorMsg {
    pub fn from(md: Metadata, err: &error::Error) -> ErrorMsg {
        ErrorMsg {
            md: md,
            err: String::from(err.description()),
        }
    }
}

impl Egress for ErrorMsg {
    fn msg_type(&self) -> u32 {
        wire::XS_ERROR
    }

    fn md(&self) -> &Metadata {
        &self.md
    }
}

pub struct WatchEvent {
    pub md: Metadata,
    pub node: watch::WPath,
    pub token: watch::WPath,
}

impl WatchEvent {
    pub fn new(watch: watch::Watch) -> WatchEvent {
        WatchEvent {
            md: Metadata {
                conn: watch.conn,
                req_id: 0,
                tx_id: 0,
            },
            node: watch.node,
            token: watch.token,
        }
    }
}

impl Egress for WatchEvent {
    fn msg_type(&self) -> u32 {
        wire::XS_WATCH_EVENT
    }

    fn md(&self) -> &Metadata {
        &self.md
    }

    fn encode(&self) -> (wire::Header, wire::Body) {

        // convert to wire::Body
        let body = wire::Body(vec![&self.node, &self.token]
                                  .iter()
                                  .map(|p| {
                                           let mut p = p.as_bytes().to_owned();
                                           p.push(b'\0');
                                           p
                                       })
                                  .collect());

        let header = wire::Header {
            msg_type: self.msg_type(),
            req_id: self.md().req_id,
            tx_id: self.md().tx_id,
            len: body.len() as u32,
        };

        (header, body)
    }
}
