//! Functions for building AST representations of higher-level values.
use rustc::hir;
use rustc::hir::def_id::{DefId, LOCAL_CRATE};
use rustc::hir::map::Node::*;
use rustc::hir::map::definitions::DefPathData;
use rustc::ty::{self, TyCtxt, GenericParamDefKind};
use rustc::ty::subst::Subst;
use syntax::ast::*;
use syntax::codemap::DUMMY_SP;
use syntax::ptr::P;
use syntax::symbol::keywords;
use rustc::middle::cstore::{ExternCrate, ExternCrateSource};

use ast_manip::make_ast::mk;
use command::{Registry, DriverCommand};
use driver::Phase;


/// Build an AST representing a `ty::Ty`.
pub fn reflect_tcx_ty<'a, 'gcx, 'tcx>(tcx: TyCtxt<'a, 'gcx, 'tcx>,
                                      ty: ty::Ty<'tcx>) -> P<Ty> {
    reflect_tcx_ty_inner(tcx, ty, false)
}

fn reflect_tcx_ty_inner<'a, 'gcx, 'tcx>(tcx: TyCtxt<'a, 'gcx, 'tcx>,
                                        ty: ty::Ty<'tcx>,
                                        infer_args: bool) -> P<Ty> {
    use rustc::ty::TypeVariants::*;
    match ty.sty {
        TyBool => mk().ident_ty("bool"),
        TyChar => mk().ident_ty("char"),
        TyInt(ity) => mk().ident_ty(ity.ty_to_string()),
        TyUint(uty) => mk().ident_ty(uty.ty_to_string()),
        TyFloat(fty) => mk().ident_ty(fty.ty_to_string()),
        TyAdt(def, substs) => {
            if infer_args {
                let (qself, path) = reflect_path(tcx, def.did);
                mk().qpath_ty(qself, path)
            } else {
                let substs = substs.types().collect::<Vec<_>>();
                let (qself, path) = reflect_path_inner(tcx, def.did, Some(&substs));
                mk().qpath_ty(qself, path)
            }
        },
        TyStr => mk().ident_ty("str"),
        TyArray(ty, len) => mk().array_ty(
            reflect_tcx_ty(tcx, ty),
            mk().lit_expr(mk().int_lit(len.unwrap_usize(tcx) as u128, "usize"))
        ),
        TySlice(ty) => mk().slice_ty(reflect_tcx_ty(tcx, ty)),
        TyRawPtr(mty) => mk().set_mutbl(mty.mutbl).ptr_ty(reflect_tcx_ty(tcx, mty.ty)),
        TyRef(_, ty, m) => mk().set_mutbl(m).ref_ty(reflect_tcx_ty(tcx, ty)),
        TyFnDef(_, _) => mk().infer_ty(), // unsupported (type cannot be named)
        TyFnPtr(_) => mk().infer_ty(), // TODO
        TyForeign(_) => mk().infer_ty(), // TODO ???
        TyDynamic(_, _) => mk().infer_ty(), // TODO
        TyClosure(_, _) => mk().infer_ty(), // unsupported (type cannot be named)
        TyGenerator(_, _, _) => mk().infer_ty(), // unsupported (type cannot be named)
        TyNever => mk().never_ty(),
        TyTuple(tys) => mk().tuple_ty(tys.iter().map(|&ty| reflect_tcx_ty(tcx, ty)).collect()),
        TyProjection(_) => mk().infer_ty(), // TODO
        TyAnon(_, _) => mk().infer_ty(), // TODO
        // (Note that, despite the name, `TyAnon` *can* be named - it's `impl SomeTrait`.)
        TyParam(param) => {
            if infer_args {
                mk().infer_ty()
            } else {
                mk().ident_ty(param.name)
            }
        },
        TyInfer(_) => mk().infer_ty(),
        TyError => mk().infer_ty(), // unsupported
        TyGeneratorWitness(_) => mk().infer_ty(), // TODO ?
    }
}

/// Build a path referring to a specific def.
pub fn reflect_path(tcx: TyCtxt, id: DefId) -> (Option<QSelf>, Path) {
    reflect_path_inner(tcx, id, None)
}

/// Build a path referring to a specific def.
fn reflect_path_inner<'a, 'gcx, 'tcx>(tcx: TyCtxt<'a, 'gcx, 'tcx>,
                                      id: DefId,
                                      opt_substs: Option<&[ty::Ty<'tcx>]>)
                                      -> (Option<QSelf>, Path) {
    let mut segments = Vec::new();
    let mut qself = None;

    // Build the path in reverse order.  Push the name of the current def first, then the name of
    // its parent, and so on.  We flip the path around at the end.
    let mut id = id;
    let mut opt_substs = opt_substs;
    loop {
        let dk = tcx.def_key(id);
        match dk.disambiguated_data.data {
            DefPathData::CrateRoot => {
                if id.krate == LOCAL_CRATE {
                    segments.push(mk().path_segment(keywords::CrateRoot.ident()));
                    break;
                } else {
                    if let Some(ExternCrate { src: ExternCrateSource::Extern(def_id), .. }) = *tcx.extern_crate(id) {
                        // The name of the crate is the path to its `extern crate` item.
                        id = def_id;
                        continue;
                    } else {
                        // Write `::crate_name` as the name of the crate.  This is incorrect, since
                        // there's no actual `extern crate crate_name` at top level (else we'd be
                        // in the previous case), but the resulting error should be obvious to the
                        // user.
                        segments.push(mk().path_segment(tcx.crate_name(id.krate)));
                        segments.push(mk().path_segment(keywords::CrateRoot.ident()));
                        break;
                    }
                }
            },

            // No idea what this is, but it doesn't have a name, so let's ignore it.
            DefPathData::Misc => {},

            DefPathData::Impl => {
                let ty = tcx.type_of(id);
                let gen = tcx.generics_of(id);
                let num_params = gen.params.len();

                // Reflect the type.  If we have substs available, apply them to the type first.
                let ast_ty = if let Some(substs) = opt_substs {
                    let start = substs.len() - num_params;
                    let tcx_substs = substs[start..].iter().map(|&t| t.into())
                        .collect::<Vec<_>>();
                    let ty = ty.subst(tcx, &tcx_substs);
                    reflect_tcx_ty(tcx, ty)
                } else {
                    reflect_tcx_ty_inner(tcx, ty, true)
                };

                match ast_ty.node {
                    TyKind::Path(ref ty_qself, ref ty_path) => {
                        qself = ty_qself.clone();
                        segments.extend(ty_path.segments.iter().rev().cloned());
                    },
                    _ => {
                        qself = Some(QSelf {
                            ty: ast_ty.clone(),
                            path_span: DUMMY_SP,
                            position: 0,
                        });
                    },
                }

                break;
            },

            DefPathData::ValueNs(name) => {
                if segments.len() == 0 {
                    if name != "" {
                        segments.push(mk().path_segment(name));
                    }
                } else {
                    // This is a function, which the original DefId was inside of.  `::f::g` is not
                    // a valid path if `f` is a function.  Instead, we stop now, leaving `g` as the
                    // path.  This is not an absolute path, but it should be valid inside of `f`,
                    // which is the only place `g` is visible.
                    break;
                }
            },

            DefPathData::TypeNs(name) |
            DefPathData::MacroDef(name) |
            DefPathData::LifetimeDef(name) |
            DefPathData::EnumVariant(name) |
            DefPathData::Module(name) |
            DefPathData::Field(name) |
            DefPathData::GlobalMetaData(name) => {
                if name != "" {
                    segments.push(mk().path_segment(name));
                }
            },

            DefPathData::TypeParam(name) => {
                if name != "" {
                    segments.push(mk().path_segment(name));
                    break;
                }
            },

            DefPathData::ClosureExpr |
            DefPathData::Trait(_) |
            DefPathData::AssocTypeInTrait(_) |
            DefPathData::AssocTypeInImpl(_) |
            DefPathData::AnonConst |
            DefPathData::UniversalImplTrait |
            DefPathData::ExistentialImplTrait |
            DefPathData::StructCtor => {},
            // Apparently DefPathData::ImplTrait disappeared in the current nightly?
            // TODO: Add it back when it's back
        }

        // Special logic for certain node kinds
        match dk.disambiguated_data.data {
            DefPathData::ValueNs(_) |
            DefPathData::TypeNs(_) => {
                let gen = tcx.generics_of(id);
                let num_params = gen.params.iter().filter(|x| match x.kind {
                    GenericParamDefKind::Lifetime{..} => false,
                    GenericParamDefKind::Type{..} => true,
                }).count();
                if let Some(substs) = opt_substs {
                    assert!(substs.len() >= num_params);
                    let start = substs.len() - num_params;
                    let mut abpd = AngleBracketedParameterData {
                        span: DUMMY_SP,
                        lifetimes: Vec::new(),
                        types: Vec::new(),
                        bindings: Vec::new(),
                    };
                    for &ty in &substs[start..] {
                        abpd.types.push(reflect_tcx_ty(tcx, ty));
                    }
                    segments.last_mut().unwrap().parameters = abpd.into();
                    opt_substs = Some(&substs[..start]);
                }
            },

            DefPathData::StructCtor => {
                // The parent of the struct ctor in `visible_parent_map` is the parent of the
                // struct.  But we want to visit the struct first, so we can add its name.
                if let Some(parent_id) = tcx.parent_def_id(id) {
                    id = parent_id;
                    continue;
                } else {
                    break;
                }
            },

            _ => {},
        }

        let visible_parent_map = tcx.visible_parent_map(LOCAL_CRATE);
        if let Some(&parent_id) = visible_parent_map.get(&id) {
            id = parent_id;
        } else if let Some(parent_id) = tcx.parent_def_id(id) {
            id = parent_id;
        } else {
            break;
        }
    }

    segments.reverse();
    (qself, mk().path(segments))
}

/// Wrapper around `reflect_path` that checks first to ensure its argument is the sort of def that
/// has a path.  `reflect_path` will panic if called on a def with no path.
pub fn can_reflect_path(hir_map: &hir::map::Map, id: NodeId) -> bool {
    let node = match hir_map.find(id) {
        Some(x) => x,
        None => return false,
    };
    match node {
        NodeItem(_) |
        NodeForeignItem(_) |
        NodeTraitItem(_) |
        NodeImplItem(_) |
        NodeVariant(_) |
        NodeField(_) |
        NodeStructCtor(_) => true,

        NodeMacroDef(_) | // TODO: Is this right?
        NodeExpr(_) |
        NodeStmt(_) |
        NodeTy(_) |
        NodeTraitRef(_) |
        NodeBinding(_) |
        NodePat(_) |
        NodeBlock(_) |
        NodeLocal(_) |
        NodeLifetime(_) |
        NodeTyParam(_) |
        NodeAnonConst(_) |
        NodeVisibility(_) => false,
    }
}


pub fn register_commands(reg: &mut Registry) {
    reg.register("test_reflect", |_args| {
        Box::new(DriverCommand::new(Phase::Phase3, move |st, cx| {
            st.map_krate(|krate| {
                use api::*;
                use rustc::ty::TypeVariants;

                let krate = fold_nodes(krate, |e: P<Expr>| {
                    let ty = cx.node_type(e.id);

                    let e = if let TypeVariants::TyFnDef(def_id, ref substs) = ty.sty {
                        let substs = substs.types().collect::<Vec<_>>();
                        let (qself, path) = reflect_path_inner(
                            cx.ty_ctxt(), def_id, Some(&substs));
                        mk().qpath_expr(qself, path)
                    } else if let Some(def_id) = cx.try_resolve_expr(&e) {
                        let parent = cx.hir_map().get_parent(e.id);
                        let parent_body = cx.hir_map().body_owned_by(parent);
                        let tables = cx.ty_ctxt().body_tables(parent_body);
                        let hir_id = cx.hir_map().node_to_hir_id(e.id);
                        let substs = tables.node_substs(hir_id);
                        let substs = substs.types().collect::<Vec<_>>();
                        let (qself, path) = reflect_path_inner(
                            cx.ty_ctxt(), def_id, Some(&substs));
                        mk().qpath_expr(qself, path)
                    } else {
                        e
                    };

                    mk().type_expr(e, reflect_tcx_ty(cx.ty_ctxt(), ty))
                });

                krate
            });
        }))
    });
}
