//! Metrics-server utilities — Content-Type constants + route-table
//! type + JSON renderer for the connected-nodes index.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-tracer/src/Cardano/Tracer/Handlers/Metrics/Utils.hs.
//!
//! Direct port of upstream's metrics-server utility module —
//! bounded subset. This round ships the pure helpers that don't
//! depend on the unported `TracerEnv` 14-field record or the
//! Text.Blaze HTML rendering layer.
//!
//! Mapping summary:
//!
//! | Upstream                                                       | Yggdrasil                              |
//! |----------------------------------------------------------------|----------------------------------------|
//! | `newtype RouteDictionary`                                      | [`RouteDictionary`]                    |
//! | `nodeNames :: RouteDictionary -> [NodeName]`                   | [`RouteDictionary::node_names`]        |
//! | `renderJson :: RouteDictionary -> Lazy.ByteString`             | [`RouteDictionary::render_json`]       |
//! | `slugify :: Text -> Text` (via Text.Slugify)                   | [`slugify`]                            |
//! | `contentHdrJSON`                                               | [`CONTENT_HDR_JSON`]                   |
//! | `contentHdrOpenMetrics`                                        | [`CONTENT_HDR_OPEN_METRICS`]           |
//! | `contentHdrUtf8Html`                                           | [`CONTENT_HDR_UTF8_HTML`]              |
//! | `contentHdrUtf8Text`                                           | [`CONTENT_HDR_UTF8_TEXT`]              |
//! | `contentHdrPrometheus`                                         | [`CONTENT_HDR_PROMETHEUS`]             |
//! | `computeRoutes :: TracerEnv -> IO RouteDictionary`             | (deferred — see [`compute_routes_status`]) |
//! | `renderListOfConnectedNodes`                                   | [`RouteDictionary::render_html`]       |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`computeRoutes`**: depends on the unported `TracerEnv`
//!   14-field record (R383+ remaining-work item) — specifically
//!   `teConnectedNodesNames` + `teAcceptedMetrics` TVars. Once
//!   `TracerEnv` ports, the function lands as a thin wrapper that
//!   plucks the two fields and calls a [`RouteDictionary`]
//!   constructor that takes maps directly.
//! - **`Text.Blaze.Html`-rendered `renderListOfConnectedNodes`**:
//!   landed at R406 with the maud 0.27 workspace dep. The
//!   [`RouteDictionary::render_html`] renderer auto-escapes user
//!   content + matches upstream's empty-dictionary
//!   "There are no connected nodes yet." short-circuit verbatim.
//! - **`Network.HTTP.Types.ResponseHeaders`** (`[(HeaderName,
//!   ByteString)]` representation): the Rust port emits each
//!   constant as a tuple `(&'static str, &'static str)` of
//!   `(name, value)` rather than building a full `ResponseHeaders`
//!   list. Callers that need an axum / hyper `HeaderMap` can wrap
//!   these tuples at use site.
//! - **`System.Metrics.Store`**: the EKG store type is unported.
//!   The Rust [`RouteDictionary`] keeps the slug + node-name pair
//!   without the metrics-store handle; once the EKG-equivalent
//!   metrics surface lands the dictionary will gain the third
//!   element matching upstream's `(slug, (Store, NodeName))`
//!   shape.

use std::collections::BTreeMap;

use crate::types::NodeName;

/// Content-Type header for a JSON response. Mirror of upstream
/// `contentHdrJSON = [(hContentType, "application/json")]`.
pub const CONTENT_HDR_JSON: (&str, &str) = ("Content-Type", "application/json");

/// Content-Type header for an OpenMetrics 1.0.0 response. Mirror
/// of upstream `contentHdrOpenMetrics`.
pub const CONTENT_HDR_OPEN_METRICS: (&str, &str) = (
    "Content-Type",
    "application/openmetrics-text;version=1.0.0;charset=utf-8",
);

/// Content-Type header for a UTF-8 HTML response. Mirror of
/// upstream `contentHdrUtf8Html`.
pub const CONTENT_HDR_UTF8_HTML: (&str, &str) = ("Content-Type", "text/html;charset=utf-8");

/// Content-Type header for a UTF-8 plain-text response. Mirror of
/// upstream `contentHdrUtf8Text`.
pub const CONTENT_HDR_UTF8_TEXT: (&str, &str) = ("Content-Type", "text/plain;charset=utf-8");

/// Content-Type header for a Prometheus 0.0.4 plain-text response.
/// Mirror of upstream `contentHdrPrometheus`.
pub const CONTENT_HDR_PROMETHEUS: (&str, &str) =
    ("Content-Type", "text/plain;version=0.0.4;charset=utf-8");

/// Per-node URL routing table emitted by the metrics servers.
/// Mirror of upstream
/// `newtype RouteDictionary = RouteDictionary { getRouteDictionary :: [(Text, (EKG.Store, NodeName))] }`.
///
/// Yggdrasil drops the `EKG.Store` element pending a workspace EKG
/// equivalent (carve-out documented in the module docstring). The
/// pair shape preserves slug + node-name so downstream sites can
/// build URL routes (`/<slug>` → metrics for `node-name`).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RouteDictionary {
    /// Slug → node-name mapping. Each pair is `(slug, node_name)`.
    /// Order is preserved per upstream's `[(Text, _)]` list shape.
    pub get_route_dictionary: Vec<(String, NodeName)>,
}

impl RouteDictionary {
    /// Construct from a slug→node-name pair list. Mirror of
    /// upstream's record-syntax constructor.
    pub fn new(routes: Vec<(String, NodeName)>) -> Self {
        RouteDictionary {
            get_route_dictionary: routes,
        }
    }

    /// Extract the node-names. Mirror of upstream
    /// `nodeNames (RouteDictionary routeDict) = map (snd . snd) routeDict`.
    /// Note: upstream's tuple is `(slug, (Store, NodeName))`, so
    /// `snd . snd` yields the node-name; Yggdrasil's tuple is
    /// `(slug, NodeName)`, so it yields the second element directly.
    pub fn node_names(&self) -> Vec<NodeName> {
        self.get_route_dictionary
            .iter()
            .map(|(_, name)| name.clone())
            .collect()
    }

    /// Render the dictionary as a JSON object mapping
    /// `node_name → "/slug"`. Mirror of upstream
    /// `renderJson (RouteDictionary routeDict) = encode do
    /// Map.fromList [...]`.
    ///
    /// Returns a JSON-encoded byte vector — uses BTreeMap for
    /// deterministic key ordering (matches upstream's
    /// Data.Map.fromList semantics, which produces a sorted map).
    pub fn render_json(&self) -> Vec<u8> {
        let map: BTreeMap<String, String> = self
            .get_route_dictionary
            .iter()
            .map(|(slug, name)| (name.clone(), format!("/{slug}")))
            .collect();
        serde_json::to_vec(&map).unwrap_or_default()
    }

    /// Render the dictionary as an HTML index page listing each
    /// connected node with a link to its per-node metrics route.
    /// Mirror of upstream
    /// `renderListOfConnectedNodes :: Text -> RouteDictionary -> Lazy.ByteString`.
    ///
    /// `metrics_title` is the page title (rendered in `<title>`).
    /// Returns the page as a byte vector. When the dictionary is
    /// empty, returns the canonical "no nodes yet" message verbatim
    /// matching upstream's
    /// `"There are no connected nodes yet."` short-circuit.
    pub fn render_html(&self, metrics_title: &str) -> Vec<u8> {
        if self.get_route_dictionary.is_empty() {
            return b"There are no connected nodes yet.".to_vec();
        }
        let names: Vec<String> = self
            .get_route_dictionary
            .iter()
            .map(|(_, name)| name.clone())
            .collect();
        let page = maud::html! {
            html {
                head {
                    title { (metrics_title) }
                }
                body {
                    ul {
                        @for name in &names {
                            li {
                                a href={ "/" (slugify(name)) } { (name) }
                            }
                        }
                    }
                }
            }
        };
        page.into_string().into_bytes()
    }
}

/// URL-friendly slug from a free-form node-name. Mirror of upstream's
/// `Text.Slugify.slugify` import — implemented inline since the
/// upstream `text-slugify` crate has no Rust analog (and the rule
/// is small enough to ship without a dependency).
///
/// Lowercases ASCII letters, replaces runs of non-alphanumeric
/// characters with `-`, and trims leading/trailing `-`. Non-ASCII
/// characters are dropped (upstream's `slugify` does the same with
/// its default settings).
pub fn slugify(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev_was_dash = true; // suppress leading dashes
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_was_dash = false;
        } else if !prev_was_dash {
            out.push('-');
            prev_was_dash = true;
        }
    }
    // Strip trailing dash if any.
    if out.ends_with('-') {
        out.pop();
    }
    out
}

/// Status descriptor for the carve-out `computeRoutes` entry-point.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ComputeRoutesStatus {
    /// One-line summary of the deferral.
    pub status: &'static str,
    /// Reason — references the missing upstream port.
    pub depends_on: &'static str,
    /// Round-number marker for tracking the deferred work.
    pub deferred_round: &'static str,
}

/// Get the deferral-status descriptor for `computeRoutes`.
pub fn compute_routes_status() -> ComputeRoutesStatus {
    ComputeRoutesStatus {
        status: "deferred",
        depends_on: "TracerEnv 14-field record (teConnectedNodesNames + teAcceptedMetrics TVars); EKG.Store equivalent",
        deferred_round: "R392+",
    }
}

/// Status descriptor for the previously-carved-out
/// `renderListOfConnectedNodes` HTML renderer. Closed at R406 with
/// the maud 0.27 dep land + [`RouteDictionary::render_html`].
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct RenderHtmlStatus {
    /// One-line summary of the closure status.
    pub status: &'static str,
    /// Round at which the renderer landed.
    pub closed_at_round: &'static str,
}

/// Get the closure-status descriptor for the HTML renderer. R406
/// closes the carve-out: the actual renderer is
/// [`RouteDictionary::render_html`].
pub fn render_html_status() -> RenderHtmlStatus {
    RenderHtmlStatus {
        status: "closed at R406",
        closed_at_round: "R406",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_hdr_json_matches_upstream() {
        assert_eq!(CONTENT_HDR_JSON.0, "Content-Type");
        assert_eq!(CONTENT_HDR_JSON.1, "application/json");
    }

    #[test]
    fn content_hdr_open_metrics_matches_upstream() {
        assert_eq!(
            CONTENT_HDR_OPEN_METRICS.1,
            "application/openmetrics-text;version=1.0.0;charset=utf-8",
        );
    }

    #[test]
    fn content_hdr_utf8_html_matches_upstream() {
        assert_eq!(CONTENT_HDR_UTF8_HTML.1, "text/html;charset=utf-8");
    }

    #[test]
    fn content_hdr_utf8_text_matches_upstream() {
        assert_eq!(CONTENT_HDR_UTF8_TEXT.1, "text/plain;charset=utf-8");
    }

    #[test]
    fn content_hdr_prometheus_matches_upstream() {
        assert_eq!(
            CONTENT_HDR_PROMETHEUS.1,
            "text/plain;version=0.0.4;charset=utf-8",
        );
    }

    #[test]
    fn route_dictionary_default_is_empty() {
        let rd = RouteDictionary::default();
        assert!(rd.get_route_dictionary.is_empty());
        assert!(rd.node_names().is_empty());
    }

    #[test]
    fn route_dictionary_new_round_trip() {
        let rd = RouteDictionary::new(vec![
            ("alpha".to_string(), "alpha-pool".to_string()),
            ("beta".to_string(), "beta-pool".to_string()),
        ]);
        assert_eq!(rd.get_route_dictionary.len(), 2);
        assert_eq!(rd.node_names(), vec!["alpha-pool", "beta-pool"]);
    }

    #[test]
    fn render_json_emits_node_to_route_mapping() {
        let rd = RouteDictionary::new(vec![
            ("alpha".to_string(), "alpha-pool".to_string()),
            ("beta".to_string(), "beta-pool".to_string()),
        ]);
        let json = rd.render_json();
        let json_str = String::from_utf8(json).expect("utf8");
        // BTreeMap → sorted key order (alpha-pool first).
        let value: serde_json::Value = serde_json::from_str(&json_str).expect("parses");
        assert_eq!(value["alpha-pool"], "/alpha");
        assert_eq!(value["beta-pool"], "/beta");
    }

    #[test]
    fn render_json_for_empty_dictionary_emits_empty_object() {
        let rd = RouteDictionary::default();
        let json = rd.render_json();
        let json_str = String::from_utf8(json).expect("utf8");
        assert_eq!(json_str, "{}");
    }

    #[test]
    fn render_json_keys_sorted_alphabetically() {
        let rd = RouteDictionary::new(vec![
            ("zulu".to_string(), "zulu-pool".to_string()),
            ("alpha".to_string(), "alpha-pool".to_string()),
            ("mike".to_string(), "mike-pool".to_string()),
        ]);
        let json = String::from_utf8(rd.render_json()).expect("utf8");
        // Verify alphabetical key order in the serialized JSON.
        let alpha_pos = json.find("alpha-pool").expect("has alpha-pool");
        let mike_pos = json.find("mike-pool").expect("has mike-pool");
        let zulu_pos = json.find("zulu-pool").expect("has zulu-pool");
        assert!(alpha_pos < mike_pos);
        assert!(mike_pos < zulu_pos);
    }

    #[test]
    fn slugify_lowercases_ascii_alphanumeric() {
        assert_eq!(slugify("FooBar123"), "foobar123");
    }

    #[test]
    fn slugify_replaces_spaces_with_dashes() {
        assert_eq!(slugify("Hello World"), "hello-world");
    }

    #[test]
    fn slugify_collapses_runs_of_non_alphanumeric() {
        assert_eq!(slugify("foo!!!bar"), "foo-bar");
        assert_eq!(slugify("foo___bar"), "foo-bar");
    }

    #[test]
    fn slugify_strips_leading_and_trailing_dashes() {
        assert_eq!(slugify("-foo-"), "foo");
        assert_eq!(slugify("  foo  "), "foo");
        assert_eq!(slugify("!!!foo!!!"), "foo");
    }

    #[test]
    fn slugify_drops_non_ascii_characters() {
        // Non-ASCII chars become dashes (they're not alphanumeric
        // in ASCII), then collapsed.
        assert_eq!(slugify("café"), "caf");
        assert_eq!(slugify("ümlaut"), "mlaut");
    }

    #[test]
    fn slugify_empty_string() {
        assert_eq!(slugify(""), "");
    }

    #[test]
    fn slugify_only_punctuation_yields_empty() {
        assert_eq!(slugify("!!!"), "");
        assert_eq!(slugify("---"), "");
    }

    #[test]
    fn compute_routes_status_describes_deferral() {
        let s = compute_routes_status();
        assert_eq!(s.status, "deferred");
        assert!(s.depends_on.contains("TracerEnv"));
        assert_eq!(s.deferred_round, "R392+");
    }

    #[test]
    fn render_html_status_describes_closure() {
        let s = render_html_status();
        assert_eq!(s.status, "closed at R406");
        assert_eq!(s.closed_at_round, "R406");
    }

    #[test]
    fn render_html_empty_dictionary_returns_no_nodes_message() {
        let rd = RouteDictionary::default();
        let html = rd.render_html("Yggdrasil Tracer");
        let s = String::from_utf8(html).expect("utf8");
        assert_eq!(s, "There are no connected nodes yet.");
    }

    #[test]
    fn render_html_with_one_node_emits_canonical_html_page() {
        let rd = RouteDictionary::new(vec![("alpha".to_string(), "alpha-pool".to_string())]);
        let html = rd.render_html("Yggdrasil Tracer");
        let s = String::from_utf8(html).expect("utf8");
        // Title + body + per-node link.
        assert!(s.contains("<title>Yggdrasil Tracer</title>"));
        assert!(s.contains("<a href=\"/alpha-pool\">alpha-pool</a>"));
    }

    #[test]
    fn render_html_with_multiple_nodes_emits_each_link() {
        let rd = RouteDictionary::new(vec![
            ("alpha".to_string(), "alpha-pool".to_string()),
            ("beta".to_string(), "beta pool!".to_string()),
        ]);
        let html = rd.render_html("Yggdrasil Tracer");
        let s = String::from_utf8(html).expect("utf8");
        assert!(s.contains("alpha-pool"));
        assert!(s.contains("beta pool!"));
        // The slug for the link uses slugify (so "beta pool!" → "beta-pool").
        assert!(s.contains("/beta-pool"));
    }

    #[test]
    fn render_html_escapes_user_supplied_node_names() {
        // maud auto-escapes content; verify a script-tagged name doesn't
        // produce raw <script>.
        let rd = RouteDictionary::new(vec![(
            "x".to_string(),
            "<script>alert(1)</script>".to_string(),
        )]);
        let html = rd.render_html("Title");
        let s = String::from_utf8(html).expect("utf8");
        assert!(!s.contains("<script>alert(1)</script>"));
        assert!(s.contains("&lt;script&gt;"));
    }
}
