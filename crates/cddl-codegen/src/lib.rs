//! Small deterministic CDDL parsing and Rust generation helpers.

/// Rust code generation from parsed schema definitions.
pub mod generator;
/// Parsing of the supported CDDL subset.
pub mod parser;

/// Generated Rust module output and generation entry point.
pub use generator::{GeneratedModule, generate_module};
/// Parsed schema definitions and parsing entry points.
pub use parser::{
    ArrayItem, FieldKey, ParsedField, ParsedType, TypeDefinition, TypeExpr, parse_schema,
};
