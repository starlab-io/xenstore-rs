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

use error::{Error, Result};
use rand::Rng;
use std::boxed::Box;
use std::collections::HashMap;
use super::wire;
use super::store::{ChangeSet, Store, AppliedChange};

/// The Root Transaction Id.
pub const ROOT_TRANSACTION: wire::TxId = 0;

/// The `TransactionList` type.
///
/// Used to access transactions by TxId as well as start and end transactions.
pub struct TransactionList<R: Rng + ?Sized> {
    list: HashMap<wire::TxId, ChangeSet>,
    rng: Box<R>,
}

/// The `TransactionStatus` type.
///
/// Used to specify whether a transaction succeeded or failed.
#[derive(Debug)]
pub enum TransactionStatus {
    /// Successful transaction
    Success,
    /// Failed transaction
    Failure,
}

impl<R: Rng + ?Sized> TransactionList<R> {
    /// Create a new instance of the `TransactionList`.
    pub fn new(rng: Box<R>) -> TransactionList<R> {
        TransactionList::<R> {
            list: HashMap::new(),
            rng: rng,
        }
    }

    /// Generate a random TxId
    fn generate_txid(&mut self) -> wire::TxId {
        loop {
            // Get a random transaction id
            let id = self.rng.next_u32();
            // If the transaction id is not currently used
            if id != ROOT_TRANSACTION && !self.list.contains_key(&id) {
                // make it the one we will use for this transaction
                return id;
            }
        }
    }

    /// Start a new transaction.
    ///
    /// Returns the `TxId` associated with the new transaction.
    pub fn start(&mut self, store: &Store) -> wire::TxId {
        let next_id = self.generate_txid();
        let changes = ChangeSet::new(store);

        self.list.insert(next_id, changes);
        next_id
    }

    /// Get a reference to a `ChangeSet`.
    ///
    /// # Errors
    ///
    /// * `Error::ENOENT` if the transaction id cannot be found in the list
    pub fn get(&self, tx_id: wire::TxId) -> Result<&ChangeSet> {
        self.list
            .get(&tx_id)
            .ok_or(Error::ENOENT(format!("failed to find transaction {}", tx_id)))
    }

    /// Put a reference to a `ChangeSet`.
    ///
    /// # Errors
    ///
    /// * `Error::ENOENT` if the transaction id cannot be found in the list
    pub fn put(&mut self, tx_id: wire::TxId, changes: ChangeSet) -> Result<()> {
        self.list
            .get_mut(&tx_id)
            .ok_or(Error::ENOENT(format!("failed to find transaction {}", tx_id)))
            .map(|change_set| {
                *change_set = changes;
                ()
            })
    }

    /// End a transaction.
    ///
    /// Given an `TxId` and a `TransactionStatus`, complete the transaction
    /// on success by merging the contents of the transaction store with the root
    /// transaction.
    ///
    /// # Errors
    ///
    /// * `Error::EINVAL` if the root transaction is being ended
    /// * `Error::ENOENT` if the transaction id cannot be found in the list
    pub fn end(&mut self,
               store: &mut Store,
               tx_id: wire::TxId,
               success: TransactionStatus)
               -> Result<Option<Vec<AppliedChange>>> {
        let changes = try!(self.list
            .remove(&tx_id)
            .ok_or(Error::ENOENT(format!("failed to find transaction {}", tx_id))));

        Ok(match success {
            TransactionStatus::Success => store.apply(changes),
            TransactionStatus::Failure => None,
        })
    }
}

#[cfg(test)]
mod test {
    use rand::{Rng, thread_rng};
    use std::boxed::Box;
    use std::num::Wrapping;
    use super::super::error::Error;
    use super::super::path::Path;
    use super::super::store::{Value, DOM0_DOMAIN_ID, Store, ChangeSet};
    use super::*;

    #[test]
    fn check_transaction_id_reuse() {
        struct TestRng {
            next: Wrapping<u32>,
        }

        impl Rng for TestRng {
            fn next_u32(&mut self) -> u32 {
                let cur = self.next;
                self.next += Wrapping(1);
                cur.0
            }
        }

        let store = Store::new();

        let mut txns = TransactionList::new(Box::new(TestRng { next: Wrapping(0) }));
        assert_eq!(txns.start(&store), 1);

        let mut txns = TransactionList::new(Box::new(TestRng { next: Wrapping(u32::max_value()) }));
        assert_eq!(txns.start(&store), u32::max_value());
        assert_eq!(txns.start(&store), 1);
    }

    #[test]
    fn transaction_changeset_can_be_retrieved() {
        let store = Store::new();
        let mut txns = TransactionList::new(Box::new(thread_rng()));

        // Create a new transaction
        let tx_id = txns.start(&store);

        // And verify that it can be retrieved
        txns.get(tx_id).unwrap();
    }

    #[test]
    fn transaction_changeset_can_be_stored() {
        let path = Path::from(DOM0_DOMAIN_ID, "/basic/path");
        let value = Value::from("value");

        let store = Store::new();
        let mut txns = TransactionList::new(Box::new(thread_rng()));

        // Create a new transaction
        let tx_id = txns.start(&store);

        // And verify that it can be retrieved
        let changes = {
            let changes = txns.get(tx_id).unwrap();

            // Write to the transaction
            store.write(&changes, DOM0_DOMAIN_ID, path.clone(), value.clone()).unwrap()
        };

        // Store it back in the transaction store
        txns.put(tx_id, changes).unwrap();

        // And verify that it can be retrieved
        let changes = txns.get(tx_id).unwrap();

        // And we can read the values that we stored in it.
        let v = store.read(&changes, DOM0_DOMAIN_ID, &path).unwrap();

        assert_eq!(v, value);
    }

    #[test]
    fn transaction_ends_with_success() {
        let path = Path::from(DOM0_DOMAIN_ID, "/basic/path");
        let value = Value::from("value");

        let mut store = Store::new();
        let mut txns = TransactionList::new(Box::new(thread_rng()));

        // Create a new transaction
        let tx_id = txns.start(&store);

        // And verify that it can be retrieved
        let changes = {
            let changes = txns.get(tx_id).unwrap();

            // Write to the transaction
            store.write(&changes, DOM0_DOMAIN_ID, path.clone(), value.clone()).unwrap()
        };

        // Store it back in the transaction store
        txns.put(tx_id, changes).unwrap();

        // End the transaction with success
        txns.end(&mut store, tx_id, TransactionStatus::Success).unwrap();

        // And we can read the values that we stored in it.
        let v = store.read(&ChangeSet::new(&store), DOM0_DOMAIN_ID, &path).unwrap();

        assert_eq!(v, value);
    }

    #[test]
    fn transaction_ends_with_failure() {
        let path = Path::from(DOM0_DOMAIN_ID, "/basic/path");
        let value = Value::from("value");

        let mut store = Store::new();
        let mut txns = TransactionList::new(Box::new(thread_rng()));

        // Create a new transaction
        let tx_id = txns.start(&store);

        // And verify that it can be retrieved
        let changes = {
            let changes = txns.get(tx_id).unwrap();

            // Write to the transaction
            store.write(&changes, DOM0_DOMAIN_ID, path.clone(), value.clone()).unwrap()
        };

        // Store it back in the transaction store
        txns.put(tx_id, changes).unwrap();

        // End the transaction with failure
        txns.end(&mut store, tx_id, TransactionStatus::Failure).unwrap();

        // And we cannot read the values that we stored in it because they were
        // not applied to the store
        match store.read(&ChangeSet::new(&store), DOM0_DOMAIN_ID, &path) {
            Err(Error::ENOENT(_)) => assert!(true, "the value was not in the store"),
            Ok(_) => assert!(false, "found the value in the store"),
            _ => assert!(false, "some other error was returned"),
        }
    }

    #[test]
    fn transaction_ends_with_success_colliding() {
        let path = Path::from(DOM0_DOMAIN_ID, "/basic/path");
        let value_external = Value::from("value external");
        let value = Value::from("value");

        let mut store = Store::new();
        let mut txns = TransactionList::new(Box::new(thread_rng()));

        // Create a new transaction
        let tx_id = txns.start(&store);

        // Write to the store
        let changes = store.write(&ChangeSet::new(&store),
                   DOM0_DOMAIN_ID,
                   path.clone(),
                   value_external.clone())
            .unwrap();
        store.apply(changes).unwrap();

        // And we cannot read the values that we stored in it because they were
        // not applied to the store
        let v = store.read(&ChangeSet::new(&store), DOM0_DOMAIN_ID, &path).unwrap();
        assert_eq!(v, value_external);

        // get the transaction we created earlier
        let changes = {
            let changes = txns.get(tx_id).unwrap();

            // Write to the transaction
            store.write(&changes, DOM0_DOMAIN_ID, path.clone(), value.clone()).unwrap()
        };

        let v = store.read(&changes, DOM0_DOMAIN_ID, &path).unwrap();
        // The value was stored in the changeset
        assert_eq!(v, value);

        // Store it back in the transaction store
        txns.put(tx_id, changes).unwrap();

        // End the transaction with success
        txns.end(&mut store, tx_id, TransactionStatus::Success).unwrap();

        // And we cannot read the values that we stored in it because they were
        // not applied to the store
        let v = store.read(&ChangeSet::new(&store), DOM0_DOMAIN_ID, &path).unwrap();

        // Instead, we get back the original value
        assert_eq!(v, value_external);
    }

}
