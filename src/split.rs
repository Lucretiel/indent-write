// Helper for splitting slices around

/**
This type describes a slice that has been split in half, into a head and tail,
at a given point. The primary use of this type is that it can "resplit" itself,
whereby the tail is split the left half of that new split is shifted into the
head:

`[HHHH AAAABBBB] -> [HHHHAAAA BBBB]`
*/
pub struct Split<'a, T> {
    slice: &'a [T],

    // Invariant: 0 <= point <= slice.len()
    point: usize,
}

impl<'a, T> Split<'a, T> {
    /// Create a new `Split` where the entire slice is in the tail. Use
    /// [`resplit_tail`][Self::resplit_tail] to move parts of it into the head.
    #[inline]
    #[must_use]
    pub fn new(slice: &'a [T]) -> Self {
        Self { slice, point: 0 }
    }

    /// Get the front part of the split
    #[inline]
    #[must_use]
    pub fn head(&self) -> &'a [T] {
        debug_assert!(self.point <= self.slice.len());

        // Safety: an invariant of the type is that self.point is in bounds
        unsafe { self.slice.get_unchecked(..self.point) }
    }

    /// Get the rear part of the split
    #[inline]
    #[must_use]
    pub fn tail(&self) -> &'a [T] {
        debug_assert!(self.point <= self.slice.len());

        // Safety: an invariant of the type is that self.point is in bounds
        unsafe { self.slice.get_unchecked(self.point..) }
    }

    /**
    Update the split by splitting the tail, and joining the left half of that
    split with the head of this one. All elements matching `pred` are joined
    with the head.

    `HHH AAAAABBBBB -> HHHAAAAA BBBBB`

    Returns `None` if no non-matching elements are in the tail.
    */
    #[inline]
    #[must_use]
    pub fn resplit_tail(&self, pred: impl Fn(&T) -> bool) -> Option<Self> {
        self.tail()
            .iter()
            .position(move |b| !pred(b))
            // Safety: self.point is the length of the head, and tail_point is
            // definitely less than the length of the tail, so their sum is
            // definitely in bounds for the overall slice.
            .map(|tail_point| Self {
                slice: self.slice,
                point: self.point + tail_point,
            })
    }
}
