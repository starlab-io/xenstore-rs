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

extern crate bytes;
extern crate futures;
#[macro_use]
extern crate log;
extern crate rand;
extern crate tokio_io;
extern crate tokio_proto;
extern crate tokio_service;

pub mod codec;
pub mod connection;
pub mod error;
pub mod message;
pub mod path;
pub mod server;
pub mod store;
pub mod system;
pub mod transaction;
pub mod watch;
pub mod wire;
