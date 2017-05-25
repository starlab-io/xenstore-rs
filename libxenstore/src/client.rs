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

use futures::{BoxFuture};
use std::io;
use std::path::Path;
use tokio_core::reactor::Handle;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_io::codec::Framed;
use tokio_proto::pipeline::{ClientProto, ClientService};
use tokio_service::Service;
use tokio_uds::UnixStream;
use tokio_uds_proto::UnixClient;
use wire;

struct XenStoreProto;

impl<T: AsyncRead + AsyncWrite + 'static> ClientProto<T> for XenStoreProto {
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

struct XenClient {
    inner: ClientService<UnixStream, XenStoreProto>,
}

impl Service for XenClient {
    // These types must match the corresponding protocol types:
    type Request = (wire::Header, wire::Body);
    type Response = (wire::Header, wire::Body);

    // For non-streaming protocols, service errors are always io::Error
    type Error = io::Error;

    // The future for computing the response; box it for simplicity.
    type Future = BoxFuture<Self::Response, Self::Error>;

    // Produce a future for computing a response from a request.
    fn call(&self, req: Self::Request) -> Self::Future {
        Box::new(self.inner.call(req))
    }
}

pub struct Client {
    inner: XenClient,
}

impl Client {
    pub fn connect<P: AsRef<Path>>(path: P, handle: &Handle) -> Result<Client, io::Error> {
        UnixClient::new(XenStoreProto).connect(path, handle).map(|conn| {
            Client { inner: XenClient { inner: conn } }
        })
    }
}
