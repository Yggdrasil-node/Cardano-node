//! FIFO queue used by the transaction generator.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/TxGenerator/Internal/Fifo.hs`.
//! Ports the paired-list FIFO operations used by `FundQueue`.

use std::collections::VecDeque;

/// Mirror of upstream `Fifo a`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Fifo<T> {
    front: VecDeque<T>,
    rear: Vec<T>,
}

impl<T> Default for Fifo<T> {
    fn default() -> Self {
        Self::empty_fifo()
    }
}

impl<T> Fifo<T> {
    /// Mirror of upstream `emptyFifo`.
    pub fn empty_fifo() -> Self {
        Self {
            front: VecDeque::new(),
            rear: Vec::new(),
        }
    }

    /// Mirror of upstream `insert`.
    pub fn insert(mut self, item: T) -> Self {
        self.rear.insert(0, item);
        self
    }

    /// Mirror of upstream `remove`.
    pub fn remove(mut self) -> Option<(Self, T)> {
        let item = self.remove_front()?;
        Some((self, item))
    }

    /// Mirror of upstream `removeN`.
    pub fn remove_n(mut self, count: usize) -> Option<(Self, Vec<T>)> {
        let mut removed = Vec::with_capacity(count);
        for _ in 0..count {
            removed.push(self.remove_front()?);
        }
        Some((self, removed))
    }

    fn remove_front(&mut self) -> Option<T> {
        if self.front.is_empty() {
            while let Some(item) = self.rear.pop() {
                self.front.push_back(item);
            }
        }
        self.front.pop_front()
    }
}

impl<T: Clone> Fifo<T> {
    /// Mirror of upstream `toList`.
    pub fn to_list(&self) -> Vec<T> {
        self.front
            .iter()
            .cloned()
            .chain(self.rear.iter().rev().cloned())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_to_list_preserve_arrival_order() {
        let queue = Fifo::empty_fifo().insert(1).insert(2).insert(3);

        assert_eq!(queue.to_list(), vec![1, 2, 3]);
    }

    #[test]
    fn remove_dequeues_front_then_reverses_rear() {
        let queue = Fifo::empty_fifo().insert("a").insert("b");
        let (queue, first) = queue.remove().expect("first");
        let (queue, second) = queue.remove().expect("second");

        assert_eq!(first, "a");
        assert_eq!(second, "b");
        assert!(queue.remove().is_none());
    }

    #[test]
    fn remove_n_fails_when_queue_is_too_short() {
        let queue = Fifo::empty_fifo().insert(1);

        assert!(queue.remove_n(2).is_none());
    }
}
