/// A minimal BlockFetch state machine surface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlockFetchState {
    Idle,
    Requesting,
    Streaming,
}
