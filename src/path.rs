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

use std::path;
use super::wire;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Path(path::PathBuf);

pub fn get_domain_path(dom_id: wire::DomainId) -> Path {
    Path(path::PathBuf::from(format!("/local/domain/{}/", dom_id)))
}

impl Path {
    pub fn from(dom_id: wire::DomainId, s: &str) -> Path {
        let input = path::PathBuf::from(s);
        let internal = {
            if input.is_absolute() {
                input
            } else {
                let mut real = get_domain_path(dom_id);
                real.0.push(input);
                real.0
            }
        };

        Path(internal)
    }

    pub fn basename(self: &Path) -> Option<String> {
        self.0
            .as_path()
            .file_name()
            .and_then(|bn| bn.to_str())
            .map(|bn| bn.to_owned())
    }

    pub fn parent(self: &Path) -> Option<Path> {
        self.0
            .as_path()
            .parent()
            .map(|parent| Path(parent.to_path_buf()))
    }

    pub fn push(self: &Path, component: &str) -> Path {
        let mut path = self.0.clone();
        path.push(component);
        Path(path)
    }

    pub fn is_child(self: &Path, parent: &Path) -> bool {
        self.0.starts_with(&parent.0)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn is_child() {
        let root = Path::from(0, "/");
        let child = Path::from(0, "/root/filesystem/test");
        let parent = child.parent().unwrap();
        let grandparent = parent.parent().unwrap();

        assert_eq!(child.is_child(&parent), true);
        assert_eq!(parent.is_child(&child), false);

        assert_eq!(child.is_child(&grandparent), true);
        assert_eq!(child.is_child(&root), true);
    }
}
