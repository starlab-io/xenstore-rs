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

extern crate mio;

use connection;
use futures::{future, Future, BoxFuture};
use message::ingress;
use std::io;
use std::sync::{Arc, Mutex};
use store;
use system::System;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_io::codec::Framed;
use tokio_proto::pipeline::ServerProto;
use tokio_service::Service;
use wire;

pub struct XenStoreProto;

impl<T: AsyncRead + AsyncWrite + 'static> ServerProto<T> for XenStoreProto {
    /// For this protocol style, `Request` matches the `Item` type of the codec's `Encoder`
    type Request = (wire::Header, wire::Body);

    /// For this protocol style, `Response` matches the `Item` type of the codec's `Decoder`
    type Response = (wire::Header, wire::Body);

    /// A bit of boilerplate to hook in the codec:
    type Transport = Framed<T, wire::XenStoreCodec>;
    type BindTransport = Result<Self::Transport, io::Error>;
    fn bind_transport(&self, io: T) -> Self::BindTransport {
        Ok(io.framed(wire::XenStoreCodec))
    }
}

pub struct XenStoredService {
    // datastore system objects
    pub system: Arc<Mutex<System>>,
}

impl Service for XenStoredService {
    // These types must match the corresponding protocol types:
    type Request = (wire::Header, wire::Body);
    type Response = (wire::Header, wire::Body);

    // For non-streaming protocols, service errors are always io::Error
    type Error = io::Error;

    // The future for computing the response; box it for simplicity.
    type Future = BoxFuture<Self::Response, Self::Error>;

    // Produce a future for computing a response from a request.
    fn call(&self, req: Self::Request) -> Self::Future {
        // grab a lock to the System object, it won't fail since
        // we are running single-threaded since that's how xenstored
        // works
        let mut sys = self.system.lock().unwrap();

        // create the connection object that is currently required
        // future refactors will have to change this to know which
        // socket the data came from but right now we just have one
        // socket. We also only currently support dom0 communication
        // so hardcode dom0
        let token = mio::Token(0);
        let conn = connection::ConnId::new(token, store::DOM0_DOMAIN_ID);

        // parse the incoming request (header, body) and process it
        let msg = ingress::parse(conn, &req.0, req.1).process(&mut sys);

        // take the response and encode it to (header, body), this throws
        // away any watches that may have fired so this will need to be
        // fixed in the future
        let (hdr, body) = msg.msg.encode();

        // return the completed future
        future::ok((hdr, body)).boxed()
    }
}
