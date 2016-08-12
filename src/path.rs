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
pub enum Path {
    Absolute(path::PathBuf),
    Relative(wire::DomainId, path::PathBuf),
}

impl Path {
    pub fn from(dom_id: wire::DomainId, s: &str) -> Path {
        let internal = path::PathBuf::from(s);
        if internal.is_absolute() {
            return Path::Absolute(internal);
        } else {
            return Path::Relative(dom_id, internal);
        }
    }

    pub fn realpath(self: &Path) -> Path {
        match *self {
            Path::Absolute(_) => self.clone(),
            Path::Relative(d, ref p) => {
                let mut real = path::PathBuf::from(format!("/local/domain/{}/", d));
                real.push(p);

                Path::Absolute(real)
            }
        }
    }

    pub fn basename(self: &Path) -> Option<String> {
        match self.realpath() {
            Path::Absolute(realpath) => {
                realpath.as_path()
                    .file_name()
                    .and_then(|bn| bn.to_str())
                    .map(|bn| bn.to_owned())
            }
            _ => unreachable!(),
        }
    }

    pub fn parent(self: &Path) -> Option<Path> {
        match self.realpath() {
            Path::Absolute(realpath) => {
                realpath.as_path()
                    .parent()
                    .map(|parent| Path::Absolute(parent.to_path_buf()))
            }
            _ => unreachable!(),
        }
    }

    pub fn push(self: &Path, component: &str) -> Path {
        match *self {
            Path::Absolute(ref path) => {
                let mut path = path.clone();
                path.push(component);
                Path::Absolute(path)
            }
            Path::Relative(d, ref path) => {
                let mut path = path.clone();
                path.push(component);
                Path::Relative(d, path)
            }
        }
    }

    pub fn is_child(self: &Path, parent: &Path) -> bool {
        let (child, parent) = match (self.realpath(), parent.realpath()) {
            (Path::Absolute(child), Path::Absolute(parent)) => (child, parent),
            (_, _) => unreachable!(),
        };

        child.starts_with(&parent)
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
