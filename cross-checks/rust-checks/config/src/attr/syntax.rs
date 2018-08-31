
extern crate syntax;

use {XCheckType, ItemConfig, DefaultsConfig, FunctionConfig, StructConfig};
use super::{ArgList, ArgValue};

use std::convert::TryInto;

use self::syntax::ast;

pub fn get_item_args(mi: &ast::MetaItem) -> ArgList<'static> {
    if let Some(ref items) = mi.meta_item_list() {
        ArgList::from_map(items.iter().map(|item| {
            match item.node {
                ast::NestedMetaItemKind::MetaItem(ref mi) => {
                    let kw = unsafe {
                        let kw_str = mi.name().interned().as_str();
                        // FIXME: this looks unsafe, but mi.name().as_str()
                        // returns an InternedString whose sole member is
                        // a &'static str (which we're forcing the conversion to)
                        // Ideally, InternedString's as_ref() or deref() would
                        // correctly return a &'static str reference
                        ::std::mem::transmute::<&str, &'static str>(kw_str.as_ref())
                    };
                    match mi.node {
                        ast::MetaItemKind::Word => (kw, ArgValue::Nothing),

                        ast::MetaItemKind::NameValue(ref val) => {
                            match val.node {
                                ast::LitKind::Str(ref s, ast::StrStyle::Cooked) =>
                                    (kw, ArgValue::Str(String::from(&*s.as_str()))),

                                ast::LitKind::Int(i, _) => (kw, ArgValue::Int(i)),

                                _ => panic!("invalid tag value for by_value: {:?}", *val)
                            }
                        },

                        ast::MetaItemKind::List(_) => {
                            (kw, ArgValue::List(get_item_args(mi)))
                        }
                    }
                },
                _ => panic!("unknown item passed to by_value: {:?}", *item)
            }
        }).collect())
    } else {
        Default::default()
    }
}

fn parse_xcheck_type(name: &'static str, arg: &ArgValue) -> XCheckType {
    match name {
        "default"  => XCheckType::Default,
        "none"     => XCheckType::None,
        "disabled" => XCheckType::Disabled,

        "djb2" => XCheckType::Djb2(String::from(arg.as_str())),
        "fixed" => {
            match *arg {
                // TODO: handle LitKind::Str

                ArgValue::Int(id128) => {
                    if let Ok(id64) = id128.try_into() {
                        XCheckType::Fixed(id64)
                    } else {
                        panic!("invalid u32 for cross_check id: {}", id128)
                    }
                },

                _ => panic!("invalid literal for cross_check id: {:?}", arg)
            }
        },
        "custom" => XCheckType::Custom(String::from(arg.as_str())),
        _ => panic!("unknown cross-check type: {}", name)
     }
}

pub fn parse_xcheck_arglist(args: &ArgList<'static>, or_default: bool) -> Option<XCheckType> {
    if args.len() > 1 {
        panic!("expected single argument for cross-check type attribute");
    }
    args.iter().next()
        .map(|(name, ref arg)| parse_xcheck_type(name, arg))
        .or(if or_default { Some(XCheckType::Default) } else { None })
}

pub fn parse_xcheck_arg(arg: &ArgValue<'static>, or_default: bool) -> Option<XCheckType> {
    let res = match *arg {
        ArgValue::Nothing => None,
        ArgValue::List(ref l) => parse_xcheck_arglist(l, or_default),
        _ => panic!("unexpected argument to all_args():{:?}", *arg)
    };
    res.or(if or_default { Some(XCheckType::Default) } else { None })
}

pub fn parse_attr_config(item_xcfg: &mut ItemConfig, mi: &ast::MetaItem) {
    assert!(mi.name() == "cross_check");
    match item_xcfg {
        &mut ItemConfig::Defaults(ref mut d) => parse_defaults_attr_config(d, mi),
        &mut ItemConfig::Function(ref mut f) => parse_function_attr_config(f, mi),
        &mut ItemConfig::Struct(ref mut s)   => parse_struct_attr_config(s, mi),
        _ => panic!("unexpected item: {:#?}", item_xcfg)
    }
}

fn parse_defaults_attr_config(d: &mut DefaultsConfig, mi: &ast::MetaItem) {
    let args = get_item_args(mi);
    for (name, arg) in args.iter() {
        match *name {
            "disabled" | "none" => d.disable_xchecks = Some(true),
            "enabled"  | "yes"  => d.disable_xchecks = Some(false),
            "entry"    => d.entry    = parse_xcheck_arg(&arg, true),
            "exit"     => d.exit     = parse_xcheck_arg(&arg, true),
            "all_args" => d.all_args = parse_xcheck_arg(&arg, true),
            "ret"      => d.ret      = parse_xcheck_arg(&arg, true),
            _ => panic!("unexpected cross_check item: {}", name)
        }
    }
}

fn parse_function_attr_config(f: &mut FunctionConfig, mi: &ast::MetaItem) {
    let args = get_item_args(mi);
    for (name, arg) in args.iter() {
        match *name {
            "disabled" | "none" => f.disable_xchecks = Some(true),
            "enabled"  | "yes"  => f.disable_xchecks = Some(false),
            "entry"    => f.entry    = parse_xcheck_arg(&arg, true),
            "exit"     => f.exit     = parse_xcheck_arg(&arg, true),
            "all_args" => f.all_args = parse_xcheck_arg(&arg, true),
            "ret"      => f.ret      = parse_xcheck_arg(&arg, true),
            "args"     => {
                // Parse per-argument cross-check types
                f.args.extend(arg.as_list().iter().filter_map(|(name, arg)| {
                    if let ArgValue::List(ref l) = *arg {
                        let arg_xcheck = parse_xcheck_arglist(l, false)
                            .expect(&format!("expected valid cross-check type \
                                              for argument: {}", name));
                        Some((String::from(*name), arg_xcheck))
                    } else { None }
                }));
            }
            "ahasher"  => f.ahasher  = Some(String::from(arg.as_str())),
            "shasher"  => f.shasher  = Some(String::from(arg.as_str())),
            // TODO: handle entry_extra and exit_extra for Function
            _ => panic!("unexpected cross_check item: {}", name)
        }
    }
}

fn parse_struct_attr_config(s: &mut StructConfig, mi: &ast::MetaItem) {
    let args = get_item_args(mi);
    for (name, arg) in args.iter() {
        match *name {
            "disabled" | "none" => s.disable_xchecks = Some(true),
            "enabled"  | "yes"  => s.disable_xchecks = Some(false),
            "ahasher"  => s.ahasher  = Some(String::from(arg.as_str())),
            "shasher"  => s.shasher  = Some(String::from(arg.as_str())),
            "field_hasher" => s.field_hasher = Some(String::from(arg.as_str())),
            "custom_hash"  => s.custom_hash  = Some(String::from(arg.as_str())),
            "custom_hash_format" => s.custom_hash_format =
                Some(::CustomHashFormat::from_str(arg.as_str())),
            _ => panic!("unexpected cross_check item: {}", name)
        }
    }
}
