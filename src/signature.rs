use std::fmt::Write;

use rustdoc_types::{
    Abi, AssocItemConstraint, AssocItemConstraintKind, Crate, DynTrait, FunctionHeader,
    FunctionPointer, FunctionSignature, GenericArg, GenericArgs, GenericBound, GenericParamDef,
    GenericParamDefKind, Generics, ItemEnum, MacroKind, Path, StructKind, Term, TraitBoundModifier,
    Type, VariantKind, Visibility, WherePredicate,
};

/// Renders a human-readable signature for a rustdoc item.
///
/// Returns `None` for items without renderable signatures (impl blocks, use items).
/// The caller should construct a fallback like `"{visibility}{kind} {name}"`.
pub(crate) fn render_signature(item: &rustdoc_types::Item, krate: &Crate) -> Option<String> {
    let vis = render_visibility(&item.visibility);
    let name = item.name.as_deref().unwrap_or("");

    match &item.inner {
        ItemEnum::Struct(s) => Some(render_struct_sig(&vis, name, s, krate)),
        ItemEnum::Enum(e) => Some(render_enum_sig(&vis, name, &e.generics)),
        ItemEnum::Union(u) => Some(render_union_sig(&vis, name, u, krate)),
        ItemEnum::Trait(t) => Some(render_trait_sig(&vis, name, t)),
        ItemEnum::TraitAlias(ta) => Some(render_trait_alias_sig(&vis, name, ta)),
        ItemEnum::Function(f) => Some(render_function_sig(&vis, name, f)),
        ItemEnum::TypeAlias(ta) => Some(render_type_alias_sig(&vis, name, ta)),
        ItemEnum::Constant { type_, .. } => {
            Some(format!("{vis}const {name}: {}", render_type(type_)))
        }
        ItemEnum::AssocConst { type_, .. } => Some(format!("const {name}: {}", render_type(type_))),
        ItemEnum::Static(s) => Some(render_static_sig(&vis, name, s)),
        ItemEnum::Macro(_) => Some(format!("macro_rules! {name}")),
        ItemEnum::ProcMacro(pm) => Some(render_proc_macro_sig(name, pm)),
        ItemEnum::Module(_) => Some(format!("{vis}mod {name}")),
        ItemEnum::Variant(v) => Some(render_variant_sig(name, v, krate)),
        ItemEnum::StructField(ty) => {
            let fvis = render_visibility(&item.visibility);
            Some(format!("{fvis}{name}: {}", render_type(ty)))
        }
        ItemEnum::AssocType {
            generics,
            bounds,
            type_,
        } => Some(render_assoc_type_sig(
            name,
            generics,
            bounds,
            type_.as_ref(),
        )),
        ItemEnum::Primitive(_) => Some(format!("{vis}primitive {name}")),
        // Items without renderable signatures
        ItemEnum::Impl(_)
        | ItemEnum::Use(_)
        | ItemEnum::ExternCrate { .. }
        | ItemEnum::ExternType => None,
    }
}

/// Renders a struct signature.
fn render_struct_sig(vis: &str, name: &str, s: &rustdoc_types::Struct, krate: &Crate) -> String {
    let mut sig = format!("{vis}struct {name}");
    render_generics_inline(&s.generics, &mut sig);
    match &s.kind {
        StructKind::Unit => {}
        StructKind::Tuple(fields) => render_tuple_fields(fields, krate, &mut sig),
        StructKind::Plain {
            fields,
            has_stripped_fields,
        } => render_plain_struct_fields(fields, *has_stripped_fields, krate, &mut sig),
    }
    render_where_clause(&s.generics, &mut sig);
    sig
}

/// Renders tuple struct fields: `(Type1, Type2)`.
fn render_tuple_fields(fields: &[Option<rustdoc_types::Id>], krate: &Crate, sig: &mut String) {
    sig.push('(');
    let parts: Vec<String> = fields
        .iter()
        .map(|field_id| {
            field_id
                .and_then(|id| krate.index.get(&id))
                .map_or("_".to_string(), |field_item| {
                    if let ItemEnum::StructField(ty) = &field_item.inner {
                        let fvis = render_visibility(&field_item.visibility);
                        format!("{fvis}{}", render_type(ty))
                    } else {
                        "_".to_string()
                    }
                })
        })
        .collect();
    sig.push_str(&parts.join(", "));
    sig.push(')');
}

/// Renders plain struct fields: `{ pub field1: Type, pub field2: Type }`.
fn render_plain_struct_fields(
    fields: &[rustdoc_types::Id],
    has_stripped_fields: bool,
    krate: &Crate,
    sig: &mut String,
) {
    let pub_fields: Vec<String> = fields
        .iter()
        .filter_map(|id| krate.index.get(id))
        .filter(|fi| matches!(fi.visibility, Visibility::Public))
        .filter_map(|fi| {
            if let ItemEnum::StructField(ty) = &fi.inner {
                Some(format!(
                    "pub {}: {}",
                    fi.name.as_deref().unwrap_or("_"),
                    render_type(ty)
                ))
            } else {
                None
            }
        })
        .collect();

    if !pub_fields.is_empty() {
        sig.push_str(" { ");
        sig.push_str(&pub_fields.join(", "));
        if has_stripped_fields {
            sig.push_str(", /* private fields */");
        }
        sig.push_str(" }");
    }
}

/// Renders an enum signature.
fn render_enum_sig(vis: &str, name: &str, generics: &Generics) -> String {
    let mut sig = format!("{vis}enum {name}");
    render_generics_inline(generics, &mut sig);
    render_where_clause(generics, &mut sig);
    sig
}

/// Renders a union signature.
fn render_union_sig(vis: &str, name: &str, u: &rustdoc_types::Union, krate: &Crate) -> String {
    let mut sig = format!("{vis}union {name}");
    render_generics_inline(&u.generics, &mut sig);
    let field_strs: Vec<String> = u
        .fields
        .iter()
        .filter_map(|id| krate.index.get(id))
        .filter_map(|fi| {
            if let ItemEnum::StructField(ty) = &fi.inner {
                let fvis = render_visibility(&fi.visibility);
                Some(format!(
                    "{fvis}{}: {}",
                    fi.name.as_deref().unwrap_or("_"),
                    render_type(ty)
                ))
            } else {
                None
            }
        })
        .collect();
    if !field_strs.is_empty() {
        sig.push_str(" { ");
        sig.push_str(&field_strs.join(", "));
        sig.push_str(" }");
    }
    render_where_clause(&u.generics, &mut sig);
    sig
}

/// Renders a trait signature.
fn render_trait_sig(vis: &str, name: &str, t: &rustdoc_types::Trait) -> String {
    let mut sig = String::new();
    sig.push_str(vis);
    if t.is_unsafe {
        sig.push_str("unsafe ");
    }
    if t.is_auto {
        sig.push_str("auto ");
    }
    sig.push_str("trait ");
    sig.push_str(name);
    render_generics_inline(&t.generics, &mut sig);
    if !t.bounds.is_empty() {
        sig.push_str(": ");
        let bounds: Vec<String> = t.bounds.iter().map(render_generic_bound).collect();
        sig.push_str(&bounds.join(" + "));
    }
    render_where_clause(&t.generics, &mut sig);
    sig
}

/// Renders a trait alias signature.
fn render_trait_alias_sig(vis: &str, name: &str, ta: &rustdoc_types::TraitAlias) -> String {
    let mut sig = format!("{vis}trait {name}");
    render_generics_inline(&ta.generics, &mut sig);
    if !ta.params.is_empty() {
        sig.push_str(" = ");
        let bounds: Vec<String> = ta.params.iter().map(render_generic_bound).collect();
        sig.push_str(&bounds.join(" + "));
    }
    render_where_clause(&ta.generics, &mut sig);
    sig
}

/// Renders a function or method signature.
fn render_function_sig(vis: &str, name: &str, f: &rustdoc_types::Function) -> String {
    let mut sig = String::new();
    sig.push_str(vis);
    render_fn_qualifiers(&f.header, &mut sig);
    sig.push_str("fn ");
    sig.push_str(name);
    render_generics_inline(&f.generics, &mut sig);
    render_fn_params(&f.sig, &mut sig);
    render_fn_return(&f.sig, &mut sig);
    render_where_clause(&f.generics, &mut sig);
    sig
}

/// Renders a type alias signature.
fn render_type_alias_sig(vis: &str, name: &str, ta: &rustdoc_types::TypeAlias) -> String {
    let mut sig = format!("{vis}type {name}");
    render_generics_inline(&ta.generics, &mut sig);
    sig.push_str(" = ");
    sig.push_str(&render_type(&ta.type_));
    render_where_clause(&ta.generics, &mut sig);
    sig
}

/// Renders a static item signature.
fn render_static_sig(vis: &str, name: &str, s: &rustdoc_types::Static) -> String {
    let mut sig = format!("{vis}static ");
    if s.is_mutable {
        sig.push_str("mut ");
    }
    let _ = write!(sig, "{name}: {}", render_type(&s.type_));
    sig
}

/// Renders a proc macro signature.
fn render_proc_macro_sig(name: &str, pm: &rustdoc_types::ProcMacro) -> String {
    let kind_str = match pm.kind {
        MacroKind::Bang => "#[proc_macro]",
        MacroKind::Attr => "#[proc_macro_attribute]",
        MacroKind::Derive => "#[proc_macro_derive]",
    };
    format!("{kind_str} {name}")
}

/// Renders a variant signature.
fn render_variant_sig(name: &str, v: &rustdoc_types::Variant, krate: &Crate) -> String {
    match &v.kind {
        VariantKind::Plain => name.to_string(),
        VariantKind::Tuple(fields) => {
            let types: Vec<String> = fields
                .iter()
                .map(|field_id| {
                    field_id
                        .and_then(|id| krate.index.get(&id))
                        .map_or("_".to_string(), |fi| {
                            if let ItemEnum::StructField(ty) = &fi.inner {
                                render_type(ty)
                            } else {
                                "_".to_string()
                            }
                        })
                })
                .collect();
            format!("{name}({})", types.join(", "))
        }
        VariantKind::Struct {
            fields,
            has_stripped_fields: _,
        } => {
            let field_strs: Vec<String> = fields
                .iter()
                .filter_map(|id| krate.index.get(id))
                .filter_map(|fi| {
                    if let ItemEnum::StructField(ty) = &fi.inner {
                        Some(format!(
                            "{}: {}",
                            fi.name.as_deref().unwrap_or("_"),
                            render_type(ty)
                        ))
                    } else {
                        None
                    }
                })
                .collect();
            format!("{name} {{ {} }}", field_strs.join(", "))
        }
    }
}

/// Renders an associated type signature.
fn render_assoc_type_sig(
    name: &str,
    generics: &Generics,
    bounds: &[GenericBound],
    type_: Option<&Type>,
) -> String {
    let mut sig = format!("type {name}");
    render_generics_inline(generics, &mut sig);
    if !bounds.is_empty() {
        sig.push_str(": ");
        let bs: Vec<String> = bounds.iter().map(render_generic_bound).collect();
        sig.push_str(&bs.join(" + "));
    }
    if let Some(ty) = type_ {
        sig.push_str(" = ");
        sig.push_str(&render_type(ty));
    }
    render_where_clause(generics, &mut sig);
    sig
}

/// Renders visibility prefix (e.g. "pub ", "pub(crate) ", or "").
fn render_visibility(vis: &Visibility) -> String {
    match vis {
        Visibility::Public => "pub ".to_string(),
        Visibility::Default | Visibility::Crate | Visibility::Restricted { .. } => String::new(),
    }
}

/// Renders generic parameters inline (the `<...>` part).
fn render_generics_inline(generics: &Generics, out: &mut String) {
    let params: Vec<String> = generics
        .params
        .iter()
        .filter(|p| !is_synthetic_param(p))
        .map(render_generic_param)
        .collect();

    if !params.is_empty() {
        out.push('<');
        out.push_str(&params.join(", "));
        out.push('>');
    }
}

/// Returns true if a generic parameter is synthetic (compiler-generated from `impl Trait`).
fn is_synthetic_param(param: &GenericParamDef) -> bool {
    matches!(
        &param.kind,
        GenericParamDefKind::Type {
            is_synthetic: true,
            ..
        }
    )
}

/// Renders a single generic parameter definition.
fn render_generic_param(param: &GenericParamDef) -> String {
    match &param.kind {
        GenericParamDefKind::Lifetime { outlives } => {
            let mut s = param.name.clone();
            if !outlives.is_empty() {
                s.push_str(": ");
                s.push_str(&outlives.join(" + "));
            }
            s
        }
        GenericParamDefKind::Type {
            bounds,
            default,
            is_synthetic: _,
        } => {
            let mut s = param.name.clone();
            if !bounds.is_empty() {
                s.push_str(": ");
                let bs: Vec<String> = bounds.iter().map(render_generic_bound).collect();
                s.push_str(&bs.join(" + "));
            }
            if let Some(default_ty) = default {
                s.push_str(" = ");
                s.push_str(&render_type(default_ty));
            }
            s
        }
        GenericParamDefKind::Const { type_, default } => {
            let mut s = format!("const {}: {}", param.name, render_type(type_));
            if let Some(d) = default {
                let _ = write!(s, " = {d}");
            }
            s
        }
    }
}

/// Renders a where clause, appending to `out` if non-empty.
fn render_where_clause(generics: &Generics, out: &mut String) {
    if generics.where_predicates.is_empty() {
        return;
    }

    out.push_str(" where ");
    let preds: Vec<String> = generics
        .where_predicates
        .iter()
        .map(render_where_predicate)
        .collect();
    out.push_str(&preds.join(", "));
}

/// Renders a single where predicate.
fn render_where_predicate(pred: &WherePredicate) -> String {
    match pred {
        WherePredicate::BoundPredicate {
            type_,
            bounds,
            generic_params,
        } => {
            let mut s = String::new();
            if !generic_params.is_empty() {
                s.push_str("for<");
                let params: Vec<String> = generic_params.iter().map(render_generic_param).collect();
                s.push_str(&params.join(", "));
                s.push_str("> ");
            }
            s.push_str(&render_type(type_));
            s.push_str(": ");
            let bs: Vec<String> = bounds.iter().map(render_generic_bound).collect();
            s.push_str(&bs.join(" + "));
            s
        }
        WherePredicate::LifetimePredicate { lifetime, outlives } => {
            let mut s = lifetime.clone();
            if !outlives.is_empty() {
                s.push_str(": ");
                s.push_str(&outlives.join(" + "));
            }
            s
        }
        WherePredicate::EqPredicate { lhs, rhs } => {
            format!("{} = {}", render_type(lhs), render_term(rhs))
        }
    }
}

/// Renders a generic bound (trait bound or lifetime).
fn render_generic_bound(bound: &GenericBound) -> String {
    match bound {
        GenericBound::TraitBound {
            trait_,
            generic_params,
            modifier,
        } => {
            let mut s = String::new();
            if !generic_params.is_empty() {
                s.push_str("for<");
                let params: Vec<String> = generic_params.iter().map(render_generic_param).collect();
                s.push_str(&params.join(", "));
                s.push_str("> ");
            }
            match modifier {
                TraitBoundModifier::Maybe => s.push('?'),
                TraitBoundModifier::MaybeConst => s.push_str("~const "),
                TraitBoundModifier::None => {}
            }
            s.push_str(&render_path(trait_));
            s
        }
        GenericBound::Outlives(lt) => lt.clone(),
        GenericBound::Use(args) => {
            let parts: Vec<String> = args
                .iter()
                .map(|a| match a {
                    rustdoc_types::PreciseCapturingArg::Lifetime(lt) => lt.clone(),
                    rustdoc_types::PreciseCapturingArg::Param(p) => p.clone(),
                })
                .collect();
            format!("use<{}>", parts.join(", "))
        }
    }
}

/// Renders a type path with optional generic arguments.
fn render_path(path: &Path) -> String {
    let mut s = path.path.clone();
    if let Some(args) = &path.args {
        s.push_str(&render_generic_args(args));
    }
    s
}

/// Renders generic arguments (the `<...>` or `(...)` part of a path).
fn render_generic_args(args: &GenericArgs) -> String {
    match args {
        GenericArgs::AngleBracketed { args, constraints } => {
            if args.is_empty() && constraints.is_empty() {
                return String::new();
            }
            let mut parts: Vec<String> = args.iter().map(render_generic_arg).collect();
            for c in constraints {
                parts.push(render_assoc_item_constraint(c));
            }
            format!("<{}>", parts.join(", "))
        }
        GenericArgs::Parenthesized { inputs, output } => {
            let inputs_str: Vec<String> = inputs.iter().map(render_type).collect();
            let mut s = format!("({})", inputs_str.join(", "));
            if let Some(out) = output {
                let _ = write!(s, " -> {}", render_type(out));
            }
            s
        }
        GenericArgs::ReturnTypeNotation => "(..)".to_string(),
    }
}

/// Renders a single generic argument.
fn render_generic_arg(arg: &GenericArg) -> String {
    match arg {
        GenericArg::Lifetime(lt) => lt.clone(),
        GenericArg::Type(ty) => render_type(ty),
        GenericArg::Const(c) => c.expr.clone(),
        GenericArg::Infer => "_".to_string(),
    }
}

/// Renders an associated item constraint (e.g. `Item = u32` or `IntoIter: Clone`).
fn render_assoc_item_constraint(c: &AssocItemConstraint) -> String {
    let mut s = c.name.clone();
    if let Some(args) = &c.args {
        s.push_str(&render_generic_args(args));
    }
    match &c.binding {
        AssocItemConstraintKind::Equality(term) => {
            let _ = write!(s, " = {}", render_term(term));
        }
        AssocItemConstraintKind::Constraint(bounds) => {
            s.push_str(": ");
            let bs: Vec<String> = bounds.iter().map(render_generic_bound).collect();
            s.push_str(&bs.join(" + "));
        }
    }
    s
}

/// Renders a term (type or constant).
fn render_term(term: &Term) -> String {
    match term {
        Term::Type(ty) => render_type(ty),
        Term::Constant(c) => c.expr.clone(),
    }
}

/// Renders a `rustdoc_types::Type` to a string.
fn render_type(ty: &Type) -> String {
    match ty {
        Type::ResolvedPath(path) => render_path(path),

        Type::Generic(name) | Type::Primitive(name) => name.clone(),

        Type::BorrowedRef {
            lifetime,
            is_mutable,
            type_,
        } => {
            let mut s = String::from("&");
            if let Some(lt) = lifetime {
                let _ = write!(s, "{lt} ");
            }
            if *is_mutable {
                s.push_str("mut ");
            }
            s.push_str(&render_type(type_));
            s
        }

        Type::RawPointer { is_mutable, type_ } => {
            if *is_mutable {
                format!("*mut {}", render_type(type_))
            } else {
                format!("*const {}", render_type(type_))
            }
        }

        Type::Tuple(types) => {
            if types.is_empty() {
                "()".to_string()
            } else {
                let parts: Vec<String> = types.iter().map(render_type).collect();
                format!("({})", parts.join(", "))
            }
        }

        Type::Slice(ty) => format!("[{}]", render_type(ty)),

        Type::Array { type_, len } => format!("[{}; {len}]", render_type(type_)),

        Type::ImplTrait(bounds) => {
            let bs: Vec<String> = bounds.iter().map(render_generic_bound).collect();
            format!("impl {}", bs.join(" + "))
        }

        Type::DynTrait(dyn_trait) => render_dyn_trait(dyn_trait),

        Type::FunctionPointer(fp) => render_fn_pointer(fp),

        Type::QualifiedPath {
            name,
            args,
            self_type,
            trait_,
        } => {
            let mut s = String::new();
            if let Some(tr) = trait_ {
                let _ = write!(
                    s,
                    "<{} as {}>::{name}",
                    render_type(self_type),
                    render_path(tr)
                );
            } else {
                let _ = write!(s, "{}::{name}", render_type(self_type));
            }
            if let Some(a) = args {
                s.push_str(&render_generic_args(a));
            }
            s
        }

        Type::Infer => "_".to_string(),

        Type::Pat { type_, .. } => render_type(type_),
    }
}

/// Renders a dyn trait type.
fn render_dyn_trait(dt: &DynTrait) -> String {
    let mut parts: Vec<String> = dt
        .traits
        .iter()
        .map(|pt| {
            let mut s = String::new();
            if !pt.generic_params.is_empty() {
                s.push_str("for<");
                let params: Vec<String> =
                    pt.generic_params.iter().map(render_generic_param).collect();
                s.push_str(&params.join(", "));
                s.push_str("> ");
            }
            s.push_str(&render_path(&pt.trait_));
            s
        })
        .collect();

    if let Some(lt) = &dt.lifetime {
        parts.push(lt.clone());
    }

    format!("dyn {}", parts.join(" + "))
}

/// Renders a function pointer type.
fn render_fn_pointer(fp: &FunctionPointer) -> String {
    let mut s = String::new();
    if !fp.generic_params.is_empty() {
        s.push_str("for<");
        let params: Vec<String> = fp.generic_params.iter().map(render_generic_param).collect();
        s.push_str(&params.join(", "));
        s.push_str("> ");
    }
    render_fn_qualifiers(&fp.header, &mut s);
    s.push_str("fn(");
    let params: Vec<String> = fp
        .sig
        .inputs
        .iter()
        .map(|(_, ty)| render_type(ty))
        .collect();
    s.push_str(&params.join(", "));
    if fp.sig.is_c_variadic {
        if !fp.sig.inputs.is_empty() {
            s.push_str(", ");
        }
        s.push_str("...");
    }
    s.push(')');
    if let Some(output) = &fp.sig.output {
        let rendered = render_type(output);
        if rendered != "()" {
            let _ = write!(s, " -> {rendered}");
        }
    }
    s
}

/// Renders function qualifiers (const, async, unsafe, extern).
fn render_fn_qualifiers(header: &FunctionHeader, out: &mut String) {
    if header.is_const {
        out.push_str("const ");
    }
    if header.is_async {
        out.push_str("async ");
    }
    if header.is_unsafe {
        out.push_str("unsafe ");
    }
    render_abi(&header.abi, out);
}

/// Renders an ABI qualifier string, if not the default Rust ABI.
fn render_abi(abi: &Abi, out: &mut String) {
    let abi_str = match abi {
        Abi::Rust => return,
        Abi::C { unwind } => {
            if *unwind {
                "C-unwind"
            } else {
                "C"
            }
        }
        Abi::Cdecl { unwind } => {
            if *unwind {
                "cdecl-unwind"
            } else {
                "cdecl"
            }
        }
        Abi::Stdcall { unwind } => {
            if *unwind {
                "stdcall-unwind"
            } else {
                "stdcall"
            }
        }
        Abi::Fastcall { unwind } => {
            if *unwind {
                "fastcall-unwind"
            } else {
                "fastcall"
            }
        }
        Abi::Aapcs { unwind } => {
            if *unwind {
                "aapcs-unwind"
            } else {
                "aapcs"
            }
        }
        Abi::Win64 { unwind } => {
            if *unwind {
                "win64-unwind"
            } else {
                "win64"
            }
        }
        Abi::SysV64 { unwind } => {
            if *unwind {
                "sysv64-unwind"
            } else {
                "sysv64"
            }
        }
        Abi::System { unwind } => {
            if *unwind {
                "system-unwind"
            } else {
                "system"
            }
        }
        Abi::Other(s) => s.as_str(),
    };
    let _ = write!(out, "extern \"{abi_str}\" ");
}

/// Renders function parameters (the `(...)` part).
fn render_fn_params(sig: &FunctionSignature, out: &mut String) {
    out.push('(');
    let parts: Vec<String> = sig
        .inputs
        .iter()
        .map(|(param_name, ty)| render_fn_param(param_name, ty))
        .collect();
    out.push_str(&parts.join(", "));
    if sig.is_c_variadic {
        if !sig.inputs.is_empty() {
            out.push_str(", ");
        }
        out.push_str("...");
    }
    out.push(')');
}

/// Renders a single function parameter, handling `self` specially.
fn render_fn_param(param_name: &str, ty: &Type) -> String {
    if param_name == "self" {
        match ty {
            Type::BorrowedRef {
                lifetime,
                is_mutable,
                type_: inner,
            } if matches!(inner.as_ref(), Type::Generic(n) if n == "Self") => {
                let mut s = String::from("&");
                if let Some(lt) = lifetime {
                    let _ = write!(s, "{lt} ");
                }
                if *is_mutable {
                    s.push_str("mut ");
                }
                s.push_str("self");
                s
            }
            Type::Generic(n) if n == "Self" => "self".to_string(),
            _ => format!("self: {}", render_type(ty)),
        }
    } else {
        format!("{param_name}: {}", render_type(ty))
    }
}

/// Renders function return type (the ` -> Type` part), omitting for unit return.
fn render_fn_return(sig: &FunctionSignature, out: &mut String) {
    if let Some(output) = &sig.output {
        let rendered = render_type(output);
        if rendered != "()" {
            let _ = write!(out, " -> {rendered}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Loads the fixture crate JSON.
    fn load_fixture() -> Crate {
        let json = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("test-fixtures/groxide_test_api.json"),
        )
        .expect("fixture JSON should exist");
        serde_json::from_str(&json).expect("fixture JSON should parse")
    }

    /// Finds an item by name and inner kind in the fixture crate.
    fn find_item<'a>(krate: &'a Crate, name: &str, kind: &str) -> &'a rustdoc_types::Item {
        krate
            .index
            .values()
            .find(|item| item.name.as_deref() == Some(name) && item_kind_str(&item.inner) == kind)
            .unwrap_or_else(|| panic!("fixture item {kind}:{name} not found"))
    }

    fn item_kind_str(inner: &ItemEnum) -> &'static str {
        match inner {
            ItemEnum::Module(_) => "module",
            ItemEnum::Struct(_) => "struct",
            ItemEnum::Enum(_) => "enum",
            ItemEnum::Union(_) => "union",
            ItemEnum::Trait(_) => "trait",
            ItemEnum::TraitAlias(_) => "trait_alias",
            ItemEnum::Function(_) => "function",
            ItemEnum::TypeAlias(_) => "type_alias",
            ItemEnum::Constant { .. } => "constant",
            ItemEnum::AssocConst { .. } => "assoc_const",
            ItemEnum::Static(_) => "static",
            ItemEnum::Macro(_) => "macro",
            ItemEnum::ProcMacro(_) => "proc_macro",
            ItemEnum::Variant(_) => "variant",
            ItemEnum::StructField(_) => "struct_field",
            ItemEnum::AssocType { .. } => "assoc_type",
            ItemEnum::Impl(_) => "impl",
            ItemEnum::Use(_) => "use",
            ItemEnum::ExternCrate { .. } => "extern_crate",
            ItemEnum::ExternType => "extern_type",
            ItemEnum::Primitive(_) => "primitive",
        }
    }

    // ---- Struct signatures ----

    #[test]
    fn render_produces_struct_with_generic_bound() {
        let krate = load_fixture();
        let item = find_item(&krate, "GenericStruct", "struct");
        let sig = render_signature(item, &krate).unwrap();
        assert_eq!(sig, "pub struct GenericStruct<T: Clone>");
    }

    #[test]
    fn render_produces_struct_with_pub_fields() {
        let krate = load_fixture();
        let item = find_item(&krate, "SimpleStruct", "struct");
        let sig = render_signature(item, &krate).unwrap();
        assert_eq!(
            sig,
            "pub struct SimpleStruct { pub name: String, pub count: u32 }"
        );
    }

    #[test]
    fn render_produces_struct_with_only_private_fields_omits_body() {
        let krate = load_fixture();
        let item = find_item(&krate, "GenericStruct", "struct");
        let sig = render_signature(item, &krate).unwrap();
        assert_eq!(sig, "pub struct GenericStruct<T: Clone>");
    }

    #[test]
    fn render_produces_struct_no_generics() {
        let krate = load_fixture();
        let item = find_item(&krate, "Stack", "struct");
        let sig = render_signature(item, &krate).unwrap();
        assert_eq!(sig, "pub struct Stack<T>");
    }

    // ---- Enum signatures ----

    #[test]
    fn render_produces_enum_no_generics() {
        let krate = load_fixture();
        let item = find_item(&krate, "Direction", "enum");
        let sig = render_signature(item, &krate).unwrap();
        assert_eq!(sig, "pub enum Direction");
    }

    #[test]
    fn render_produces_enum_no_generics_shape() {
        let krate = load_fixture();
        let item = find_item(&krate, "Shape", "enum");
        let sig = render_signature(item, &krate).unwrap();
        assert_eq!(sig, "pub enum Shape");
    }

    // ---- Function signatures ----

    #[test]
    fn render_produces_function_with_params_and_return() {
        let krate = load_fixture();
        let item = find_item(&krate, "add", "function");
        let sig = render_signature(item, &krate).unwrap();
        assert_eq!(sig, "pub fn add(a: i32, b: i32) -> i32");
    }

    #[test]
    fn render_produces_function_with_generics() {
        let krate = load_fixture();
        let item = find_item(&krate, "generic_fn", "function");
        let sig = render_signature(item, &krate).unwrap();
        assert_eq!(
            sig,
            "pub fn generic_fn<T: std::fmt::Display, U: Into<String>>(value: T, _label: U) -> String"
        );
    }

    #[test]
    fn render_produces_function_no_return() {
        let krate = load_fixture();
        let item = find_item(&krate, "push", "function");
        let sig = render_signature(item, &krate).unwrap();
        assert_eq!(sig, "pub fn push(&mut self, value: T)");
    }

    // ---- Trait signatures ----

    #[test]
    fn render_produces_trait_no_bounds() {
        let krate = load_fixture();
        let item = find_item(&krate, "Stringify", "trait");
        let sig = render_signature(item, &krate).unwrap();
        assert_eq!(sig, "pub trait Stringify");
    }

    #[test]
    fn render_produces_trait_with_supertraits() {
        let krate = load_fixture();
        let item = find_item(&krate, "Describable", "trait");
        let sig = render_signature(item, &krate).unwrap();
        assert_eq!(sig, "pub trait Describable: Stringify + std::fmt::Debug");
    }

    // ---- Type alias signatures ----

    #[test]
    fn render_produces_type_alias_with_generics() {
        let krate = load_fixture();
        let item = find_item(&krate, "Result", "type_alias");
        let sig = render_signature(item, &krate).unwrap();
        assert_eq!(sig, "pub type Result<T> = std::result::Result<T, String>");
    }

    #[test]
    fn render_produces_type_alias_fn_pointer() {
        let krate = load_fixture();
        let item = find_item(&krate, "Callback", "type_alias");
        let sig = render_signature(item, &krate).unwrap();
        assert_eq!(sig, "pub type Callback = fn(i32) -> bool");
    }

    // ---- Constant signatures ----

    #[test]
    fn render_produces_constant() {
        let krate = load_fixture();
        let item = find_item(&krate, "MAX_BUFFER_SIZE", "constant");
        let sig = render_signature(item, &krate).unwrap();
        assert_eq!(sig, "pub const MAX_BUFFER_SIZE: usize");
    }

    #[test]
    fn render_produces_constant_str() {
        let krate = load_fixture();
        let item = find_item(&krate, "DEFAULT_GREETING", "constant");
        let sig = render_signature(item, &krate).unwrap();
        assert_eq!(sig, "pub const DEFAULT_GREETING: &str");
    }

    // ---- Macro signatures ----

    #[test]
    fn render_produces_macro_rules() {
        let krate = load_fixture();
        let item = find_item(&krate, "greet", "macro");
        let sig = render_signature(item, &krate).unwrap();
        assert_eq!(sig, "macro_rules! greet");
    }

    // ---- Static signatures ----

    #[test]
    fn render_produces_static() {
        let krate = load_fixture();
        let item = find_item(&krate, "GLOBAL_COUNTER", "static");
        let sig = render_signature(item, &krate).unwrap();
        assert_eq!(
            sig,
            "pub static GLOBAL_COUNTER: std::sync::atomic::AtomicUsize"
        );
    }

    #[test]
    fn render_produces_static_str() {
        let krate = load_fixture();
        let item = find_item(&krate, "VERSION", "static");
        let sig = render_signature(item, &krate).unwrap();
        assert_eq!(sig, "pub static VERSION: &str");
    }

    // ---- Union signatures ----

    #[test]
    fn render_produces_union_with_fields() {
        let krate = load_fixture();
        let item = find_item(&krate, "IntOrFloat", "union");
        let sig = render_signature(item, &krate).unwrap();
        assert_eq!(sig, "pub union IntOrFloat { pub i: i32, pub f: f32 }");
    }

    // ---- Variant signatures ----

    #[test]
    fn render_produces_plain_variant() {
        let krate = load_fixture();
        let item = find_item(&krate, "North", "variant");
        let sig = render_signature(item, &krate).unwrap();
        assert_eq!(sig, "North");
    }

    #[test]
    fn render_produces_tuple_variant() {
        let krate = load_fixture();
        let item = find_item(&krate, "Circle", "variant");
        let sig = render_signature(item, &krate).unwrap();
        assert_eq!(sig, "Circle(f64)");
    }

    #[test]
    fn render_produces_struct_variant() {
        let krate = load_fixture();
        let item = find_item(&krate, "Rectangle", "variant");
        let sig = render_signature(item, &krate).unwrap();
        assert_eq!(sig, "Rectangle { width: f64, height: f64 }");
    }

    // ---- Field signatures ----

    #[test]
    fn render_produces_field() {
        let krate = load_fixture();
        let item = find_item(&krate, "name", "struct_field");
        let sig = render_signature(item, &krate).unwrap();
        assert_eq!(sig, "pub name: String");
    }

    // ---- Module signatures ----

    #[test]
    fn render_produces_module() {
        let krate = load_fixture();
        let item = find_item(&krate, "containers", "module");
        let sig = render_signature(item, &krate).unwrap();
        assert_eq!(sig, "pub mod containers");
    }

    // ---- Associated type signatures ----

    #[test]
    fn render_produces_assoc_type() {
        let krate = load_fixture();
        let item = find_item(&krate, "Input", "assoc_type");
        let sig = render_signature(item, &krate).unwrap();
        assert_eq!(sig, "type Input");
    }

    // ---- Associated const signatures ----

    #[test]
    fn render_produces_assoc_const() {
        let krate = load_fixture();
        let item = find_item(&krate, "MAX_ITEMS", "assoc_const");
        let sig = render_signature(item, &krate).unwrap();
        assert_eq!(sig, "const MAX_ITEMS: usize");
    }

    // ---- Method signatures (functions inside impl blocks) ----

    #[test]
    fn render_produces_method_with_self_ref() {
        let krate = load_fixture();
        let item = find_item(&krate, "value", "function");
        let sig = render_signature(item, &krate).unwrap();
        assert_eq!(sig, "pub fn value(&self) -> &T");
    }

    #[test]
    fn render_produces_method_consuming_self() {
        let krate = load_fixture();
        let item = find_item(&krate, "into_inner", "function");
        let sig = render_signature(item, &krate).unwrap();
        assert_eq!(sig, "pub fn into_inner(self) -> T");
    }

    #[test]
    fn render_produces_method_with_return_self() {
        let krate = load_fixture();
        let new_items: Vec<_> = krate
            .index
            .values()
            .filter(|i| {
                i.name.as_deref() == Some("new") && matches!(&i.inner, ItemEnum::Function(_))
            })
            .collect();
        let sigs: Vec<_> = new_items
            .iter()
            .map(|i| render_signature(i, &krate).unwrap())
            .collect();
        assert!(
            sigs.iter().any(|s| s == "pub fn new(value: T) -> Self"),
            "Expected 'pub fn new(value: T) -> Self' among: {sigs:?}"
        );
    }

    // ---- Impl and Use items return None ----

    #[test]
    fn render_returns_none_for_impl() {
        let krate = load_fixture();
        let item = krate
            .index
            .values()
            .find(|i| matches!(&i.inner, ItemEnum::Impl(_)))
            .expect("fixture should have impl items");
        assert!(render_signature(item, &krate).is_none());
    }

    #[test]
    fn render_returns_none_for_use() {
        let krate = load_fixture();
        let use_item = krate
            .index
            .values()
            .find(|i| matches!(&i.inner, ItemEnum::Use(_)));
        if let Some(item) = use_item {
            assert!(render_signature(item, &krate).is_none());
        }
    }

    // ---- Deep function ----

    #[test]
    fn render_produces_deep_fn() {
        let krate = load_fixture();
        let item = find_item(&krate, "deep_fn", "function");
        let sig = render_signature(item, &krate).unwrap();
        assert_eq!(sig, "pub fn deep_fn() -> &'static str");
    }

    // ---- Trait method signatures ----

    #[test]
    fn render_produces_trait_required_method() {
        let krate = load_fixture();
        let item = krate
            .index
            .values()
            .find(|i| {
                i.name.as_deref() == Some("stringify")
                    && matches!(&i.inner, ItemEnum::Function(f) if !f.has_body)
            })
            .expect("fixture should have stringify required method");
        let sig = render_signature(item, &krate).unwrap();
        assert_eq!(sig, "fn stringify(&self) -> String");
    }

    #[test]
    fn render_produces_trait_provided_method() {
        let krate = load_fixture();
        let item = find_item(&krate, "debug_string", "function");
        let sig = render_signature(item, &krate).unwrap();
        assert_eq!(sig, "fn debug_string(&self) -> String");
    }

    // ---- All item kinds produce reasonable signatures ----

    #[test]
    fn render_covers_all_item_kinds_in_fixture() {
        let krate = load_fixture();

        let mut covered_kinds = std::collections::HashSet::new();
        let mut none_kinds = std::collections::HashSet::new();

        for item in krate.index.values() {
            let kind = item_kind_str(&item.inner);
            match render_signature(item, &krate) {
                Some(sig) => {
                    assert!(
                        !sig.is_empty(),
                        "empty sig for {kind}:{}",
                        item.name.as_deref().unwrap_or("?")
                    );
                    covered_kinds.insert(kind);
                }
                None => {
                    none_kinds.insert(kind);
                }
            }
        }

        let expected_some = [
            "struct",
            "enum",
            "union",
            "trait",
            "function",
            "type_alias",
            "constant",
            "static",
            "macro",
            "variant",
            "struct_field",
            "module",
            "assoc_type",
            "assoc_const",
        ];
        for kind in expected_some {
            assert!(
                covered_kinds.contains(kind),
                "Expected {kind} to produce a signature, but it was not found in covered kinds: {covered_kinds:?}"
            );
        }

        let expected_none = ["impl"];
        for kind in expected_none {
            assert!(
                none_kinds.contains(kind),
                "Expected {kind} to return None, but it was not found in none kinds: {none_kinds:?}"
            );
        }
    }
}
