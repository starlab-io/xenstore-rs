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

use connection;
use std::collections::HashSet;
use std::sync::MutexGuard;
use super::path;
use store;
use system;
use transaction;
use watch::Watch;
use wire;

pub type Mfn = u64;
pub type EvtChnPort = u16;

#[derive(Clone, Copy, Debug)]
pub struct Metadata {
    pub conn: connection::ConnId,
    pub req_id: wire::ReqId,
    pub tx_id: wire::TxId,
}

pub mod egress;
pub mod ingress;

pub struct Response {
    pub msg: Box<egress::Egress>,
    pub watch_events: Option<HashSet<Watch>>,
}

impl Response {
    fn new(msg: Box<egress::Egress>) -> Response {
        Response {
            msg: msg,
            watch_events: None,
        }
    }

    fn new_with_events(msg: Box<egress::Egress>, events: HashSet<Watch>) -> Response {
        Response {
            msg: msg,
            watch_events: Some(events),
        }
    }
}

pub trait ProcessMessage {
    fn process(&self, &mut MutexGuard<system::System>) -> Response;
}

/// process an incoming directory request
impl ProcessMessage for ingress::Directory {
    fn process(&self, sys: &mut MutexGuard<system::System>) -> Response {
        sys.do_store(self.md.conn,
                      self.md.tx_id,
                      |store, changes| store.directory(changes, self.md.conn.dom_id, &self.path))
            .map(|entries| {
                     Response::new(Box::new(egress::Directory {
                                                md: self.md,
                                                paths: entries,
                                            }))
                 })
            .unwrap_or_else(|e| Response::new(Box::new(egress::ErrorMsg::from(self.md, &e))))
    }
}

/// process an incoming read request
impl ProcessMessage for ingress::Read {
    fn process(&self, sys: &mut MutexGuard<system::System>) -> Response {
        sys.do_store(self.md.conn,
                      self.md.tx_id,
                      |store, changes| store.read(changes, self.md.conn.dom_id, &self.path))
            .map(|value| {
                     Response::new(Box::new(egress::Read {
                                                md: self.md,
                                                value: value,
                                            }))
                 })
            .unwrap_or_else(|e| Response::new(Box::new(egress::ErrorMsg::from(self.md, &e))))
    }
}

/// process an incoming get permissions request
impl ProcessMessage for ingress::GetPerms {
    fn process(&self, sys: &mut MutexGuard<system::System>) -> Response {
        sys.do_store(self.md.conn,
                      self.md.tx_id,
                      |store, changes| store.get_perms(changes, self.md.conn.dom_id, &self.path))
            .map(|perms| {
                     Response::new(Box::new(egress::GetPerms {
                                                md: self.md,
                                                perms: perms,
                                            }))
                 })
            .unwrap_or_else(|e| Response::new(Box::new(egress::ErrorMsg::from(self.md, &e))))
    }
}

/// process an incoming make directory request
impl ProcessMessage for ingress::Mkdir {
    fn process(&self, sys: &mut MutexGuard<system::System>) -> Response {
        let mut sys = sys;
        sys.do_store_mut(self.md.conn, self.md.tx_id, |store, changes| {
                store.mkdir(changes, self.md.conn.dom_id, self.path.clone())
            })
            .map(|watch_events| {
                     Response::new_with_events(Box::new(egress::Mkdir { md: self.md }),
                                               watch_events)
                 })
            .unwrap_or_else(|e| Response::new(Box::new(egress::ErrorMsg::from(self.md, &e))))
    }
}

/// process an incoming remove request
impl ProcessMessage for ingress::Remove {
    fn process(&self, sys: &mut MutexGuard<system::System>) -> Response {
        let mut sys = sys;
        sys.do_store_mut(self.md.conn,
                          self.md.tx_id,
                          |store, changes| store.rm(changes, self.md.conn.dom_id, &self.path))
            .map(|watch_events| {
                     Response::new_with_events(Box::new(egress::Remove { md: self.md }),
                                               watch_events)
                 })
            .unwrap_or_else(|e| Response::new(Box::new(egress::ErrorMsg::from(self.md, &e))))
    }
}

/// process an incoming watch request
impl ProcessMessage for ingress::Watch {
    fn process(&self, sys: &mut MutexGuard<system::System>) -> Response {
        let mut sys = sys;
        sys.do_watch_mut(|watches| {
                              watches.watch(self.md.conn, self.node.clone(), self.token.clone())
                          })
            .map(|_| Response::new(Box::new(egress::Watch { md: self.md })))
            .unwrap_or_else(|e| Response::new(Box::new(egress::ErrorMsg::from(self.md, &e))))
    }
}

/// process an incoming unwatch request
impl ProcessMessage for ingress::Unwatch {
    fn process(&self, sys: &mut MutexGuard<system::System>) -> Response {
        let mut sys = sys;
        sys.do_watch_mut(|watches| {
                              watches.unwatch(self.md.conn, self.node.clone(), self.token.clone())
                          })
            .map(|_| Response::new(Box::new(egress::Unwatch { md: self.md })))
            .unwrap_or_else(|e| Response::new(Box::new(egress::ErrorMsg::from(self.md, &e))))
    }
}

/// process an incoming transaction start request
impl ProcessMessage for ingress::TransactionStart {
    fn process(&self, sys: &mut MutexGuard<system::System>) -> Response {
        let mut sys = sys;
        let tx_id = sys.do_transaction_mut(|txns, store| txns.start(self.md.conn, &store));
        Response::new(Box::new(egress::TransactionStart {
                                   md: self.md,
                                   tx_id: tx_id,
                               }))
    }
}

/// process an incoming transaction end request
impl ProcessMessage for ingress::TransactionEnd {
    fn process(&self, sys: &mut MutexGuard<system::System>) -> Response {
        let mut sys = sys;
        let complete = if self.value {
            transaction::TransactionStatus::Success
        } else {
            transaction::TransactionStatus::Failure
        };

        sys.do_transaction_mut(|txns, store| txns.end(store, self.md.conn, self.md.tx_id, complete))
            .map(|changes| {
                     let watch_events = sys.do_watch_mut(|watch_list| watch_list.fire(changes));
                     Response::new_with_events(Box::new(egress::TransactionEnd { md: self.md }),
                                               watch_events)
                 })
            .unwrap_or_else(|e| Response::new(Box::new(egress::ErrorMsg::from(self.md, &e))))
    }
}

/// process an incoming release request
impl ProcessMessage for ingress::Release {
    fn process(&self, _: &mut MutexGuard<system::System>) -> Response {
        Response::new(Box::new(egress::Release { md: self.md }))
    }
}

/// process an incoming get domain path request
impl ProcessMessage for ingress::GetDomainPath {
    fn process(&self, _: &mut MutexGuard<system::System>) -> Response {
        Response::new(Box::new(egress::GetDomainPath {
                                   md: self.md,
                                   path: path::get_domain_path(self.md.conn.dom_id),
                               }))
    }
}

/// process an incoming resume request
impl ProcessMessage for ingress::Resume {
    fn process(&self, _: &mut MutexGuard<system::System>) -> Response {
        Response::new(Box::new(egress::Resume { md: self.md }))
    }
}

/// process an incoming restrict request
impl ProcessMessage for ingress::Restrict {
    fn process(&self, _: &mut MutexGuard<system::System>) -> Response {
        Response::new(Box::new(egress::Restrict { md: self.md }))
    }
}

/// process an error that occurred while parsing
impl ProcessMessage for ingress::ErrorMsg {
    fn process(&self, _: &mut MutexGuard<system::System>) -> Response {
        Response::new(Box::new(egress::ErrorMsg::from(self.md, &self.err)))
    }
}

/// process an incoming write request
impl ProcessMessage for ingress::Write {
    fn process(&self, sys: &mut MutexGuard<system::System>) -> Response {
        let mut sys = sys;
        sys.do_store_mut(self.md.conn, self.md.tx_id, |store, changes| {
                store.write(changes,
                            self.md.conn.dom_id,
                            self.path.clone(),
                            self.rest[0].clone())
            })
            .map(|watch_events| {
                     let msg = Box::new(egress::Write { md: self.md });
                     Response::new_with_events(msg, watch_events)
                 })
            .unwrap_or_else(|e| Response::new(Box::new(egress::ErrorMsg::from(self.md, &e))))
    }
}

/// process an incoming set_perms request
impl ProcessMessage for ingress::SetPerms {
    fn process(&self, sys: &mut MutexGuard<system::System>) -> Response {
        let perms = self.rest
            .iter()
            .map(|s| {
                // FIXME: get rid of the unwraps here
                let id = s[1..].parse::<wire::DomainId>().unwrap();
                let perm = match s.chars().nth(0).unwrap() {
                    'r' => store::Perm::Read,
                    'w' => store::Perm::Write,
                    'b' => store::Perm::Both,
                    _ => store::Perm::None,
                };

                store::Permission {
                    id: id,
                    perm: perm,
                }
            })
            .collect();

        let mut sys = sys;
        sys.do_store_mut(self.md.conn, self.md.tx_id, |store, changes| {
                store.set_perms(changes, self.md.conn.dom_id, &self.path, perms)
            })
            .map(|watch_events| {
                     Response::new_with_events(Box::new(egress::SetPerms { md: self.md }),
                                               watch_events)
                 })
            .unwrap_or_else(|e| Response::new(Box::new(egress::ErrorMsg::from(self.md, &e))))
    }
}
