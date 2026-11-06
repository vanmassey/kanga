//! KangaFS: a small fixed-capacity, volatile in-memory filesystem.
//!
//! This first filesystem deliberately avoids a heap and disk driver. It gives
//! the shell real file operations while keeping storage bounded and explicit.

pub const MAX_FILES: usize = 16;
pub const NAME_CAPACITY: usize = 24;
pub const FILE_CAPACITY: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidName,
    NameTooLong,
    FileTooLarge,
    DirectoryFull,
}

#[derive(Clone, Copy)]
struct File {
    used: bool,
    name: [u8; NAME_CAPACITY],
    name_len: usize,
    data: [u8; FILE_CAPACITY],
    data_len: usize,
}

impl File {
    const EMPTY: Self = Self {
        used: false,
        name: [0; NAME_CAPACITY],
        name_len: 0,
        data: [0; FILE_CAPACITY],
        data_len: 0,
    };

    fn name(&self) -> &str {
        // Names are validated as ASCII before being stored.
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("")
    }
}

/// A bounded root directory stored entirely in kernel RAM.
/// Contents are lost when Kanga reboots; disk persistence is a later layer.
pub struct FileSystem {
    files: [File; MAX_FILES],
}

impl FileSystem {
    pub const fn new() -> Self {
        Self {
            files: [File::EMPTY; MAX_FILES],
        }
    }

    pub fn write(&mut self, name: &[u8], contents: &[u8]) -> Result<(), Error> {
        let name = validate_name(name)?;
        if contents.len() > FILE_CAPACITY {
            return Err(Error::FileTooLarge);
        }

        let existing = self
            .files
            .iter()
            .position(|file| file.used && file.name() == name);
        let slot = match existing {
            Some(index) => index,
            None => self
                .files
                .iter()
                .position(|file| !file.used)
                .ok_or(Error::DirectoryFull)?,
        };

        let file = &mut self.files[slot];
        file.name = [0; NAME_CAPACITY];
        file.name[..name.len()].copy_from_slice(name.as_bytes());
        file.name_len = name.len();
        file.data = [0; FILE_CAPACITY];
        file.data[..contents.len()].copy_from_slice(contents);
        file.data_len = contents.len();
        file.used = true;
        Ok(())
    }

    pub fn read(&self, name: &[u8]) -> Option<&[u8]> {
        let name = core::str::from_utf8(name).ok()?;
        self.files
            .iter()
            .find(|file| file.used && file.name().eq_ignore_ascii_case(name))
            .map(|file| &file.data[..file.data_len])
    }

    pub fn delete(&mut self, name: &[u8]) -> bool {
        let Ok(name) = validate_name(name) else {
            return false;
        };
        if let Some(file) = self
            .files
            .iter_mut()
            .find(|file| file.used && file.name() == name)
        {
            *file = File::EMPTY;
            true
        } else {
            false
        }
    }

    pub fn entry(&self, index: usize) -> Option<(&str, usize)> {
        let file = self.files.get(index)?;
        if file.used {
            Some((file.name(), file.data_len))
        } else {
            None
        }
    }

    pub const fn capacity(&self) -> usize {
        MAX_FILES
    }

    pub const fn file_capacity(&self) -> usize {
        FILE_CAPACITY
    }

    pub fn used(&self) -> usize {
        self.files.iter().filter(|file| file.used).count()
    }
}

fn validate_name(name: &[u8]) -> Result<&str, Error> {
    if name.is_empty() {
        return Err(Error::InvalidName);
    }
    if name.len() > NAME_CAPACITY {
        return Err(Error::NameTooLong);
    }
    if !name
        .iter()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'.' | b'_' | b'-'))
    {
        return Err(Error::InvalidName);
    }
    core::str::from_utf8(name).map_err(|_| Error::InvalidName)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_read_overwrite_and_delete() {
        let mut fs = FileSystem::new();
        fs.write(b"note.txt", b"hello").unwrap();
        assert_eq!(fs.read(b"note.txt"), Some(&b"hello"[..]));
        fs.write(b"note.txt", b"updated").unwrap();
        assert_eq!(fs.read(b"note.txt"), Some(&b"updated"[..]));
        assert!(fs.delete(b"note.txt"));
        assert_eq!(fs.read(b"note.txt"), None);
    }

    #[test]
    fn rejects_invalid_names_and_oversized_files() {
        let mut fs = FileSystem::new();
        assert_eq!(fs.write(b"bad name", b"x"), Err(Error::InvalidName));
        assert_eq!(
            fs.write(b"large.bin", &[0; FILE_CAPACITY + 1]),
            Err(Error::FileTooLarge)
        );
    }

    #[test]
    fn enforces_directory_capacity() {
        let mut fs = FileSystem::new();
        for index in 0..MAX_FILES {
            let name = [b'f', b'0' + index as u8];
            fs.write(&name, b"x").unwrap();
        }
        assert_eq!(fs.write(b"overflow", b"x"), Err(Error::DirectoryFull));
        assert_eq!(fs.used(), MAX_FILES);
    }
}
