//! General-purpose AST manipulation utilities.  Everything in here works strictly on the AST, with
//! no reliance on HIR or TyCtxt information.

// Modules with simple APIs are private, with their public definitions reexported.
mod ast_equiv;
mod fold;
mod fold_node;
mod get_node_id;
mod get_span;
mod output_exprs;
mod seq_edit;
mod visit;
mod visit_node;

pub use self::ast_equiv::AstEquiv;
pub use self::fold::Fold;
pub use self::fold_node::{FoldNode, fold_nodes};
pub use self::get_node_id::GetNodeId;
pub use self::get_span::GetSpan;
pub use self::output_exprs::fold_output_exprs;
pub use self::seq_edit::{fold_blocks, fold_modules};
pub use self::visit::Visit;
pub use self::visit_node::{VisitNode, visit_nodes, visit_nodes_post};

// Modules with more complex APIs are left as `pub`.
pub mod fn_edit;
pub mod lr_expr;
pub mod make_ast;
pub mod util;
