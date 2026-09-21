pub type Record<'a> = &'a [u8];

pub trait TableIterator {
    type Item<'a>
    where
        Self: 'a;

    fn next(&mut self) -> Option<Self::Item<'_>>;
}
