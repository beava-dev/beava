//! beava-core: shared library for Beava.

pub mod agg_apply;
pub mod agg_buffer;
pub mod agg_compile;
pub mod agg_descriptor;
pub mod agg_geo;
pub mod agg_op;
pub mod agg_schema;
pub mod agg_state;
pub mod agg_state_decay;
pub mod agg_state_table;
pub mod agg_state_velocity;
pub mod agg_where;
pub mod agg_windowed;
pub mod bincode_safe_json;
pub mod config;
pub mod defaults;
pub mod eval;
pub mod expr;
pub mod expr_builtins;
pub mod op_chain;
pub mod op_node;
pub mod register_validate;
pub mod registry;
pub mod registry_diff;
pub mod row;
pub mod schema;
pub mod schema_propagate;
pub mod sketches;
pub mod snapshot_body;
pub mod wire;

/// Compile-time crate version exposed for banner / diagnostics.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_non_empty() {
        assert!(!VERSION.is_empty());
    }
}
