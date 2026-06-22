use rustc_hash::FxHashMap;

/// Interns stack frame address slices into compact integer IDs.
///
/// Two stacks with the same frame sequence always get the same ID.
/// IDs start at 1; 0 is reserved for "no stack".
#[derive(Debug, Default)]
pub struct StackInterner {
    map: FxHashMap<Vec<u64>, u32>,
    /// The interned stacks in insertion order; index + 1 == stack_id.
    stacks: Vec<Vec<u64>>,
}

impl StackInterner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the ID for this stack, interning it if new.
    pub fn intern(&mut self, frames: &[u64]) -> u32 {
        if let Some(&id) = self.map.get(frames) {
            return id;
        }
        let id = self.stacks.len() as u32 + 1;
        let owned = frames.to_vec();
        self.map.insert(owned.clone(), id);
        self.stacks.push(owned);
        id
    }

    /// Retrieve the frames for an ID (1-based). Returns `None` for id == 0 or out of range.
    pub fn get(&self, id: u32) -> Option<&[u64]> {
        if id == 0 {
            return None;
        }
        self.stacks.get(id as usize - 1).map(|v| v.as_slice())
    }

    pub fn len(&self) -> usize {
        self.stacks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.stacks.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_stack_same_id() {
        let mut si = StackInterner::new();
        let a = si.intern(&[0x1000, 0x2000]);
        let b = si.intern(&[0x1000, 0x2000]);
        assert_eq!(a, b);
    }

    #[test]
    fn different_stacks_different_ids() {
        let mut si = StackInterner::new();
        let a = si.intern(&[0x1000]);
        let b = si.intern(&[0x2000]);
        assert_ne!(a, b);
    }

    #[test]
    fn get_round_trip() {
        let mut si = StackInterner::new();
        let frames = vec![0x1000u64, 0x2000, 0x3000];
        let id = si.intern(&frames);
        assert_eq!(si.get(id), Some(frames.as_slice()));
    }

    #[test]
    fn get_zero_returns_none() {
        let si = StackInterner::new();
        assert!(si.get(0).is_none());
    }

    #[test]
    fn ids_start_at_one() {
        let mut si = StackInterner::new();
        assert_eq!(si.intern(&[0x1000]), 1);
        assert_eq!(si.intern(&[0x2000]), 2);
    }
}
