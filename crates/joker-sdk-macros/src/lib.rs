//! The macros of the Joker Rust SDK (ref/04 §4.2–§4.3): `#[joker::app]` and
//! `#[joker::cycle]`. Use them through `joker-sdk` (`joker::app`,
//! `joker::cycle`); the generated code refers to the crate as `::joker`.

mod cbor;

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{
    Attribute, Expr, ExprLit, Fields, FnArg, GenericArgument, ItemFn, ItemStruct, Lit, Meta,
    PathArguments, ReturnType, Token, Type,
};

use crate::cbor::Cbor;

/// The world the SDK generates (ref/04 §4.1: `joker-sdk 0.y` gives `joker:runtime@0.y.0`).
const WORLD: &str = "joker:runtime@0.1.0";
const SDK_VERSION: &str = env!("CARGO_PKG_VERSION");
const RESERVED: &[&str] = &[
    "now",
    "dt",
    "kv",
    "secrets",
    "snapshot",
    "subscribe",
    "sleep",
    "sleep_until",
    "log",
    "cmd",
    "state",
    "http",
    "random",
    "meta",
];

fn err(span: Span, message: impl std::fmt::Display) -> syn::Error {
    syn::Error::new(span, message.to_string())
}

/// Declares the app struct (ref/04 §4.2).
///
/// ```ignore
/// #[joker::app(name = "light", version = "0.1.0")]
/// pub struct Light {
///     #[output]
///     led: Output<bool>,
///     #[param(unit = "ms", range = "20..5000", default = 500.0)]
///     half_period: Param<f64>,
/// }
/// ```
///
/// Arguments: `name` (default: the package name with `-` → `_`), `version`
/// (default: the package version) and `proposal_buffer` (`"16 KiB"`). Each
/// field has one of `#[input]`, `#[output]` or `#[param]`, with `unit`,
/// `quantity`, `optional`, and for parameters `range` and `default`.
#[proc_macro_attribute]
pub fn app(attr: TokenStream, item: TokenStream) -> TokenStream {
    match expand_app(attr.into(), item.into()) {
        Ok(tokens) => tokens.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

/// Declares a cycle task (ref/04 §4.3.1).
///
/// ```ignore
/// #[joker::cycle(period = "20ms", wcet = "5us")]
/// fn blink(io: &mut CycleIo<Light>, st: &mut Blink) -> Result<()> { … }
/// ```
///
/// `period` is required; `wcet` must not exceed it. The state type, when
/// there is one, starts at `Default`.
#[proc_macro_attribute]
pub fn cycle(attr: TokenStream, item: TokenStream) -> TokenStream {
    match expand_cycle(attr.into(), item.into()) {
        Ok(tokens) => tokens.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

fn args(attr: TokenStream2) -> syn::Result<Vec<Meta>> {
    Ok(Punctuated::<Meta, Token![,]>::parse_terminated
        .parse2(attr)?
        .into_iter()
        .collect())
}

fn lit_str(expr: &Expr) -> syn::Result<String> {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Str(s), ..
        }) => Ok(s.value()),
        other => Err(err(other.span(), "expected a string literal")),
    }
}

fn lit_number(expr: &Expr) -> syn::Result<f64> {
    let (negative, inner) = match expr {
        Expr::Unary(u) if matches!(u.op, syn::UnOp::Neg(_)) => (true, &*u.expr),
        other => (false, other),
    };
    let v = match inner {
        Expr::Lit(ExprLit {
            lit: Lit::Float(f), ..
        }) => f.base10_parse::<f64>()?,
        Expr::Lit(ExprLit {
            lit: Lit::Int(i), ..
        }) => i.base10_parse::<f64>()?,
        Expr::Lit(ExprLit {
            lit: Lit::Bool(b), ..
        }) => f64::from(u8::from(b.value)),
        other => return Err(err(other.span(), "expected a number")),
    };
    Ok(if negative { -v } else { v })
}

/// Parses a duration literal (`"20ms"`, `"5us"`, `"1.5s"`, `"90m"`) into ns.
fn duration_ns(text: &str, span: Span) -> syn::Result<u64> {
    let bad = || {
        err(
            span,
            format!(
                "J-E0705: invalid duration `{text}`; expected e.g. \"200ms\", \"1.5s\", \"90m\""
            ),
        )
    };
    let t = text.trim();
    let split = t
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .ok_or_else(bad)?;
    let (number, unit) = t.split_at(split);
    let value: f64 = number.parse().map_err(|_| bad())?;
    let scale = match unit.trim() {
        "ns" => 1.0,
        "us" | "µs" => 1e3,
        "ms" => 1e6,
        "s" => 1e9,
        "m" | "min" => 60e9,
        "h" => 3600e9,
        "d" => 86_400e9,
        _ => return Err(bad()),
    };
    let ns = value * scale;
    if !ns.is_finite() || ns < 0.0 || ns.fract() != 0.0 || ns > u64::MAX as f64 {
        return Err(bad());
    }
    Ok(ns as u64)
}

fn size_bytes(text: &str, span: Span) -> syn::Result<u64> {
    let t = text.trim();
    let split = t.find(|c: char| !c.is_ascii_digit()).unwrap_or(t.len());
    let (number, unit) = t.split_at(split);
    let n: u64 = number.parse().map_err(|_| {
        err(
            span,
            format!("invalid size `{text}`; expected e.g. \"16 KiB\""),
        )
    })?;
    let scale = match unit.trim() {
        "" | "B" => 1,
        "KiB" => 1024,
        "MiB" => 1024 * 1024,
        _ => {
            return Err(err(
                span,
                format!("invalid size `{text}`; expected e.g. \"16 KiB\""),
            ));
        }
    };
    Ok(n * scale)
}

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some('a'..='z'))
        && s.len() <= 32
        && chars.all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_'))
}

fn doc_of(attrs: &[Attribute]) -> Option<String> {
    let mut lines = Vec::new();
    for attr in attrs {
        if !attr.path().is_ident("doc") {
            continue;
        }
        if let Meta::NameValue(nv) = &attr.meta
            && let Ok(s) = lit_str(&nv.value)
        {
            lines.push(s.trim().to_owned());
        }
    }
    let first: Vec<&str> = lines
        .iter()
        .map(String::as_str)
        .skip_while(|l| l.is_empty())
        .take_while(|l| !l.is_empty())
        .collect();
    (!first.is_empty()).then(|| first.join(" "))
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum Kind {
    Input,
    Output,
    Param,
}

struct Port {
    field: syn::Ident,
    vis: syn::Visibility,
    kind: Kind,
    ty: Type,
    meta: Cbor,
}

fn value_type(ty: &Type) -> Option<(u64, bool)> {
    let Type::Path(p) = ty else { return None };
    let ident = p.path.get_ident()?.to_string();
    Some(match ident.as_str() {
        "bool" => (0, false),
        "i64" => (1, true),
        "u64" => (2, true),
        "f64" => (3, true),
        "f32" => (4, true),
        _ => return None,
    })
}

fn declaration(ty: &Type) -> Option<(String, Type)> {
    let Type::Path(p) = ty else { return None };
    let last = p.path.segments.last()?;
    let PathArguments::AngleBracketed(a) = &last.arguments else {
        return None;
    };
    let Some(GenericArgument::Type(inner)) = a.args.first() else {
        return None;
    };
    Some((last.ident.to_string(), inner.clone()))
}

fn suggestion(ty: &Type) -> &'static str {
    let text = quote!(#ty).to_string();
    match text.as_str() {
        "i8" | "i16" | "i32" | "isize" => "i64",
        "u8" | "u16" | "u32" | "usize" => "u64",
        "String" | "& str" => "BoundedString<N>",
        _ => "bool, i64, u64, f64 or f32",
    }
}

fn parse_port(field: &syn::Field) -> syn::Result<(Port, Vec<Attribute>)> {
    let ident = field
        .ident
        .clone()
        .ok_or_else(|| err(field.span(), "the app struct has named fields"))?;
    let name = ident.to_string();
    if !is_ident(&name) || name.starts_with("joker") || RESERVED.contains(&name.as_str()) {
        return Err(err(
            ident.span(),
            format!("J-E0709: port name `{name}` is reserved or invalid"),
        ));
    }
    let mut kind = None;
    let mut args_of = Vec::new();
    let mut kept = Vec::new();
    for attr in &field.attrs {
        let k = if attr.path().is_ident("input") {
            Kind::Input
        } else if attr.path().is_ident("output") {
            Kind::Output
        } else if attr.path().is_ident("param") {
            Kind::Param
        } else {
            kept.push(attr.clone());
            continue;
        };
        if kind.replace(k).is_some() {
            return Err(err(attr.span(), "a port has exactly one port attribute"));
        }
        if let Meta::List(list) = &attr.meta {
            args_of = args(list.tokens.clone())?;
        }
    }
    let kind = kind.ok_or_else(|| {
        err(
            ident.span(),
            format!("field `{name}` needs a port attribute: #[input], #[output] or #[param]"),
        )
    })?;
    let (decl, inner) = declaration(&field.ty).ok_or_else(|| {
        err(
            field.ty.span(),
            "expected a declaration type: Input<T>, Output<T> or Param<T>",
        )
    })?;
    let expected = match kind {
        Kind::Input => "Input",
        Kind::Output => "Output",
        Kind::Param => "Param",
    };
    if decl != expected {
        return Err(err(
            field.ty.span(),
            format!(
                "#[{}] needs a field of type {expected}<T>",
                expected.to_lowercase()
            ),
        ));
    }
    let (code, numeric) = value_type(&inner).ok_or_else(|| {
        let text = quote!(#inner).to_string();
        err(
            inner.span(),
            format!(
                "J-E0707: `{text}` is not a port type; use {}",
                suggestion(&inner)
            ),
        )
    })?;
    let mut unit = None;
    let mut quantity = None;
    let mut optional = false;
    let mut range: Option<(Option<f64>, Option<f64>)> = None;
    let mut default = None;
    for arg in &args_of {
        match arg {
            Meta::NameValue(nv) if nv.path.is_ident("unit") => unit = Some(lit_str(&nv.value)?),
            Meta::NameValue(nv) if nv.path.is_ident("quantity") => {
                quantity = Some(lit_str(&nv.value)?)
            }
            Meta::Path(p) if p.is_ident("optional") && kind != Kind::Param => optional = true,
            Meta::NameValue(nv) if nv.path.is_ident("range") && kind == Kind::Param => {
                range = Some(parse_range(&lit_str(&nv.value)?, nv.value.span())?);
            }
            Meta::NameValue(nv) if nv.path.is_ident("default") && kind == Kind::Param => {
                default = Some((lit_number(&nv.value)?, nv.value.span()));
            }
            other => {
                return Err(err(
                    other.span(),
                    "unknown or unsupported port argument (the M0 SDK serves unit, quantity, optional, range and default)",
                ));
            }
        }
    }
    if numeric && unit.is_none() {
        return Err(err(
            ident.span(),
            format!(
                "J-E0703: numeric port `{name}` needs a unit; use unit = \"1\" for a dimensionless value"
            ),
        ));
    }
    if let (Some((min, max)), Some((v, span))) = (range, default)
        && (min.is_some_and(|m| v < m) || max.is_some_and(|m| v > m))
    {
        return Err(err(
            span,
            format!("J-E0706: default {v} is outside range {min:?}..{max:?}"),
        ));
    }
    let mut meta = vec![
        (0, Cbor::Text(name.clone())),
        (
            1,
            Cbor::Uint(match kind {
                Kind::Input => 0,
                Kind::Output => 1,
                Kind::Param => 2,
            }),
        ),
        (2, Cbor::Uint(code)),
    ];
    if let Some(u) = unit {
        meta.push((3, Cbor::Text(u)));
    }
    if let Some(q) = quantity {
        meta.push((4, Cbor::Text(q)));
    }
    if optional {
        meta.push((5, Cbor::Bool(true)));
    }
    if let Some((min, max)) = range {
        let mut r = Vec::new();
        if let Some(m) = min {
            r.push((0, number(m)));
        }
        if let Some(m) = max {
            r.push((1, number(m)));
        }
        meta.push((7, Cbor::Map(r)));
    }
    if let Some((v, _)) = default {
        meta.push((
            8,
            if code == 0 {
                Cbor::Bool(v != 0.0)
            } else {
                Cbor::Float(v)
            },
        ));
    }
    if let Some(d) = doc_of(&field.attrs) {
        meta.push((10, Cbor::Text(d)));
    }
    Ok((
        Port {
            field: ident,
            vis: field.vis.clone(),
            kind,
            ty: inner,
            meta: Cbor::Map(meta),
        },
        kept,
    ))
}

fn number(v: f64) -> Cbor {
    if v >= 0.0 && v.fract() == 0.0 && v < 9.0e15 {
        Cbor::Uint(v as u64)
    } else {
        Cbor::Float(v)
    }
}

fn parse_range(text: &str, span: Span) -> syn::Result<(Option<f64>, Option<f64>)> {
    let bad = || {
        err(
            span,
            format!("J-E0706: invalid range `{text}`; expected \"a..b\""),
        )
    };
    let (a, b) = text.split_once("..").ok_or_else(bad)?;
    let b = b.strip_prefix('=').unwrap_or(b);
    let parse = |s: &str| -> syn::Result<Option<f64>> {
        let s = s.trim();
        if s.is_empty() {
            Ok(None)
        } else {
            s.parse::<f64>().map(Some).map_err(|_| bad())
        }
    };
    let (min, max) = (parse(a)?, parse(b)?);
    if let (Some(lo), Some(hi)) = (min, max)
        && lo > hi
    {
        return Err(bad());
    }
    Ok((min, max))
}

fn bytes_tokens(bytes: &[u8]) -> TokenStream2 {
    let len = bytes.len();
    quote!(#len, [#(#bytes),*])
}

fn expand_app(attr: TokenStream2, item: TokenStream2) -> syn::Result<TokenStream2> {
    let mut item: ItemStruct = syn::parse2(item)?;
    let mut name = std::env::var("CARGO_PKG_NAME")
        .unwrap_or_default()
        .replace('-', "_");
    let mut version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let mut capacity = None;
    for arg in args(attr)? {
        match &arg {
            Meta::NameValue(nv) if nv.path.is_ident("name") => name = lit_str(&nv.value)?,
            Meta::NameValue(nv) if nv.path.is_ident("version") => version = lit_str(&nv.value)?,
            Meta::NameValue(nv) if nv.path.is_ident("proposal_buffer") => {
                let n = size_bytes(&lit_str(&nv.value)?, nv.value.span())?;
                if !(1024..=1024 * 1024).contains(&n) {
                    return Err(err(
                        nv.value.span(),
                        "proposal_buffer must be between 1 KiB and 1 MiB",
                    ));
                }
                capacity = Some(n);
            }
            other => {
                return Err(err(
                    other.span(),
                    "unknown or unsupported app argument (the M0 SDK serves name, version and proposal_buffer)",
                ));
            }
        }
    }
    if !is_ident(&name) || name.starts_with("joker") {
        return Err(err(
            Span::call_site(),
            format!(
                "J-E0710: app name `{name}` must match [a-z][a-z0-9_]{{0,31}} and not start with \"joker\""
            ),
        ));
    }
    if !is_semver(&version) {
        return Err(err(
            Span::call_site(),
            format!("J-E0710: app version `{version}` is not SemVer"),
        ));
    }
    if !item.generics.params.is_empty() {
        return Err(err(
            item.generics.span(),
            "the app struct has no generic parameters",
        ));
    }
    let Fields::Named(fields) = &mut item.fields else {
        return Err(err(item.span(), "the app struct has named fields"));
    };
    let mut ports = Vec::new();
    for field in fields.named.iter_mut() {
        let (port, kept) = parse_port(field)?;
        field.attrs = kept;
        ports.push(port);
    }
    let mut meta = vec![
        (0, Cbor::Uint(1)),
        (1, Cbor::Text(name.clone())),
        (2, Cbor::Text(version)),
        (3, Cbor::Text(WORLD.to_owned())),
        (
            4,
            Cbor::Map(vec![
                (0, Cbor::Text("rust".to_owned())),
                (1, Cbor::Text(SDK_VERSION.to_owned())),
                (2, Cbor::Uint(0)),
                (5, Cbor::Text("joker-sdk".to_owned())),
            ]),
        ),
        (5, Cbor::Uint(1)),
        (
            6,
            Cbor::Array(ports.iter().map(|p| p.meta.clone()).collect()),
        ),
        (7, Cbor::Array(Vec::new())),
        (8, Cbor::Array(Vec::new())),
        (10, Cbor::Array(Vec::new())),
        (11, Cbor::Array(Vec::new())),
    ];
    if let Some(c) = capacity {
        meta.push((16, Cbor::Map(vec![(0, Cbor::Uint(c))])));
    }
    if let Some(d) = doc_of(&item.attrs) {
        meta.push((17, Cbor::Text(d)));
    }
    let fragment = Cbor::Map(vec![(0, Cbor::Uint(0)), (1, Cbor::Map(meta))]).to_bytes();
    let fragment = bytes_tokens(&fragment);
    let app = &item.ident;
    let view = format_ident!("__{}Cycle", app);
    let vis = &item.vis;
    let view_fields = ports.iter().map(|p| {
        let (field, fvis, ty) = (&p.field, &p.vis, &p.ty);
        let handle = match p.kind {
            Kind::Input => quote!(::joker::CycleInput<'a, #ty>),
            Kind::Output => quote!(::joker::CycleOutput<'a, #ty>),
            Kind::Param => quote!(::joker::ParamHandle<'a, #ty>),
        };
        quote!(#fvis #field: #handle)
    });
    let view_init = ports.iter().enumerate().map(|(i, p)| {
        let field = &p.field;
        let name = field.to_string();
        let index = i as u32;
        let ctor = match p.kind {
            Kind::Input => quote!(::joker::CycleInput),
            Kind::Output => quote!(::joker::CycleOutput),
            Kind::Param => quote!(::joker::ParamHandle),
        };
        quote!(#field: #ctor::__new(cx, #index, #name))
    });
    let decls = ports.iter().map(|p| {
        let name = p.field.to_string();
        let ty = &p.ty;
        let kind = match p.kind {
            Kind::Input => quote!(Input),
            Kind::Output => quote!(Output),
            Kind::Param => quote!(Param),
        };
        quote!(::joker::__private::PortDecl {
            name: #name,
            kind: ::joker::__private::PortKind::#kind,
            ty: <#ty as ::joker::PortValue>::TYPE,
        })
    });
    let doc = format!("The cycle view of [`{app}`], generated by `#[joker::app]`.");
    let derives_debug = item.attrs.iter().any(|a| {
        a.path().is_ident("derive")
            && a.parse_args_with(Punctuated::<syn::Path, Token![,]>::parse_terminated)
                .is_ok_and(|paths| {
                    paths
                        .iter()
                        .any(|p| p.segments.last().is_some_and(|s| s.ident == "Debug"))
                })
    });
    let debug = (!derives_debug).then(|| {
        let text = app.to_string();
        quote! {
            impl ::core::fmt::Debug for #app {
                fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                    f.write_str(#text)
                }
            }
        }
    });
    Ok(quote! {
        #[allow(dead_code)]
        #item

        #debug

        #[doc = #doc]
        #[allow(non_camel_case_types, dead_code)]
        #vis struct #view<'a> {
            #(#view_fields,)*
        }

        impl ::joker::App for #app {
            type Cycle<'a> = #view<'a>;
            const NAME: &'static str = #name;
            const PORTS: &'static [::joker::__private::PortDecl] = &[#(#decls),*];

            fn cycle_view<'a>(cx: &'a ::joker::__private::CycleCx<'a>) -> Self::Cycle<'a> {
                #view { #(#view_init,)* }
            }
        }

        ::joker::__private::sys::meta_fragment!(__JOKER_META_APP, #fragment);

        ::joker::__private::inventory::submit! {
            ::joker::__private::AppEntry {
                name: #name,
                ports: <#app as ::joker::App>::PORTS,
            }
        }

        ::joker::__private::sys::export_guest!(::joker::__private::Runtime);
    })
}

fn is_semver(v: &str) -> bool {
    let core = v.split(['-', '+']).next().unwrap_or("");
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.chars().all(|c| c.is_ascii_digit())
                && (p.len() == 1 || !p.starts_with('0'))
        })
}

fn view_app(arg: &FnArg) -> Option<Type> {
    let FnArg::Typed(pt) = arg else { return None };
    let Type::Reference(r) = &*pt.ty else {
        return None;
    };
    r.mutability?;
    let (name, app) = declaration(&r.elem)?;
    (name == "CycleIo").then_some(app)
}

fn state_type(arg: &FnArg) -> Option<Type> {
    let FnArg::Typed(pt) = arg else { return None };
    let Type::Reference(r) = &*pt.ty else {
        return None;
    };
    r.mutability?;
    Some((*r.elem).clone())
}

fn expand_cycle(attr: TokenStream2, item: TokenStream2) -> syn::Result<TokenStream2> {
    let func: ItemFn = syn::parse2(item)?;
    let ident = &func.sig.ident;
    let name = ident.to_string();
    let signature_error = || {
        err(
            func.sig.span(),
            "J-E0712: expected `fn(&mut CycleIo<App>, &mut State) -> Result<()>` or `fn(&mut CycleIo<App>) -> Result<()>`",
        )
    };
    if !is_ident(&name) {
        return Err(err(
            ident.span(),
            format!("task name `{name}` must match [a-z][a-z0-9_]{{0,31}}"),
        ));
    }
    if func.sig.asyncness.is_some()
        || !func.sig.generics.params.is_empty()
        || matches!(func.sig.output, ReturnType::Default)
    {
        return Err(signature_error());
    }
    let inputs: Vec<&FnArg> = func.sig.inputs.iter().collect();
    let app = inputs
        .first()
        .and_then(|a| view_app(a))
        .ok_or_else(signature_error)?;
    let state = match inputs.as_slice() {
        [_] => None,
        [_, s] => Some(state_type(s).ok_or_else(signature_error)?),
        _ => return Err(signature_error()),
    };
    let mut period = None;
    let mut wcet = None;
    let mut optional = false;
    for arg in args(attr)? {
        match &arg {
            Meta::NameValue(nv) if nv.path.is_ident("period") => {
                period = Some(duration_ns(&lit_str(&nv.value)?, nv.value.span())?);
            }
            Meta::NameValue(nv) if nv.path.is_ident("wcet") => {
                wcet = Some((
                    duration_ns(&lit_str(&nv.value)?, nv.value.span())?,
                    nv.value.span(),
                ));
            }
            Meta::Path(p) if p.is_ident("optional") => optional = true,
            Meta::NameValue(nv) if nv.path.is_ident("optional") => {
                optional =
                    matches!(&nv.value, Expr::Lit(ExprLit { lit: Lit::Bool(b), .. }) if b.value);
            }
            other => {
                return Err(err(
                    other.span(),
                    "unknown or unsupported cycle argument (the M0 SDK serves period, wcet and optional)",
                ));
            }
        }
    }
    let period = period.ok_or_else(|| {
        err(
            Span::call_site(),
            format!("J-E0714: cycle task `{name}` needs period = \"…\""),
        )
    })?;
    if period < 100_000 {
        return Err(err(
            Span::call_site(),
            format!("J-E0714: cycle task `{name}`: the period is below 100 µs"),
        ));
    }
    let mut task = vec![
        (0, Cbor::Text(name.clone())),
        (1, Cbor::Uint(0)),
        (2, Cbor::Uint(period)),
    ];
    if let Some((w, span)) = wcet {
        if w > period {
            return Err(err(
                span,
                format!("J-E0714: cycle task `{name}`: wcet is above the period"),
            ));
        }
        task.push((3, Cbor::Uint(w)));
    }
    if optional {
        task.push((4, Cbor::Bool(true)));
    }
    if let Some(d) = doc_of(&func.attrs) {
        task.push((8, Cbor::Text(d)));
    }
    let fragment = Cbor::Map(vec![(0, Cbor::Uint(1)), (1, Cbor::Map(task))]).to_bytes();
    let fragment = bytes_tokens(&fragment);
    let section = format_ident!("__JOKER_META_TASK_{}", name.to_uppercase());
    let (run, init) = match state {
        Some(s) => (
            quote!(cx.run::<#app, #s>(#ident)),
            quote!(::joker::__private::new_state::<#s>),
        ),
        None => (
            quote!(cx.run_stateless::<#app>(#ident)),
            quote!(|| ::core::option::Option::None),
        ),
    };
    Ok(quote! {
        #func

        ::joker::__private::sys::meta_fragment!(#section, #fragment);

        ::joker::__private::inventory::submit! {
            ::joker::__private::CycleTask {
                name: #name,
                run: |cx: &mut ::joker::__private::TaskCx<'_>| #run,
                state: #init,
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        let s = Span::call_site();
        assert_eq!(duration_ns("20ms", s).unwrap(), 20_000_000);
        assert_eq!(duration_ns("5us", s).unwrap(), 5_000);
        assert_eq!(duration_ns("1.5s", s).unwrap(), 1_500_000_000);
        assert_eq!(duration_ns("90m", s).unwrap(), 5_400_000_000_000);
        assert!(duration_ns("20", s).is_err());
        assert!(duration_ns("ms", s).is_err());
        assert!(duration_ns("0.5ns", s).is_err());
    }

    #[test]
    fn ranges_sizes_and_versions() {
        let s = Span::call_site();
        assert_eq!(
            parse_range("20..5000", s).unwrap(),
            (Some(20.0), Some(5000.0))
        );
        assert_eq!(parse_range("0..=1", s).unwrap(), (Some(0.0), Some(1.0)));
        assert_eq!(parse_range("..3", s).unwrap(), (None, Some(3.0)));
        assert!(parse_range("5..1", s).is_err());
        assert_eq!(size_bytes("16 KiB", s).unwrap(), 16384);
        assert!(is_semver("0.1.0"));
        assert!(is_semver("1.4.2-rc.1"));
        assert!(!is_semver("0.1"));
        assert!(!is_semver("01.1.0"));
    }

    #[test]
    fn the_light_app_fragment() {
        let item: TokenStream2 = quote! {
            /// Blinks an LED.
            pub struct Light {
                #[output]
                led: Output<bool>,
                #[param(unit = "ms", range = "20..5000", default = 500.0)]
                half_period: Param<f64>,
            }
        };
        let out = expand_app(quote!(name = "light", version = "0.1.0"), item)
            .unwrap()
            .to_string();
        assert!(out.contains("__JOKER_META_APP"));
        assert!(out.contains("export_guest"));
        let bad = quote! { pub struct Light { #[input] t: Input<f64> } };
        let e = expand_app(quote!(name = "light"), bad)
            .unwrap_err()
            .to_string();
        assert!(e.starts_with("J-E0703"), "{e}");
        let bad = quote! { pub struct Light { #[input(unit = "1")] t: Input<i32> } };
        let e = expand_app(quote!(name = "light"), bad)
            .unwrap_err()
            .to_string();
        assert!(e.starts_with("J-E0707"), "{e}");
        let bad = quote! { pub struct Light { #[param(unit = "1", range = "0..1", default = 2.0)] k: Param<f64> } };
        let e = expand_app(quote!(name = "k"), bad).unwrap_err().to_string();
        assert!(e.starts_with("J-E0706"), "{e}");
        let e = expand_app(
            quote!(name = "Joker"),
            quote!(
                pub struct A {}
            ),
        )
        .unwrap_err()
        .to_string();
        assert!(e.starts_with("J-E0710"), "{e}");
    }

    #[test]
    fn cycle_signatures() {
        let ok =
            quote! { fn blink(io: &mut CycleIo<Light>, st: &mut Blink) -> Result<()> { Ok(()) } };
        assert!(expand_cycle(quote!(period = "20ms", wcet = "5us"), ok.clone()).is_ok());
        let e = expand_cycle(quote!(wcet = "5us"), ok.clone())
            .unwrap_err()
            .to_string();
        assert!(e.starts_with("J-E0714"), "{e}");
        let e = expand_cycle(quote!(period = "20ms", wcet = "30ms"), ok)
            .unwrap_err()
            .to_string();
        assert!(e.starts_with("J-E0714"), "{e}");
        let bad = quote! { fn blink(io: &CycleIo<Light>) -> Result<()> { Ok(()) } };
        let e = expand_cycle(quote!(period = "20ms"), bad)
            .unwrap_err()
            .to_string();
        assert!(e.starts_with("J-E0712"), "{e}");
    }
}
