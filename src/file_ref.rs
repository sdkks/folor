/// Unique identity of a file, stable across renames.
/// Derived from stat(2): (st_dev, st_ino).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileRef {
    pub device: u64,
    pub inode: u64,
}
