#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChainSyncState {
    Idle,
    AwaitReply,
    RollForward,
    RollBackward,
}
