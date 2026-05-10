//! Notification subsystem — parent shell for the
//! `handlers/notifications/` sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none.
//!
//! Yggdrasil-side parent shell for the
//! `Cardano.Tracer.Handlers.Notifications.*` namespace. The upstream
//! namespace has no aggregate `Notifications.hs` file — it's a
//! directory of peer leaves (`Types.hs`, `Check.hs`, `Send.hs`,
//! `Email.hs`, `Settings.hs`, `Timer.hs`, `Utils.hs`) — so this
//! file exists only to declare the sub-modules below it.
//!
//! Layout mapping:
//!
//! | Upstream                                            | Yggdrasil                |
//! |-----------------------------------------------------|--------------------------|
//! | `Notifications/Types.hs`                            | `types.rs`               |
//! | `Notifications/Check.hs`                            | `check.rs` (pending)     |
//! | `Notifications/Settings.hs`                         | `settings.rs` (pending)  |
//! | `Notifications/Timer.hs`                            | `timer.rs` (pending)     |
//! | `Notifications/Send.hs`                             | `send.rs` (pending)      |
//! | `Notifications/Email.hs`                            | `email.rs` (pending)     |
//! | `Notifications/Utils.hs`                            | `utils.rs` (pending)     |

pub mod types;
