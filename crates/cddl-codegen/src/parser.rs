use thiserror::Error;

/// A named CDDL type definition parsed from the supported schema subset.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedType {
    pub name: String,
    pub definition: TypeDefinition,
}

/// The supported CDDL shapes understood by the current parser.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypeDefinition {
    /// A type alias: `name = type_expr`
    Alias(TypeExpr),
    /// A fixed-length array/tuple: `name = [field1, field2, ...]`
    Array(Vec<ArrayItem>),
    /// A map/record: `name = { key: type, ... }`
    Map(Vec<ParsedField>),
    /// A group choice (sum type): `name = [tag, ...] // [tag, ...] // ...`
    ///
    /// Each variant is an array alternative separated by `//`.
    ///
    /// Reference: RFC 8610 §2.2 — group choices.
    GroupChoice(Vec<Vec<ArrayItem>>),
}

/// A parsed type expression, capturing the base type and optional constraints.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypeExpr {
    /// A plain named type reference or builtin: `uint`, `bytes`, `hash32`.
    Named(String),
    /// A size-constrained type: `uint .size 4`, `bytes .size 32`.
    Sized(String, u64),
    /// A variable-size-range constrained type: `bytes .size 0..64`,
    /// `text .size 0..128`.  Reference: RFC 8610 §3.8.1 — `.size`.
    SizeRange(String, RangeBound),
    /// A value-range constrained type: `uint .le 65535`, `uint .ge 1`,
    /// `uint .lt 100`, `uint .gt 0`.  Reference: RFC 8610 §3.8 — control
    /// operators `.le`, `.ge`, `.lt`, `.gt`.
    ValueRange(String, RangeBound),
    /// A variable-length sequence: `[* element_type]`.
    VarArray(Box<TypeExpr>),
    /// An optional alternative with nil: `type / nil`.
    Optional(Box<TypeExpr>),
    /// A CBOR-tagged type: `#6.N(inner_type)`.
    ///
    /// Reference: RFC 8949 §3.4 — CBOR tags.
    Tagged(u64, Box<TypeExpr>),
}

/// A range bound carried by [`TypeExpr::SizeRange`] and [`TypeExpr::ValueRange`].
///
/// Mirrors the bound shapes expressible in CDDL control operators: closed
/// ranges (`N..M`), open ranges (`N..` and `..M`), and the strict variants of
/// the inequality operators (`.lt`, `.gt`).  An exact-equal constraint is
/// already represented by [`TypeExpr::Sized`] and is kept distinct so existing
/// fixed-size fast paths (e.g. `bytes .size 32` → `[u8; 32]`) remain
/// bit-identical.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RangeBound {
    /// `N..` or `.ge N` — value/length must be ≥ N.
    AtLeast(u64),
    /// `..N` or `.le N` — value/length must be ≤ N.
    AtMost(u64),
    /// `N..M` — value/length must be in `N..=M`.
    Between(u64, u64),
    /// `.lt N` — value must be strictly less than N.
    StrictlyLess(u64),
    /// `.gt N` — value must be strictly greater than N.
    StrictlyGreater(u64),
}

/// Discriminator for `.size` / `.le` / `.ge` / `.lt` / `.gt` constraint kinds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConstraintKind {
    Size,
    Le,
    Ge,
    Lt,
    Gt,
}

/// An item in a CDDL array definition, which may be named or positional.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArrayItem {
    /// Field name if provided (`name: type`), otherwise `None` for positional.
    pub name: Option<String>,
    pub ty: TypeExpr,
}

/// The key used for a map field—either a string label or an integer index.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FieldKey {
    /// A string-labeled field: `fee: uint`.
    Label(String),
    /// An integer-keyed field: `0: type` (CBOR map with integer keys).
    Index(u64),
}

/// A parsed map field within a CDDL map definition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedField {
    pub key: FieldKey,
    pub ty: TypeExpr,
    pub optional: bool,
}

/// Errors surfaced while parsing the supported CDDL subset.
#[derive(Debug, Error, Eq, PartialEq)]
pub enum ParseError {
    #[error("schema input is empty")]
    Empty,
    #[error("definition is missing '=': {0}")]
    MissingAssignment(String),
    #[error("type name is invalid: {0}")]
    InvalidTypeName(String),
    #[error("type definition is empty for: {0}")]
    EmptyDefinition(String),
    #[error("map field is invalid: {0}")]
    InvalidField(String),
    #[error("invalid size constraint: {0}")]
    InvalidSize(String),
    #[error("invalid range or inequality constraint: {0}")]
    InvalidConstraint(String),
}

/// Parses a restricted, deterministic subset of CDDL into named definitions.
///
/// Supported constructs:
/// - Comments (`;`), multi-line definitions via nesting depth tracking.
/// - Aliases: `name = type_expr`
/// - Flat arrays: `name = [item1, item2, ...]` with optional named fields.
/// - Maps: `name = { key: type, ... }` with string or integer keys.
/// - Size constraints: `uint .size N`, `bytes .size N`.
/// - Size ranges: `bytes .size N..M`, `text .size N..M`, `bytes .size N..`,
///   `bytes .size ..M` (RFC 8610 §3.8.1).
/// - Inequality constraints: `uint .le N`, `uint .ge N`, `uint .lt N`,
///   `uint .gt N` (RFC 8610 §3.8.4–§3.8.7).
/// - Variable-length arrays: `[* type]`.
/// - Optional fields: `? key: type`.
/// - Nil alternatives: `type / nil`.
pub fn parse_schema(schema: &str) -> Result<Vec<ParsedType>, ParseError> {
    let mut parsed = Vec::new();
    let statements = collect_statements(schema);

    for line in statements {
        let line = line.trim();

        let (name, definition) = line
            .split_once('=')
            .ok_or_else(|| ParseError::MissingAssignment(line.to_string()))?;
        let name = name.trim();
        let definition = definition.trim();

        if !is_valid_type_name(name) {
            return Err(ParseError::InvalidTypeName(name.to_string()));
        }

        if definition.is_empty() {
            return Err(ParseError::EmptyDefinition(name.to_string()));
        }

        parsed.push(ParsedType {
            name: name.to_string(),
            definition: parse_definition(definition)?,
        });
    }

    if parsed.is_empty() {
        return Err(ParseError::Empty);
    }

    Ok(parsed)
}

/// Collects logical CDDL statements from raw source, joining multi-line
/// definitions by tracking brace/bracket nesting depth and group-choice
/// continuation (`//`).
fn collect_statements(schema: &str) -> Vec<String> {
    // First pass: strip comments and collect non-empty lines.
    let lines: Vec<&str> = schema
        .lines()
        .map(|l| strip_comment(l).trim())
        .filter(|l| !l.is_empty())
        .collect();

    let mut statements = Vec::new();
    let mut current = String::new();
    let mut nesting_depth = 0_i32;

    for (i, &line) in lines.iter().enumerate() {
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(line);
        nesting_depth += nesting_delta(line);

        // Check if the next non-empty line starts with `//` (group-choice
        // continuation). If so, don't flush yet.
        let next_is_continuation = lines.get(i + 1).is_some_and(|next| next.starts_with("//"));

        if nesting_depth <= 0 && !current.trim().ends_with('=') && !next_is_continuation {
            statements.push(current.trim().to_string());
            current.clear();
            nesting_depth = 0;
        }
    }

    if !current.trim().is_empty() {
        statements.push(current.trim().to_string());
    }

    statements
}

fn nesting_delta(line: &str) -> i32 {
    line.chars().fold(0_i32, |depth, ch| {
        depth
            + match ch {
                '{' | '[' => 1,
                '}' | ']' => -1,
                _ => 0,
            }
    })
}

fn parse_definition(definition: &str) -> Result<TypeDefinition, ParseError> {
    if definition.starts_with('{') && definition.ends_with('}') {
        return parse_map(definition);
    }

    // Group choice: `[a, b] // [c, d] // ...`
    // Must check before single array.
    if definition.contains("//") {
        return parse_group_choice(definition);
    }

    if definition.starts_with('[') && definition.ends_with(']') {
        let inner = definition[1..definition.len() - 1].trim();
        // [* type] is a variable-length array type, not a fixed-position tuple.
        if inner.starts_with('*') {
            return Ok(TypeDefinition::Alias(parse_type_expr(definition)?));
        }
        return parse_array(inner);
    }

    Ok(TypeDefinition::Alias(parse_type_expr(definition)?))
}

/// Parses a type expression string into a `TypeExpr`.
///
/// Handles: plain names, `.size N` constraints, `.size N..M` size ranges,
/// `.le`/`.ge`/`.lt`/`.gt` inequality constraints, `[* type]` var-arrays,
/// and `type / nil` optionals.
fn parse_type_expr(expr: &str) -> Result<TypeExpr, ParseError> {
    let expr = expr.trim();

    // CBOR tag annotation: #6.N(inner_type)
    if let Some(rest) = expr.strip_prefix("#6.") {
        return parse_tagged_type(rest);
    }

    // Variable-length array: [* type]
    if expr.starts_with('[') && expr.ends_with(']') {
        let inner = expr[1..expr.len() - 1].trim();
        if let Some(rest) = inner.strip_prefix('*') {
            let element = parse_type_expr(rest.trim())?;
            return Ok(TypeExpr::VarArray(Box::new(element)));
        }
    }

    // Nil alternative: type / nil
    if let Some((left, right)) = split_nil_alternative(expr) {
        if right == "nil" {
            return Ok(TypeExpr::Optional(Box::new(parse_type_expr(left)?)));
        }
    }

    // Generalized constraint dispatch: .size N, .size N..M, .le N, .ge N,
    // .lt N, .gt N.  `.size` is dispatched as either Sized (single integer,
    // fast path) or SizeRange.  Inequality operators always emit ValueRange.
    if let Some((base, kind, value_str)) = split_constraint(expr) {
        return parse_constraint(expr, base, kind, value_str);
    }

    Ok(TypeExpr::Named(expr.to_string()))
}

/// Builds a [`TypeExpr`] for a parsed constraint kind + value-string pair.
fn parse_constraint(
    full_expr: &str,
    base: &str,
    kind: ConstraintKind,
    value_str: &str,
) -> Result<TypeExpr, ParseError> {
    match kind {
        ConstraintKind::Size => {
            // Fast path: single integer maps to Sized so the existing
            // [u8; N] / uN fixed-bit decode path stays bit-identical.
            if let Ok(n) = value_str.parse::<u64>() {
                return Ok(TypeExpr::Sized(base.to_string(), n));
            }
            // Otherwise parse as a range bound (`N..M`, `N..`, `..M`).
            let bound = parse_range_bound(value_str)
                .map_err(|_| ParseError::InvalidSize(full_expr.to_string()))?;
            Ok(TypeExpr::SizeRange(base.to_string(), bound))
        }
        ConstraintKind::Le => {
            let n = value_str
                .parse::<u64>()
                .map_err(|_| ParseError::InvalidConstraint(full_expr.to_string()))?;
            Ok(TypeExpr::ValueRange(
                base.to_string(),
                RangeBound::AtMost(n),
            ))
        }
        ConstraintKind::Ge => {
            let n = value_str
                .parse::<u64>()
                .map_err(|_| ParseError::InvalidConstraint(full_expr.to_string()))?;
            Ok(TypeExpr::ValueRange(
                base.to_string(),
                RangeBound::AtLeast(n),
            ))
        }
        ConstraintKind::Lt => {
            let n = value_str
                .parse::<u64>()
                .map_err(|_| ParseError::InvalidConstraint(full_expr.to_string()))?;
            Ok(TypeExpr::ValueRange(
                base.to_string(),
                RangeBound::StrictlyLess(n),
            ))
        }
        ConstraintKind::Gt => {
            let n = value_str
                .parse::<u64>()
                .map_err(|_| ParseError::InvalidConstraint(full_expr.to_string()))?;
            Ok(TypeExpr::ValueRange(
                base.to_string(),
                RangeBound::StrictlyGreater(n),
            ))
        }
    }
}

/// Parses the remainder after `#6.` — expects `N(inner_type)`.
fn parse_tagged_type(rest: &str) -> Result<TypeExpr, ParseError> {
    let open = rest
        .find('(')
        .ok_or_else(|| ParseError::InvalidSize(format!("#6.{rest}")))?;
    if !rest.ends_with(')') {
        return Err(ParseError::InvalidSize(format!("#6.{rest}")));
    }
    let tag_str = &rest[..open];
    let tag: u64 = tag_str
        .parse()
        .map_err(|_| ParseError::InvalidSize(format!("#6.{rest}")))?;
    let inner_str = &rest[open + 1..rest.len() - 1];
    let inner = parse_type_expr(inner_str)?;
    Ok(TypeExpr::Tagged(tag, Box::new(inner)))
}

/// Splits `type / nil` alternatives. Only recognizes the simple case
/// of exactly one `/` with `nil` on one side.
fn split_nil_alternative(expr: &str) -> Option<(&str, &str)> {
    let idx = expr.find('/')?;
    let left = expr[..idx].trim();
    let right = expr[idx + 1..].trim();
    if left.is_empty() || right.is_empty() {
        return None;
    }
    Some((left, right))
}

/// Splits a constrained type expression into `(base, kind, value_string)`.
///
/// Recognised constraint kinds: `.size`, `.le`, `.ge`, `.lt`, `.gt`.  The
/// search is left-to-right and stops at the first hit.  Constraint operator
/// boundaries are detected by looking for the leading `.` plus a known
/// keyword followed by whitespace or end-of-string, so identifiers like
/// `mySize` or `belt` cannot collide.
fn split_constraint(expr: &str) -> Option<(&str, ConstraintKind, &str)> {
    const OPERATORS: &[(&str, ConstraintKind)] = &[
        (".size", ConstraintKind::Size),
        (".le", ConstraintKind::Le),
        (".ge", ConstraintKind::Ge),
        (".lt", ConstraintKind::Lt),
        (".gt", ConstraintKind::Gt),
    ];

    let bytes = expr.as_bytes();
    let mut best: Option<(usize, usize, ConstraintKind)> = None;
    for (op, kind) in OPERATORS {
        let mut search_from = 0usize;
        while let Some(rel) = expr[search_from..].find(op) {
            let idx = search_from + rel;
            let end = idx + op.len();
            // Operator must be followed by whitespace or end-of-string,
            // otherwise it's part of an identifier (e.g. `.size` vs `.sized`).
            let next_ok = end == bytes.len() || bytes[end].is_ascii_whitespace();
            // And must start at the beginning or after whitespace, so it
            // can't be embedded mid-identifier (e.g. `foo.size` where `foo.`
            // is not actually allowed CDDL but we guard for safety).
            let prev_ok = idx == 0 || bytes[idx - 1].is_ascii_whitespace();
            if next_ok && prev_ok {
                if best.is_none_or(|(b, _, _)| idx < b) {
                    best = Some((idx, end, *kind));
                }
                break;
            }
            search_from = end;
        }
    }

    let (idx, end, kind) = best?;
    let base = expr[..idx].trim();
    let value_str = expr[end..].trim();
    if base.is_empty() || value_str.is_empty() {
        return None;
    }
    Some((base, kind, value_str))
}

/// Parses a CDDL range-bound expression: `N`, `N..M`, `N..`, `..M`.
///
/// A bare `N` is intentionally rejected — the caller (`parse_constraint`)
/// dispatches single integers to [`TypeExpr::Sized`] before falling back
/// to this helper.
fn parse_range_bound(s: &str) -> Result<RangeBound, ParseError> {
    let s = s.trim();
    // Strip parens if present: `(0..128)` → `0..128`.
    let s = s.strip_prefix('(').unwrap_or(s);
    let s = s.strip_suffix(')').unwrap_or(s);
    let s = s.trim();

    if let Some((lo, hi)) = s.split_once("..") {
        let lo = lo.trim();
        let hi = hi.trim();
        match (lo.is_empty(), hi.is_empty()) {
            (true, true) => Err(ParseError::InvalidConstraint(s.to_string())),
            (false, true) => {
                let n = lo
                    .parse::<u64>()
                    .map_err(|_| ParseError::InvalidConstraint(s.to_string()))?;
                Ok(RangeBound::AtLeast(n))
            }
            (true, false) => {
                let n = hi
                    .parse::<u64>()
                    .map_err(|_| ParseError::InvalidConstraint(s.to_string()))?;
                Ok(RangeBound::AtMost(n))
            }
            (false, false) => {
                let n = lo
                    .parse::<u64>()
                    .map_err(|_| ParseError::InvalidConstraint(s.to_string()))?;
                let m = hi
                    .parse::<u64>()
                    .map_err(|_| ParseError::InvalidConstraint(s.to_string()))?;
                if n > m {
                    return Err(ParseError::InvalidConstraint(s.to_string()));
                }
                Ok(RangeBound::Between(n, m))
            }
        }
    } else {
        Err(ParseError::InvalidConstraint(s.to_string()))
    }
}

fn parse_array(body: &str) -> Result<TypeDefinition, ParseError> {
    let items = split_items(body);
    let mut result = Vec::new();

    for item in items {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }

        // Check for named field: `name: type` (but not integer-keyed)
        if let Some((name, ty_str)) = try_split_named_field(item) {
            result.push(ArrayItem {
                name: Some(name.to_string()),
                ty: parse_type_expr(ty_str)?,
            });
        } else {
            result.push(ArrayItem {
                name: None,
                ty: parse_type_expr(item)?,
            });
        }
    }

    Ok(TypeDefinition::Array(result))
}

/// Attempts to split `name: type` for array fields. Returns `None` if
/// the left side looks like an integer (which would be an integer key).
fn try_split_named_field(item: &str) -> Option<(&str, &str)> {
    let (name, ty) = item.split_once(':')?;
    let name = name.trim();
    let ty = ty.trim();

    // Reject if the "name" is actually an integer (for int-keyed maps).
    if name.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }

    if name.is_empty() || ty.is_empty() {
        return None;
    }

    Some((name, ty))
}

/// Parses a group choice: `[a, b] // [c, d] // ...`
///
/// Each alternative must be a bracket-delimited array. The `//` separator
/// is only recognized at the top level (not inside brackets).
fn parse_group_choice(definition: &str) -> Result<TypeDefinition, ParseError> {
    let alternatives = split_group_alternatives(definition);
    let mut variants = Vec::new();

    for alt in &alternatives {
        let alt = alt.trim();
        if !alt.starts_with('[') || !alt.ends_with(']') {
            return Err(ParseError::EmptyDefinition(format!(
                "group choice alternative must be an array: {alt}"
            )));
        }
        let inner = alt[1..alt.len() - 1].trim();
        let items = split_items(inner);
        let mut fields = Vec::new();
        for item in items {
            let item = item.trim();
            if item.is_empty() {
                continue;
            }
            if let Some((name, ty_str)) = try_split_named_field(item) {
                fields.push(ArrayItem {
                    name: Some(name.to_string()),
                    ty: parse_type_expr(ty_str)?,
                });
            } else {
                fields.push(ArrayItem {
                    name: None,
                    ty: parse_type_expr(item)?,
                });
            }
        }
        variants.push(fields);
    }

    Ok(TypeDefinition::GroupChoice(variants))
}

/// Splits a definition on `//` delimiters, respecting bracket nesting.
fn split_group_alternatives(definition: &str) -> Vec<String> {
    let mut alternatives = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    let chars: Vec<char> = definition.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];
        match ch {
            '[' | '{' | '(' => {
                depth += 1;
                current.push(ch);
            }
            ']' | '}' | ')' => {
                depth -= 1;
                current.push(ch);
            }
            '/' if depth == 0 && i + 1 < chars.len() && chars[i + 1] == '/' => {
                alternatives.push(current.trim().to_string());
                current.clear();
                i += 2; // skip both '/'
                continue;
            }
            _ => current.push(ch),
        }
        i += 1;
    }

    if !current.trim().is_empty() {
        alternatives.push(current.trim().to_string());
    }

    alternatives
}

fn parse_map(definition: &str) -> Result<TypeDefinition, ParseError> {
    let body = &definition[1..definition.len() - 1];
    let mut fields = Vec::new();

    for field in split_items(body) {
        let field = field.trim().to_string();
        if field.is_empty() {
            continue;
        }

        // Check for optional marker: `? key: type`
        let (optional, rest) = if let Some(rest) = field.strip_prefix('?') {
            (true, rest.trim())
        } else {
            (false, field.as_str())
        };

        let Some((key_str, ty_str)) = rest.split_once(':') else {
            return Err(ParseError::InvalidField(field));
        };

        let key_str = key_str.trim();
        let ty_str = ty_str.trim();

        if key_str.is_empty() || ty_str.is_empty() {
            return Err(ParseError::InvalidField(field));
        }

        // Determine if the key is an integer index or a string label.
        let key = if let Ok(index) = key_str.parse::<u64>() {
            FieldKey::Index(index)
        } else {
            FieldKey::Label(key_str.to_string())
        };

        fields.push(ParsedField {
            key,
            ty: parse_type_expr(ty_str)?,
            optional,
        });
    }

    Ok(TypeDefinition::Map(fields))
}

fn split_items(body: &str) -> Vec<String> {
    body.split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn strip_comment(line: &str) -> &str {
    line.split_once(';').map_or(line, |(content, _)| content)
}

fn is_valid_type_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };

    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
}
