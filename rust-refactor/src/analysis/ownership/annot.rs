//! Read user annotations, in the form of marks on type nodes, and use them to populate parts of
//! the analysis context.

use std::cmp;
use std::collections::HashMap;
use std::str::FromStr;

use arena::SyncDroplessArena;
use rustc::hir::def_id::DefId;
use rustc_data_structures::indexed_vec::IndexVec;
use syntax::ast;
use syntax::symbol::Symbol;
use syntax::visit::{self, Visitor};

use ast_manip::Visit;
use command::CommandState;
use driver;
use type_map::{self, TypeSource};

use super::{LTy, LFnSig, ConcretePerm, PermVar, Var};
use super::constraint::{ConstraintSet, Perm};
use super::context::Ctxt;


struct LTySource<'c, 'a: 'c, 'tcx: 'a> {
    cx: &'c mut Ctxt<'a, 'tcx>,

    // XXX - bit of a hack.  We keep the def id of the last call to `fn_sig`, and refer to that
    // inside the map_types callback to figure out the right scope for any SigVars in the type.
    // This relies on the fact that map_types invokes the next TypeSource method only once all
    // callback invocations resulting for the previous TypeSource call have been made.
    last_sig_did: Option<DefId>,
}

impl<'c, 'a, 'tcx> TypeSource for LTySource<'c, 'a, 'tcx> {
    type Type = LTy<'tcx>;
    type Signature = LFnSig<'tcx>;

    fn expr_type(&mut self, _e: &ast::Expr) -> Option<Self::Type> {
        self.last_sig_did = None;
        None
    }

    fn pat_type(&mut self, _p: &ast::Pat) -> Option<Self::Type> {
        self.last_sig_did = None;
        None
    }

    fn def_type(&mut self, did: DefId) -> Option<Self::Type> {
        self.last_sig_did = None;
        Some(self.cx.static_ty(did))
    }

    fn fn_sig(&mut self, did: DefId) -> Option<Self::Signature> {
        self.last_sig_did = Some(did);
        Some(self.cx.variant_func_sig(did))
    }

    fn closure_sig(&mut self, _did: DefId) -> Option<Self::Signature> {
        self.last_sig_did = None;
        // TODO - Need to implement this properly if we ever add closure support.
        None
    }
}

pub fn handle_marks<'a, 'tcx>(cx: &mut Ctxt<'a, 'tcx>,
                              st: &CommandState,
                              dcx: &driver::Ctxt<'a, 'tcx>) {
    let mut fixed_vars = Vec::new();
    {
        let source = LTySource {
            cx: cx,
            last_sig_did: None,
        };

        type_map::map_types(dcx.hir_map(), source, &st.krate(), |source, ast_ty, lty| {
            eprintln!("match {:?} ({:?}) with {:?}", ast_ty, ast_ty.id, lty);
            if st.marked(ast_ty.id, "box") {
                if let Some(p) = lty.label {
                    fixed_vars.push((p, source.last_sig_did, ConcretePerm::Move));
                }
            }

            if st.marked(ast_ty.id, "mut") {
                if let Some(p) = lty.label {
                    fixed_vars.push((p, source.last_sig_did, ConcretePerm::Write));
                }
            }

            if st.marked(ast_ty.id, "ref") {
                if let Some(p) = lty.label {
                    fixed_vars.push((p, source.last_sig_did, ConcretePerm::Read));
                }
            }
        });
    }

    // For any marked types that are in signatures, add constraints to the parent function's cset.
    for (p, did, min_perm) in fixed_vars {
        eprintln!("FIXED VAR: {:?} = {:?} (in {:?})", p, min_perm, did);
        match p {
            PermVar::Static(v) => {
                let new_perm = cmp::max(min_perm, cx.static_assign[v]);
                cx.static_assign[v] = new_perm;
            },
            PermVar::Sig(_) => {
                let did = did.expect("expected DefId for SigVar");
                cx.variant_summ(did).1.inst_cset
                    .add(Perm::Concrete(min_perm),
                         Perm::var(p));
            }
            _ => panic!("expected Static or Sig var, but got {:?}", p),
        }
    }
}



struct AttrVisitor<'ast> {
    def_attrs: Vec<(ast::NodeId, &'ast [ast::Attribute])>,
}

impl<'ast> Visitor<'ast> for AttrVisitor<'ast> {
    fn visit_item(&mut self, i: &'ast ast::Item) {
        match i.node {
            ast::ItemKind::Fn(..) |
            ast::ItemKind::Static(..) |
            ast::ItemKind::Const(..) => {
                if i.attrs.len() > 0 {
                    self.def_attrs.push((i.id, &i.attrs));
                }
            },
            _ => {},
        }

        visit::walk_item(self, i);
    }

    fn visit_impl_item(&mut self, i: &'ast ast::ImplItem) {
        match i.node {
            ast::ImplItemKind::Method(..) |
            ast::ImplItemKind::Const(..) => {
                if i.attrs.len() > 0 {
                    self.def_attrs.push((i.id, &i.attrs));
                }
            },
            _ => {},
        }

        visit::walk_impl_item(self, i);
    }

    fn visit_struct_field(&mut self, sf: &'ast ast::StructField) {
        if sf.attrs.len() > 0 {
            self.def_attrs.push((sf.id, &sf.attrs));
        }

        visit::walk_struct_field(self, sf);
    }
}

pub fn handle_attrs<'a, 'hir, 'tcx>(cx: &mut Ctxt<'a, 'tcx>,
                                    st: &CommandState,
                                    dcx: &driver::Ctxt<'a, 'tcx>) {

    let krate = st.krate();
    let mut v = AttrVisitor {
        def_attrs: Vec::new(),
    };
    krate.visit(&mut v);

    eprintln!("HANDLE_ATTRS: found {} annotated defs", v.def_attrs.len());


    // Primary variant ID for each variant group (keyed by group name)
    let mut variant_group_primary = HashMap::new();

    for (node_id, attrs) in v.def_attrs {
        let def_id = match_or!([dcx.hir_map().opt_local_def_id(node_id)] Some(x) => x; continue);

        // Handle `ownership_variant_of` first.
        let mut is_variant = false;
        if let Some(attr) = attrs.iter().filter(|a| a.check_name("ownership_variant_of")).next() {
            let meta = match_or!([attr.meta()] Some(x) => x;
                    panic!("bad meta item in #[ownership_variant_of] (for {:?})", def_id));
            let group_name = parse_variant_of(&meta)
                .unwrap_or_else(|e| panic!("bad #[ownership_variant_of] for {:?}: {}",
                                           def_id, e));
            let primary = *variant_group_primary.entry(group_name).or_insert(def_id);
            cx.add_variant(primary, def_id);

            let num_mono = attrs.iter().filter(|a| a.check_name("ownership_mono")).count();
            assert!(num_mono == 1, "functions with #[ownership_variant_of] \
                        must also have #[ownership_mono] (on {:?})", def_id);

            is_variant = true;
        }
        let is_variant = is_variant;

        for attr in attrs {
            let meta = match_or!([attr.meta()] Some(x) => x; continue);
            match &meta.name().as_str() as &str {
                "ownership_constraints" => {
                    let cset = parse_ownership_constraints(&meta, dcx.ty_arena())
                        .unwrap_or_else(|e| panic!("bad #[ownership_constraints] for {:?}: {}",
                                                   def_id, e));

                    eprintln!("found constraints for {:?}:", def_id);
                    for &(a, b) in cset.iter() {
                        eprintln!("  {:?} <= {:?}", a, b);
                    }

                    let (func, _var) = cx.variant_summ(def_id);
                    assert!(!func.cset_provided,
                            "{} can only have one #[ownership_constraint] annotation (on {:?})",
                            if is_variant { "variant set" } else { "function" }, def_id);
                    func.sig_cset.import(&cset);
                    func.cset_provided = true;
                },

                "ownership_mono" => {
                    let (suffix, assign) = parse_mono_sig(&meta)
                        .unwrap_or_else(|e| panic!("bad #[ownership_mono] for {:?}: {}",
                                                   def_id, e));

                    let (func, _variant, mono) = cx.add_mono(def_id);
                    mono.assign = assign;
                    mono.suffix = suffix;
                    func.monos_provided = true;
                },

                "ownership_static" => {
                    let assign = parse_static_assign(&meta)
                        .unwrap_or_else(|e| panic!("bad #[ownership_static] for {:?}: {}",
                                                   def_id, e));

                    let ty = cx.static_ty(def_id);
                    let mut iter = assign.into_iter();
                    ty.for_each_label(&mut |p| {
                        if let Some(PermVar::Static(v)) = *p {
                            let c = iter.next().unwrap_or_else(|| panic!("not enough permissions \
                                        in #[ownership_static] for {:?}", def_id));
                            cx.static_assign[v] = c;
                        }
                    });
                },

                _ => {},
            }
        }
    }
}



fn meta_item_list(meta: &ast::MetaItem) -> Result<&[ast::NestedMetaItem], &'static str> {
    match meta.node {
        ast::MetaItemKind::List(ref xs) => Ok(xs),
        _ => Err("expected MetaItemKind::List"),
    }
}

fn meta_item_word(meta: &ast::MetaItem) -> Result<(), &'static str> {
    match meta.node {
        ast::MetaItemKind::Word => Ok(()),
        _ => Err("expected MetaItemKind::List"),
    }
}

fn nested_meta_item(nmeta: &ast::NestedMetaItem) -> Result<&ast::MetaItem, &'static str> {
    match nmeta.node {
        ast::NestedMetaItemKind::MetaItem(ref m) => Ok(m),
        _ => Err("expected NestedMetaItemKind::MetaItem"),
    }
}

fn nested_str(nmeta: &ast::NestedMetaItem) -> Result<Symbol, &'static str> {
    match nmeta.node {
        ast::NestedMetaItemKind::Literal(ref lit) => {
            match lit.node {
                ast::LitKind::Str(s, _) => Ok(s),
                _ => Err("expected str"),
            }
        },
        _ => Err("expected str"),
    }
}

fn parse_ownership_constraints<'tcx>(meta: &ast::MetaItem,
                                     arena: &'tcx SyncDroplessArena)
                                     -> Result<ConstraintSet<'tcx>, &'static str> {
    let args = meta_item_list(meta)?;

    let mut cset = ConstraintSet::new();
    for arg in args {
        let arg = nested_meta_item(arg)?;
        if !arg.check_name("le") {
            return Err("expected `le(a, b)` in `ownership_constraints`");
        }

        let perms = meta_item_list(arg)?;
        if perms.len() != 2 {
            return Err("expected exactly two arguments in `le`");
        }

        let a = parse_perm(nested_meta_item(&perms[0])?, arena)?;
        let b = parse_perm(nested_meta_item(&perms[1])?, arena)?;
        cset.add(a, b);
    }

    Ok(cset)
}

fn parse_perm<'tcx>(meta: &ast::MetaItem,
                    arena: &'tcx SyncDroplessArena)
                    -> Result<Perm<'tcx>, &'static str> {
    if meta.check_name("min") {
        let args = meta_item_list(meta)?;
        if args.len() == 0 {
            return Err("`min` requires at least one argument");
        }

        let mut perms = Vec::with_capacity(args.len());
        for arg in args {
            let arg_meta = nested_meta_item(arg)?;
            let perm = parse_perm(arg_meta, arena)?;
            perms.push(perm);
        }

        let perms = arena.alloc_slice(&perms);
        Ok(Perm::Min(perms))
    } else {
        meta_item_word(meta)?;

        let name = meta.name().as_str();
        match &name as &str {
            "READ" => return Ok(Perm::read()),
            "WRITE" => return Ok(Perm::write()),
            "MOVE" => return Ok(Perm::move_()),
            _ => {},
        }

        if !name.starts_with("_") {
            return Err("invalid permission variable");
        }
        let idx = FromStr::from_str(&name[1..]).map_err(|_| "invalid permission variable")?;
        Ok(Perm::SigVar(Var(idx)))
    }
}

fn parse_concrete(meta: &ast::MetaItem) -> Result<ConcretePerm, &'static str> {
    meta_item_word(meta)?;

    let name = meta.name().as_str();
    match &name as &str {
        "READ" => Ok(ConcretePerm::Read),
        "WRITE" => Ok(ConcretePerm::Write),
        "MOVE" => Ok(ConcretePerm::Move),
        _ => Err("expected permission value (READ, WRITE, or MOVE)"),
    }
}

fn parse_mono_sig(meta: &ast::MetaItem)
                  -> Result<(String, IndexVec<Var, ConcretePerm>), &'static str> {
    let args = meta_item_list(meta)?;

    if args.len() == 0 {
        return Err("expected variant name in #[ownership_mono]");
    }

    let suffix = nested_str(&args[0])?;
    let suffix = (&suffix.as_str() as &str).to_owned();

    let mut assign = IndexVec::with_capacity(args.len() - 1);

    for arg in &args[1..] {
        let arg = nested_meta_item(arg)?;
        let perm = parse_concrete(arg)?;
        assign.push(perm);
    }

    Ok((suffix, assign))
}

fn parse_static_assign(meta: &ast::MetaItem) -> Result<Vec<ConcretePerm>, &'static str> {
    let args = meta_item_list(meta)?;

    let mut assign = Vec::with_capacity(args.len());

    for arg in args {
        let arg = nested_meta_item(arg)?;
        let perm = parse_concrete(arg)?;
        assign.push(perm);
    }

    Ok(assign)
}

fn parse_variant_of(meta: &ast::MetaItem) -> Result<Symbol, &'static str> {
    let args = meta_item_list(meta)?;
    if args.len() != 1 {
        return Err("expected exactly one argument in #[ownership_variant_of]");
    }
    nested_str(&args[0])
}
