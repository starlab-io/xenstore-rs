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
extern crate rustc_serialize;

use self::mio::{TryRead, TryWrite};
use self::mio::unix::{UnixListener, UnixStream};
use self::mio::util::Slab;
use std::io;

const SERVER: mio::Token = mio::Token(0);

pub struct Server {
    // main UNIX socket for the server
    sock: UnixListener,
    // listen of connections accepted by the server
    conns: Slab<Connection>,
}

impl Server {
    /// Create new server listening on a socket
    pub fn new(sock: UnixListener) -> Server {
        // create a slab with a capacity of 1024. need to skip Token(0).
        let slab = Slab::new_starting_at(mio::Token(1), 1024);

        Server {
            sock: sock,
            conns: slab,
        }
    }

    /// Register the server instance with the event loop
    pub fn register(&mut self, event_loop: &mut mio::EventLoop<Server>) -> io::Result<()> {

        debug!("register server socket to event loop");

        event_loop.register(&self.sock,
                            SERVER,
                            mio::EventSet::readable(),
                            mio::PollOpt::edge())
    }

    /// Accept a new connection to the server
    fn accept(&mut self, event_loop: &mut mio::EventLoop<Server>) {

        debug!("accept new socket");

        let sock = match self.sock.accept() {
            Ok(Some(sock)) => {
                debug!("accepted connection");
                sock
            }
            Ok(None) => {
                trace!("socket wasn't actually ready");
                return;
            }
            Err(e) => {
                error!("accept errored: {}", e);
                self.close(event_loop);
                return;
            }
        };

        // create a new connect and attempt to add it to our connection list
        let insert = self.conns.insert_with(|token| Connection::new(sock, token));

        match insert {
            Some(token) => {
                // successful insert so we must register
                let conn = self.find_conn_by_token(token);
                match conn.register(event_loop) {
                    Ok(_) => {}
                    Err(e) => {
                        error!("Failed to register {:?} connection with event loop: {:?}",
                               token,
                               e);
                        conn.close();
                    }
                }
            }
            None => {
                // insert didn't work, things will go out of scope and clean up
                error!("Failed to insert conncetion into list");
            }
        }
    }

    /// Close the server
    fn close(&mut self, event_loop: &mut mio::EventLoop<Server>) {
        event_loop.shutdown();
    }

    /// Find a connection in the slab based on a token
    fn find_conn_by_token<'a>(&'a mut self, token: mio::Token) -> &'a mut Connection {
        &mut self.conns[token]
    }
}

impl mio::Handler for Server {
    type Timeout = ();
    type Message = ();

    fn ready(&mut self,
             event_loop: &mut mio::EventLoop<Server>,
             token: mio::Token,
             events: mio::EventSet) {

        debug!("{:?} connection, events = {:?}", token, events);

        match token {
            // server socket processing
            SERVER => {
                // the server only ever needs to accept connections
                self.accept(event_loop)
            }
            // all other sockets process through their handler
            _ => {
                // process the connection
                let is_closed = {
                    let ref mut conn = self.find_conn_by_token(token);
                    conn.ready(event_loop, events);
                    conn.is_closed()
                };

                // if the result was to close it then remove it
                if is_closed {
                    self.conns.remove(token);
                }
            }
        }
    }
}

struct Connection {
    // accepted socket
    sock: UnixStream,
    // identifying token for the event loop
    token: mio::Token,
    // current state of this connection
    state: State,
}

impl Connection {
    fn new(sock: UnixStream, token: mio::Token) -> Connection {
        Connection {
            sock: sock,
            token: token,
            state: State::AwaitingHeader(Vec::<u8>::with_capacity(16)),
        }
    }

    fn ready(&mut self, event_loop: &mut mio::EventLoop<Server>, events: mio::EventSet) {

        debug!("CONN: {:?}. EVENTS: {:?} STATE: {:?}",
               self.token,
               events,
               self.state);

        let result = match self.state {
            State::AwaitingHeader(..) |
            State::AwaitingPayload(..) => {
                assert!(events.is_readable(),
                        "CONN: {:?} unexpected events: {:?}",
                        self.token,
                        events);
                self.read()
            }
            State::Write(..) => {
                assert!(events.is_writable(),
                        "CONN: {:?} unexpected events: {:?}",
                        self.token,
                        events);
                self.write()
            }
            _ => unimplemented!(),
        };

        match result {
            // if we processed this and there was an error shut 'er down
            Err(_) => self.close(),
            Ok(_) => {
                if let Err(_) = self.reregister(event_loop) {
                    // if we couldn't reregister shut 'er down
                    self.close();
                }
            }
        }
    }

    /// Register the connection for events from the event loop
    fn register(&mut self, event_loop: &mut mio::EventLoop<Server>) -> io::Result<()> {

        let event_set = match self.state {
            State::AwaitingHeader(..) => mio::EventSet::readable(),
            _ => panic!("initial state was not awaiting header"),
        };

        debug!("CONN: {:?} register to event loop for events: {:?}",
               self.token,
               event_set);

        event_loop.register(&self.sock,
                      self.token,
                      event_set,
                      mio::PollOpt::edge() | mio::PollOpt::oneshot())
            .or_else(|e| {
                error!("CONN: {:?} Failed to register: {:?}", self.token, e);
                Err(e)
            })
    }

    /// Reregister the connection for events from the event loop
    fn reregister(&mut self, event_loop: &mut mio::EventLoop<Server>) -> io::Result<()> {

        let event_set = match self.state {
            State::AwaitingHeader(..) => mio::EventSet::readable(),
            State::AwaitingPayload(..) => mio::EventSet::readable(),
            State::Write(..) => mio::EventSet::writable(),
            State::Closed => {
                return event_loop.deregister(&self.sock);
            }
        };

        debug!("CONN: {:?} reregister to event loop for events: {:?}",
               self.token,
               event_set);

        event_loop.reregister(&self.sock,
                        self.token,
                        event_set,
                        mio::PollOpt::edge() | mio::PollOpt::oneshot())
            .or_else(|e| {
                error!("CONN: {:?} Failed to reregister: {:?}", self.token, e);
                Err(e)
            })
    }


    /// Handle read events for the connection from the event loop
    fn read(&mut self) -> io::Result<()> {
        // edge triggering requires us to drain the whole
        match self.sock.try_read_buf(self.state.mut_read_buf()) {
            Ok(Some(0)) => {
                // Remote end closed the connection so close our side
                self.close();
            }
            Ok(Some(n)) => {
                debug!("Read {:?} bytes from {:?} connection", n, self.token);
                debug!("packet: {:?}", self.state.read_buf());

                match self.state {
                    State::AwaitingHeader(..) => {
                        // parse header here and get size of payload
                        self.state.transition_awaiting_payload(2);
                    }
                    State::AwaitingPayload(len, _) => {
                        // if we got all the bytes we needed
                        if n == len {
                            let resp = vec![1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
                            self.state.transition_write_msg(resp);
                        }
                    }
                    _ => unreachable!(),
                }
            }
            Ok(None) => {
                // nothing to read
                debug!("Read 0 bytes from {:?} connection", self.token);
            }
            Err(e) => {
                error!("Failed to read from {:?} connection: {:?}", self.token, e);
                return Err(e);
            }
        }
        Ok(())
    }

    /// Handle write events for the connection from the event loop
    fn write(&mut self) -> io::Result<()> {
        match self.sock.try_write_buf(self.state.mut_write_buf()) {
            // some (or all) of the data was written
            Ok(Some(n)) => {
                if self.state.write_buf().get_ref().capacity() == n {
                    // all bytes written so transition back to wait for header
                    self.state.transition_awaiting_header();
                }
            }
            // the socket wasn't actually ready try again
            Ok(None) => {}
            Err(e) => {
                error!("CONN: {:?} failed to write: {:?}", self.token, e);
                return Err(e);
            }
        }

        Ok(())
    }

    /// Close this connection
    fn close(&mut self) {
        debug!("CONN: {:?} closed", self.token);
        self.state = State::Closed
    }

    /// Reports if this connection is closed
    fn is_closed(&self) -> bool {
        match self.state {
            State::Closed => true,
            _ => false,
        }
    }
}

#[derive(Debug)]
enum State {
    // read the header into the buffer
    AwaitingHeader(Vec<u8>),
    // read the payload into the buffer
    AwaitingPayload(usize, Vec<u8>),
    // write the message out
    Write(io::Cursor<Vec<u8>>),
    // closed and time to clean up
    Closed,
}

impl State {
    fn mut_read_buf(&mut self) -> &mut Vec<u8> {
        match *self {
            State::AwaitingHeader(ref mut buf) => buf,
            State::AwaitingPayload(_, ref mut buf) => buf,
            _ => panic!("connection not in waiting for header state"),
        }
    }

    fn read_buf(&self) -> &[u8] {
        match *self {
            State::AwaitingHeader(ref buf) => buf,
            State::AwaitingPayload(_, ref buf) => buf,
            _ => panic!("connection not in waiting for header state"),
        }
    }

    fn mut_write_buf(&mut self) -> &mut io::Cursor<Vec<u8>> {
        match *self {
            State::Write(ref mut buf) => buf,
            _ => panic!("connection not in writing state"),
        }
    }

    fn write_buf(&self) -> &io::Cursor<Vec<u8>> {
        match *self {
            State::Write(ref buf) => buf,
            _ => panic!("connection not in writing state"),
        }
    }

    fn transition_awaiting_header(&mut self) {
        *self = State::AwaitingHeader(Vec::<u8>::with_capacity(16))
    }

    fn transition_awaiting_payload(&mut self, len: usize) {
        *self = State::AwaitingPayload(len, Vec::<u8>::with_capacity(4096))
    }

    fn transition_write_msg(&mut self, msg: Vec<u8>) {
        *self = State::Write(io::Cursor::new(msg));
    }
}