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

use self::mio::Token;
use wire::DomainId;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ConnId {
    pub token: Token,
    pub dom_id: DomainId,
}

impl ConnId {
    pub fn new(token: Token, dom_id: DomainId) -> ConnId {
        ConnId {
            token: token,
            dom_id: dom_id,
        }
    }
}
