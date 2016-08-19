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

use std::collections::HashSet;
use super::error::Result;
use super::transaction::*;
use super::watch::*;
use super::wire;
use super::store::*;

pub struct System {
    store: Store,
    watches: WatchList,
    txns: TransactionList,
}

impl System {
    pub fn new(store: Store, watches: WatchList, txns: TransactionList) -> System {
        System {
            store: store,
            watches: watches,
            txns: txns,
        }
    }

    pub fn do_store_mut<F>(&mut self,
                           dom_id: wire::DomainId,
                           tx_id: wire::TxId,
                           thunk: F)
                           -> Result<HashSet<Watch>>
        where F: FnOnce(&mut Store, &ChangeSet) -> Result<ChangeSet>
    {
        let changes = {
            let root_changeset = ChangeSet::new(&self.store);
            // If the transaction ID is the root transaction
            let changeset = match tx_id {
                // return a root changeset
                ROOT_TRANSACTION => &root_changeset,
                // otherwise, look up the transaction ID and return that instead
                _ => try!(self.txns.get(dom_id, tx_id)),
            };

            // Once we have a changeset, apply the thunk to the data store and
            // the changeset, returning a new ChangeSet
            try!(thunk(&mut self.store, changeset))
        };

        Ok(match tx_id {
            // If the transaction ID is the root transaction
            ROOT_TRANSACTION => {
                // Apply the changes to the data store
                let applied = self.store.apply(changes);
                // and fire any watches associated with the changes
                self.watches.fire(applied)
            }
            // otherwise
            _ => {
                // just store the changes back with the transaction id
                try!(self.txns.put(dom_id, tx_id, changes));
                // and return no watches
                HashSet::new()
            }
        })
    }

    pub fn do_store<F, R>(&self, dom_id: wire::DomainId, tx_id: wire::TxId, thunk: F) -> Result<R>
        where F: FnOnce(&Store, &ChangeSet) -> Result<R>
    {
        let root_changeset = ChangeSet::new(&self.store);
        // If the transaction ID is the root transaction
        let changeset = match tx_id {
            // return a root changeset
            ROOT_TRANSACTION => &root_changeset,
            // otherwise, look up the transaction ID and return that instead
            _ => try!(self.txns.get(dom_id, tx_id)),
        };

        // Once we have a changeset, apply the thunk to the data store and
        // the changeset, return the result
        thunk(&self.store, changeset)
    }

    pub fn do_watch_mut<F, R>(&mut self, thunk: F) -> R
        where F: FnOnce(&mut WatchList) -> R
    {
        // Do the watch operation
        thunk(&mut self.watches)
    }

    pub fn do_transaction_mut<F, R>(&mut self, thunk: F) -> R
        where F: FnOnce(&mut TransactionList, &mut Store) -> R
    {
        // Do the transaction operation
        thunk(&mut self.txns, &mut self.store)
    }
}

#[cfg(test)]
mod test {
    use super::super::path;
    use super::super::store;
    use super::super::transaction;
    use super::super::watch;
    use super::*;

    #[test]
    fn test_do_full_test() {
        let path = path::Path::try_from(store::DOM0_DOMAIN_ID, "/root/file/path").unwrap();
        let value = store::Value::from("value");

        let mut system = System::new(store::Store::new(),
                                     watch::WatchList::new(),
                                     transaction::TransactionList::new());

        // set up a watch
        system.do_watch_mut(|watch_list| {
                watch_list.watch(store::DOM0_DOMAIN_ID, watch::WPath::Normal(path.clone()))
            })
            .unwrap();

        // create a transaction
        let tx_id =
            system.do_transaction_mut(|txlst, store| txlst.start(store::DOM0_DOMAIN_ID, store));

        // add the value in the transaction
        let fired_watches = system.do_store_mut(store::DOM0_DOMAIN_ID, tx_id, |store, changes| {
                store.write(changes, store::DOM0_DOMAIN_ID, path.clone(), value.clone())
            })
            .unwrap();
        assert_eq!(fired_watches.len(), 0);

        // end the transaction
        let changes = system.do_transaction_mut(|txlst, store| {
                txlst.end(store,
                          store::DOM0_DOMAIN_ID,
                          tx_id,
                          transaction::TransactionStatus::Success)
            })
            .unwrap();

        // fire watches
        let fired_watches = system.do_watch_mut(|watch_list| watch_list.fire(changes));

        assert_eq!(fired_watches.len(), 1);
    }
}
