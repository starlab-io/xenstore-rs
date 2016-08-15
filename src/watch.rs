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
use super::error::{Error, Result};
use super::path::Path;
use super::store::{self, AppliedChange};
use super::wire;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Watch {
    dom_id: wire::DomainId,
    path: Path,
}

impl Watch {
    pub fn new(dom_id: wire::DomainId, path: Path) -> Watch {
        Watch {
            dom_id: dom_id,
            path: path,
        }
    }
}

pub struct WatchList {
    watches: HashSet<Watch>,
}

impl WatchList {
    pub fn new() -> WatchList {
        WatchList { watches: HashSet::new() }
    }

    pub fn watch(&mut self, dom_id: wire::DomainId, path: Path) -> Result<()> {
        if !self.watches.insert(Watch::new(dom_id, path.clone())) {
            return Err(Error::EEXIST(format!("watch {:?} already exists for domain {:?}",
                                             path,
                                             dom_id)));
        }
        Ok(())
    }

    pub fn unwatch(&mut self, dom_id: wire::DomainId, path: Path) -> Result<()> {
        if !self.watches.remove(&Watch::new(dom_id, path.clone())) {
            return Err(Error::ENOENT(format!("watch {:?} did not exist for domain {:?}",
                                             path,
                                             dom_id)));
        }
        Ok(())
    }

    pub fn fire(&self, applied_changes: Option<Vec<AppliedChange>>) -> Vec<Watch> {
        if let Some(changes) = applied_changes {
            changes.iter()
                .flat_map(|change| {
                    self.watches
                        .iter()
                        .filter(|watch| {
                            change.path.is_child(&watch.path) &&
                            change.perms_ok(watch.dom_id, store::PERM_READ)
                        })
                        .cloned()
                        .collect::<Vec<Watch>>()
                })
                .collect::<Vec<Watch>>()
        } else {
            vec![]
        }
    }
}

#[cfg(test)]
mod test {
    use std::collections::HashSet;
    use std::iter::FromIterator;
    use super::super::path::Path;
    use super::super::store::{self, Value, DOM0_DOMAIN_ID, Store, ChangeSet};
    use super::*;

    #[test]
    fn basic_watch() {
        let mut watch_list = WatchList::new();
        let mut store = Store::new();
        let path = Path::from(DOM0_DOMAIN_ID, "/root/file/path");
        let value = Value::from("value");

        watch_list.watch(DOM0_DOMAIN_ID, path.clone()).unwrap();

        let changes = store.write(&ChangeSet::new(&store),
                   DOM0_DOMAIN_ID,
                   path.clone(),
                   value.clone())
            .unwrap();

        let applied = store.apply(changes);
        let watches = watch_list.fire(applied);

        assert_eq!(watches.len(), 1);
        assert_eq!(watches[0].dom_id, DOM0_DOMAIN_ID);
        assert_eq!(watches[0].path, path);
    }

    #[test]
    fn basic_watch_no_permission() {
        let mut watch_list = WatchList::new();
        let mut store = Store::new();
        let path = Path::from(DOM0_DOMAIN_ID, "/root/file/path");
        let value = Value::from("value");

        watch_list.watch(DOM0_DOMAIN_ID, path.clone()).unwrap();
        watch_list.watch(1, path.clone()).unwrap();

        let changes = store.write(&ChangeSet::new(&store),
                   DOM0_DOMAIN_ID,
                   path.clone(),
                   value.clone())
            .unwrap();

        let applied = store.apply(changes);
        let watches = watch_list.fire(applied);

        assert_eq!(watches.len(), 1);
        assert_eq!(watches[0].dom_id, DOM0_DOMAIN_ID);
        assert_eq!(watches[0].path, path);
    }

    #[test]
    fn basic_watch_with_permission() {
        let mut watch_list = WatchList::new();
        let mut store = Store::new();
        let path = Path::from(DOM0_DOMAIN_ID, "/root/file/path");
        let value = Value::from("value");

        watch_list.watch(DOM0_DOMAIN_ID, path.clone()).unwrap();
        watch_list.watch(1, path.clone()).unwrap();

        let changes = store.write(&ChangeSet::new(&store),
                   DOM0_DOMAIN_ID,
                   path.clone(),
                   value.clone())
            .unwrap();

        let changes = store.set_perms(&changes,
                       DOM0_DOMAIN_ID,
                       &path,
                       vec![store::Permission {
                                id: 1,
                                perm: store::PERM_NONE,
                            }])
            .unwrap();

        let applied = store.apply(changes);
        let watches = watch_list.fire(applied);

        let watches: HashSet<Watch> = HashSet::from_iter(watches.iter()
            .cloned());

        assert_eq!(watches.len(), 2);
        assert_eq!(watches.contains(&Watch::new(DOM0_DOMAIN_ID, path.clone())),
                   true);
        assert_eq!(watches.contains(&Watch::new(1, path.clone())), true);
    }
}
