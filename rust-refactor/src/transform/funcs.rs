use std::collections::{HashMap, HashSet};
use rustc::hir::def_id::DefId;
use rustc::ty::TypeVariants;
use rustc_target::spec::abi::Abi;
use syntax::ast::*;
use syntax::attr;
use syntax::codemap::Spanned;
use syntax::fold::{self, Folder};
use syntax::ptr::P;
use syntax::util::small_vector::SmallVector;

use api::*;
use command::{CommandState, Registry};
use driver::{self, Phase};
use transform::Transform;
use util::IntoSymbol;
use util::HirDefExt;


/// Turn free functions into methods in an impl.  
pub struct ToMethod;

impl Transform for ToMethod {
    fn transform(&self, krate: Crate, st: &CommandState, cx: &driver::Ctxt) -> Crate {
        // (1) Find the impl we're inserting into.

        let mut dest = None;

        let krate = fold_nodes(krate, |i: P<Item>| {
            // We're looking for an inherent impl (no `TraitRef`) marked with a cursor.
            if !st.marked(i.id, "dest") ||
               !matches!([i.node] ItemKind::Impl(_, _, _, _, None, _, _)) {
                return SmallVector::one(i);
            }

            if dest.is_none() {
                dest = Some(i.clone());
            }

            SmallVector::one(i)
        });

        if dest.is_none() {
            return krate;
        }
        let dest = dest.unwrap();


        // (2) Collect all marked functions, removing them from the AST.  Note that we collect only
        // free functions, not trait or impl methods.

        struct FnInfo {
            item: P<Item>,

            decl: P<FnDecl>,
            unsafety: Unsafety,
            constness: Spanned<Constness>,
            abi: Abi,
            generics: Generics,
            block: P<Block>,

            /// Index of the argument that will be replaced with `self`, or `None` if this function
            /// is being turned into a static method.
            arg_idx: Option<usize>,
        }
        let mut fns = Vec::new();

        let krate = fold_modules(krate, |curs| {
            while let Some(arg_idx) = curs.advance_until_match(|i| {
                // Find the argument under the cursor.
                let decl = match_or!([i.node] ItemKind::Fn(ref decl, ..) => decl; return None);
                for (idx, arg) in decl.inputs.iter().enumerate() {
                    if st.marked(arg.id, "target") {
                        return Some(Some(idx));
                    }
                }
                if st.marked(i.id, "target") {
                    return Some(None);
                }
                None
            }) {
                let i = curs.remove();
                unpack!([i.node.clone()]
                        ItemKind::Fn(decl, unsafety, constness, abi, generics, block));
                fns.push(FnInfo {
                    item: i,
                    decl, unsafety, constness, abi, generics, block,
                    arg_idx,
                });
            }
        });

        // Build a hash table with info needed to rewrite references to marked functions.
        struct FnRefInfo {
            ident: Ident,
            arg_idx: Option<usize>,
        }
        let fn_ref_info = fns.iter().map(|f| {
            (cx.node_def_id(f.item.id),
             FnRefInfo {
                 ident: f.item.ident.clone(),
                 arg_idx: f.arg_idx,
             })
        }).collect::<HashMap<_, _>>();


        // (3) Rewrite function signatures and bodies, replacing the marked arg with `self`.
        for f in &mut fns {
            // Functions that are being turned into static methods don't need any changes.
            let arg_idx = match_or!([f.arg_idx] Some(x) => x; continue);
            let mut inputs = f.decl.inputs.clone();

            // Remove the marked arg and inspect it.
            let arg = inputs.remove(arg_idx);

            let mode = match arg.pat.node {
                PatKind::Ident(mode, _, _) => mode,
                _ => panic!("unsupported argument pattern (expected ident): {:?}", arg.pat),
            };

            let pat_ty = cx.node_type(arg.pat.id);
            let self_ty = cx.def_type(cx.node_def_id(dest.id));
            let arg_hir_id = cx.hir_map().node_to_hir_id(arg.pat.id);

            // Build the new `self` argument and insert it.
            let self_kind = {
                if pat_ty == self_ty {
                    match mode {
                        BindingMode::ByValue(mutbl) => Some(SelfKind::Value(mutbl)),
                        BindingMode::ByRef(mutbl) => Some(SelfKind::Region(None, mutbl)),
                    }
                } else {
                    match pat_ty.sty {
                        TypeVariants::TyRef(_, ty, _) if ty == self_ty => {
                            match arg.ty.node {
                                TyKind::Rptr(ref lt, ref mty) =>
                                    Some(SelfKind::Region(lt.clone(), mty.mutbl)),
                                _ => None,
                            }
                        },
                        _ => None,
                    }
                }
            };
            let self_kind = match self_kind {
                Some(x) => x,
                None => panic!("unsupported argument type (expected {:?} or a ref): {:?}",
                               self_ty, pat_ty),
            };

            inputs.insert(0, mk().self_arg(self_kind));

            // Update `decl`
            f.decl = f.decl.clone().map(|fd| FnDecl { inputs: inputs, .. fd });

            // Rewrite references to the marked argument within the function body.

            // FIXME: rustc changed how locals args are represented, and we
            // don't have a Def for locals any more, and thus no def_id. We need
            // to fix this in path_edit.rs
            f.block = fold_resolved_paths(f.block.clone(), cx, |qself, path, def| {
                match cx.def_to_hir_id(&def) {
                    Some(hir_id) =>
                        if hir_id == arg_hir_id {
                            assert!(qself.is_none());
                            return (None, mk().path(vec!["self"]));
                        } else {
                            (qself, path)
                        },
                    None => (qself, path)
                }
            });
        }


        // (4) Find the destination impl again, and fill it in with the new methods.

        let mut fns = Some(fns);

        let krate = fold_nodes(krate, |i: P<Item>| {
            if i.id != dest.id || fns.is_none() {
                return SmallVector::one(i);
            }

            SmallVector::one(i.map(|i| {
                unpack!([i.node] ItemKind::Impl(
                        unsafety, polarity, generics, defaultness, trait_ref, ty, items));
                let mut items = items;
                let fns = fns.take().unwrap();
                items.extend(fns.into_iter().map(|f| {
                    let sig = MethodSig {
                        unsafety: f.unsafety,
                        constness: f.constness,
                        abi: f.abi,
                        decl: f.decl,
                    };
                    ImplItem {
                        id: DUMMY_NODE_ID,
                        ident: f.item.ident.clone(),
                        vis: f.item.vis.clone(),
                        defaultness: Defaultness::Final,
                        attrs: f.item.attrs.clone(),
                        generics: f.generics,
                        node: ImplItemKind::Method(sig, f.block),
                        span: f.item.span,
                        tokens: None,
                    }
                }));
                Item {
                    node: ItemKind::Impl(
                              unsafety, polarity, generics, defaultness, trait_ref, ty, items),
                    .. i
                }
            }))
        });


        // (5) Find all uses of marked functions, and rewrite them into method calls.

        let krate = fold_nodes(krate, |e: P<Expr>| {
            if !matches!([e.node] ExprKind::Call(..)) {
                return e;
            }

            unpack!([e.node.clone()] ExprKind::Call(func, args));
            let def_id = match_or!([cx.try_resolve_expr(&func)] Some(x) => x; return e);
            let info = match_or!([fn_ref_info.get(&def_id)] Some(x) => x; return e);

            // At this point, we know `func` is a reference to a marked function, and we have the
            // function's `FnRefInfo`.

            if let Some(arg_idx) = info.arg_idx {
                // Move the `self` argument into the first position.
                let mut args = args;
                let self_arg = args.remove(arg_idx);
                args.insert(0, self_arg);

                e.map(|e| {
                    Expr {
                        node: ExprKind::MethodCall(
                                  mk().path_segment(&info.ident),
                                  args),
                        .. e
                    }
                })
            } else {
                // There is no `self` argument, but change the function reference to the new path.
                let mut new_path = cx.def_path(cx.node_def_id(dest.id));
                new_path.segments.push(mk().path_segment(&info.ident));

                e.map(|e| {
                    Expr {
                        node: ExprKind::Call(mk().path_expr(new_path), args),
                        .. e
                    }
                })
            }
        });


        krate
    }

    fn min_phase(&self) -> Phase {
        Phase::Phase3
    }
}


// TODO: Reimplement FixUnusedUnsafe for updated rust.  Previously we implemented this pass by
// consulting the `TyCtxt::used_unsafe` set, but this set no longer exists in more recent versions.
// Instead, the "unused unsafe" diagnostics are emitted directly by the effect checking pass.  One
// possible new implementation strategy is to collect `rustc`'s diagnostics while running the
// driver, and consult them here to figure out which `unsafe`s are unused.
//
// Note: There was also a `fix_unused_unsafe` test case, which was removed in the same commit that
// added this comment.

/*
/// Find unused `unsafe` blocks and turn them into ordinary blocks.
pub struct FixUnusedUnsafe;

impl Transform for FixUnusedUnsafe {
    fn transform(&self, krate: Crate, _st: &CommandState, cx: &driver::Ctxt) -> Crate {
        let krate = fold_nodes(krate, |b: P<Block>| {
            if b.rules == BlockCheckMode::Unsafe(UnsafeSource::UserProvided) &&
               !cx.ty_ctxt().used_unsafe.borrow().contains(&b.id) {
                b.map(|b| Block {
                    rules: BlockCheckMode::Default,
                    .. b
                })
            } else {
                b
            }
        });

        krate
    }

    fn min_phase(&self) -> Phase {
        Phase::Phase3
    }
}
*/


/// Turn `unsafe fn f() { ... }` into `fn f() { unsafe { ... } }`.
pub struct SinkUnsafe;

struct SinkUnsafeFolder<'a> {
    st: &'a CommandState,
}

impl<'a> Folder for SinkUnsafeFolder<'a> {
    fn fold_item(&mut self, i: P<Item>) -> SmallVector<P<Item>> {
        let i = if self.st.marked(i.id, "target") {
            i.map(|mut i| {
                match i.node {
                    ItemKind::Fn(_, ref mut unsafety, _, _, _, ref mut block) => {
                        sink_unsafe(unsafety, block);
                    },
                    _ => {},
                }
                i
            })
        } else {
            i
        };


        fold::noop_fold_item(i, self)
    }

    fn fold_impl_item(&mut self, mut i: ImplItem) -> SmallVector<ImplItem> {
        if self.st.marked(i.id, "target") {
            match i.node {
                ImplItemKind::Method(MethodSig { ref mut unsafety, .. }, ref mut block) => {
                    sink_unsafe(unsafety, block);
                },
                _ => {},
            }
        }

        fold::noop_fold_impl_item(i, self)
    }
}

fn sink_unsafe(unsafety: &mut Unsafety, block: &mut P<Block>) {
    if *unsafety == Unsafety::Unsafe {
        *unsafety = Unsafety::Normal;
        *block = mk().block(vec![
            mk().expr_stmt(mk().block_expr(mk().unsafe_().block(
                        block.stmts.clone())))]);
    }
}

impl Transform for SinkUnsafe {
    fn transform(&self, krate: Crate, st: &CommandState, _cx: &driver::Ctxt) -> Crate {
        krate.fold(&mut SinkUnsafeFolder { st })
    }
}


pub struct WrapExtern;

impl Transform for WrapExtern {
    fn transform(&self, krate: Crate, st: &CommandState, cx: &driver::Ctxt) -> Crate {
        // (1) Collect the marked externs.
        #[derive(Debug)]
        struct FuncInfo {
            id: NodeId,
            def_id: DefId,
            ident: Ident,
            decl: P<FnDecl>,
        }
        let mut fns = Vec::new();

        visit_nodes(&krate, |fi: &ForeignItem| {
            if !st.marked(fi.id, "target") {
                return;
            }

            match fi.node {
                ForeignItemKind::Fn(ref decl, _) => {
                    fns.push(FuncInfo {
                        id: fi.id,
                        def_id: cx.node_def_id(fi.id),
                        ident: fi.ident.clone(),
                        decl: decl.clone(),
                    });
                },

                _ => {},
            }
        });

        info!("found {} fns", fns.len());
        for i in &fns {
            info!("  {:?}", i);
        }

        // (2) Generate wrappers in the destination module.
        let mut dest_path = None;
        let krate = fold_nodes(krate, |i: P<Item>| {
            if !st.marked(i.id, "dest") {
                return SmallVector::one(i);
            }

            if dest_path.is_some() {
                info!("warning: found multiple \"dest\" marks");
                return SmallVector::one(i);
            }
            dest_path = Some(cx.def_path(cx.node_def_id(i.id)));

            SmallVector::one(i.map(|i| {
                unpack!([i.node] ItemKind::Mod(m));
                let mut m = m;

                for f in &fns {
                    let func_path = cx.def_path(cx.node_def_id(f.id));
                    let arg_exprs = f.decl.inputs.iter().map(|arg| {
                        // TODO: match_arg("__i: __t", arg).ident("__i")
                        match arg.pat.node {
                            PatKind::Ident(BindingMode::ByValue(Mutability::Immutable),
                                           ident,
                                           None) => {
                                mk().ident_expr(ident)
                            },
                            _ => panic!("bad pattern in {:?}: {:?}", f.ident, arg.pat),
                        }
                    }).collect();
                    let body = mk().block(vec![
                            mk().expr_stmt(mk().call_expr(
                                    mk().path_expr(func_path),
                                    arg_exprs))]);
                    m.items.push(mk().pub_().unsafe_().fn_item(&f.ident, &f.decl, body));

                }

                Item {
                    node: ItemKind::Mod(m),
                    .. i
                }
            }))
        });

        if dest_path.is_none() {
            info!("warning: found no \"dest\" mark");
            return krate;
        }
        let dest_path = dest_path.unwrap();

        // (3) Rewrite call sites to use the new wrappers.
        let ident_map = fns.iter().map(|f| (f.def_id, f.ident)).collect::<HashMap<_, _>>();
        let krate = fold_resolved_paths(krate, cx, |qself, path, def| {
            match def.opt_def_id() {
                Some(def_id) if ident_map.contains_key(&def_id) => {
                    let ident = ident_map.get(&def_id).unwrap();
                    let mut new_path = dest_path.clone();
                    new_path.segments.push(mk().path_segment(ident));
                    (qself, new_path)
                },
                _ => (qself, path),
            }
        });

        krate
    }

    fn min_phase(&self) -> Phase {
        Phase::Phase3
    }
}


/// Generate wrappers for API functions.  Each marked definition of an `extern` function will be
/// split into a normal function and an `extern` wrapper.
pub struct WrapApi;

impl Transform for WrapApi {
    fn transform(&self, krate: Crate, st: &CommandState, _cx: &driver::Ctxt) -> Crate {
        fold_nodes(krate, |i: P<Item>| {
            if !st.marked(i.id, "target") {
                return SmallVector::one(i);
            }

            if !matches!([i.node] ItemKind::Fn(..)) {
                return SmallVector::one(i);
            }

            let (decl, old_abi) = expect!([i.node]
                ItemKind::Fn(ref decl, _, _, abi, _, _) => (decl.clone(), abi));

            let symbol =
                if let Some(sym) = attr::first_attr_value_str_by_name(&i.attrs, "export_name") {
                    sym
                } else if attr::contains_name(&i.attrs, "no_mangle") {
                    i.ident.name
                } else {
                    warn!("marked function `{:?}` does not have a stable symbol", i.ident.name);
                    return SmallVector::one(i);
                };

            let i = i.map(|mut i| {
                i.attrs.retain(|attr| {
                    attr.path != "no_mangle" &&
                    attr.path != "export_name"
                });

                match i.node {
                    ItemKind::Fn(_, _, _, ref mut abi, _, _) => *abi = Abi::Rust,
                    _ => unreachable!(),
                }

                i
            });

            // Pick distinct names for the arguments in the wrapper.
            let mut used_names = HashSet::new();

            let arg_names = decl.inputs.iter().enumerate().map(|(idx, arg)| {
                let base = match arg.pat.node {
                    // Use the name from the original function, if there is one.  Otherwise, fall
                    // back on `arg0`, `arg1`, ...
                    PatKind::Ident(_, ref ident, _) => ident.name,
                    _ => format!("arg{}", idx).into_symbol(),
                };

                let name;
                if !used_names.contains(&base) {
                    name = base;
                } else {
                    let mut i = 0;
                    loop {
                        let gen_name = format!("{}_{}", base.as_str(), i).into_symbol();
                        if !used_names.contains(&gen_name) {
                            name = gen_name;
                            break;
                        }
                        i += 1;
                    }
                }

                used_names.insert(name);
                name
            }).collect::<Vec<_>>();

            // Generate the wrapper.
            let wrapper_decl = decl.clone().map(|decl| {
                let new_inputs = decl.inputs.iter().zip(arg_names.iter()).map(|(arg, &name)| {
                    mk().arg(&arg.ty, mk().ident_pat(name))
                }).collect();
                FnDecl {
                    inputs: new_inputs,
                    .. decl
                }
            });

            let wrapper_args = arg_names.iter().map(|&name| mk().ident_expr(name)).collect();

            let wrapper =
                mk().vis(i.vis.clone()).unsafe_().abi(old_abi)
                        .str_attr("export_name", symbol).fn_item(
                    format!("{}_wrapper", symbol.as_str()),
                    wrapper_decl,
                    mk().block(vec![
                        mk().expr_stmt(mk().call_expr(
                                mk().path_expr(vec![i.ident.name]),
                                wrapper_args,
                        ))
                    ])
                );

            let mut v = SmallVector::new();
            v.push(i);
            v.push(wrapper);
            v
        })
    }

    fn min_phase(&self) -> Phase {
        Phase::Phase3
    }
}


pub fn register_commands(reg: &mut Registry) {
    use super::mk;

    reg.register("func_to_method", |_args| mk(ToMethod));
    // TODO: Reimplement fix_unused_unsafe (see other TODO comment above)
    //reg.register("fix_unused_unsafe", |_args| mk(FixUnusedUnsafe));
    reg.register("sink_unsafe", |_args| mk(SinkUnsafe));
    reg.register("wrap_extern", |_args| mk(WrapExtern));
    reg.register("wrap_api", |_args| mk(WrapApi));
}
