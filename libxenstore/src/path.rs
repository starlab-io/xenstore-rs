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

use std::iter::{IntoIterator, Iterator};
use std::os::unix::ffi::OsStrExt;
use std::path;
use super::error::{Error, Result};
use super::wire;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Path(path::PathBuf);
pub struct ParentIterator(Option<path::PathBuf>);

const MAX_RELATIVE: usize = 2048;
const MAX_ABSOLUTE: usize = 3072;

impl Iterator for ParentIterator {
    type Item = Path;

    fn next(&mut self) -> Option<Path> {
        let current = match self.0 {
            Some(ref c) => c.to_owned(),
            None => {
                return None;
            }
        };

        Some(Path(match current.parent() {
                      Some(ref p) => {
            self.0 = Some(p.to_path_buf());
            current.clone()
        }
                      None => {
            self.0 = None;
            path::PathBuf::from("/")
        }
                  }))
    }
}

impl IntoIterator for Path {
    type Item = Path;
    type IntoIter = ParentIterator;

    fn into_iter(self) -> Self::IntoIter {
        ParentIterator(Some(self.0.clone()))
    }
}

pub fn get_domain_path(dom_id: wire::DomainId) -> Path {
    Path(path::PathBuf::from(format!("/local/domain/{}/", dom_id)))
}

impl Path {
    pub fn try_from(dom_id: wire::DomainId, s: &str) -> Result<Path> {
        if s == "" {
            return Err(Error::EINVAL("empty path is not allowed".into()));
        }

        if s.contains("//") {
            return Err(Error::EINVAL("doubled / is not allowed".into()));
        }

        if s != "/" && s.ends_with("/") {
            return Err(Error::EINVAL("trailing / is not allowed".into()));
        }

        let input = path::PathBuf::from(s);
        let internal = {
            if input.is_absolute() {
                if s.len() > MAX_ABSOLUTE {
                    return Err(Error::EINVAL(format!("absolute path must be less than {} \
                                                      characters",
                                                     MAX_ABSOLUTE)));
                }

                input
            } else {
                if s.len() > MAX_RELATIVE {
                    return Err(Error::EINVAL(format!("relative path must be less than {} \
                                                      characters",
                                                     MAX_RELATIVE)));
                }

                let mut real = get_domain_path(dom_id);
                real.0.push(input);
                real.0
            }
        };

        Ok(Path(internal))
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_os_str().as_bytes()
    }

    pub fn basename(&self) -> Option<String> {
        self.0
            .as_path()
            .file_name()
            .and_then(|bn| bn.to_str())
            .map(|bn| bn.to_owned())
    }

    pub fn parent(&self) -> Option<Path> {
        self.0
            .as_path()
            .parent()
            .map(|parent| Path(parent.to_path_buf()))
    }

    pub fn push(&self, component: &str) -> Path {
        let mut path = self.0.clone();
        path.push(component);
        Path(path)
    }

    pub fn is_child(&self, parent: &Path) -> bool {
        self.0.starts_with(&parent.0)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    #[should_panic]
    fn empty_path() {
        Path::try_from(0, "").unwrap();
    }

    #[test]
    #[should_panic]
    fn doubled_slash() {
        Path::try_from(0, "/root//bar").unwrap();
    }

    #[test]
    #[should_panic]
    fn trailing_slash() {
        Path::try_from(0, "/root/").unwrap();
    }

    #[test]
    #[should_panic]
    fn long_relative() {
        let mut s = String::new();
        for _ in 0..(super::MAX_RELATIVE + 1) {
            s.push('a');
        }

        Path::try_from(1, &s).unwrap();
    }

    #[test]
    fn max_relative() {
        let mut s = String::new();
        for _ in 0..super::MAX_RELATIVE {
            s.push('a');
        }

        Path::try_from(1, &s).unwrap();
    }

    #[test]
    #[should_panic]
    fn long_absolute() {
        let mut s = String::from("/");
        for _ in 1..(super::MAX_ABSOLUTE + 1) {
            s.push('a');
        }

        Path::try_from(1, &s).unwrap();
    }

    #[test]
    fn max_absolute() {
        let mut s = String::from("/");
        for _ in 1..super::MAX_ABSOLUTE {
            s.push('a');
        }

        Path::try_from(1, &s).unwrap();
    }

    #[test]
    fn is_child() {
        let root = Path::try_from(0, "/").unwrap();
        let child = Path::try_from(0, "/root/filesystem/test").unwrap();
        let parent = child.parent().unwrap();
        let grandparent = parent.parent().unwrap();

        assert_eq!(child.is_child(&parent), true);
        assert_eq!(parent.is_child(&child), false);

        assert_eq!(child.is_child(&grandparent), true);
        assert_eq!(child.is_child(&root), true);
    }

    #[test]
    fn iterator() {
        let path = Path::try_from(0, "/root/filesystem/test").unwrap();
        let mut iter = path.into_iter();

        assert_eq!(iter.next(),
                   Some(Path::try_from(0, "/root/filesystem/test").unwrap()));
        assert_eq!(iter.next(),
                   Some(Path::try_from(0, "/root/filesystem").unwrap()));
        assert_eq!(iter.next(), Some(Path::try_from(0, "/root").unwrap()));
        assert_eq!(iter.next(), Some(Path::try_from(0, "/").unwrap()));
        assert_eq!(iter.next().is_none(), true);
    }
}
