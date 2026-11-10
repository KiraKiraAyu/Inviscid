use crate::markdown::ParsedLine;
use std::{
    ops::{Deref, Range},
    sync::Arc,
};

/// Parsed source rows shared by immutable layout snapshots.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParsedLineStore(Arc<Vec<ParsedLine>>);

impl ParsedLineStore {
    pub fn get(&self, line: usize) -> Option<&ParsedLine> {
        self.0.get(line)
    }

    pub fn range(&self, range: Range<usize>) -> &[ParsedLine] {
        &self.0[range]
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub(crate) fn make_mut(&mut self) -> &mut Vec<ParsedLine> {
        Arc::make_mut(&mut self.0)
    }
}

impl From<Arc<Vec<ParsedLine>>> for ParsedLineStore {
    fn from(lines: Arc<Vec<ParsedLine>>) -> Self {
        Self(lines)
    }
}

// Transitional slice access while consumers migrate to range queries.
impl Deref for ParsedLineStore {
    type Target = Arc<Vec<ParsedLine>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::ParsedLineStore;
    use crate::markdown::MarkdownScanner;
    use std::sync::Arc;

    #[test]
    fn ranges_and_edits_preserve_retained_snapshots() {
        let original = MarkdownScanner::scan_document(&["one".into(), "two".into()]);
        let mut store = ParsedLineStore::from(Arc::new(original.clone()));
        let snapshot = store.clone();
        assert_eq!(store.len(), 2);
        assert_eq!(store.get(1), original.get(1));
        assert_eq!(store.range(0..1), &original[..1]);
        store.make_mut().remove(0);
        assert_eq!(snapshot.as_slice(), original);
        assert_eq!(store.get(0), original.get(1));
    }
}
