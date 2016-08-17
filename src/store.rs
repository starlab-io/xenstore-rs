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

use std::collections::{HashMap, HashSet, LinkedList};
use std::num::Wrapping;
use super::error::{Result, Error};
use super::wire;
use super::path::Path;

/// The Dom0 Domain Id.
pub const DOM0_DOMAIN_ID: wire::DomainId = 0;

pub type Basename = String;
pub type Value = String;

bitflags! {
    pub flags Perm: u32 {
        const PERM_NONE  = 0x00000000,
        const PERM_READ  = 0x00000001,
        const PERM_WRITE = 0x00000002,
        const PERM_OWNER = 0x00000004,
    }
}

#[derive(PartialEq, Clone, Debug)]
pub struct Permission {
    pub id: wire::DomainId,
    pub perm: Perm,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub path: Path,
    pub value: Value,
    pub children: HashSet<Basename>,
    pub permissions: Vec<Permission>,
}

fn perms_ok(dom_id: wire::DomainId, permissions: &[Permission], perm: Perm) -> bool {
    let mask = PERM_READ | PERM_WRITE | PERM_OWNER;

    if dom_id == DOM0_DOMAIN_ID || permissions[0].id == dom_id {
        return (mask & perm) == perm;
    }

    if let Some(p) = permissions.iter().find(|p| p.id == dom_id) {
        return (p.perm & perm) == perm;
    }

    return permissions[0].perm & perm == perm;
}

impl Node {
    pub fn perms_ok(&self, dom_id: wire::DomainId, perm: Perm) -> bool {
        perms_ok(dom_id, &self.permissions, perm)
    }
}

pub struct Store {
    generation: Wrapping<u64>,
    store: HashMap<Path, Node>,
}

#[derive(Clone, Debug)]
pub enum Change {
    Write(Node),
    Remove(Node),
}

impl Change {
    pub fn path(&self) -> &Path {
        match *self {
            Change::Write(ref node) => &node.path,
            Change::Remove(ref node) => &node.path,
        }
    }
}

#[derive(Clone)]
pub struct ChangeSet {
    parent: Wrapping<u64>,
    changes: HashMap<Path, Change>,
}

impl ChangeSet {
    pub fn new(from: &Store) -> ChangeSet {
        ChangeSet {
            parent: from.generation,
            changes: HashMap::new(),
        }
    }

    fn insert(&mut self, change: Change) -> Option<Change> {
        self.changes.insert(change.path().clone(), change)
    }
}

#[derive(Debug)]
pub enum AppliedChange {
    Write(Path, Vec<Permission>),
    Remove(Path),
    IntroduceDomain,
    ReleaseDomain,
}

impl AppliedChange {
    pub fn perms_ok(&self, dom_id: wire::DomainId, perm: Perm) -> bool {
        match *self {
            AppliedChange::Write(_, ref permissions) => perms_ok(dom_id, permissions, perm),
            AppliedChange::Remove(_) => true,
            AppliedChange::IntroduceDomain => true,
            AppliedChange::ReleaseDomain => true,
        }
    }
}

/// Insert manual entries into a Store
fn manual_entry(store: &mut HashMap<Path, Node>, name: Path, child_list: Vec<Basename>) {
    let children = child_list.iter().cloned().collect::<HashSet<Basename>>();

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

impl Store {
    pub fn new() -> Store {
        let mut store = HashMap::new();

        manual_entry(&mut store,
                     Path::from(DOM0_DOMAIN_ID, "/"),
                     vec![Basename::from("tool")]);
        manual_entry(&mut store,
                     Path::from(DOM0_DOMAIN_ID, "/tool"),
                     vec![Basename::from("xenstored")]);
        manual_entry(&mut store,
                     Path::from(DOM0_DOMAIN_ID, "/tool/xenstored"),
                     vec![]);
        Store {
            generation: Wrapping(0),
            store: store,
        }
    }

    pub fn apply(&mut self, change_set: ChangeSet) -> Option<Vec<AppliedChange>> {
        if self.generation != change_set.parent {
            return None;
        }

        let mut applied = vec![];

        for (ref path, ref change) in change_set.changes {
            match change {
                &Change::Write(ref node) => {
                    self.store.insert(path.clone(), node.clone());
                    applied.push(AppliedChange::Write(path.clone(), node.permissions.clone()));
                }
                &Change::Remove(_) => {
                    self.store.remove(path);
                    applied.push(AppliedChange::Remove(path.clone()));
                }
            }
        }

        self.generation += Wrapping(1);
        Some(applied)
    }

    fn get_node<'a>(&'a self,
                    change_set: &'a ChangeSet,
                    dom_id: wire::DomainId,
                    path: &Path,
                    perm: Perm)
                    -> Result<&'a Node> {
        let node = {
            if change_set.changes.contains_key(path) {
                match change_set.changes.get(path).unwrap() {
                    &Change::Write(ref node) => Ok(node),
                    &Change::Remove(_) => {
                        Err(Error::ENOENT(format!("failed to lookup {:?}", path)))
                    }
                }
            } else {
                self.store
                    .get(path)
                    .ok_or(Error::ENOENT(format!("failed to lookup {:?}", path)))
            }
        };

        node.and_then(|node| {
            if !node.perms_ok(dom_id, perm) {
                Err(Error::EACCES(format!("failed to verify permissions for {:?}", node.path)))
            } else {
                Ok(node)
            }
        })
    }

    /// Construct a new node
    #[doc(hidden)]
    fn construct_node(&self,
                      change_set: &ChangeSet,
                      dom_id: wire::DomainId,
                      path: Path,
                      value: Value)
                      -> Result<LinkedList<Node>> {

        // Get a list of paths that need to be created
        let paths_to_create = path.clone()
            .into_iter()
            .take_while(|ref path| {
                match self.get_node(change_set, dom_id, path, PERM_WRITE) {
                    Err(Error::ENOENT(_)) => true,
                    _ => false,
                }
            })
            .collect::<LinkedList<Path>>();

        // If we are trying to construct a node and cannot, it is due to access restritions
        if paths_to_create.is_empty() {
            return Err(Error::EACCES(format!("could not create {:?}", path)));
        }

        // Get a copy of the highest parent that does not need to be created
        let parent_path = paths_to_create.back()
            .unwrap()
            .parent()
            .unwrap();
        let mut list = match self.get_node(change_set, dom_id, &parent_path, PERM_WRITE) {
            Ok(parent) => {
                let mut lst = LinkedList::new();
                lst.push_back(parent.clone());
                lst
            }
            Err(Error::ENOENT(_)) => unreachable!(),
            Err(err) => return Err(err),
        };

        // Modify and create all of the nodes necessary
        for path in paths_to_create.iter().rev() {
            // Grab the immediate parent, since we need to insert this as a child
            let node = {
                let mut parent = list.front_mut().unwrap();
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
                    value: Value::from(""),
                    children: HashSet::new(),
                    permissions: permissions,
                }
            };

            list.push_front(node);
        }

        // All of the created nodes had an empty value, so we need
        // to set the real value on the last created node (the one
        // we ultimately set out to create).
        list.front_mut().unwrap().value = value;

        Ok(list)
    }

    /// Write a `Value` at `Path` inside of the current transaction.
    pub fn write(&self,
                 change_set: &ChangeSet,
                 dom_id: wire::DomainId,
                 path: Path,
                 value: Value)
                 -> Result<ChangeSet> {
        let node = {
            self.get_node(change_set, dom_id, &path, PERM_WRITE)
                .map(|n| n.clone())
        };

        let mut changes = change_set.clone();

        match node {
            Ok(mut node) => {
                node.value = value;
                changes.insert(Change::Write(node));
            }
            _ => {
                let nodes = try!(self.construct_node(change_set, dom_id, path, value));

                for node in nodes.iter() {
                    changes.insert(Change::Write(node.clone()));
                }
            }
        }
        Ok(changes)
    }

    /// Read a `Value` from `Path` inside of the current transaction.
    ///
    /// # Errors
    ///
    /// * `Error::ENOENT` when the path does not exist in the transaction.
    pub fn read(&self,
                change_set: &ChangeSet,
                dom_id: wire::DomainId,
                path: &Path)
                -> Result<Value> {
        self.get_node(change_set, dom_id, path, PERM_READ)
            .map(|node| node.value.clone())
    }

    /// Make a new directory `Path` inside of the current transaction.
    pub fn mkdir(&self,
                 change_set: &ChangeSet,
                 dom_id: wire::DomainId,
                 path: Path)
                 -> Result<ChangeSet> {
        let mut changes = change_set.clone();

        match self.get_node(change_set, dom_id, &path, PERM_WRITE) {
            Err(Error::ENOENT(_)) => {
                let nodes = try!(self.construct_node(change_set, dom_id, path, Value::from("")));

                for node in nodes.iter() {
                    changes.insert(Change::Write(node.clone()));
                }

                Ok(changes)
            }
            Ok(_) => Ok(changes),
            Err(e) => Err(e),
        }
    }

    /// Get a list of directories at `Path` inside the current transaction.
    ///
    /// # Errors
    ///
    /// * `Error::ENOENT` when the path does not exist in the transaction.
    pub fn directory(&self,
                     change_set: &ChangeSet,
                     dom_id: wire::DomainId,
                     path: &Path)
                     -> Result<Vec<Basename>> {
        self.get_node(change_set, dom_id, path, PERM_READ)
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
    pub fn rm(&self,
              change_set: &ChangeSet,
              dom_id: wire::DomainId,
              path: &Path)
              -> Result<ChangeSet> {
        if path == &Path::from(DOM0_DOMAIN_ID, "/") {
            return Err(Error::EINVAL(format!("cannot remove root directory")));
        }

        let basename = path.basename().unwrap();
        let parent = path.parent().unwrap();

        let mut changes = change_set.clone();

        // need to remove entry from the parent first
        let parent_node = try!(self.get_node(&changes, dom_id, &parent, PERM_WRITE)
            .map(|node| {
                let mut children = node.children.clone();
                children.remove(&basename);
                Node { children: children, ..node.clone() }
            }));
        changes.insert(Change::Write(parent_node));

        let mut remove = LinkedList::new();
        remove.push_back(path.clone());

        while let Some(path) = remove.pop_front() {
            // Grab a list of all of the children
            let node = {
                try!(self.get_node(change_set, dom_id, &path, PERM_WRITE))
            };

            // And recursively remove all of its children
            for child in node.children.iter() {
                let path = path.push(&child);
                remove.push_back(path);
            }

            // Then remove the child node
            changes.insert(Change::Remove(node.clone()));
        }

        Ok(changes)
    }

    /// Get the permissions for a node.
    ///
    /// # Errors
    ///
    /// * `Error::ENOENT` when the path does not exist in the transaction.
    pub fn get_perms(&self,
                     change_set: &ChangeSet,
                     dom_id: wire::DomainId,
                     path: &Path)
                     -> Result<Vec<Permission>> {
        self.get_node(change_set, dom_id, path, PERM_READ)
            .map(|node| node.permissions.clone())
    }

    /// Set the permissions for a node.
    ///
    /// # Errors
    ///
    /// * `Error::ENOENT` when the path does not exist in the transaction.
    pub fn set_perms(&self,
                     change_set: &ChangeSet,
                     dom_id: wire::DomainId,
                     path: &Path,
                     permissions: Vec<Permission>)
                     -> Result<ChangeSet> {
        let node = {
            try!(self.get_node(change_set, dom_id, path, PERM_WRITE)
                .map(|node| node.clone()))
        };

        let mut changes = change_set.clone();
        changes.insert(Change::Write(Node { permissions: permissions, ..node }));
        Ok(changes)
    }
}

#[cfg(test)]
mod test {
    use std::num::Wrapping;
    use super::super::error::Error;
    use super::super::path::Path;
    use super::*;

    #[test]
    fn basic_write() {
        let store = Store::new();
        let path = Path::from(DOM0_DOMAIN_ID, "/basic");
        let value = Value::from("value");

        let changes = store.write(&ChangeSet::new(&store),
                   DOM0_DOMAIN_ID,
                   path.clone(),
                   value.clone())
            .unwrap();

        assert_eq!(changes.changes.contains_key(&path), true);
        let change = changes.changes.get(&path).unwrap();
        match change {
            &Change::Write(ref node) => assert_eq!(node.value, value),
            _ => panic!(),
        }
    }

    #[test]
    fn basic_read() {
        let store = Store::new();
        let path = Path::from(DOM0_DOMAIN_ID, "/basic");
        let value = Value::from("value");

        let changes = store.write(&ChangeSet::new(&store),
                   DOM0_DOMAIN_ID,
                   path.clone(),
                   value.clone())
            .unwrap();

        let read = store.read(&changes, DOM0_DOMAIN_ID, &path)
            .unwrap();

        assert_eq!(read, value);
    }

    #[test]
    fn basic_applied_write_and_read() {
        let mut store = Store::new();
        let path = Path::from(DOM0_DOMAIN_ID, "/basic");
        let value = Value::from("value");

        let changes = store.write(&ChangeSet::new(&store),
                   DOM0_DOMAIN_ID,
                   path.clone(),
                   value.clone())
            .unwrap();

        store.apply(changes)
            .unwrap();
        assert_eq!(store.generation, Wrapping(1));

        let read = store.read(&ChangeSet::new(&store), DOM0_DOMAIN_ID, &path)
            .unwrap();

        assert_eq!(read, value);
    }

    #[test]
    fn recursive_write() {
        let store = Store::new();
        let path = Path::from(DOM0_DOMAIN_ID, "/basic/path");
        let parent = path.parent().unwrap();
        let value = Value::from("value");

        let changes = store.write(&ChangeSet::new(&store),
                   DOM0_DOMAIN_ID,
                   path.clone(),
                   value.clone())
            .unwrap();

        let read = store.read(&changes, DOM0_DOMAIN_ID, &path)
            .unwrap();

        assert_eq!(read, value);

        let read_parent = store.read(&changes, DOM0_DOMAIN_ID, &parent)
            .unwrap();

        assert_eq!(read_parent, "");
    }

    #[test]
    fn basic_mkdir() {
        let store = Store::new();
        let path = Path::from(DOM0_DOMAIN_ID, "/basic");

        let changes = store.mkdir(&ChangeSet::new(&store), DOM0_DOMAIN_ID, path.clone())
            .unwrap();

        // verify the path was created
        let read = store.read(&changes, DOM0_DOMAIN_ID, &path)
            .unwrap();
        assert_eq!(read, "");
    }

    #[test]
    fn recursive_mkdir() {
        let store = Store::new();
        let path = Path::from(DOM0_DOMAIN_ID, "/basic/path");
        let parent = path.parent().unwrap();

        let changes = store.mkdir(&ChangeSet::new(&store), DOM0_DOMAIN_ID, path.clone())
            .unwrap();

        // verify the parent directory was created
        let read = store.read(&changes, DOM0_DOMAIN_ID, &parent)
            .unwrap();
        assert_eq!(read, "");

        // verify the path was created
        let read = store.read(&changes, DOM0_DOMAIN_ID, &path)
            .unwrap();
        assert_eq!(read, "");
    }

    #[test]
    fn basic_directory() {
        let store = Store::new();
        let path1 = Path::from(DOM0_DOMAIN_ID, "/basic/path1");
        let path2 = Path::from(DOM0_DOMAIN_ID, "/basic/path2");
        let parent = path1.parent().unwrap();

        let mut changes = store.mkdir(&ChangeSet::new(&store), DOM0_DOMAIN_ID, path1.clone())
            .unwrap();
        changes = store.mkdir(&changes, DOM0_DOMAIN_ID, path2.clone())
            .unwrap();

        // grab a list of all subdirectories
        let subdirs = store.directory(&changes, DOM0_DOMAIN_ID, &parent)
            .unwrap();
        assert_eq!(subdirs,
                   vec![Basename::from("path1"), Basename::from("path2")]);
    }

    #[test]
    fn rm_deletes_all_directories() {
        let store = Store::new();

        let path1 = Path::from(DOM0_DOMAIN_ID, "/basic/path1");
        let path2 = Path::from(DOM0_DOMAIN_ID, "/basic/path2");
        let basic = path1.parent()
            .unwrap();
        let mut changes = store.mkdir(&ChangeSet::new(&store), DOM0_DOMAIN_ID, path1.clone())
            .unwrap();
        changes = store.mkdir(&changes, DOM0_DOMAIN_ID, path2.clone())
            .unwrap();

        changes = store.rm(&changes, DOM0_DOMAIN_ID, &basic)
            .unwrap();

        // verify the parent directory was removed
        match store.read(&changes, DOM0_DOMAIN_ID, &basic) {
            Err(Error::ENOENT(_)) => assert!(true),
            Err(ref e) => assert!(false, format!("unexpected error returned {:?}", e)),
            Ok(_) => assert!(false, format!("failed to remove {:?}", basic)),
        }

        // verify the path1 directory was removed
        match store.read(&changes, DOM0_DOMAIN_ID, &path1) {
            Err(Error::ENOENT(_)) => assert!(true),
            Err(ref e) => assert!(false, format!("unexpected error returned {:?}", e)),
            Ok(_) => assert!(false, format!("failed to remove {:?}", basic)),
        }

        // verify the path2 directory was removed
        match store.read(&changes, DOM0_DOMAIN_ID, &path2) {
            Err(Error::ENOENT(_)) => assert!(true),
            Err(ref e) => assert!(false, format!("unexpected error returned {:?}", e)),
            Ok(_) => assert!(false, format!("failed to remove {:?}", basic)),
        }

        // verify the root still exists
        let read = store.read(&changes, DOM0_DOMAIN_ID, &Path::from(DOM0_DOMAIN_ID, "/"))
            .unwrap();
        assert_eq!(read, "");
    }

    #[test]
    fn rm_cannot_delete_root() {
        let store = Store::new();

        let path1 = Path::from(DOM0_DOMAIN_ID, "/basic/path1");

        let changes = store.mkdir(&ChangeSet::new(&store), DOM0_DOMAIN_ID, path1.clone())
            .unwrap();

        let rslt = store.rm(&changes, DOM0_DOMAIN_ID, &Path::from(DOM0_DOMAIN_ID, "/"));

        match rslt {
            Ok(_) => assert!(false, "removed the root directory"),
            Err(Error::EINVAL(_)) => assert!(true, "it is invalid to try and remove /"),
            Err(_) => assert!(false, "unknown error"),
        }
    }

    #[test]
    fn rm_removes_from_parent() {
        let store = Store::new();

        // Create the global state
        let path1 = Path::from(DOM0_DOMAIN_ID, "/basic/path1");
        let path2 = Path::from(DOM0_DOMAIN_ID, "/basic/path2");
        let basic = path1.parent()
            .unwrap();

        let mut changes = store.mkdir(&ChangeSet::new(&store), DOM0_DOMAIN_ID, path1.clone())
            .unwrap();
        changes = store.mkdir(&changes, DOM0_DOMAIN_ID, path2.clone())
            .unwrap();

        changes = store.rm(&changes, DOM0_DOMAIN_ID, &path1)
            .unwrap();

        // verify the path1 directory was removed
        match store.read(&changes, DOM0_DOMAIN_ID, &path1) {
            Err(Error::ENOENT(_)) => assert!(true),
            Err(ref e) => assert!(false, format!("unexpected error returned {:?}", e)),
            Ok(_) => assert!(false, format!("failed to remove {:?}", basic)),
        }

        let subdirs = store.directory(&changes, DOM0_DOMAIN_ID, &basic).unwrap();
        assert_eq!(subdirs, vec![String::from("path2")]);
    }

    #[test]
    fn get_root_permissions() {
        let store = Store::new();
        let permissions = store.get_perms(&ChangeSet::new(&store),
                       DOM0_DOMAIN_ID,
                       &Path::from(DOM0_DOMAIN_ID, "/"))
            .unwrap();

        assert_eq!(permissions,
                   vec![Permission {
                            id: DOM0_DOMAIN_ID,
                            perm: PERM_NONE,
                        }]);
    }

    #[test]
    fn get_local_permissions() {
        let store = Store::new();

        let mut changes = store.mkdir(&ChangeSet::new(&store),
                   DOM0_DOMAIN_ID,
                   Path::from(DOM0_DOMAIN_ID, "/local/domain/1"))
            .unwrap();

        changes = store.set_perms(&changes,
                       DOM0_DOMAIN_ID,
                       &Path::from(DOM0_DOMAIN_ID, "/local/domain/1"),
                       vec![Permission {
                                id: 1,
                                perm: PERM_NONE,
                            }])
            .unwrap();

        let path = Path::from(1, "foo");
        let value = Value::from("value");
        store.write(&changes, 1, path.clone(), value.clone())
            .unwrap();
    }

    #[test]
    fn permissions_idempotent() {
        let store = Store::new();
        let path = Path::from(DOM0_DOMAIN_ID, "/local/domain/1");

        let mut changes = store.mkdir(&ChangeSet::new(&store), DOM0_DOMAIN_ID, path.clone())
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

        changes = store.set_perms(&changes, DOM0_DOMAIN_ID, &path, perms.clone())
            .unwrap();

        let read = store.get_perms(&changes, DOM0_DOMAIN_ID, &path)
            .unwrap();

        assert_eq!(perms, read);
    }

    #[test]
    fn permissions_inherit() {
        let store = Store::new();
        let path = Path::from(DOM0_DOMAIN_ID, "/local/domain/1");

        let mut changes = store.mkdir(&ChangeSet::new(&store), DOM0_DOMAIN_ID, path.clone())
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

        changes = store.set_perms(&changes, DOM0_DOMAIN_ID, &path, perms.clone())
            .unwrap();

        let path = path.push("foo");
        changes = store.write(&changes, 1, path.clone(), Value::from("bar"))
            .unwrap();

        let read = store.get_perms(&changes, 1, &path)
            .unwrap();

        assert_eq!(perms, read);
    }

    #[test]
    fn permissions_inherit_no_overwrite_owner() {
        let store = Store::new();
        let path = Path::from(DOM0_DOMAIN_ID, "/local/domain/1");

        let mut changes = store.mkdir(&ChangeSet::new(&store), DOM0_DOMAIN_ID, path.clone())
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

        changes = store.set_perms(&changes, DOM0_DOMAIN_ID, &path, perms.clone())
            .unwrap();

        let path = path.push("foo");
        changes = store.write(&changes, DOM0_DOMAIN_ID, path.clone(), Value::from("bar"))
            .unwrap();

        let read = store.get_perms(&changes, 1, &path)
            .unwrap();

        assert_eq!(perms, read);
    }

    #[test]
    fn block_cross_domain_reads() {
        let store = Store::new();

        let mut changes = store.mkdir(&ChangeSet::new(&store),
                   DOM0_DOMAIN_ID,
                   Path::from(DOM0_DOMAIN_ID, "/local/domain/1"))
            .unwrap();

        changes = store.set_perms(&changes,
                       DOM0_DOMAIN_ID,
                       &Path::from(DOM0_DOMAIN_ID, "/local/domain/1"),
                       vec![Permission {
                                id: 1,
                                perm: PERM_NONE,
                            }])
            .unwrap();

        let path = Path::from(1, "foo");
        let value = Value::from("value");
        changes = store.write(&changes, 1, path.clone(), value.clone())
            .unwrap();

        // Check the domain 2 is blocked
        let v = store.read(&changes, 2, &path);
        match v {
            Ok(_) => assert!(false, "allowed cross-domain read"),
            Err(Error::EACCES(..)) => assert!(true, "blocked cross-domain read"),
            Err(_) => assert!(false, "unknown error"),
        }

        // Check the Dom0 is still allowed
        let read = store.read(&changes, DOM0_DOMAIN_ID, &path).unwrap();
        assert_eq!(read, value);
    }

    #[test]
    fn block_cross_domain_writes() {
        let store = Store::new();

        let mut changes = store.mkdir(&ChangeSet::new(&store),
                   DOM0_DOMAIN_ID,
                   Path::from(DOM0_DOMAIN_ID, "/local/domain/1"))
            .unwrap();

        changes = store.set_perms(&changes,
                       DOM0_DOMAIN_ID,
                       &Path::from(DOM0_DOMAIN_ID, "/local/domain/1"),
                       vec![Permission {
                                id: 1,
                                perm: PERM_NONE,
                            }])
            .unwrap();

        let path = Path::from(1, "foo");
        let value = Value::from("value");
        changes = store.write(&changes, 1, path.clone(), value.clone())
            .unwrap();

        // Check the domain 2 is blocked
        let v = store.write(&changes, 2, path.clone(), Value::from("new value"));
        match v {
            Ok(_) => assert!(false, "allowed cross-domain write"),
            Err(Error::EACCES(..)) => assert!(true, "blocked cross-domain write"),
            Err(_) => assert!(false, "unknown error"),
        }

        // Check the Dom0 is still allowed
        changes = store.write(&changes,
                   DOM0_DOMAIN_ID,
                   path.clone(),
                   Value::from("new value"))
            .unwrap();

        let read = store.read(&changes, 1, &path).unwrap();
        assert_eq!(read, Value::from("new value"));
    }

    #[test]
    fn block_cross_domain_rm() {
        let store = Store::new();

        let mut changes = store.mkdir(&ChangeSet::new(&store),
                   DOM0_DOMAIN_ID,
                   Path::from(DOM0_DOMAIN_ID, "/local/domain/1"))
            .unwrap();

        changes = store.set_perms(&changes,
                       DOM0_DOMAIN_ID,
                       &Path::from(DOM0_DOMAIN_ID, "/local/domain/1"),
                       vec![Permission {
                                id: 1,
                                perm: PERM_NONE,
                            }])
            .unwrap();

        let path = Path::from(1, "foo");
        let value = Value::from("value");
        changes = store.write(&changes, 1, path.clone(), value.clone())
            .unwrap();

        // Check the domain 2 is blocked
        let v = store.rm(&changes, 2, &path);
        match v {
            Ok(_) => assert!(false, "allowed cross-domain rm"),
            Err(Error::EACCES(..)) => assert!(true, "blocked cross-domain rm"),
            Err(_) => assert!(false, "unknown error"),
        }

        // Check the Dom0 is still allowed
        store.rm(&changes, DOM0_DOMAIN_ID, &path).unwrap();
    }

    #[test]
    fn block_cross_domain_directory() {
        let store = Store::new();
        let domain = Path::from(DOM0_DOMAIN_ID, "/local/domain/1");

        let mut changes = store.mkdir(&ChangeSet::new(&store), DOM0_DOMAIN_ID, domain.clone())
            .unwrap();

        changes = store.set_perms(&changes,
                       DOM0_DOMAIN_ID,
                       &domain,
                       vec![Permission {
                                id: 1,
                                perm: PERM_NONE,
                            }])
            .unwrap();

        let path = Path::from(1, "foo");
        let value = Value::from("value");
        changes = store.write(&changes, 1, path.clone(), value.clone())
            .unwrap();

        // Check the domain 2 is blocked
        let v = store.directory(&changes, 2, &domain);
        match v {
            Ok(_) => assert!(false, "allowed cross-domain subdir"),
            Err(Error::EACCES(..)) => assert!(true, "blocked cross-domain subdir"),
            Err(_) => assert!(false, "unknown error"),
        }

        // Check the Dom0 is still allowed
        store.directory(&changes, DOM0_DOMAIN_ID, &domain).unwrap();
    }
}
