use api::*;
use command::{CommandState, Registry};
use driver::{self, Phase};
use syntax::ast::*;
use syntax::ptr::P;
use transform::Transform;

struct CharLits {
}

impl Transform for CharLits {
    fn min_phase(&self) -> Phase { Phase::Phase2 }
    fn transform(&self, krate: Crate, st: &CommandState, cx: &driver::Ctxt) -> Crate {

        let pattern = parse_expr(cx.session(), "__number as libc::c_char");
        let mut mcx = MatchCtxt::new(st, cx);
        mcx.set_type("__number", BindingType::Expr);

        let krate = fold_match_with(mcx, pattern.clone(), krate, |e, bnd| {
            let field: &P<Expr> = bnd.expr("__number");
            if let ExprKind::Lit(ref l) = field.node {
                if let LitKind::Int(i, _) = l.node {
                    if i < 256 {
                        let mut bnd = Bindings::new();
                        bnd.add_expr("__number", mk().lit_expr(mk().char_lit(i as u8 as char)));
                        return pattern.clone().subst(st, cx, &bnd)
                    }
                }
            }
            e
        });

        krate
    }
}

pub fn register_commands(reg: &mut Registry) {
    use super::mk;

    reg.register("char_literals", |_args| mk(CharLits{}))
}