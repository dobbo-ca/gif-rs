//! A byte cursor over the input. It mirrors giflib's `InternalRead`: a short read is an error
//! and the cursor never goes past the end.

pub(crate) struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// The whole input.
    pub(crate) fn data(&self) -> &'a [u8] {
        self.data
    }

    /// Offset of the next byte.
    pub(crate) fn pos(&self) -> usize {
        self.pos
    }

    /// The next `n` bytes, or `None` (and no move) if fewer are left.
    pub(crate) fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let s = self.data.get(self.pos..end)?;
        self.pos = end;
        Some(s)
    }

    pub(crate) fn byte(&mut self) -> Option<u8> {
        self.take(1)?.first().copied()
    }
}
