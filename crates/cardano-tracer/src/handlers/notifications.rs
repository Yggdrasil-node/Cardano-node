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
//! | `Notifications/Check.hs`                            | `check.rs`               |
//! | `Notifications/Settings.hs`                         | `settings.rs`            |
//! | `Notifications/Timer.hs`                            | `timer.rs`               |
//! | `Notifications/Send.hs`                             | `send.rs` (pending)      |
//! | `Notifications/Email.hs`                            | `email.rs` (pending)     |
//! | `Notifications/Utils.hs`                            | `utils.rs`               |

pub mod check;
pub mod settings;
pub mod timer;
pub mod types;
pub mod utils;
