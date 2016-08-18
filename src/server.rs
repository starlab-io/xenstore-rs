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

use message::ingress;
use self::mio::{TryRead, TryWrite};
use self::mio::unix::{UnixListener, UnixStream};
use self::mio::util::Slab;
use std::cell::{RefCell, RefMut};
use std::io;
use store;
use system::System;
use wire;

const SERVER: mio::Token = mio::Token(0);

pub struct Server {
    // main UNIX socket for the server
    sock: UnixListener,
    // listen of connections accepted by the server
    conns: Slab<RefCell<Connection>>,
    // datastore system objects
    system: RefCell<System>,
}

impl Server {
    /// Create new server listening on a socket
    pub fn new(sock: UnixListener, system: System) -> Server {
        // create a slab with a capacity of 1024. need to skip Token(0).
        let slab = Slab::new_starting_at(mio::Token(1), 1024);

        Server {
            sock: sock,
            conns: slab,
            system: RefCell::new(system),
        }
    }

    /// Register the server instance with the event loop
    pub fn register(&self, event_loop: &mut mio::EventLoop<Server>) -> io::Result<()> {

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
        let insert = self.conns.insert_with(|token| RefCell::new(Connection::new(sock, token)));

        match insert {
            Some(token) => {
                // successful insert so we must register
                let conn_ = self.find_conn_by_token(token);
                let mut conn = conn_.borrow_mut();
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
    fn close(&self, event_loop: &mut mio::EventLoop<Server>) {
        event_loop.shutdown();
    }

    /// Find a connection in the slab based on a token
    fn find_conn_by_token(&self, token: mio::Token) -> &RefCell<Connection> {
        &self.conns[token]
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
                    let ref conn_ = self.find_conn_by_token(token);
                    let mut conn = conn_.borrow_mut();
                    conn.ready(event_loop, events, self.system.borrow_mut());
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
            state: State::transition_awaiting_header(),
        }
    }

    fn ready(&mut self,
             event_loop: &mut mio::EventLoop<Server>,
             events: mio::EventSet,
             system: RefMut<System>) {

        debug!("CONN: {:?}. EVENTS: {:?} STATE: {:?}",
               self.token,
               events,
               self.state);

        if events.is_error() {
            debug!("CONN: {:?} unexpected connection error", self.token);
            self.close();
            return;
        }

        if events.is_hup() {
            debug!("CONN: {:?} connection was closed by remote", self.token);
            self.close();
            return;
        }

        let result = match self.state {
            State::AwaitingHeader(..) |
            State::AwaitingBody(..) => {
                assert!(events.is_readable(),
                        "CONN: {:?} unexpected events: {:?}",
                        self.token,
                        events);
                self.read(system)
            }
            State::WriteHeader(..) |
            State::WriteBody(..) => {
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
            Err(e) => {
                error!("CONN: {:?} failed read|write: {:?}", self.token, e);
                self.close();
            }
            Ok(_) => {
                if let Err(_) = self.reregister(event_loop) {
                    // if we couldn't reregister shut 'er down
                    self.close();
                }
            }
        }
    }

    /// Register the connection for events from the event loop
    fn register(&self, event_loop: &mut mio::EventLoop<Server>) -> io::Result<()> {

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
    fn reregister(&self, event_loop: &mut mio::EventLoop<Server>) -> io::Result<()> {

        let event_set = match self.state {
            State::AwaitingHeader(..) => mio::EventSet::readable(),
            State::AwaitingBody(..) => mio::EventSet::readable(),
            State::WriteHeader(..) => mio::EventSet::writable(),
            State::WriteBody(..) => mio::EventSet::writable(),
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
    fn read(&mut self, system: RefMut<System>) -> io::Result<()> {
        let new_state = match self.state {
            State::AwaitingHeader(ref mut buf) => {
                if let Some(header) = try!(Self::read_header(&mut self.sock, buf)) {
                    Some(State::transition_awaiting_body(header))
                } else {
                    None
                }
            }
            State::AwaitingBody(ref header, ref mut buf) => {
                try!(Self::read_body(&mut self.sock, header, buf)).map(|body| {
                    // when we successfully have a message body parse the entire thing
                    // if we got a successful message back we need to actually process
                    // encode the response for being transmitted
                    let (resp_hdr, resp_body) = ingress::parse(store::DOM0_DOMAIN_ID, header, body)
                        .process(&system)
                        .encode();
                    State::transition_write_header(resp_hdr, resp_body)
                })
            }
            _ => unreachable!(),
        };

        debug!("CONN: {:?} STATE CHANGE: {:?}", self.token, new_state);

        // if the state was updated then save it
        if let Some(new_state) = new_state {
            self.state = new_state;
        }

        Ok(())
    }

    /// Read the header from the socket
    fn read_header<R: io::Read>(input: &mut R,
                                buf: &mut Vec<u8>)
                                -> io::Result<Option<wire::Header>> {
        // read as much as we can
        match try!(input.try_read_buf(buf)) {
            Some(n) if n > 0 => {
                debug!("recv: {:?} bytes", n);
                // if we got some data try to parser the header
                Ok(wire::Header::parse(&buf))
            }
            Some(_) => Err(io::Error::new(io::ErrorKind::UnexpectedEof, "0 bytes read")),
            None => Ok(None),
        }
    }

    /// Read the body from the socket
    fn read_body<R: io::Read>(input: &mut R,
                              header: &wire::Header,
                              buf: &mut Vec<u8>)
                              -> io::Result<Option<wire::Body>> {
        try!(input.try_read_buf(buf));
        Ok(wire::Body::parse(header, buf))
    }

    /// Handle write events for the connection from the event loop
    fn write(&mut self) -> io::Result<()> {
        let new_state = match self.state {
            State::WriteHeader(ref mut header, ref mut body) => {
                match try!(Self::write_bytes(&mut self.sock, header)) {
                    Some(_) => Some(State::transition_write_body(body)),
                    None => None,
                }
            }
            State::WriteBody(ref mut body) => {
                match try!(Self::write_bytes(&mut self.sock, body)) {
                    Some(_) => Some(State::transition_awaiting_header()),
                    None => None,
                }
            }
            _ => unreachable!(),
        };

        // if the state was updated then save it
        if let Some(new_state) = new_state {
            debug!("CONN: {:?} STATE CHANGE: {:?}", self.token, new_state);
            self.state = new_state;
        }

        Ok(())
    }

    /// Write a bag of bytes back to the client
    fn write_bytes<W: io::Write>(output: &mut W,
                                 bytes: &mut io::Cursor<Vec<u8>>)
                                 -> io::Result<Option<()>> {
        // write out the header
        let res = try!(output.try_write_buf(bytes));

        // check that everything was sent
        if let Some(n) = res {
            // if we sent everything then its a success
            if n == bytes.position() as usize {
                return Ok(Some(()));
            }
        }
        Ok(None)
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
    // read the body into the buffer
    AwaitingBody(wire::Header, Vec<u8>),
    // write the response header out
    WriteHeader(io::Cursor<Vec<u8>>, wire::Body),
    // write the response body out
    WriteBody(io::Cursor<Vec<u8>>),
    // closed and time to clean up
    Closed,
}

impl State {
    fn transition_awaiting_header() -> State {
        State::AwaitingHeader(Vec::<u8>::with_capacity(wire::HEADER_SIZE))
    }

    fn transition_awaiting_body(header: wire::Header) -> State {
        let len = header.len();
        State::AwaitingBody(header, Vec::<u8>::with_capacity(len))
    }

    fn transition_write_header(header: wire::Header, body: wire::Body) -> State {
        State::WriteHeader(io::Cursor::new(header.to_vec()), body)
    }

    fn transition_write_body(body: &mut wire::Body) -> State {
        State::WriteBody(io::Cursor::new(body.to_vec()))
    }
}
