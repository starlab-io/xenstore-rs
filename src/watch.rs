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
pub enum WPath {
    Normal(Path),
    IntroduceDomain,
    ReleaseDomain,
}

impl WPath {
    pub fn try_from(dom_id: wire::DomainId, s: &str) -> Result<WPath> {
        match s {
            "@introduceDomain" => Ok(WPath::IntroduceDomain),
            "@releaseDomain" => Ok(WPath::ReleaseDomain),
            _ => Path::try_from(dom_id, s).map(WPath::Normal),
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Watch {
    dom_id: wire::DomainId,
    path: WPath,
}

impl Watch {
    pub fn new(dom_id: wire::DomainId, path: WPath) -> Watch {
        Watch {
            dom_id: dom_id,
            path: path,
        }
    }

    pub fn matches(&self, change: &AppliedChange) -> bool {
        match (change, &self.path) {
            (&AppliedChange::Write(ref cpath, _), &WPath::Normal(ref wpath)) => {
                cpath == wpath && change.perms_ok(self.dom_id, store::Perm::Read)
            }
            (&AppliedChange::IntroduceDomain, &WPath::IntroduceDomain) => true,
            (&AppliedChange::ReleaseDomain, &WPath::ReleaseDomain) => true,
            (_, _) => false,
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

    pub fn watch(&mut self, dom_id: wire::DomainId, path: WPath) -> Result<()> {
        if !self.watches.insert(Watch::new(dom_id, path.clone())) {
            return Err(Error::EEXIST(format!("watch {:?} already exists for domain {:?}",
                                             path,
                                             dom_id)));
        }
        Ok(())
    }

    pub fn unwatch(&mut self, dom_id: wire::DomainId, path: WPath) -> Result<()> {
        if !self.watches.remove(&Watch::new(dom_id, path.clone())) {
            return Err(Error::ENOENT(format!("watch {:?} did not exist for domain {:?}",
                                             path,
                                             dom_id)));
        }
        Ok(())
    }

    pub fn reset(&mut self, dom_id: wire::DomainId) -> Result<()> {
        let to_remove = self.watches
            .iter()
            .filter(|watch| watch.dom_id == dom_id)
            .cloned()
            .collect::<Vec<Watch>>();
        for watch in to_remove {
            self.watches.remove(&watch);
        }
        Ok(())
    }

    pub fn fire_single(&self, single: &AppliedChange) -> HashSet<Watch> {
        self.watches
            .iter()
            .filter(|watch| watch.matches(single))
            .cloned()
            .collect::<HashSet<Watch>>()
    }

    pub fn fire(&self, applied_changes: Option<Vec<AppliedChange>>) -> HashSet<Watch> {
        if let Some(changes) = applied_changes {
            changes.iter()
                .flat_map(|change| self.fire_single(&change))
                .collect::<HashSet<Watch>>()
        } else {
            HashSet::new()
        }
    }
}

#[cfg(test)]
mod test {
    use super::super::path::Path;
    use super::super::store::{self, Value, DOM0_DOMAIN_ID, Store, AppliedChange, ChangeSet};
    use super::*;

    #[test]
    fn basic_watch() {
        let mut watch_list = WatchList::new();
        let mut store = Store::new();
        let path = Path::try_from(DOM0_DOMAIN_ID, "/root/file/path").unwrap();
        let value = Value::from("value");

        watch_list.watch(DOM0_DOMAIN_ID, WPath::Normal(path.clone())).unwrap();

        let changes = store.write(&ChangeSet::new(&store),
                   DOM0_DOMAIN_ID,
                   path.clone(),
                   value.clone())
            .unwrap();

        let applied = store.apply(changes);
        let watches = watch_list.fire(applied);

        assert_eq!(watches.len(), 1);
        assert_eq!(watches.contains(&Watch {
                       dom_id: DOM0_DOMAIN_ID,
                       path: WPath::Normal(path),
                   }),
                   true);
    }

    #[test]
    fn basic_watch_no_permission() {
        let mut watch_list = WatchList::new();
        let mut store = Store::new();
        let path = Path::try_from(DOM0_DOMAIN_ID, "/root/file/path").unwrap();
        let value = Value::from("value");

        watch_list.watch(DOM0_DOMAIN_ID, WPath::Normal(path.clone())).unwrap();
        watch_list.watch(1, WPath::Normal(path.clone())).unwrap();

        let changes = store.write(&ChangeSet::new(&store),
                   DOM0_DOMAIN_ID,
                   path.clone(),
                   value.clone())
            .unwrap();

        let applied = store.apply(changes);
        let watches = watch_list.fire(applied);

        assert_eq!(watches.len(), 1);
        assert_eq!(watches.contains(&Watch {
                       dom_id: DOM0_DOMAIN_ID,
                       path: WPath::Normal(path),
                   }),
                   true);
    }

    #[test]
    fn basic_watch_with_permission() {
        let mut watch_list = WatchList::new();
        let mut store = Store::new();
        let path = Path::try_from(DOM0_DOMAIN_ID, "/root/file/path").unwrap();
        let value = Value::from("value");

        watch_list.watch(DOM0_DOMAIN_ID, WPath::Normal(path.clone())).unwrap();
        watch_list.watch(1, WPath::Normal(path.clone())).unwrap();

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
                                perm: store::Perm::None,
                            }])
            .unwrap();

        let applied = store.apply(changes);
        let watches = watch_list.fire(applied);

        assert_eq!(watches.len(), 2);
        assert_eq!(watches.contains(&Watch::new(DOM0_DOMAIN_ID, WPath::Normal(path.clone()))),
                   true);
        assert_eq!(watches.contains(&Watch::new(1, WPath::Normal(path.clone()))),
                   true);
    }

    #[test]
    fn basic_watch_parent() {
        let mut watch_list = WatchList::new();
        let mut store = Store::new();
        let path = Path::try_from(DOM0_DOMAIN_ID, "/root/file/path").unwrap();
        let value = Value::from("value");

        watch_list.watch(DOM0_DOMAIN_ID, WPath::Normal(path.parent().unwrap())).unwrap();

        let changes = store.write(&ChangeSet::new(&store),
                   DOM0_DOMAIN_ID,
                   path.clone(),
                   value.clone())
            .unwrap();

        let applied = store.apply(changes);
        let watches = watch_list.fire(applied);

        assert_eq!(watches.len(), 1);
        assert_eq!(watches.contains(&Watch {
                       dom_id: DOM0_DOMAIN_ID,
                       path: WPath::Normal(path.parent().unwrap()),
                   }),
                   true);

        let changes = store.write(&ChangeSet::new(&store),
                   DOM0_DOMAIN_ID,
                   path.clone(),
                   Value::from("value 2"))
            .unwrap();

        let applied = store.apply(changes);
        let watches = watch_list.fire(applied);

        assert_eq!(watches.len(), 0);
    }

    #[test]
    fn basic_watch_remove() {
        let mut watch_list = WatchList::new();
        let mut store = Store::new();
        let path = Path::try_from(DOM0_DOMAIN_ID, "/root/file/path").unwrap();
        let value = Value::from("value");

        watch_list.watch(DOM0_DOMAIN_ID, WPath::Normal(path.parent().unwrap())).unwrap();
        watch_list.watch(DOM0_DOMAIN_ID, WPath::Normal(path.clone())).unwrap();

        let changes = store.write(&ChangeSet::new(&store),
                   DOM0_DOMAIN_ID,
                   path.clone(),
                   value.clone())
            .unwrap();

        let applied = store.apply(changes);
        let watches = watch_list.fire(applied);

        assert_eq!(watches.len(), 2);
        assert_eq!(watches.contains(&Watch {
                       dom_id: DOM0_DOMAIN_ID,
                       path: WPath::Normal(path.parent().unwrap()),
                   }),
                   true);
        assert_eq!(watches.contains(&Watch {
                       dom_id: DOM0_DOMAIN_ID,
                       path: WPath::Normal(path.clone()),
                   }),
                   true);

        let changes = store.rm(&ChangeSet::new(&store), DOM0_DOMAIN_ID, &path)
            .unwrap();

        let applied = store.apply(changes);
        let watches = watch_list.fire(applied);

        assert_eq!(watches.len(), 1);
        assert_eq!(watches.contains(&Watch {
                       dom_id: DOM0_DOMAIN_ID,
                       path: WPath::Normal(path.parent().unwrap()),
                   }),
                   true);
    }

    #[test]
    fn basic_watch_introduce_domain() {
        let mut watch_list = WatchList::new();

        watch_list.watch(DOM0_DOMAIN_ID, WPath::IntroduceDomain).unwrap();
        watch_list.watch(DOM0_DOMAIN_ID, WPath::ReleaseDomain).unwrap();

        let watches = watch_list.fire_single(&AppliedChange::IntroduceDomain);

        assert_eq!(watches.len(), 1);
        assert_eq!(watches.contains(&Watch {
                       dom_id: DOM0_DOMAIN_ID,
                       path: WPath::IntroduceDomain,
                   }),
                   true);
    }

    #[test]
    fn basic_watch_release_domain() {
        let mut watch_list = WatchList::new();

        watch_list.watch(DOM0_DOMAIN_ID, WPath::IntroduceDomain).unwrap();
        watch_list.watch(DOM0_DOMAIN_ID, WPath::ReleaseDomain).unwrap();

        let watches = watch_list.fire_single(&AppliedChange::ReleaseDomain);

        assert_eq!(watches.len(), 1);
        assert_eq!(watches.contains(&Watch {
                       dom_id: DOM0_DOMAIN_ID,
                       path: WPath::ReleaseDomain,
                   }),
                   true);
    }

    #[test]
    fn basic_watch_reset() {
        let mut watch_list = WatchList::new();

        watch_list.watch(DOM0_DOMAIN_ID, WPath::IntroduceDomain).unwrap();
        watch_list.watch(DOM0_DOMAIN_ID, WPath::ReleaseDomain).unwrap();
        watch_list.watch(1, WPath::ReleaseDomain).unwrap();

        watch_list.reset(DOM0_DOMAIN_ID).unwrap();

        assert_eq!(watch_list.watches.len(), 1);
        assert_eq!(watch_list.watches.contains(&Watch {
                       dom_id: 1,
                       path: WPath::ReleaseDomain,
                   }),
                   true);
    }
}
