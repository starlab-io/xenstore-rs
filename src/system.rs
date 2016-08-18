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
use std::sync::Mutex;
use super::error::Result;
use super::transaction::*;
use super::watch::*;
use super::wire;
use super::store::*;

use std::ops::{Deref, DerefMut};

lazy_static! {
    static ref STORE: Mutex<Store> = {
        Mutex::new(Store::new())
    };

    static ref WATCHES: Mutex<WatchList> = {
        Mutex::new(WatchList::new())
    };

    static ref TXNS: Mutex<TransactionList> = {
        Mutex::new(TransactionList::new())
    };
}

pub fn do_store_mut<F>(dom_id: wire::DomainId,
                       tx_id: wire::TxId,
                       thunk: F)
                       -> Result<HashSet<Watch>>
    where F: FnOnce(&mut Store, &ChangeSet) -> Result<ChangeSet>
{
    // Create unlocked guards for our three "globals"
    let mut store = STORE.lock().unwrap();
    let mut watches = WATCHES.lock().unwrap();
    let mut txns = TXNS.lock().unwrap();

    let changes = {
        let root_changeset = ChangeSet::new(store.deref());
        // If the transaction ID is the root transaction
        let changeset = match tx_id {
            // return a root changeset
            ROOT_TRANSACTION => &root_changeset,
            // otherwise, look up the transaction ID and return that instead
            _ => try!(txns.deref().get(dom_id, tx_id)),
        };

        // Once we have a changeset, apply the thunk to the data store and
        // the changeset, returning a new ChangeSet
        try!(thunk(store.deref_mut(), changeset))
    };

    Ok(match tx_id {
        // If the transaction ID is the root transaction
        ROOT_TRANSACTION => {
            // Apply the changes to the data store
            let applied = store.deref_mut().apply(changes);
            // and fire any watches associated with the changes
            watches.deref_mut().fire(applied)
        }
        // otherwise
        _ => {
            // just store the changes back with the transaction id
            try!(txns.deref_mut().put(dom_id, tx_id, changes));
            // and return no watches
            HashSet::new()
        }
    })
}

pub fn do_store<F, R>(dom_id: wire::DomainId, tx_id: wire::TxId, thunk: F) -> Result<R>
    where F: FnOnce(&Store, &ChangeSet) -> Result<R>
{
    // Create unlocked guards for the two "globals" we need
    let store = STORE.lock().unwrap();
    let txns = TXNS.lock().unwrap();

    let root_changeset = ChangeSet::new(store.deref());
    // If the transaction ID is the root transaction
    let changeset = match tx_id {
        // return a root changeset
        ROOT_TRANSACTION => &root_changeset,
        // otherwise, look up the transaction ID and return that instead
        _ => try!(txns.deref().get(dom_id, tx_id)),
    };

    // Once we have a changeset, apply the thunk to the data store and
    // the changeset, return the result
    thunk(store.deref(), changeset)
}

pub fn do_watch_mut<F, R>(thunk: F) -> R
    where F: FnOnce(&mut WatchList) -> R
{
    // Create unlocked guards for the "global" we need
    let mut watches = WATCHES.lock().unwrap();

    // Do the watch operation
    thunk(watches.deref_mut())
}

pub fn do_transaction_mut<F, R>(thunk: F) -> R
    where F: FnOnce(&mut TransactionList, &mut Store) -> R
{
    // Create unlocked guards for the two "globals" we need
    let mut store = STORE.lock().unwrap();
    let mut txns = TXNS.lock().unwrap();

    // Do the transaction operation
    thunk(txns.deref_mut(), store.deref_mut())
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

        // set up a watch
        do_watch_mut(|watch_list| {
                watch_list.watch(store::DOM0_DOMAIN_ID, watch::WPath::Normal(path.clone()))
            })
            .unwrap();

        // create a transaction
        let tx_id = do_transaction_mut(|txlst, store| txlst.start(store::DOM0_DOMAIN_ID, store));

        // add the value in the transaction
        let fired_watches = do_store_mut(store::DOM0_DOMAIN_ID, tx_id, |store, changes| {
                store.write(changes, store::DOM0_DOMAIN_ID, path.clone(), value.clone())
            })
            .unwrap();
        assert_eq!(fired_watches.len(), 0);

        // end the transaction
        let changes = do_transaction_mut(|txlst, store| {
                txlst.end(store,
                          store::DOM0_DOMAIN_ID,
                          tx_id,
                          transaction::TransactionStatus::Success)
            })
            .unwrap();

        // fire watches
        let fired_watches = do_watch_mut(|watch_list| watch_list.fire(changes));

        assert_eq!(fired_watches.len(), 1);
    }
}
