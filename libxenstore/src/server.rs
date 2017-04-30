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

use connection;
use message::ingress;
use message::egress;
use self::mio::{TryRead, TryWrite};
use self::mio::unix::{UnixListener, UnixStream};
use self::mio::util::Slab;
use std::cell::{RefCell, RefMut};
use std::collections::{HashSet, VecDeque};
use std::io;
use store;
use system::System;
use watch::Watch;
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
                        conn.close(&mut self.system.borrow_mut());
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
    type Message = HashSet<Watch>;

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
                    conn.ready(event_loop, events, &mut self.system.borrow_mut());
                    conn.is_closed()
                };

                // if the result was to close it then remove it
                if is_closed {
                    self.conns.remove(token);
                }
            }
        }
    }

    fn notify(&mut self, event_loop: &mut mio::EventLoop<Server>, msg: HashSet<Watch>) {
        let mut msg = msg;

        for watch in msg.drain() {
            let ref conn_ = self.find_conn_by_token(watch.conn.token);
            let mut conn = conn_.borrow_mut();
            debug!("watch: {:?}", watch);
            conn.enqueue(Box::new(egress::WatchEvent::new(watch)),
                         event_loop,
                         &mut self.system.borrow_mut())
        }
    }
}

type Buffer = io::Cursor<Vec<u8>>;
type TransmissionQueue = VecDeque<Buffer>;

struct Connection {
    // accepted socket
    sock: UnixStream,
    // identifying token for the event loop
    conn: connection::ConnId,
    // current state of this connection
    state: State,
    // outgoing messages enqueued for transmission
    tx_q: TransmissionQueue,
}

impl Connection {
    fn new(sock: UnixStream, token: mio::Token) -> Connection {
        Connection {
            sock: sock,
            conn: connection::ConnId::new(token, store::DOM0_DOMAIN_ID),
            state: State::transition_awaiting_header(),
            tx_q: TransmissionQueue::new(),
        }
    }

    fn enqueue(&mut self,
               msg: Box<egress::Egress>,
               event_loop: &mut mio::EventLoop<Server>,
               system: &mut RefMut<System>) {
        let (hdr, body) = msg.encode();
        self.tx_q.push_back(Buffer::new(hdr.to_vec()));
        self.tx_q.push_back(Buffer::new(body.to_vec()));

        if let State::AwaitingHeader(..) = self.state {
            self.state = State::transition_write();
            if let Err(_) = self.reregister(event_loop) {
                // if we couldn't reregister shut 'er down
                self.close(system);
            }
        }
    }

    fn ready(&mut self,
             event_loop: &mut mio::EventLoop<Server>,
             events: mio::EventSet,
             system: &mut RefMut<System>) {

        debug!("CONN: {:?}. EVENTS: {:?} STATE: {:?}",
               self.conn.token,
               events,
               self.state);

        if events.is_error() {
            debug!("CONN: {:?} unexpected connection error", self.conn.token);
            self.close(system);
            return;
        }

        if events.is_hup() {
            debug!("CONN: {:?} connection was closed by remote",
                   self.conn.token);
            self.close(system);
            return;
        }

        let result = match self.state {
            State::AwaitingHeader(..) |
            State::AwaitingBody(..) => {
                assert!(events.is_readable(),
                        "CONN: {:?} unexpected events: {:?}",
                        self.conn.token,
                        events);
                self.read(system, event_loop)
            }
            State::Write => {
                assert!(events.is_writable(),
                        "CONN: {:?} unexpected events: {:?}",
                        self.conn.token,
                        events);
                self.write()
            }
            _ => unimplemented!(),
        };

        match result {
            // if we processed this and there was an error shut 'er down
            Err(e) => {
                error!("CONN: {:?} failed read|write: {:?}", self.conn.token, e);
                self.close(system);
            }
            Ok(_) => {
                if let Err(_) = self.reregister(event_loop) {
                    // if we couldn't reregister shut 'er down
                    self.close(system);
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
               self.conn.token,
               event_set);

        event_loop.register(&self.sock,
                            self.conn.token,
                            event_set,
                            mio::PollOpt::edge() | mio::PollOpt::oneshot())
            .or_else(|e| {
                         error!("CONN: {:?} Failed to register: {:?}", self.conn.token, e);
                         Err(e)
                     })
    }

    /// Reregister the connection for events from the event loop
    fn reregister(&self, event_loop: &mut mio::EventLoop<Server>) -> io::Result<()> {

        let event_set = match self.state {
            State::AwaitingHeader(..) => mio::EventSet::readable(),
            State::AwaitingBody(..) => mio::EventSet::readable(),
            State::Write => mio::EventSet::writable(),
            State::Closed => {
                return event_loop.deregister(&self.sock);
            }
        };

        debug!("CONN: {:?} reregister to event loop for events: {:?}",
               self.conn.token,
               event_set);

        event_loop.reregister(&self.sock,
                              self.conn.token,
                              event_set,
                              mio::PollOpt::edge() | mio::PollOpt::oneshot())
            .or_else(|e| {
                         error!("CONN: {:?} Failed to reregister: {:?}", self.conn.token, e);
                         Err(e)
                     })
    }


    /// Handle read events for the connection from the event loop
    fn read(&mut self,
            system: &mut RefMut<System>,
            event_loop: &mut mio::EventLoop<Server>)
            -> io::Result<()> {
        let conn = self.conn;

        let new_state = match self.state {
            State::AwaitingHeader(ref mut buf) => {
                if let Some(header) = try!(Self::read_header(&mut self.sock, buf)) {
                    Some((State::transition_awaiting_body(header), None))
                } else {
                    None
                }
            }
            State::AwaitingBody(ref header, ref mut buf) => {
                try!(Self::read_body(&mut self.sock, header, buf)).map(|body| {
                    // when we successfully have a message body parse the entire thing
                    // if we got a successful message back we need to actually process
                    // encode the response for being transmitted
                    let msg = ingress::parse(conn, header, body).process(system);
                    (State::transition_write(), Some(msg))
                })
            }
            _ => unreachable!(),
        };

        // if the state was updated then save it
        if let Some((new_state, tx)) = new_state {
            debug!("CONN: {:?} STATE CHANGE: {:?}", self.conn.token, new_state);

            self.state = new_state;

            // enqueue outgoing tx messages onto our tx queue
            if let Some(tx) = tx {
                self.enqueue(tx.msg, event_loop, system);
                if let Some(watch_events) = tx.watch_events {
                    let sender = event_loop.channel();
                    let _ = sender.send(watch_events);
                }
            }
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
        let success = match self.state {
            State::Write => {
                let header = self.tx_q.front_mut().unwrap();
                try!(Self::write_bytes(&mut self.sock, header)).is_some()
            }
            _ => unreachable!(),
        };

        // if the state was updated then save it
        if success {
            // if we were successful, we consumed the head of the queue
            self.tx_q.pop_front();

            if self.tx_q.is_empty() {
                let new_state = State::transition_awaiting_header();
                debug!("CONN: {:?} STATE CHANGE: {:?}", self.conn.token, new_state);
                self.state = new_state;
            }
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
    fn close(&mut self, system: &mut RefMut<System>) {
        debug!("CONN: {:?} closed", self.conn.token);
        self.state = State::Closed;
        let _ = system.do_watch_mut(|watches| watches.reset(self.conn));
        let _ = system.do_transaction_mut(|txns, _| txns.reset(self.conn));
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
    // write the data out
    Write,
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

    fn transition_write() -> State {
        State::Write
    }
}
