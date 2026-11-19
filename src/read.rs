use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::path::PathBuf;

#[derive(Clone, Copy)]
pub struct FileId(u16);

#[derive(Clone, Copy)]
pub struct RevisionId(u32);
pub struct SpanId(u32);

struct Span {
    start: u32,
    length: u32,
}

pub struct SpanRef {
    file: FileId,
    revision: RevisionId,
    span: Span,
}

pub struct File {
    contents: String,
    revision: RevisionId,
    relative_path: PathBuf,
}

pub struct Reader {
    files: Vec<File>,
    file_map: BTreeMap<String, FileId>,
    root_path: PathBuf,
}

pub struct FileSet {
    data: Vec<u64>
}

impl Reader {
    /// Get the FileId for a relative-path
    pub fn intern_file(&mut self, relative_path: &str) -> FileId {
        match self.file_map.entry(relative_path.to_owned()) {
            Entry::Occupied(e) => *e.get(),
            Entry::Vacant(e) => {
                let id = FileId(u16::try_from(self.files.len()).unwrap());
                e.insert(id);
                self.files.push(File{
                    contents: String::new(),
                    revision: RevisionId(0),
                    relative_path: PathBuf::from(relative_path),
                });
                id
            }
        }
    }

    pub fn file(&self, id: FileId) -> &File {
        &self.files[id.0 as usize]
    }

    pub fn file_mut(&mut self, id: FileId) -> &mut File {
        &mut self.files[id.0 as usize]
    }

    pub fn update_file_contents(&mut self, id: FileId, new_contents: String) -> RevisionId {
        let file = self.file_mut(id);
        let rev  = RevisionId(file.revision.0 + 1);
        file.revision = rev;
        file.contents = new_contents;
        rev
    }

    pub fn update_file_from_disk(&mut self, id: FileId) -> Option<RevisionId> {
        // construct the full path
        None
    }
}
