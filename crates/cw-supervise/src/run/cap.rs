//! How much of a stream is kept, and what happens past that.

/// The most bytes of one stream a run keeps, and what happens past that.
///
/// Caps are per stream and per call. Both policies return the captured
/// prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cap {
    limit: usize,
    overflow: Overflow,
}

/// What a run does once a stream passes its [`Cap`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overflow {
    /// Keep reading and discarding, and mark the stream truncated. The child
    /// runs on to its exit or its deadline.
    Truncate,
    /// Stop the whole group at once; the run ends with
    /// [`End::OutputLimit`](crate::End::OutputLimit).
    Kill,
}

impl Cap {
    /// Keep the first `limit` bytes; read and discard the rest, so the child
    /// never blocks on a full pipe.
    #[must_use]
    pub const fn truncate(limit: usize) -> Self {
        Self {
            limit,
            overflow: Overflow::Truncate,
        }
    }

    /// Keep the first `limit` bytes; past them, stop the group.
    #[must_use]
    pub const fn kill(limit: usize) -> Self {
        Self {
            limit,
            overflow: Overflow::Kill,
        }
    }

    /// The most bytes kept.
    #[must_use]
    pub const fn limit(self) -> usize {
        self.limit
    }

    /// What happens past the limit.
    #[must_use]
    pub const fn overflow(self) -> Overflow {
        self.overflow
    }
}
