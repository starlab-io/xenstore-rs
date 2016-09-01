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

use std::str;
use super::*;
use super::super::{connection, path, watch, wire};
use super::super::error::{Error, Result};

pub trait IngressPath {
    fn new(Metadata, path::Path) -> Self;
}

pub trait IngressWPath {
    fn new(Metadata, watch::WPath, watch::WPath) -> Self;
}

pub trait IngressPathRest {
    fn new(Metadata, path::Path, Vec<String>) -> Self;
}

pub trait IngressBool {
    fn new(Metadata, bool) -> Self;
}

pub trait IngressNoArg {
    fn new(Metadata) -> Self;
}

macro_rules! ingress_path {
    ($id:ident) => {
        pub struct $id {
            pub md: Metadata,
            pub path: path::Path,
        }

        impl IngressPath for $id {
            fn new(md: Metadata, path: path::Path) -> $id {
                $id {
                    md: md,
                    path: path,
                }
            }
        }
    }
}

macro_rules! ingress_wpath {
    ($id:ident) => {
        pub struct $id {
            pub md: Metadata,
            pub node: watch::WPath,
            pub token: watch::WPath,
        }

        impl IngressWPath for $id {
            fn new(md: Metadata, node: watch::WPath, token: watch::WPath) -> $id {
                $id {
                    md: md,
                    node: node,
                    token: token,
                }
            }
        }
    }
}

macro_rules! ingress_path_rest {
    ($id:ident) => {
        pub struct $id {
            pub md: Metadata,
            pub path: path::Path,
            pub rest: Vec<String>,
        }

        impl IngressPathRest for $id {
            fn new(md: Metadata, path: path::Path, rest: Vec<String>) -> $id {
                $id {
                    md: md,
                    path: path,
                    rest: rest,
                }
            }
        }
    }
}

macro_rules! ingress_bool {
    ($id:ident) => {
        pub struct $id {
            pub md: Metadata,
            pub value: bool,
        }

        impl IngressBool for $id {
            fn new(md: Metadata, value: bool) -> $id {
                $id {
                    md: md,
                    value: value,
                }
            }
        }
    }
}

macro_rules! ingress_no_arg {
    ($id:ident) => {
        pub struct $id {
            pub md: Metadata,
        }

        impl IngressNoArg for $id {
            fn new(md: Metadata) -> $id {
                $id {
                    md: md,
                }
            }
        }
    }
}

ingress_path!(Directory);
ingress_path!(Read);
ingress_path!(GetPerms);
ingress_path!(Mkdir);
ingress_path!(Remove);

ingress_path_rest!(Write);
ingress_path_rest!(SetPerms);

ingress_bool!(TransactionEnd);

ingress_wpath!(Watch);
ingress_wpath!(Unwatch);

ingress_no_arg!(TransactionStart);
ingress_no_arg!(Release);
ingress_no_arg!(GetDomainPath);
ingress_no_arg!(Resume);
ingress_no_arg!(Restrict);

pub struct ErrorMsg {
    pub md: Metadata,
    pub err: Error,
}

//    Debug(Metadata, Vec<String>)
//    Introduce(Metadata, Mfn, EvtChnPort)
//    IsDomainIntroduced(Metadata)
//    SetTarget(Metadata, wire::DomainId)
//    Restrict(Metadata)
//    ResetWatches(Metadata)

fn to_strs<'a>(body: &'a wire::Body) -> Result<Vec<&'a str>> {
    // parse out the Vec<Vec<u8>>
    let wire::Body(ref body) = *body;

    body.iter()
        .map(|bytes| {
            str::from_utf8(bytes).map_err(|_| Error::EINVAL(format!("bad supplied string")))
        })
        .collect()
}

fn to_path_str<'a>(body: &'a wire::Body) -> Result<&'a str> {
    // parse out the Vec<&str>
    let strs = to_strs(body);

    strs.and_then(|strs| {
        // this request must contain at most one path
        if strs.len() != 1 {
            let thanks_cargo_fmt = format!("Invalid number of paths received. Expected 1. Got: {}",
                                           strs.len());
            Err(Error::EINVAL(thanks_cargo_fmt))
        } else {
            Ok(strs[0])
        }
    })
}

fn parse_path_only<T: 'static + IngressPath + ProcessMessage>(md: Metadata,
                                                              body: wire::Body)
                                                              -> Result<Box<ProcessMessage>> {
    let dom_id = md.conn.dom_id;
    let path = try!(to_path_str(&body).and_then(|p| path::Path::try_from(dom_id, p)));

    Ok(Box::new(T::new(md, path)))
}

fn parse_wpaths<T: 'static + IngressWPath + ProcessMessage>(md: Metadata,
                                                            body: wire::Body)
                                                            -> Result<Box<ProcessMessage>> {
    let dom_id = md.conn.dom_id;
    let (node, token) = try!(to_strs(&body).and_then(|strs| {
        watch::WPath::try_from(dom_id, strs[0]).and_then(|node| {
            watch::WPath::try_from(dom_id, strs[1]).and_then(|token| Ok((node, token)))
        })
    }));

    Ok(Box::new(T::new(md, node, token)))
}

fn parse_path_rest<T: 'static + IngressPathRest + ProcessMessage>
    (md: Metadata,
     body: wire::Body)
     -> Result<Box<ProcessMessage>> {
    let dom_id = md.conn.dom_id;

    // parse out the Vec<&str>
    let strs = try!(to_strs(&body));

    // this request must contain a path and a value
    if strs.len() < 2 {
        let thanks_cargo_fmt = format!("Invalid number of strs received. Expected at least 2. \
                                        Got: {}",
                                       strs.len());
        return Err(Error::EINVAL(thanks_cargo_fmt));
    }

    let path = try!(path::Path::try_from(dom_id, strs[0]));
    let rest = strs[1..]
        .iter()
        .map(|v| v.to_string())
        .collect();

    Ok(Box::new(T::new(md, path, rest)))
}

fn parse_path_bool<T: 'static + IngressBool + ProcessMessage>(md: Metadata,
                                                              body: wire::Body)
                                                              -> Result<Box<ProcessMessage>> {
    // parse out the Vec<&str>
    let strs = try!(to_strs(&body));

    // this request must contain a path and a value
    if strs.len() != 1 {
        let thanks_cargo_fmt = format!("Invalid number of strs received. Expected 1. \
                                        Got: {}",
                                       strs.len());
        return Err(Error::EINVAL(thanks_cargo_fmt));
    }

    let value = strs[0] == "T";

    Ok(Box::new(T::new(md, value)))
}

fn parse_metadata_only<T: 'static + IngressNoArg + ProcessMessage>
    (md: Metadata)
     -> Result<Box<ProcessMessage>> {
    Ok(Box::new(T::new(md)))
}

pub fn parse(conn: connection::ConnId,
             header: &wire::Header,
             body: wire::Body)
             -> Box<ProcessMessage> {

    let md = Metadata {
        conn: conn,
        req_id: header.req_id,
        tx_id: header.tx_id,
    };

    let msg = match header.msg_type {
        wire::XS_DIRECTORY => parse_path_only::<Directory>(md, body),
        wire::XS_READ => parse_path_only::<Read>(md, body),
        wire::XS_WRITE => parse_path_rest::<Write>(md, body),
        wire::XS_GET_PERMS => parse_path_only::<GetPerms>(md, body),
        wire::XS_SET_PERMS => parse_path_rest::<SetPerms>(md, body),
        wire::XS_MKDIR => parse_path_only::<Mkdir>(md, body),
        wire::XS_RM => parse_path_only::<Remove>(md, body),
        wire::XS_WATCH => parse_wpaths::<Watch>(md, body),
        wire::XS_UNWATCH => parse_wpaths::<Unwatch>(md, body),
        wire::XS_TRANSACTION_START => parse_metadata_only::<TransactionStart>(md),
        wire::XS_TRANSACTION_END => parse_path_bool::<TransactionEnd>(md, body),
        wire::XS_RELEASE => parse_metadata_only::<Release>(md),
        wire::XS_GET_DOMAIN_PATH => parse_metadata_only::<GetDomainPath>(md),
        wire::XS_RESUME => parse_metadata_only::<Resume>(md),
        wire::XS_RESTRICT => parse_metadata_only::<Restrict>(md),
        _ => Err(Error::EINVAL(format!("bad msg id: {}", header.msg_type))),
    };

    msg.unwrap_or_else(|e| {
        Box::new(ErrorMsg {
            md: Metadata {
                conn: conn,
                req_id: header.req_id,
                tx_id: header.tx_id,
            },
            err: e,
        })
    })
}
