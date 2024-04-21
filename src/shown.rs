#[derive(PartialEq, Eq)]
pub enum Shown {
    All,
    // We could use a None variant, but the cost of an unallocated Vec is negligible, and an
    // allocated Vec could be useful to keep around for future searches.
    Few(Vec<usize>),
}

impl Shown {
    pub fn is_empty(&self) -> bool {
        use Shown::*;

        match self {
            All => false,
            Few(v) => v.is_empty(),
        }
    }

    pub fn get_vec(&self) -> Option<&Vec<usize>> {
        use Shown::*;

        match self {
            All => None,
            Few(v) => Some(v),
        }
    }

    pub fn len(&self) -> Option<usize> {
        use Shown::*;

        match self {
            All => None,
            Few(v) => Some(v.len()),
        }
    }

    pub fn get(&self, idx: usize) -> Option<usize> {
        use Shown::*;

        match self {
            All => None,
            Few(v) => v.get(idx).copied(),
        }
    }

    pub fn clear(&mut self) {
        use Shown::*;

        match self {
            All => *self = Shown::Few(Vec::new()),
            Few(v) => v.clear(),
        }
    }

    pub fn extend(&mut self, iter: impl Iterator<Item = usize>) {
        use Shown::*;

        match self {
            All => (),
            Few(v) => v.extend(iter),
        }
    }
}
