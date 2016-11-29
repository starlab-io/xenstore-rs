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

#[cfg(test)]
extern crate quickcheck;

use byteorder::{ReadBytesExt, NativeEndian, WriteBytesExt};
use std::io;

#[cfg(test)]
use self::quickcheck::{Arbitrary, Gen};

/// XenStore message types
pub const XS_DEBUG: u32 = 0;
pub const XS_DIRECTORY: u32 = 1;
pub const XS_READ: u32 = 2;
pub const XS_GET_PERMS: u32 = 3;
pub const XS_WATCH: u32 = 4;
pub const XS_UNWATCH: u32 = 5;
pub const XS_TRANSACTION_START: u32 = 6;
pub const XS_TRANSACTION_END: u32 = 7;
pub const XS_INTRODUCE: u32 = 8;
pub const XS_RELEASE: u32 = 9;
pub const XS_GET_DOMAIN_PATH: u32 = 10;
pub const XS_WRITE: u32 = 11;
pub const XS_MKDIR: u32 = 12;
pub const XS_RM: u32 = 13;
pub const XS_SET_PERMS: u32 = 14;
pub const XS_WATCH_EVENT: u32 = 15;
pub const XS_ERROR: u32 = 16;
pub const XS_IS_DOMAIN_INTRODUCED: u32 = 17;
pub const XS_RESUME: u32 = 18;
pub const XS_SET_TARGET: u32 = 19;
pub const XS_RESTRICT: u32 = 20;
pub const XS_RESET_WATCHES: u32 = 21;
pub const XS_INVALID: u32 = 0xffff;

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

pub type ReqId = u32;
pub type TxId = u32;
pub type DomainId = u32;

/// A `Header` is always 16 bytes long
pub const HEADER_SIZE: usize = 16;
/// A `Body` is at most 4k
pub const BODY_SIZE: usize = 4096;

/// The `Header` type that is generic to all messages
#[derive(Clone, Debug, PartialEq)]
pub struct Header {
    pub msg_type: u32,
    pub req_id: ReqId,
    pub tx_id: TxId,
    pub len: u32,
}

impl Header {
    /// Parse the header
    pub fn parse(bytes: &[u8]) -> Option<Header> {
        let mut input = io::Cursor::new(bytes);
        let msg_type = try_opt!(input.read_u32::<NativeEndian>().ok());
        let req_id = try_opt!(input.read_u32::<NativeEndian>().ok());
        let tx_id = try_opt!(input.read_u32::<NativeEndian>().ok());
        let len = try_opt!(input.read_u32::<NativeEndian>().ok());

        Some(Header {
            msg_type: msg_type,
            req_id: req_id,
            tx_id: tx_id,
            len: len,
        })
    }

    /// Output the header as a vector of bytes
    pub fn to_vec(&self) -> Vec<u8> {
        let mut ret = io::Cursor::new(vec![0u8; HEADER_SIZE]);
        ret.write_u32::<NativeEndian>(self.msg_type).unwrap();
        ret.write_u32::<NativeEndian>(self.req_id).unwrap();
        ret.write_u32::<NativeEndian>(self.tx_id).unwrap();
        ret.write_u32::<NativeEndian>(self.len).unwrap();

        ret.into_inner()
    }

    /// Provide the length that the body should be
    pub fn len(&self) -> usize {
        self.len as usize
    }
}

#[cfg(test)]
impl Arbitrary for Header {
    fn arbitrary<G: Gen>(g: &mut G) -> Header {
        Header {
            msg_type: u32::arbitrary(g),
            req_id: u32::arbitrary(g),
            tx_id: u32::arbitrary(g),
            len: u32::arbitrary(g),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Body(pub Vec<Vec<u8>>);

impl Body {
    pub fn parse(header: &Header, body: &[u8]) -> Option<Body> {
        if header.len as usize != body.len() {
            return None;
        }

        // check that we're null terminated
        match body.len() {
            0 => return None,
            n @ _ => {
                if body[n - 1] != b'\0' {
                    return None;
                }
            }
        }

        // break the payload at NULL characters
        let res: Vec<Vec<u8>> = body.split(|b| *b == b'\0')
            .filter(|f| f.len() != 0)
            .map(|f| f.to_owned())
            .collect();

        Some(Body(res))
    }

    /// Output the body as a vector of bytes
    pub fn to_vec(&mut self) -> Vec<u8> {
        let mut ret = Vec::<u8>::with_capacity(BODY_SIZE);

        // every field is separated by a NULL byte
        for field in &self.0 {
            if !field.is_empty() {
                ret.extend_from_slice(&field);
                ret.push(b'\0');
            }
        }

        ret
    }
}

#[cfg(test)]
mod tests {

    use super::{Body, Header};
    use super::quickcheck::{quickcheck, Arbitrary, Gen};

    #[test]
    fn header_parse_values() {
        let hdr = vec![1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0, 4, 0, 0, 0];
        let header = Header::parse(&hdr).unwrap();

        assert_eq!(header.msg_type, 1);
        assert_eq!(header.req_id, 2);
        assert_eq!(header.tx_id, 3);
        assert_eq!(header.len, 4);
    }

    #[test]
    fn header_idempotent() {
        fn prop(hdr: Header) -> bool {
            let bytes = hdr.to_vec();
            let decoded_hdr = Header::parse(&bytes).unwrap();

            decoded_hdr == hdr
        }

        quickcheck(prop as fn(Header) -> bool);
    }

    #[test]
    fn header_parse() {
        fn prop(bytes: Vec<u8>) -> bool {
            // if its less than 16 bytes then it should fail to parse
            // otherwise it should be good
            let expected = match bytes.len() {
                0...15 => false,
                _ => true,
            };

            // did it parse
            let result = Header::parse(&bytes).is_some();

            // logical biconditional people
            // that's the negation of exclusive or
            // which is true when both inputs are the same
            !(expected ^ result)
        }

        quickcheck(prop as fn(Vec<u8>) -> bool);
    }

    #[test]
    fn body_parse() {

        #[derive(Clone, Debug, PartialEq)]
        struct BodyBytes(Vec<u8>);

        impl Arbitrary for BodyBytes {
            fn arbitrary<G: Gen>(g: &mut G) -> BodyBytes {
                let size = g.gen_range(0, 4096);
                let mut vec = Vec::<u8>::with_capacity(size);
                g.fill_bytes(&mut vec);

                BodyBytes(vec)
            }
        }

        fn prop(bytes: BodyBytes) -> bool {
            // get the byte vector
            let bytes = bytes.0;

            // if its not NULL terminated then we need to bail
            let expected = match bytes.len() {
                0 => false,
                _ => bytes[bytes.len() - 1] == b'\0',
            };

            // build a header
            let header = Header {
                msg_type: 0,
                req_id: 0,
                tx_id: 0,
                len: bytes.len() as u32,
            };

            // did it parse
            let result = Body::parse(&header, &bytes).is_some();

            // moar logical biconditional
            !(expected ^ result)
        }

        quickcheck(prop as fn(BodyBytes) -> bool);
    }

}
