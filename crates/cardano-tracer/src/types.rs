//! Runtime-state types for the `cardano-tracer` binary.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-tracer/src/Cardano/Tracer/Types.hs.
//!
//! Direct ports of the upstream type aliases + newtypes that
//! describe the tracer's mutable runtime state. The deeper
//! `EKG.Store` / `MetricsLocalStore` / `DataPointRequestor` types
//! depend on upstream's `iohk-monitoring-framework` and
//! `trace-forwarder` libraries; Yggdrasil keeps those slots opaque
//! at this layer (`Box<dyn Any + Send + Sync>`-like placeholders)
//! until the trace-forwarder mini-protocol port is wired.
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`System.Metrics.EKG.Store`** + **`MetricsLocalStore`**: lands
//!   when the EKG-equivalent metrics-aggregation layer is ported
//!   (Handlers/Metrics rounds).
//! - **`Trace.Forward.Utils.DataPoint.DataPointRequestor`**: lands
//!   when the trace-forwarder mini-protocol port is wired.
//! - **`Data.Bimap.Bimap`**: replaced by [`ConnectedNodesNames`]'s
//!   forward-and-reverse `HashMap` pair below; the same bidirectional
//!   lookup contract is preserved without the `bimap` ecosystem
//!   dependency.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock};

use crate::configuration::LoggingParams;

/// Unique identifier of a connected tracer-side node, derived from
/// the upstream `remoteAddress` of the `ConnectionId` in
/// `ouroboros-network`.
///
/// Upstream: `newtype NodeId = NodeId Text`.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct NodeId(pub String);

impl NodeId {
    /// Construct from any string-like value.
    pub fn new(id: impl Into<String>) -> Self {
        NodeId(id.into())
    }

    /// Borrow the underlying string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Operator-facing name of a connected node — received via the
/// trace-forwarder's `NodeInfo` handshake.
///
/// Upstream: `type NodeName = Text`.
pub type NodeName = String;

/// Stop-signal for protocols on the acceptor's side. When set to
/// `true`, the trace acceptor's protocol loops exit at their next
/// poll point.
///
/// Upstream: `type ProtocolsBrake = TVar Bool`.
#[derive(Clone, Debug, Default)]
pub struct ProtocolsBrake(pub Arc<RwLock<bool>>);

impl ProtocolsBrake {
    /// Construct in the running state (`false`).
    pub fn new() -> Self {
        ProtocolsBrake::default()
    }

    /// Engage the brake; signal protocols to stop at next poll.
    pub fn engage(&self) {
        if let Ok(mut guard) = self.0.write() {
            *guard = true;
        }
    }

    /// Read the current brake state.
    pub fn is_engaged(&self) -> bool {
        self.0.read().map(|g| *g).unwrap_or(false)
    }
}

/// Set of currently-connected nodes by id. Serves as the canonical
/// source of truth: nodes are added on connect and removed on
/// disconnect.
///
/// Upstream: `type ConnectedNodes = TVar (Set NodeId)`.
#[derive(Clone, Debug, Default)]
pub struct ConnectedNodes(pub Arc<RwLock<HashSet<NodeId>>>);

impl ConnectedNodes {
    /// Construct an empty set.
    pub fn new() -> Self {
        ConnectedNodes::default()
    }

    /// Insert a node id; returns `true` if the id was newly inserted.
    pub fn insert(&self, id: NodeId) -> bool {
        match self.0.write() {
            Ok(mut guard) => guard.insert(id),
            Err(_) => false,
        }
    }

    /// Remove a node id; returns `true` if the id was present.
    pub fn remove(&self, id: &NodeId) -> bool {
        match self.0.write() {
            Ok(mut guard) => guard.remove(id),
            Err(_) => false,
        }
    }

    /// Return `true` if a node is currently connected.
    pub fn contains(&self, id: &NodeId) -> bool {
        self.0.read().map(|g| g.contains(id)).unwrap_or(false)
    }

    /// Snapshot the current set of connected ids.
    pub fn snapshot(&self) -> Vec<NodeId> {
        self.0
            .read()
            .map(|g| g.iter().cloned().collect())
            .unwrap_or_default()
    }
}

/// Bidirectional `NodeId` ↔ `NodeName` mapping received via the
/// trace-forwarder's `NodeInfo` handshake.
///
/// Upstream: `type ConnectedNodesNames = TVar (Bimap NodeId NodeName)`.
/// The Rust port uses two parallel `HashMap`s instead of pulling
/// in the `bimap` crate; the same bidirectional-lookup contract is
/// preserved.
#[derive(Clone, Debug, Default)]
pub struct ConnectedNodesNames {
    inner: Arc<RwLock<ConnectedNodesNamesInner>>,
}

#[derive(Clone, Debug, Default)]
struct ConnectedNodesNamesInner {
    forward: HashMap<NodeId, NodeName>,
    reverse: HashMap<NodeName, NodeId>,
}

impl ConnectedNodesNames {
    /// Construct an empty bidirectional map.
    pub fn new() -> Self {
        ConnectedNodesNames::default()
    }

    /// Insert a `NodeId` ↔ `NodeName` association. If either side
    /// already has a binding, the old binding is replaced and its
    /// counterpart is also removed from the reverse map (mirrors
    /// `Data.Bimap.insert`'s replace-both-sides semantic).
    pub fn insert(&self, id: NodeId, name: NodeName) {
        if let Ok(mut guard) = self.inner.write() {
            // Remove any pre-existing forward binding for this id
            // and clear its reverse entry.
            if let Some(old_name) = guard.forward.remove(&id) {
                guard.reverse.remove(&old_name);
            }
            // Same for any pre-existing reverse binding for this
            // name.
            if let Some(old_id) = guard.reverse.remove(&name) {
                guard.forward.remove(&old_id);
            }
            guard.forward.insert(id.clone(), name.clone());
            guard.reverse.insert(name, id);
        }
    }

    /// Look up the name associated with a node id.
    pub fn name_of(&self, id: &NodeId) -> Option<NodeName> {
        self.inner
            .read()
            .ok()
            .and_then(|g| g.forward.get(id).cloned())
    }

    /// Look up the node id associated with a name.
    pub fn id_of(&self, name: &str) -> Option<NodeId> {
        self.inner
            .read()
            .ok()
            .and_then(|g| g.reverse.get(name).cloned())
    }

    /// Remove the binding for a node id (if any). Returns the name
    /// that was bound, if there was one.
    pub fn remove_id(&self, id: &NodeId) -> Option<NodeName> {
        if let Ok(mut guard) = self.inner.write() {
            if let Some(name) = guard.forward.remove(id) {
                guard.reverse.remove(&name);
                return Some(name);
            }
        }
        None
    }

    /// Snapshot the current bindings as a `Vec<(NodeId, NodeName)>`.
    pub fn snapshot(&self) -> Vec<(NodeId, NodeName)> {
        self.inner
            .read()
            .map(|g| {
                g.forward
                    .iter()
                    .map(|(id, name)| (id.clone(), name.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Generic registry mapping `Key` → `Value` behind a mutex. Mirror
/// of upstream `newtype Registry a b = Registry { getRegistry :: MVar (Map a b) }`.
///
/// Used by the tracer's logs-handler subsystem to manage open log
/// file handles keyed by `(NodeName, LoggingParams)`. The Rust port
/// uses `Mutex<HashMap<_>>` instead of MVar/Map since Rust's
/// `HashMap` already provides the lookup-and-insert atomicity.
#[derive(Clone, Debug, Default)]
pub struct Registry<Key, Value>
where
    Key: Eq + std::hash::Hash + Clone,
    Value: Clone,
{
    inner: Arc<Mutex<HashMap<Key, Value>>>,
}

impl<Key, Value> Registry<Key, Value>
where
    Key: Eq + std::hash::Hash + Clone,
    Value: Clone,
{
    /// Construct an empty registry.
    pub fn new() -> Self {
        Registry {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Insert a binding; returns the previous value if the key was
    /// already present.
    pub fn insert(&self, key: Key, value: Value) -> Option<Value> {
        self.inner
            .lock()
            .ok()
            .and_then(|mut g| g.insert(key, value))
    }

    /// Look up a value by key.
    pub fn get(&self, key: &Key) -> Option<Value> {
        self.inner.lock().ok().and_then(|g| g.get(key).cloned())
    }

    /// Remove a binding; returns the value that was bound, if any.
    pub fn remove(&self, key: &Key) -> Option<Value> {
        self.inner.lock().ok().and_then(|mut g| g.remove(key))
    }

    /// Number of currently-registered bindings.
    pub fn len(&self) -> usize {
        self.inner.lock().map(|g| g.len()).unwrap_or(0)
    }

    /// Returns `true` when the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Snapshot the registry contents as a `Vec<(Key, Value)>`.
    /// Order is unspecified (HashMap iteration order). Used by
    /// [`crate::utils::read_registry`] / [`crate::utils::modify_registry`]
    /// to mirror upstream's `readRegistry :: Registry a b -> IO (Map.Map a b)`
    /// snapshot semantics.
    pub fn snapshot(&self) -> Vec<(Key, Value)> {
        match self.inner.lock() {
            Ok(g) => g.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            Err(_) => Vec::new(),
        }
    }
}

/// Composite key for the log-handle registry.
///
/// Upstream: `type HandleRegistryKey = (NodeName, LoggingParams)`.
pub type HandleRegistryKey = (NodeName, LoggingParams);

/// Open-file-handle registry. Keys are `(NodeName, LoggingParams)`;
/// values pair an opaque OS file handle (placeholder) with the
/// resolved file path. Mirror of upstream
/// `type HandleRegistry = Registry HandleRegistryKey (Handle, FilePath)`.
///
/// **Carve-out:** the upstream `System.IO.Handle` is replaced by
/// `()` here as a placeholder until the file-rotator round wires
/// real file-handle plumbing. Operators reading this layer should
/// expect the value to gain a real handle once the rotator lands.
pub type HandleRegistry = Registry<HandleRegistryKey, ((), std::path::PathBuf)>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::configuration::{LogFormat, LogMode};

    #[test]
    fn node_id_round_trip() {
        let id = NodeId::new("node-1");
        assert_eq!(id.as_str(), "node-1");
        assert_eq!(id.to_string(), "node-1");
    }

    #[test]
    fn node_id_ord_is_lexicographic() {
        assert!(NodeId::new("a") < NodeId::new("b"));
    }

    #[test]
    fn protocols_brake_starts_disengaged() {
        let b = ProtocolsBrake::new();
        assert!(!b.is_engaged());
    }

    #[test]
    fn protocols_brake_engage_takes_effect() {
        let b = ProtocolsBrake::new();
        b.engage();
        assert!(b.is_engaged());
    }

    #[test]
    fn connected_nodes_insert_remove() {
        let cn = ConnectedNodes::new();
        let id = NodeId::new("a");
        assert!(cn.insert(id.clone()));
        assert!(cn.contains(&id));
        // Re-insert returns false (already present).
        assert!(!cn.insert(id.clone()));
        assert!(cn.remove(&id));
        assert!(!cn.contains(&id));
        // Remove of absent returns false.
        assert!(!cn.remove(&id));
    }

    #[test]
    fn connected_nodes_snapshot_round_trip() {
        let cn = ConnectedNodes::new();
        cn.insert(NodeId::new("a"));
        cn.insert(NodeId::new("b"));
        let mut snap = cn.snapshot();
        snap.sort();
        assert_eq!(snap, vec![NodeId::new("a"), NodeId::new("b")]);
    }

    #[test]
    fn connected_nodes_names_bidirectional_lookup() {
        let cnn = ConnectedNodesNames::new();
        let id = NodeId::new("node-1");
        let name = "my-spo-node".to_string();
        cnn.insert(id.clone(), name.clone());
        assert_eq!(cnn.name_of(&id), Some(name.clone()));
        assert_eq!(cnn.id_of(&name), Some(id.clone()));
    }

    #[test]
    fn connected_nodes_names_replace_id_clears_old_name() {
        let cnn = ConnectedNodesNames::new();
        let id = NodeId::new("node-1");
        cnn.insert(id.clone(), "old-name".to_string());
        cnn.insert(id.clone(), "new-name".to_string());
        // Forward: id → new-name
        assert_eq!(cnn.name_of(&id), Some("new-name".to_string()));
        // Reverse: new-name → id; old-name → None
        assert_eq!(cnn.id_of("new-name"), Some(id.clone()));
        assert_eq!(cnn.id_of("old-name"), None);
    }

    #[test]
    fn connected_nodes_names_replace_name_clears_old_id() {
        let cnn = ConnectedNodesNames::new();
        cnn.insert(NodeId::new("a"), "shared-name".to_string());
        cnn.insert(NodeId::new("b"), "shared-name".to_string());
        // Reverse: shared-name → b
        assert_eq!(cnn.id_of("shared-name"), Some(NodeId::new("b")));
        // Forward: a → None (replaced); b → shared-name
        assert_eq!(cnn.name_of(&NodeId::new("a")), None);
        assert_eq!(
            cnn.name_of(&NodeId::new("b")),
            Some("shared-name".to_string())
        );
    }

    #[test]
    fn connected_nodes_names_remove_id_clears_both_directions() {
        let cnn = ConnectedNodesNames::new();
        cnn.insert(NodeId::new("a"), "name-a".to_string());
        cnn.insert(NodeId::new("b"), "name-b".to_string());
        let removed = cnn.remove_id(&NodeId::new("a"));
        assert_eq!(removed, Some("name-a".to_string()));
        assert_eq!(cnn.name_of(&NodeId::new("a")), None);
        assert_eq!(cnn.id_of("name-a"), None);
        // b is unaffected.
        assert_eq!(cnn.name_of(&NodeId::new("b")), Some("name-b".to_string()));
    }

    #[test]
    fn registry_insert_get_remove() {
        let r: Registry<String, i32> = Registry::new();
        assert!(r.is_empty());
        assert_eq!(r.insert("foo".to_string(), 42), None);
        assert_eq!(r.len(), 1);
        assert_eq!(r.get(&"foo".to_string()), Some(42));
        // Re-insert returns the previous value.
        assert_eq!(r.insert("foo".to_string(), 99), Some(42));
        assert_eq!(r.get(&"foo".to_string()), Some(99));
        // Remove returns the bound value.
        assert_eq!(r.remove(&"foo".to_string()), Some(99));
        assert!(r.is_empty());
    }

    #[test]
    fn handle_registry_key_round_trip() {
        let key: HandleRegistryKey = (
            "node-1".to_string(),
            LoggingParams {
                root: std::path::PathBuf::from("/var/log"),
                mode: LogMode::FileMode,
                format: LogFormat::ForMachine,
            },
        );
        let registry = HandleRegistry::new();
        let value = ((), std::path::PathBuf::from("/var/log/node-1.log"));
        registry.insert(key.clone(), value.clone());
        assert_eq!(registry.get(&key), Some(value));
    }
}
