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
use std::collections::{LinkedList, HashMap, HashSet};
use std::num::Wrapping;
use std::sync::{Mutex, Arc};
use super::wire;
use super::path::Path;

bitflags! {
    pub flags Perm: u32 {
        const PERM_NONE  = 0x00000000,
        const PERM_READ  = 0x00000001,
        const PERM_WRITE = 0x00000002,
        const PERM_OWNER = 0x00000004,
    }
}

/// The Root Transaction Id.
pub const ROOT_TRANSACTION: wire::TxId = 0;

/// The Dom0 Domain Id.
pub const DOM0_DOMAIN_ID: wire::DomainId = 0;

pub type Basename = String;
pub type Value = String;

#[derive(PartialEq, Clone, Debug)]
pub struct Permission {
    pub id: wire::DomainId,
    pub perm: Perm,
}

#[derive(Clone, Debug)]
struct Node {
    pub path: Path,
    pub value: Value,
    pub children: HashSet<Basename>,
    pub permissions: Vec<Permission>,
}

impl Node {
    pub fn perms_ok(&self, dom_id: wire::DomainId, perm: Perm) -> bool {
        let mask = PERM_READ | PERM_WRITE | PERM_OWNER;

        if dom_id == DOM0_DOMAIN_ID || self.permissions[0].id == dom_id {
            return (mask & perm) == perm;
        }

        for p in self.permissions.iter() {
            if p.id == dom_id {
                return (p.perm & perm) == perm;
            }
        }

        return self.permissions[0].perm & perm == perm;
    }
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

    store.insert(name.clone(),
                 Node {
                     path: name,
                     value: Value::from(""),
                     children: children,
                     permissions: vec![Permission {
                                           id: DOM0_DOMAIN_ID,
                                           perm: PERM_NONE,
                                       }],
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

    /// Get a reference to a node
    #[doc(hidden)]
    fn get_node<'a>(self: &'a Transaction,
                    dom_id: wire::DomainId,
                    path: &Path,
                    perm: Perm)
                    -> Result<&'a Node> {
        self.store
            .get(path)
            .ok_or(Error::ENOENT(format!("failed to find {:?}", path)))
            .and_then(|node| {
                if !node.perms_ok(dom_id, perm) {
                    Err(Error::EACCES(format!("failed to verify permissions for {:?}", node.path)))
                } else {
                    Ok(node)
                }
            })
    }

    /// Write a node into the data store
    #[doc(hidden)]
    fn write_node(self: &mut Transaction, dom_id: wire::DomainId, node: Node) -> Result<()> {
        self.store.insert(node.path.clone(), node);
        self.current_gen += Wrapping(1);
        Ok(())
    }

    /// Construct a new node
    #[doc(hidden)]
    fn construct_node(self: &Transaction,
                      dom_id: wire::DomainId,
                      path: Path,
                      value: Value)
                      -> Result<LinkedList<Node>> {
        // Get a list of all of the parent nodes that need to be modified/created
        let parent_path = path.parent().unwrap();
        let mut parent_list = match self.get_node(dom_id, &parent_path, PERM_WRITE) {
            Ok(parent) => {
                let mut lst = LinkedList::new();
                lst.push_back(parent.clone());
                lst
            }
            Err(Error::ENOENT(_)) => {
                let lst = try!(self.construct_node(dom_id, parent_path, Value::from("")));
                lst
            }
            Err(err) => return Err(err),
        };

        let node = {
            // Grab the immediate parent, since we need to insert this as a child
            let mut parent = parent_list.front_mut().unwrap();
            if let Some(basename) = path.basename() {
                parent.children.insert(basename);
            }

            // Clone the immediate parent node's permissions
            let mut permissions = parent.permissions.clone();
            if dom_id != DOM0_DOMAIN_ID {
                // except for the unprivileged domains, which own what
                // it creates
                permissions[0].id = dom_id;
            }

            // Create the node
            Node {
                path: path.clone(),
                value: value,
                children: HashSet::new(),
                permissions: permissions,
            }
        };

        // And return that as a list
        let mut list = LinkedList::new();
        list.push_front(node);
        list.append(&mut parent_list);
        Ok(list)
    }

    /// Create a new node
    #[doc(hidden)]
    fn create_node(self: &mut Transaction,
                   dom_id: wire::DomainId,
                   path: Path,
                   value: Value)
                   -> Result<()> {
        let nodes = try!(self.construct_node(dom_id, path, value));

        for node in nodes.iter() {
            try!(self.write_node(dom_id, node.clone()));
        }
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

        let node = {
            self.get_node(dom_id, &path, PERM_WRITE).map(|n| n.clone())
        };

        if let Ok(mut node) = node {
            node.value = value;
            self.write_node(dom_id, node)
        } else {
            self.create_node(dom_id, path, value)
        }
    }

    /// Read a `Value` from `Path` inside of the current transaction.
    ///
    /// # Errors
    ///
    /// * `Error::ENOENT` when the path does not exist in the transaction.
    pub fn read(self: &Transaction, dom_id: wire::DomainId, path: &Path) -> Result<Value> {
        self.get_node(dom_id, path, PERM_READ)
            .map(|node| node.value.clone())
    }

    /// Make a new directory `Path` inside of the current transaction.
    pub fn mkdir(self: &mut Transaction, dom_id: wire::DomainId, path: Path) -> Result<()> {
        match self.get_node(dom_id, &path, PERM_WRITE) {
            Err(Error::ENOENT(_)) => self.create_node(dom_id, path, Value::from("")),
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        }
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
        self.get_node(dom_id, path, PERM_READ)
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
        if path == &Path::from(DOM0_DOMAIN_ID, "/") {
            return Err(Error::EINVAL(format!("cannot remove root directory")));
        }

        let basename = path.basename().unwrap();
        let parent = path.parent().unwrap();

        // need to remove entry from the parent first
        let parent_node = try!(self.get_node(dom_id, &parent, PERM_WRITE)
            .map(|node| {
                let mut children = node.children.clone();
                children.remove(&basename);
                Node { children: children, ..node.clone() }
            }));
        try!(self.write_node(dom_id, parent_node));

        // Grab a list of all of the children
        let children = try!(self.get_node(dom_id, path, PERM_WRITE)
            .map(|node| {
                node.children
                    .iter()
                    .map(|s| s.to_owned())
                    .collect::<Vec<Basename>>()
            }));

        // And recursively remove all of its children
        for child in children {
            let path = path.push(&child);
            try!(self.rm(dom_id, &path));
        }

        // Then remove the child node
        let _ = self.store.remove(path);

        self.current_gen += Wrapping(1);
        Ok(())
    }

    /// Get the permissions for a node.
    ///
    /// # Errors
    ///
    /// * `Error::ENOENT` when the path does not exist in the transaction.
    pub fn get_perms(self: &Transaction,
                     dom_id: wire::DomainId,
                     path: &Path)
                     -> Result<Vec<Permission>> {
        self.get_node(dom_id, path, PERM_READ)
            .map(|node| node.permissions.clone())
    }

    /// Set the permissions for a node.
    ///
    /// # Errors
    ///
    /// * `Error::ENOENT` when the path does not exist in the transaction.
    pub fn set_perms(self: &mut Transaction,
                     dom_id: wire::DomainId,
                     path: &Path,
                     permissions: Vec<Permission>)
                     -> Result<()> {
        let node = {
            try!(self.get_node(dom_id, path, PERM_WRITE)
                .map(|node| node.clone()))
        };

        let new_node = Node { permissions: permissions, ..node };

        self.write_node(dom_id, new_node)
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

    #[test]
    fn rm_cannot_delete_root() {
        let txns = TransactionList::new(Box::new(thread_rng()));

        // Create the global state
        let mutex = txns.get(ROOT_TRANSACTION).unwrap();
        let guard = mutex.lock().unwrap();
        let mut global = guard.borrow_mut();
        let path1 = Path::from(DOM0_DOMAIN_ID, "/basic/path1");

        global.mkdir(DOM0_DOMAIN_ID, path1.clone())
            .unwrap();

        let rslt = global.rm(DOM0_DOMAIN_ID, &Path::from(DOM0_DOMAIN_ID, "/"));

        match rslt {
            Ok(_) => assert!(false, "removed the root directory"),
            Err(Error::EINVAL(_)) => assert!(true, "it is invalid to try and remove /"),
            Err(_) => assert!(false, "unknown error"),
        }

        // verify the root still exists
        let read = global.read(DOM0_DOMAIN_ID, &Path::from(DOM0_DOMAIN_ID, "/"))
            .unwrap();
        assert_eq!(read, "");
    }

    #[test]
    fn rm_removes_from_parent() {
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

        global.rm(DOM0_DOMAIN_ID, &path1)
            .unwrap();

        // verify the path1 directory was removed
        match global.read(DOM0_DOMAIN_ID, &path1) {
            Err(Error::ENOENT(_)) => assert!(true),
            Err(ref e) => assert!(false, format!("unexpected error returned {:?}", e)),
            Ok(_) => assert!(false, format!("failed to remove {:?}", basic)),
        }

        let subdirs = global.subdirs(DOM0_DOMAIN_ID, &basic).unwrap();
        assert_eq!(subdirs, vec![String::from("path2")]);
    }

    #[test]
    fn get_root_permissions() {
        let txns = TransactionList::new(Box::new(thread_rng()));

        let mutex = txns.get(ROOT_TRANSACTION).unwrap();
        let guard = mutex.lock().unwrap();
        let trans = guard.borrow_mut();

        let permissions = trans.get_perms(DOM0_DOMAIN_ID, &Path::from(DOM0_DOMAIN_ID, "/"))
            .unwrap();

        assert_eq!(permissions,
                   vec![Permission {
                            id: DOM0_DOMAIN_ID,
                            perm: PERM_NONE,
                        }]);
    }

    #[test]
    fn get_local_permissions() {
        let txns = TransactionList::new(Box::new(thread_rng()));

        let mutex = txns.get(ROOT_TRANSACTION).unwrap();
        let guard = mutex.lock().unwrap();
        let mut trans = guard.borrow_mut();

        trans.mkdir(DOM0_DOMAIN_ID,
                   Path::from(DOM0_DOMAIN_ID, "/local/domain/1"))
            .unwrap();

        trans.set_perms(DOM0_DOMAIN_ID,
                       &Path::from(DOM0_DOMAIN_ID, "/local/domain/1"),
                       vec![Permission {
                                id: 1,
                                perm: PERM_NONE,
                            }])
            .unwrap();

        let path = Path::from(1, "foo");
        let value = Value::from("value");
        trans.write(1, path.clone(), value.clone())
            .unwrap();
    }

    #[test]
    fn permissions_idempotent() {
        let txns = TransactionList::new(Box::new(thread_rng()));

        let mutex = txns.get(ROOT_TRANSACTION).unwrap();
        let guard = mutex.lock().unwrap();
        let mut trans = guard.borrow_mut();

        let path = Path::from(DOM0_DOMAIN_ID, "/local/domain/1");

        trans.mkdir(DOM0_DOMAIN_ID, path.clone())
            .unwrap();

        let perms = vec![
            Permission {
                id: 1,
                perm: PERM_NONE,
            },
            Permission {
                id: 2,
                perm: PERM_READ,
            },
        ];

        trans.set_perms(DOM0_DOMAIN_ID, &path, perms.clone())
            .unwrap();

        let read = trans.get_perms(DOM0_DOMAIN_ID, &path)
            .unwrap();

        assert_eq!(perms, read);
    }

    #[test]
    fn permissions_inherit() {
        let txns = TransactionList::new(Box::new(thread_rng()));

        let mutex = txns.get(ROOT_TRANSACTION).unwrap();
        let guard = mutex.lock().unwrap();
        let mut trans = guard.borrow_mut();

        let path = Path::from(DOM0_DOMAIN_ID, "/local/domain/1");

        trans.mkdir(DOM0_DOMAIN_ID, path.clone())
            .unwrap();

        let perms = vec![
            Permission {
                id: 1,
                perm: PERM_NONE,
            },
            Permission {
                id: 2,
                perm: PERM_READ,
            },
        ];

        trans.set_perms(DOM0_DOMAIN_ID, &path, perms.clone())
            .unwrap();

        let path = path.push("foo");
        trans.write(1, path.clone(), Value::from("bar"))
            .unwrap();

        let read = trans.get_perms(1, &path)
            .unwrap();

        assert_eq!(perms, read);
    }

    #[test]
    fn permissions_inherit_no_overwrite_owner() {
        let txns = TransactionList::new(Box::new(thread_rng()));

        let mutex = txns.get(ROOT_TRANSACTION).unwrap();
        let guard = mutex.lock().unwrap();
        let mut trans = guard.borrow_mut();

        let path = Path::from(DOM0_DOMAIN_ID, "/local/domain/1");

        trans.mkdir(DOM0_DOMAIN_ID, path.clone())
            .unwrap();

        let perms = vec![
            Permission {
                id: 1,
                perm: PERM_NONE,
            },
            Permission {
                id: 2,
                perm: PERM_READ,
            },
        ];

        trans.set_perms(DOM0_DOMAIN_ID, &path, perms.clone())
            .unwrap();

        let path = path.push("foo");
        trans.write(DOM0_DOMAIN_ID, path.clone(), Value::from("bar"))
            .unwrap();

        let read = trans.get_perms(1, &path)
            .unwrap();

        assert_eq!(perms, read);
    }

    #[test]
    fn block_cross_domain_reads() {
        let txns = TransactionList::new(Box::new(thread_rng()));

        let mutex = txns.get(ROOT_TRANSACTION).unwrap();
        let guard = mutex.lock().unwrap();
        let mut trans = guard.borrow_mut();

        trans.mkdir(DOM0_DOMAIN_ID,
                   Path::from(DOM0_DOMAIN_ID, "/local/domain/1"))
            .unwrap();

        trans.set_perms(DOM0_DOMAIN_ID,
                       &Path::from(DOM0_DOMAIN_ID, "/local/domain/1"),
                       vec![Permission {
                                id: 1,
                                perm: PERM_NONE,
                            }])
            .unwrap();

        let path = Path::from(1, "foo");
        let value = Value::from("value");
        trans.write(1, path.clone(), value.clone())
            .unwrap();

        // Check the domain 2 is blocked
        let v = trans.read(2, &path);
        match v {
            Ok(_) => assert!(false, "allowed cross-domain read"),
            Err(Error::EACCES(..)) => assert!(true, "blocked cross-domain read"),
            Err(_) => assert!(false, "unknown error"),
        }

        // Check the Dom0 is still allowed
        let read = trans.read(DOM0_DOMAIN_ID, &path).unwrap();
        assert_eq!(read, value);
    }

    #[test]
    fn block_cross_domain_writes() {
        let txns = TransactionList::new(Box::new(thread_rng()));

        let mutex = txns.get(ROOT_TRANSACTION).unwrap();
        let guard = mutex.lock().unwrap();
        let mut trans = guard.borrow_mut();

        trans.mkdir(DOM0_DOMAIN_ID,
                   Path::from(DOM0_DOMAIN_ID, "/local/domain/1"))
            .unwrap();

        trans.set_perms(DOM0_DOMAIN_ID,
                       &Path::from(DOM0_DOMAIN_ID, "/local/domain/1"),
                       vec![Permission {
                                id: 1,
                                perm: PERM_NONE,
                            }])
            .unwrap();

        let path = Path::from(1, "foo");
        let value = Value::from("value");
        trans.write(1, path.clone(), value.clone())
            .unwrap();

        // Check the domain 2 is blocked
        let v = trans.write(2, path.clone(), Value::from("new value"));
        match v {
            Ok(_) => assert!(false, "allowed cross-domain write"),
            Err(Error::EACCES(..)) => assert!(true, "blocked cross-domain write"),
            Err(_) => assert!(false, "unknown error"),
        }

        let read = trans.read(1, &path).unwrap();
        assert_eq!(read, value.clone());

        // Check the Dom0 is still allowed
        trans.write(DOM0_DOMAIN_ID, path.clone(), Value::from("new value")).unwrap();

        let read = trans.read(1, &path).unwrap();
        assert_eq!(read, Value::from("new value"));
    }

    #[test]
    fn block_cross_domain_rm() {
        let txns = TransactionList::new(Box::new(thread_rng()));

        let mutex = txns.get(ROOT_TRANSACTION).unwrap();
        let guard = mutex.lock().unwrap();
        let mut trans = guard.borrow_mut();

        trans.mkdir(DOM0_DOMAIN_ID,
                   Path::from(DOM0_DOMAIN_ID, "/local/domain/1"))
            .unwrap();

        trans.set_perms(DOM0_DOMAIN_ID,
                       &Path::from(DOM0_DOMAIN_ID, "/local/domain/1"),
                       vec![Permission {
                                id: 1,
                                perm: PERM_NONE,
                            }])
            .unwrap();

        let path = Path::from(1, "foo");
        let value = Value::from("value");
        trans.write(1, path.clone(), value.clone())
            .unwrap();

        // Check the domain 2 is blocked
        let v = trans.rm(2, &path);
        match v {
            Ok(_) => assert!(false, "allowed cross-domain rm"),
            Err(Error::EACCES(..)) => assert!(true, "blocked cross-domain rm"),
            Err(_) => assert!(false, "unknown error"),
        }

        let read = trans.read(1, &path).unwrap();
        assert_eq!(read, value.clone());

        // Check the Dom0 is still allowed
        trans.rm(DOM0_DOMAIN_ID, &path).unwrap();
    }

    #[test]
    fn block_cross_domain_subdir() {
        let txns = TransactionList::new(Box::new(thread_rng()));

        let mutex = txns.get(ROOT_TRANSACTION).unwrap();
        let guard = mutex.lock().unwrap();
        let mut trans = guard.borrow_mut();

        let domain = Path::from(DOM0_DOMAIN_ID, "/local/domain/1");

        trans.mkdir(DOM0_DOMAIN_ID, domain.clone())
            .unwrap();

        trans.set_perms(DOM0_DOMAIN_ID,
                       &domain,
                       vec![Permission {
                                id: 1,
                                perm: PERM_NONE,
                            }])
            .unwrap();

        let path = Path::from(1, "foo");
        let value = Value::from("value");
        trans.write(1, path.clone(), value.clone())
            .unwrap();

        // Check the domain 2 is blocked
        let v = trans.subdirs(2, &domain);
        match v {
            Ok(_) => assert!(false, "allowed cross-domain subdir"),
            Err(Error::EACCES(..)) => assert!(true, "blocked cross-domain subdir"),
            Err(_) => assert!(false, "unknown error"),
        }

        // Check the Dom0 is still allowed
        trans.subdirs(DOM0_DOMAIN_ID, &domain).unwrap();
    }
}
