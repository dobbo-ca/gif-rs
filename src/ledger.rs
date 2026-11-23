//! The working memory of one call: every buffer that scales with the input is charged before it
//! is allocated. The crate is serial, so a plain counter is enough.

use crate::error::Error;

/// Bytes booked against `Limits::max_alloc`.
#[derive(Debug, Default)]
pub(crate) struct Ledger {
    max: usize,
    used: usize,
    #[cfg(test)]
    peak: usize,
}

impl Ledger {
    /// Starts a call: `max` bytes allowed, `held` of them already taken by buffers the caller
    /// keeps (the `Decoder`'s tables and vectors).
    pub(crate) fn reset(&mut self, max: usize, held: usize) -> Result<(), Error> {
        self.max = max;
        self.used = 0;
        #[cfg(test)]
        {
            self.peak = 0;
        }
        self.reserve(held)
    }

    /// Books `n` bytes for the rest of the call.
    pub(crate) fn reserve(&mut self, n: usize) -> Result<(), Error> {
        let total = self
            .used
            .checked_add(n)
            .filter(|&t| t <= self.max)
            .ok_or(Error::AllocLimit)?;
        self.used = total;
        #[cfg(test)]
        {
            self.peak = self.peak.max(total);
        }
        Ok(())
    }

    /// The most bytes booked at once in this call.
    #[cfg(test)]
    pub(crate) fn peak(&self) -> usize {
        self.peak
    }
}
