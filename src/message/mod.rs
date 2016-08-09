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

use std::cell::RefMut;
use system;
use wire;

pub type Mfn = u64;
pub type EvtChnPort = u16;

#[derive(Clone, Copy, Debug)]
pub struct Metadata {
    pub dom_id: wire::DomainId,
    pub req_id: wire::ReqId,
    pub tx_id: wire::TxId,
}

pub mod ingress;

pub trait ProcessMessage {
    fn process(&self, &RefMut<system::System>);
}

/// process an incoming directory request
impl ProcessMessage for ingress::Directory {
    fn process(&self, _: &RefMut<system::System>) {}
}

/// process an incoming read request
impl ProcessMessage for ingress::Read {
    fn process(&self, _: &RefMut<system::System>) {}
}

/// process an incoming get permissions request
impl ProcessMessage for ingress::GetPerms {
    fn process(&self, _: &RefMut<system::System>) {}
}

/// process an incoming make directory request
impl ProcessMessage for ingress::Mkdir {
    fn process(&self, _: &RefMut<system::System>) {}
}

/// process an incoming remove request
impl ProcessMessage for ingress::Remove {
    fn process(&self, _: &RefMut<system::System>) {}
}

/// process an incoming watch request
impl ProcessMessage for ingress::Watch {
    fn process(&self, _: &RefMut<system::System>) {}
}

/// process an incoming unwatch request
impl ProcessMessage for ingress::Unwatch {
    fn process(&self, _: &RefMut<system::System>) {}
}

/// process an incoming transaction start request
impl ProcessMessage for ingress::TransactionStart {
    fn process(&self, _: &RefMut<system::System>) {}
}

/// process an incoming release request
impl ProcessMessage for ingress::Release {
    fn process(&self, _: &RefMut<system::System>) {}
}

/// process an incoming get domain path request
impl ProcessMessage for ingress::GetDomainPath {
    fn process(&self, _: &RefMut<system::System>) {}
}

/// process an incoming resume request
impl ProcessMessage for ingress::Resume {
    fn process(&self, _: &RefMut<system::System>) {}
}

/// process an incoming restrict request
impl ProcessMessage for ingress::Restrict {
    fn process(&self, _: &RefMut<system::System>) {}
}

/// process an error that occurred on the ingress path
impl ProcessMessage for ingress::ErrorMsg {
    fn process(&self, _: &RefMut<system::System>) {}
}
