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

extern crate docopt;
#[macro_use]
extern crate log;
extern crate mio;
extern crate nix;
extern crate rustc_serialize;
extern crate stderrlog;
extern crate xenstore;

use mio::unix::UnixListener;
use nix::sys::signal::{self, sigaction, SigAction, SigHandler, SaFlags, SigSet};
use std::fs::{DirBuilder, remove_file};
use std::path::PathBuf;
use xenstore::server::*;
use xenstore::store;
use xenstore::system;
use xenstore::transaction;
use xenstore::watch;

const UDS_PATH: &'static str = "/var/run/xenstored/socket";

const USAGE: &'static str = "
Usage: rxenstored [-q] [-v...]
";

#[derive(RustcDecodable)]
struct Args {
    flag_v: usize,
    flag_q: bool,
}

extern "C" fn cleanup_handler(_: nix::c_int) {
    let uds_path = PathBuf::from(UDS_PATH);
    remove_file(&uds_path)
        .ok()
        .expect("Failed to remove unix socket");
    std::process::exit(0);
}

fn main() {

    let args: Args = docopt::Docopt::new(USAGE)
        .and_then(|d| d.argv(std::env::args().into_iter()).decode())
        .unwrap_or_else(|e| e.exit());

    stderrlog::new()
        .module(module_path!())
        .module("xenstore")
        .verbosity(args.flag_v)
        .quiet(args.flag_q)
        .init()
        .unwrap();

    let action = SigAction::new(SigHandler::Handler(cleanup_handler),
                                SaFlags::empty(),
                                SigSet::empty());

    unsafe {
        sigaction(signal::SIGINT, &action)
            .ok()
            .expect("Failed to register SIGINT handler");
        sigaction(signal::SIGTERM, &action)
            .ok()
            .expect("Failed to register SIGTERM handler");
    }

    // where our Unix Socket will live, we need to create the path to it
    let uds_path = PathBuf::from(UDS_PATH);
    let uds_dir = uds_path.parent().unwrap();

    DirBuilder::new()
        .recursive(true)
        .create(uds_dir)
        .ok()
        .expect("Failed to created directory for unix socket");

    let sock = UnixListener::bind(&uds_path)
        .ok()
        .expect("Failed to create unix socket");

    let mut event_loop = mio::EventLoop::new()
        .ok()
        .expect("Failed to create event loop");

    let store = store::Store::new();
    let watches = watch::WatchList::new();
    let transactions = transaction::TransactionList::new();
    let system = system::System::new(store, watches, transactions);

    let mut server = Server::new(sock, system);

    server.register(&mut event_loop)
        .ok()
        .expect("Failed register server socket to event loop");

    event_loop.run(&mut server)
        .ok()
        .expect("Failed to start event loop");

    remove_file(&uds_path)
        .ok()
        .expect("Failed to remove unix socket");
}
