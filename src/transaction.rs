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
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::num::Wrapping;
use std::sync::{Mutex, Arc};
use super::wire;
use super::path::Path;

#[derive(Clone, Debug)]
pub enum Perm {
    Read,
    Write,
    Both,
    None,
    Owner,
}

/// The Root Transaction Id.
pub const ROOT_TRANSACTION: wire::TxId = 0;

/// The Dom0 Domain Id.
pub const DOM0_DOMAIN_ID: wire::DomainId = 0;

pub type Basename = String;
pub type Value = String;
pub type Permissions = HashMap<wire::DomainId, Perm>;

#[derive(Clone, Debug)]
struct Node {
    pub value: Value,
    pub children: HashSet<Basename>,
    pub permissions: Permissions,
}

type Store = HashMap<Path, Node>;

/// The `Transaction` type. Every operation acting on the database occurs within
/// a transaction of some sort. If an explicit transaction is not specified, the
/// operations will occur inside of the implicit global transaction.
#[derive(Debug)]
pub struct Transaction {
    tx_id: wire::TxId,
    current_gen: Wrapping<u64>,
    parent_gen: Wrapping<u64>,
    store: Store,
}

/// The `LockedTransaction` type.
///
/// Used to provide shared, multi-threaded access to a `Transaction`.
pub type LockedTransaction = Arc<Mutex<RefCell<Transaction>>>;

/// The `TransactionList` type.
///
/// Used to access transactions by TxId as well as start and end transactions.
#[derive(Debug)]
pub struct TransactionList<R: Rng + ?Sized> {
    list: HashMap<wire::TxId, LockedTransaction>,
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

/// Insert manual entries into a Store
fn manual_entry(store: &mut Store, name: Path, child_list: Vec<Basename>) {
    let mut children = HashSet::new();
    for child in child_list {
        children.insert(child);
    }

    let mut permissions = HashMap::new();
    permissions.insert(DOM0_DOMAIN_ID, Perm::None);

    store.insert(name,
                 Node {
                     value: Value::from(""),
                     children: children,
                     permissions: permissions,
                 });
}

impl<R: Rng + ?Sized> TransactionList<R> {
    /// Create a new instance of the `TransactionList`.
    pub fn new(rng: Box<R>) -> TransactionList<R> {
        let mut store = Store::new();
        manual_entry(&mut store,
                     Path::from(DOM0_DOMAIN_ID, "/"),
                     vec![Basename::from("tool")]);
        manual_entry(&mut store,
                     Path::from(DOM0_DOMAIN_ID, "/tool"),
                     vec![Basename::from("xenstored")]);
        manual_entry(&mut store,
                     Path::from(DOM0_DOMAIN_ID, "/tool/xenstored"),
                     vec![]);

        let root = Transaction {
            tx_id: ROOT_TRANSACTION,
            current_gen: Wrapping(0),
            parent_gen: Wrapping(0),
            store: store,
        };

        let mut txns = HashMap::new();
        txns.insert(ROOT_TRANSACTION, Arc::new(Mutex::new(RefCell::new(root))));

        TransactionList::<R> {
            list: txns,
            rng: rng,
        }
    }

    /// Get an instance of a `LockedTransaction`.
    ///
    /// # Errors
    ///
    /// * `Error::ENOENT` if the transaction id cannot be found in the list
    pub fn get(&self, tx_id: wire::TxId) -> Result<LockedTransaction> {
        self.list
            .get(&tx_id)
            .map(|t| t.clone())
            .ok_or(Error::ENOENT(format!("failed to find transaction {}", tx_id)))
    }

    /// Generate a random TxId
    fn generate_txid(&mut self) -> wire::TxId {
        loop {
            // Get a random transaction id
            let id = self.rng.next_u32();
            // If the transaction id is not currently used
            if !self.list.contains_key(&id) {
                // make it the one we will use for this transaction
                return id;
            }
        }
    }

    /// Start a new transaction.
    ///
    /// Returns the `TxId` associated with the new transaction.
    pub fn start(&mut self) -> wire::TxId {

        let next_id = self.generate_txid();

        let txn = {
            let mutex = self.list
                .get(&ROOT_TRANSACTION)
                .unwrap();
            let root_guard = mutex.lock()
                .unwrap();
            let root = root_guard.borrow();

            Transaction {
                tx_id: next_id,
                current_gen: root.current_gen,
                parent_gen: root.current_gen,
                store: root.store.clone(),
            }
        };

        self.list.insert(next_id, Arc::new(Mutex::new(RefCell::new(txn))));
        next_id
    }

    /// End a transaction.
    ///
    /// Given an `Transaction` and a `TransactionStatus`, complete the transaction
    /// on success by merging the contents of the transaction store with the root
    /// transaction.
    ///
    /// # Errors
    ///
    /// * `Error::EINVAL` if the root transaction is being ended
    /// * `Error::ENOENT` if the transaction id cannot be found in the list
    pub fn end(&mut self, trans: &Transaction, success: TransactionStatus) -> Result<()> {
        if trans.is_root_transaction() {
            return Err(Error::EINVAL(format!("trying to end the root transaction")));
        }

        let _ = try!(self.list
            .remove(&trans.tx_id)
            .ok_or(Error::ENOENT(format!("failed to find transaction {}", trans.tx_id))));

        match success {
            TransactionStatus::Success => {
                let root_guard = self.list
                    .get_mut(&ROOT_TRANSACTION)
                    .unwrap()
                    .lock()
                    .unwrap();
                let mut root = root_guard.borrow_mut();

                root.merge(trans)
            }
            TransactionStatus::Failure => Ok(()),
        }
    }
}

impl Transaction {
    /// Merge a child transaction into a parent transaction.
    ///
    /// # Errors
    ///
    /// * `Error::EAGAIN` if the parent transaction has changed since the child
    ///    transaction was created.
    #[doc(hidden)]
    fn merge(self: &mut Transaction, child: &Transaction) -> Result<()> {
        if self.current_gen != child.parent_gen {
            return Err(Error::EAGAIN(format!("parent transaction changed since creation")));
        }

        self.store = child.store.clone();
        self.current_gen += Wrapping(1);
        Ok(())
    }

    /// Check whether this is the root transaction.
    #[doc(hidden)]
    fn is_root_transaction(self: &Transaction) -> bool {
        self.tx_id == ROOT_TRANSACTION
    }

    /// Write a `Value` at `Path` inside of the current transaction.
    pub fn write(self: &mut Transaction,
                 dom_id: wire::DomainId,
                 path: Path,
                 value: Value)
                 -> Result<()> {
        let node = Node {
            value: value,
            children: HashSet::new(),
            permissions: Permissions::new(),
        };

        let _ = self.store.insert(path.clone(), node);
        self.current_gen += Wrapping(1);

        // Ensure that the parent paths exist when creating a new path
        match path.parent() {
            Some(parent) => {
                match self.read(dom_id, &parent) {
                    // If the parent path did not exist, write the empty string for its value
                    Err(Error::ENOENT(_)) => {
                        try!(self.write(dom_id, parent.clone(), Value::from("")))
                    }
                    Err(e) => return Err(e),
                    Ok(_) => (),
                }

                // Once the parent exists, ensure the current node is a child of that parent
                match path.basename() {
                    Some(basename) => {
                        try!(self.store
                            .get_mut(&parent)
                            .ok_or(Error::ENOENT(format!("failed to find {:?}", path)))
                            .map(|node| node.children.insert(basename)));
                    }
                    None => (),
                }
            }
            None => (),
        }

        Ok(())
    }

    /// Read a `Value` from `Path` inside of the current transaction.
    ///
    /// # Errors
    ///
    /// * `Error::ENOENT` when the path does not exist in the transaction.
    pub fn read(self: &Transaction, dom_id: wire::DomainId, path: &Path) -> Result<Value> {
        self.store
            .get(path)
            .ok_or(Error::ENOENT(format!("failed to find {:?}", path)))
            .map(|node| node.value.clone())
    }

    /// Make a new directory `Path` inside of the current transaction.
    pub fn mkdir(self: &mut Transaction, dom_id: wire::DomainId, path: Path) -> Result<()> {
        self.write(dom_id, path, Value::from(""))
    }

    /// Get a list of directories at `Path` inside the current transaction.
    ///
    /// # Errors
    ///
    /// * `Error::ENOENT` when the path does not exist in the transaction.
    pub fn subdirs(self: &mut Transaction,
                   dom_id: wire::DomainId,
                   path: &Path)
                   -> Result<Vec<Basename>> {
        self.store
            .get(path)
            .ok_or(Error::ENOENT(format!("failed to find {:?}", path)))
            .map(|node| {
                let mut subdirs = node.children
                    .iter()
                    .map(|s| s.to_owned())
                    .collect::<Vec<Basename>>();
                subdirs.sort();
                subdirs
            })
    }

    /// Remove an entry and its children from `Path` inside the current transaction.
    ///
    /// # Errors
    ///
    /// * `Error::ENOENT` when the path does not exist in the transaction.
    pub fn rm(self: &mut Transaction, dom_id: wire::DomainId, path: &Path) -> Result<()> {
        let children = try!(self.store
            .get(path)
            .ok_or(Error::ENOENT(format!("failed to find {:?}", path)))
            .map(|node| {
                node.children
                    .iter()
                    .map(|s| s.to_owned())
                    .collect::<Vec<Basename>>()
            }));
        for child in children {
            let path = path.push(&child);
            try!(self.rm(dom_id, &path));
        }

        let _ = self.store.remove(path);
        self.current_gen += Wrapping(1);
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use rand::{Rng, thread_rng};
    use std::boxed::Box;
    use std::num::Wrapping;
    use super::super::error::Error;
    use super::super::path::Path;
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

        let mut txns = TransactionList::new(Box::new(TestRng { next: Wrapping(0) }));
        assert_eq!(txns.start(), 1);

        let mut txns = TransactionList::new(Box::new(TestRng { next: Wrapping(u32::max_value()) }));
        assert_eq!(txns.start(), u32::max_value());
        assert_eq!(txns.start(), 1);
    }

    #[test]
    fn write_basic_key() {
        let txns = TransactionList::new(Box::new(thread_rng()));

        let mutex = txns.get(ROOT_TRANSACTION).unwrap();
        let guard = mutex.lock().unwrap();
        let mut trans = guard.borrow_mut();

        trans.write(DOM0_DOMAIN_ID,
                   Path::from(DOM0_DOMAIN_ID, "/basic"),
                   Value::from("value"))
            .unwrap()
    }

    #[test]
    fn read_basic_key() {
        let txns = TransactionList::new(Box::new(thread_rng()));

        let mutex = txns.get(ROOT_TRANSACTION).unwrap();
        let guard = mutex.lock().unwrap();
        let mut trans = guard.borrow_mut();

        let path = Path::from(DOM0_DOMAIN_ID, "/basic");
        let value = Value::from("value");

        trans.write(DOM0_DOMAIN_ID, path.clone(), value.clone())
            .unwrap();

        let read = trans.read(DOM0_DOMAIN_ID, &path)
            .unwrap();

        assert_eq!(read, value);
    }

    #[test]
    fn write_all_parents() {
        let txns = TransactionList::new(Box::new(thread_rng()));

        let mutex = txns.get(ROOT_TRANSACTION).unwrap();
        let guard = mutex.lock().unwrap();
        let mut trans = guard.borrow_mut();

        let path = Path::from(DOM0_DOMAIN_ID, "/basic/path");
        let parent = Path::from(DOM0_DOMAIN_ID, "/basic");
        let value = Value::from("value");

        trans.write(DOM0_DOMAIN_ID, path.clone(), value.clone())
            .unwrap();

        let read = trans.read(DOM0_DOMAIN_ID, &path)
            .unwrap();

        assert_eq!(read, value);

        let read_parent = trans.read(DOM0_DOMAIN_ID, &parent)
            .unwrap();

        assert_eq!(read_parent, "");
    }

    #[test]
    fn write_parent_exists() {
        let txns = TransactionList::new(Box::new(thread_rng()));

        let mutex = txns.get(ROOT_TRANSACTION).unwrap();
        let guard = mutex.lock().unwrap();
        let mut trans = guard.borrow_mut();

        let path = Path::from(DOM0_DOMAIN_ID, "/basic/path");
        let parent = Path::from(DOM0_DOMAIN_ID, "/basic");
        let value = Value::from("value");
        let parent_value = Value::from("parent");

        trans.write(DOM0_DOMAIN_ID, parent.clone(), parent_value.clone())
            .unwrap();

        trans.write(DOM0_DOMAIN_ID, path.clone(), value.clone())
            .unwrap();

        let read = trans.read(DOM0_DOMAIN_ID, &path)
            .unwrap();

        assert_eq!(read, value);

        let read_parent = trans.read(DOM0_DOMAIN_ID, &parent)
            .unwrap();

        assert_eq!(read_parent, parent_value);
    }

    #[test]
    fn check_transaction_sees_original_state() {
        let mut txns = TransactionList::new(Box::new(thread_rng()));

        let path = Path::from(DOM0_DOMAIN_ID, "/basic/path");
        let value = Value::from("value");

        // Create the global state
        {
            let mutex = txns.get(ROOT_TRANSACTION).unwrap();
            let guard = mutex.lock().unwrap();
            let mut global = guard.borrow_mut();

            global.write(DOM0_DOMAIN_ID, path.clone(), value.clone())
                .unwrap();
        }

        // Clone a new transaction
        let tx_id = txns.start();
        {
            let mutex = txns.get(tx_id).unwrap();
            let guard = mutex.lock().unwrap();
            let trans = guard.borrow();
            // And verify its state
            let read = trans.read(DOM0_DOMAIN_ID, &path)
                .unwrap();
            assert_eq!(read, value);
        }
    }

    #[test]
    fn check_transaction_no_external_writes() {
        let mut txns = TransactionList::new(Box::new(thread_rng()));

        let path = Path::from(DOM0_DOMAIN_ID, "/basic/path");
        let value = Value::from("value");

        // Create the global state
        {
            let mutex = txns.get(ROOT_TRANSACTION).unwrap();
            let guard = mutex.lock().unwrap();
            let mut global = guard.borrow_mut();

            global.write(DOM0_DOMAIN_ID, path.clone(), value.clone())
                .unwrap();
        }

        // Clone a new transaction
        let tx_id = txns.start();
        {
            let mutex = txns.get(tx_id).unwrap();
            let guard = mutex.lock().unwrap();
            let mut trans = guard.borrow_mut();
            // And verify its state
            let read = trans.read(DOM0_DOMAIN_ID, &path)
                .unwrap();
            assert_eq!(read, value);

            // Write a new value for our path
            let new_value = Value::from("value2");
            trans.write(DOM0_DOMAIN_ID, path.clone(), new_value.clone())
                .unwrap();
            // And verify the read
            let read = trans.read(DOM0_DOMAIN_ID, &path)
                .unwrap();
            assert_eq!(read, new_value);

            // verify the global state is unchanged
            {
                let mutex = txns.get(ROOT_TRANSACTION).unwrap();
                let guard = mutex.lock().unwrap();
                let global = guard.borrow_mut();
                let read = global.read(DOM0_DOMAIN_ID, &path)
                    .unwrap();
                assert_eq!(read, value);
            }

            // Close out the transaction
            txns.end(&trans, TransactionStatus::Success).unwrap();

            // verify the global state has the new value
            {
                let mutex = txns.get(ROOT_TRANSACTION).unwrap();
                let guard = mutex.lock().unwrap();
                let global = guard.borrow_mut();
                let read = global.read(DOM0_DOMAIN_ID, &path)
                    .unwrap();
                assert_eq!(read, new_value);
            }
        }
    }

    #[test]
    fn check_transaction_fails_no_external_writes() {
        let mut txns = TransactionList::new(Box::new(thread_rng()));

        let path = Path::from(DOM0_DOMAIN_ID, "/basic/path");
        let value = Value::from("value");

        // Create the global state
        {
            let mutex = txns.get(ROOT_TRANSACTION).unwrap();
            let guard = mutex.lock().unwrap();
            let mut global = guard.borrow_mut();

            global.write(DOM0_DOMAIN_ID, path.clone(), value.clone())
                .unwrap();
        }

        // Clone a new transaction
        let tx_id = txns.start();
        {
            let mutex = txns.get(tx_id).unwrap();
            let guard = mutex.lock().unwrap();
            let mut trans = guard.borrow_mut();
            // And verify its state
            let read = trans.read(DOM0_DOMAIN_ID, &path)
                .unwrap();
            assert_eq!(read, value);

            // Write a new value for our path
            let new_value = Value::from("value2");
            trans.write(DOM0_DOMAIN_ID, path.clone(), new_value.clone())
                .unwrap();
            // And verify the read
            let read = trans.read(DOM0_DOMAIN_ID, &path)
                .unwrap();
            assert_eq!(read, new_value);

            // verify the global state is unchanged
            {
                let mutex = txns.get(ROOT_TRANSACTION).unwrap();
                let guard = mutex.lock().unwrap();
                let global = guard.borrow_mut();
                let read = global.read(DOM0_DOMAIN_ID, &path)
                    .unwrap();
                assert_eq!(read, value);
            }

            // Close out the transaction
            txns.end(&trans, TransactionStatus::Failure).unwrap();

            // verify the global state has the original value
            {
                let mutex = txns.get(ROOT_TRANSACTION).unwrap();
                let guard = mutex.lock().unwrap();
                let global = guard.borrow_mut();
                let read = global.read(DOM0_DOMAIN_ID, &path)
                    .unwrap();
                assert_eq!(read, value);
            }
        }
    }

    #[test]
    fn check_transaction_with_external_writes() {
        let mut txns = TransactionList::new(Box::new(thread_rng()));

        let path = Path::from(DOM0_DOMAIN_ID, "/basic/path");
        let value = Value::from("value");
        let global_value = Value::from("global_value");

        // Create the global state
        {
            let mutex = txns.get(ROOT_TRANSACTION).unwrap();
            let guard = mutex.lock().unwrap();
            let mut global = guard.borrow_mut();

            global.write(DOM0_DOMAIN_ID, path.clone(), value.clone())
                .unwrap();
        }

        // Clone a new transaction
        let tx_id = txns.start();
        {
            let mutex = txns.get(tx_id).unwrap();
            let guard = mutex.lock().unwrap();
            let mut trans = guard.borrow_mut();
            // And verify its state
            let read = trans.read(DOM0_DOMAIN_ID, &path)
                .unwrap();
            assert_eq!(read, value);

            // Write a new value for our path
            let new_value = Value::from("value2");
            trans.write(DOM0_DOMAIN_ID, path.clone(), new_value.clone())
                .unwrap();
            // And verify the read
            let read = trans.read(DOM0_DOMAIN_ID, &path)
                .unwrap();
            assert_eq!(read, new_value);

            // Write a new value for the global state
            {
                let mutex = txns.get(ROOT_TRANSACTION).unwrap();
                let guard = mutex.lock().unwrap();
                let mut global = guard.borrow_mut();

                global.write(DOM0_DOMAIN_ID, path.clone(), global_value.clone())
                    .unwrap();
            }

            // Close out the transaction
            let ok = txns.end(&trans, TransactionStatus::Success);
            assert_eq!(ok.is_err(), true);

            // verify the global state was not modified
            {
                let mutex = txns.get(ROOT_TRANSACTION).unwrap();
                let guard = mutex.lock().unwrap();
                let global = guard.borrow_mut();

                let read = global.read(DOM0_DOMAIN_ID, &path)
                    .unwrap();
                assert_eq!(read, global_value);
            }
        }
    }

    #[test]
    fn check_transaction_with_external_removes() {
        let mut txns = TransactionList::new(Box::new(thread_rng()));

        let path = Path::from(DOM0_DOMAIN_ID, "/basic/path");
        let value = Value::from("value");

        // Create the global state
        {
            let mutex = txns.get(ROOT_TRANSACTION).unwrap();
            let guard = mutex.lock().unwrap();
            let mut global = guard.borrow_mut();

            global.write(DOM0_DOMAIN_ID, path.clone(), value.clone())
                .unwrap();
        }

        // Clone a new transaction
        let tx_id = txns.start();
        {
            let mutex = txns.get(tx_id).unwrap();
            let guard = mutex.lock().unwrap();
            let mut trans = guard.borrow_mut();
            // And verify its state
            let read = trans.read(DOM0_DOMAIN_ID, &path)
                .unwrap();
            assert_eq!(read, value);

            // Write a new value for our path
            let new_value = Value::from("value2");
            trans.write(DOM0_DOMAIN_ID, path.clone(), new_value.clone())
                .unwrap();
            // And verify the read
            let read = trans.read(DOM0_DOMAIN_ID, &path)
                .unwrap();
            assert_eq!(read, new_value);

            // Write a new value for the global state
            {
                let mutex = txns.get(ROOT_TRANSACTION).unwrap();
                let guard = mutex.lock().unwrap();
                let mut global = guard.borrow_mut();

                global.rm(DOM0_DOMAIN_ID, &path)
                    .unwrap();
            }

            // Close out the transaction
            let ok = txns.end(&trans, TransactionStatus::Success);
            assert_eq!(ok.is_err(), true);
        }
    }

    #[test]
    fn mkdir_creates_empty_directories() {
        let txns = TransactionList::new(Box::new(thread_rng()));

        // Create the global state
        let mutex = txns.get(ROOT_TRANSACTION).unwrap();
        let guard = mutex.lock().unwrap();
        let mut global = guard.borrow_mut();

        let path = Path::from(DOM0_DOMAIN_ID, "/basic/path");
        let parent = path.parent()
            .unwrap();

        global.mkdir(DOM0_DOMAIN_ID, path.clone())
            .unwrap();

        // verify the parent directory was created
        let read = global.read(DOM0_DOMAIN_ID, &parent)
            .unwrap();
        assert_eq!(read, "");

        // verify the path was created
        let read = global.read(DOM0_DOMAIN_ID, &path)
            .unwrap();
        assert_eq!(read, "");
    }

    #[test]
    fn mkdir_creates_root() {
        let txns = TransactionList::new(Box::new(thread_rng()));

        // Create the global state
        let mutex = txns.get(ROOT_TRANSACTION).unwrap();
        let guard = mutex.lock().unwrap();
        let mut global = guard.borrow_mut();

        let path = Path::from(DOM0_DOMAIN_ID, "/basic/path");

        global.mkdir(DOM0_DOMAIN_ID, path.clone())
            .unwrap();

        // verify the parent directory was created
        let read = global.read(DOM0_DOMAIN_ID, &Path::from(DOM0_DOMAIN_ID, "/"))
            .unwrap();
        assert_eq!(read, "");
    }

    #[test]
    fn subdirs_gets_subdirectories() {
        let txns = TransactionList::new(Box::new(thread_rng()));

        // Create the global state
        let mutex = txns.get(ROOT_TRANSACTION).unwrap();
        let guard = mutex.lock().unwrap();
        let mut global = guard.borrow_mut();

        let path1 = Path::from(DOM0_DOMAIN_ID, "/basic/path1");
        let path2 = Path::from(DOM0_DOMAIN_ID, "/basic/path2");
        let basic = path1.parent()
            .unwrap();

        global.mkdir(DOM0_DOMAIN_ID, path1.clone())
            .unwrap();
        global.mkdir(DOM0_DOMAIN_ID, path2.clone())
            .unwrap();

        // verify the parent directory was created
        let read = global.read(DOM0_DOMAIN_ID, &basic)
            .unwrap();
        assert_eq!(read, "");

        // grab a list of all subdirectories
        let subs = global.subdirs(DOM0_DOMAIN_ID, &basic)
            .unwrap();
        assert_eq!(subs, vec![Basename::from("path1"), Basename::from("path2")]);
    }

    #[test]
    fn rm_deletes_all_directories() {
        let txns = TransactionList::new(Box::new(thread_rng()));

        // Create the global state
        let mutex = txns.get(ROOT_TRANSACTION).unwrap();
        let guard = mutex.lock().unwrap();
        let mut global = guard.borrow_mut();
        let path1 = Path::from(DOM0_DOMAIN_ID, "/basic/path1");
        let path2 = Path::from(DOM0_DOMAIN_ID, "/basic/path2");
        let basic = path1.parent()
            .unwrap();
        global.mkdir(DOM0_DOMAIN_ID, path1.clone())
            .unwrap();
        global.mkdir(DOM0_DOMAIN_ID, path2.clone())
            .unwrap();

        global.rm(DOM0_DOMAIN_ID, &basic)
            .unwrap();

        // verify the parent directory was removed
        match global.read(DOM0_DOMAIN_ID, &basic) {
            Err(Error::ENOENT(_)) => assert!(true),
            Err(ref e) => assert!(false, format!("unexpected error returned {:?}", e)),
            Ok(_) => assert!(false, format!("failed to remove {:?}", basic)),
        }

        // verify the path1 directory was removed
        match global.read(DOM0_DOMAIN_ID, &path1) {
            Err(Error::ENOENT(_)) => assert!(true),
            Err(ref e) => assert!(false, format!("unexpected error returned {:?}", e)),
            Ok(_) => assert!(false, format!("failed to remove {:?}", basic)),
        }

        // verify the path2 directory was removed
        match global.read(DOM0_DOMAIN_ID, &path2) {
            Err(Error::ENOENT(_)) => assert!(true),
            Err(ref e) => assert!(false, format!("unexpected error returned {:?}", e)),
            Ok(_) => assert!(false, format!("failed to remove {:?}", basic)),
        }

        // verify the root still exists
        let read = global.read(DOM0_DOMAIN_ID, &Path::from(DOM0_DOMAIN_ID, "/"))
            .unwrap();
        assert_eq!(read, "");
    }
}
