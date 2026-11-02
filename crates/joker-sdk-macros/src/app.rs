//! `#[joker::app]` (ref/04 §4.2).

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{Attribute, Fields, ItemStruct, Meta, Token, Type};

use crate::cbor::Cbor;
use crate::util::{
    args, const_arg, doc_of, duration_arg, err, fragment, generic, is_ident, is_semver, lit_number,
    lit_str, size_bytes, type_arg, type_text,
};

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

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Kind {
    Input,
    Output,
    Param,
    Internal,
    Published,
    Counter,
    EventOut,
    Alarm,
}

impl Kind {
    fn code(self) -> u64 {
        match self {
            Kind::Input => 0,
            Kind::Output => 1,
            Kind::Param => 2,
            Kind::Internal => 3,
            Kind::Published => 4,
            Kind::Counter => 5,
            Kind::EventOut => 7,
            Kind::Alarm => 10,
        }
    }

    fn variant(self) -> TokenStream {
        match self {
            Kind::Input => quote!(Input),
            Kind::Output => quote!(Output),
            Kind::Param => quote!(Param),
            Kind::Internal => quote!(Internal),
            Kind::Published => quote!(Published),
            Kind::Counter => quote!(Counter),
            Kind::EventOut => quote!(EventOut),
            Kind::Alarm => quote!(Alarm),
        }
    }
}

struct Port {
    field: syn::Ident,
    vis: syn::Visibility,
    kind: Kind,
    inner: Option<Type>,
    cycle_writer: bool,
    ty_code: TokenStream,
    meta: Cbor,
    checks: Vec<TokenStream>,
}

/// The signal type of a scalar port type: its code, whether it is numeric,
/// and the extra `joker:meta` keys.
type ValueType = (Cbor, bool, Vec<(u64, Cbor)>);

fn value_type(ty: &Type) -> syn::Result<ValueType> {
    let bad = |suggestion: &str| {
        err(
            ty.span(),
            format!(
                "J-E0707: `{}` is not a port type; use {suggestion}",
                type_text(ty)
            ),
        )
    };
    if let Type::Reference(_) = ty {
        return Err(bad("BoundedString<N>"));
    }
    let Some((name, args)) = generic(ty) else {
        return Err(bad("bool, i64, u64, f64 or f32"));
    };
    Ok(match name.as_str() {
        "bool" => (Cbor::Uint(0), false, Vec::new()),
        "i64" => (Cbor::Uint(1), true, Vec::new()),
        "u64" => (Cbor::Uint(2), true, Vec::new()),
        "f64" => (Cbor::Uint(3), true, Vec::new()),
        "f32" => (Cbor::Uint(4), true, Vec::new()),
        "i8" | "i16" | "i32" | "i128" | "isize" => return Err(bad("i64")),
        "u8" | "u16" | "u32" | "u128" | "usize" => return Err(bad("u64")),
        "f16" => return Err(bad("f32")),
        "String" | "str" | "char" => return Err(bad("BoundedString<N>")),
        "Vec" | "Box" | "Option" => return Err(bad("bool, i64, u64, f64 or f32")),
        "Bits" => {
            let n = args.first().and_then(const_arg).unwrap_or(64);
            if !(1..=64).contains(&n) {
                return Err(err(ty.span(), "J-E0707: Bits<N> needs 1 <= N <= 64"));
            }
            (Cbor::Uint(6), false, vec![(15, Cbor::Uint(n))])
        }
        "BoundedString" | "BoundedBytes" => {
            return Err(err(
                ty.span(),
                format!(
                    "`{name}<N>` ports are not served by this SDK version yet; publish a record in an Internal<T>"
                ),
            ));
        }
        other => (
            Cbor::Uint(5),
            false,
            vec![(9, Cbor::Text(other.to_owned()))],
        ),
    })
}

fn check_unit(unit: &str, span: Span) -> syn::Result<()> {
    joker_units::Unit::parse(unit).map(|_| ()).map_err(|_| {
        err(
            span,
            format!("J0109: unit `{unit}` is not in the v1 catalog"),
        )
    })
}

fn check_quantity(q: &str, span: Span) -> syn::Result<()> {
    q.parse::<joker_units::QuantityKind>()
        .map(|_| ())
        .map_err(|_| err(span, format!("unknown quantity `{q}`")))
}

fn expect_decl(field: &syn::Field, expected: &[&str], attr: &str) -> syn::Result<(String, Type)> {
    let found = generic(&field.ty).map(|(n, _)| n);
    match (found, type_arg(&field.ty)) {
        (Some(n), Some((_, inner))) if expected.contains(&n.as_str()) => Ok((n, inner)),
        (Some(n), None) if expected.contains(&n.as_str()) => Ok((n, field.ty.clone())),
        _ => Err(err(
            field.ty.span(),
            format!(
                "#[{attr}] needs a field of type {}",
                expected
                    .iter()
                    .map(|e| if matches!(*e, "Counter" | "Alarm") {
                        (*e).to_owned()
                    } else {
                        format!("{e}<…>")
                    })
                    .collect::<Vec<_>>()
                    .join(" or ")
            ),
        )),
    }
}

fn not_served(span: Span, what: &str) -> syn::Error {
    err(
        span,
        format!("{what} are not served by this SDK version yet (AUR-284)"),
    )
}

fn device(field: &syn::Field) -> syn::Error {
    if let Some((name, args)) = generic(&field.ty)
        && name == "Many"
        && let (Some(min), Some(max)) = (
            args.get(1).and_then(const_arg),
            args.get(2).and_then(const_arg),
        )
        && !(min <= max && max <= 1024)
    {
        return err(
            field.ty.span(),
            format!("J-E0717: Many<_, {min}, {max}>: expected 0 <= MIN <= MAX <= 1024"),
        );
    }
    not_served(field.span(), "class handles (#[device])")
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
            format!("J-E0709: port name `{name}` is reserved"),
        ));
    }
    const PORT_ATTRS: &[&str] = &[
        "input", "output", "param", "internal", "publish", "alarm", "action", "mailbox", "device",
        "model",
    ];
    let mut found: Option<(String, Vec<Meta>, Span)> = None;
    let mut kept = Vec::new();
    for attr in &field.attrs {
        let Some(which) = PORT_ATTRS.iter().find(|a| attr.path().is_ident(a)) else {
            kept.push(attr.clone());
            continue;
        };
        if found.is_some() {
            return Err(err(attr.span(), "a port has exactly one port attribute"));
        }
        let list = match &attr.meta {
            Meta::List(list) => args(list.tokens.clone())?,
            _ => Vec::new(),
        };
        found = Some(((*which).to_owned(), list, attr.span()));
    }
    let (attr, list, attr_span) = found.ok_or_else(|| {
        err(
            ident.span(),
            format!(
                "field `{name}` needs a port attribute: #[input], #[output], #[param], #[internal], #[publish] or #[alarm]"
            ),
        )
    })?;
    let has = |flag: &str| {
        list.iter()
            .any(|m| matches!(m, Meta::Path(p) if p.is_ident(flag)))
    };
    let mut meta = vec![(0, Cbor::Text(name.clone()))];
    let mut checks = Vec::new();
    let mut cycle_writer = false;
    let (kind, inner, ty_code) = match attr.as_str() {
        "model" => {
            return Err(err(
                attr_span,
                "J-E0718: `#[model]` is reserved for v1.1; enable the `unstable-ml` feature",
            ));
        }
        "device" => return Err(device(field)),
        "action" => return Err(not_served(attr_span, "action ports (#[action])")),
        "mailbox" => return Err(not_served(attr_span, "mailboxes (#[mailbox])")),
        "input" | "output" if has("many") => {
            return Err(not_served(attr_span, "many ports"));
        }
        "input" if has("event") => return Err(not_served(attr_span, "input events")),
        "output" if has("event") => {
            let (_, e) = expect_decl(field, &["EventOutput"], "output(event)")?;
            let schema = generic(&e).map(|(n, _)| n).unwrap_or_default();
            meta.push((1, Cbor::Uint(Kind::EventOut.code())));
            meta.push((9, Cbor::Text(schema)));
            for arg in &list {
                if !matches!(arg, Meta::Path(p) if p.is_ident("event")) {
                    return Err(err(arg.span(), "an event output takes no other argument"));
                }
            }
            (Kind::EventOut, Some(e), quote!(9))
        }
        "input" | "output" | "param" => {
            let kind = match attr.as_str() {
                "input" => Kind::Input,
                "output" => Kind::Output,
                _ => Kind::Param,
            };
            let decl = match kind {
                Kind::Input => "Input",
                Kind::Output => "Output",
                _ => "Param",
            };
            let (_, inner) = expect_decl(field, &[decl], &attr)?;
            let (code, numeric, extra) = value_type(&inner)?;
            meta.push((1, Cbor::Uint(kind.code())));
            meta.push((2, code.clone()));
            let mut unit = None;
            let mut range: Option<(Option<f64>, Option<f64>)> = None;
            let mut default = None;
            for arg in &list {
                match arg {
                    Meta::NameValue(nv) if nv.path.is_ident("unit") => {
                        let u = lit_str(&nv.value)?;
                        check_unit(&u, nv.value.span())?;
                        unit = Some(u);
                    }
                    Meta::NameValue(nv) if nv.path.is_ident("quantity") => {
                        let q = lit_str(&nv.value)?;
                        check_quantity(&q, nv.value.span())?;
                        meta.push((4, Cbor::Text(q)));
                    }
                    Meta::Path(p) if p.is_ident("optional") && kind != Kind::Param => {
                        meta.push((5, Cbor::Bool(true)));
                    }
                    Meta::NameValue(nv) if nv.path.is_ident("range") && kind == Kind::Param => {
                        range = Some(parse_range(&lit_str(&nv.value)?, nv.value.span())?);
                    }
                    Meta::NameValue(nv) if nv.path.is_ident("default") && kind == Kind::Param => {
                        default = Some((lit_number(&nv.value)?, nv.value.span()));
                    }
                    Meta::NameValue(nv) if nv.path.is_ident("group") && kind == Kind::Output => {
                        let g = lit_str(&nv.value)?;
                        if !is_ident(&g) {
                            return Err(err(nv.value.span(), "a group name is snake_case"));
                        }
                        meta.push((18, Cbor::Text(g)));
                    }
                    other => {
                        return Err(err(other.span(), format!("unknown argument of #[{attr}]")));
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
                    format!(
                        "J-E0706: default {v} is outside range {}..{}",
                        min.map(|m| m.to_string()).unwrap_or_default(),
                        max.map(|m| m.to_string()).unwrap_or_default()
                    ),
                ));
            }
            if let Some(u) = unit {
                meta.push((3, Cbor::Text(u)));
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
                    if code == Cbor::Uint(0) {
                        Cbor::Bool(v != 0.0)
                    } else {
                        Cbor::Float(v)
                    },
                ));
            }
            meta.extend(extra);
            let ty_code = quote!(<#inner as ::joker::PortValue>::TYPE);
            (kind, Some(inner), ty_code)
        }
        "internal" => {
            let (_, inner) = expect_decl(field, &["Internal"], "internal")?;
            for arg in &list {
                match arg {
                    Meta::NameValue(nv) if nv.path.is_ident("writer") => {
                        match lit_str(&nv.value)?.as_str() {
                            "cycle" => cycle_writer = true,
                            "async" => cycle_writer = false,
                            _ => {
                                return Err(err(
                                    nv.value.span(),
                                    "writer is \"async\" or \"cycle\"",
                                ));
                            }
                        }
                    }
                    other => return Err(err(other.span(), "unknown argument of #[internal]")),
                }
            }
            let schema = generic(&inner).map(|(n, _)| n).unwrap_or_default();
            meta.push((1, Cbor::Uint(Kind::Internal.code())));
            meta.push((2, Cbor::Uint(9)));
            meta.push((9, Cbor::Text(schema.clone())));
            meta.push((11, Cbor::Uint(if cycle_writer { 0 } else { 1 })));
            let message = format!(
                "J-E0708: internal record `{schema}` may encode to more than 64 bytes; the limit is 64"
            );
            checks.push(quote! {
                const _: () = ::core::assert!(
                    <#inner as ::joker::Field>::MAX_LEN <= 64,
                    #message
                );
            });
            (Kind::Internal, Some(inner), quote!(9))
        }
        "publish" => {
            let (decl, inner) = expect_decl(field, &["Published", "Counter"], "publish")?;
            if decl == "Counter" {
                meta.push((1, Cbor::Uint(Kind::Counter.code())));
                meta.push((2, Cbor::Uint(2)));
                (Kind::Counter, None, quote!(2))
            } else {
                let (code, _, extra) = value_type(&inner)?;
                meta.push((1, Cbor::Uint(Kind::Published.code())));
                meta.push((2, code));
                for arg in &list {
                    match arg {
                        Meta::NameValue(nv) if nv.path.is_ident("unit") => {
                            let u = lit_str(&nv.value)?;
                            check_unit(&u, nv.value.span())?;
                            meta.push((3, Cbor::Text(u)));
                        }
                        Meta::NameValue(nv) if nv.path.is_ident("quantity") => {
                            let q = lit_str(&nv.value)?;
                            check_quantity(&q, nv.value.span())?;
                            meta.push((4, Cbor::Text(q)));
                        }
                        other => return Err(err(other.span(), "unknown argument of #[publish]")),
                    }
                }
                meta.extend(extra);
                let ty_code = quote!(<#inner as ::joker::PortValue>::TYPE);
                (Kind::Published, Some(inner), ty_code)
            }
        }
        _ => {
            expect_decl(field, &["Alarm"], "alarm")?;
            let mut severity = None;
            let mut message = None;
            let mut alarm = Vec::new();
            for arg in &list {
                match arg {
                    Meta::NameValue(nv) if nv.path.is_ident("severity") => {
                        severity = Some(match lit_str(&nv.value)?.as_str() {
                            "low" => 0,
                            "medium" => 1,
                            "high" => 2,
                            "critical" => 3,
                            _ => {
                                return Err(err(
                                    nv.value.span(),
                                    "severity is \"low\", \"medium\", \"high\" or \"critical\"",
                                ));
                            }
                        });
                    }
                    Meta::NameValue(nv) if nv.path.is_ident("message") => {
                        message = Some(lit_str(&nv.value)?);
                    }
                    Meta::NameValue(nv) if nv.path.is_ident("delay_on") => {
                        alarm.push((2, Cbor::Uint(duration_arg(&nv.value)?)));
                    }
                    Meta::NameValue(nv) if nv.path.is_ident("delay_off") => {
                        alarm.push((3, Cbor::Uint(duration_arg(&nv.value)?)));
                    }
                    other => return Err(err(other.span(), "unknown argument of #[alarm]")),
                }
            }
            let (Some(severity), Some(message)) = (severity, message) else {
                return Err(err(
                    attr_span,
                    "#[alarm] needs severity = \"…\" and message = \"…\"",
                ));
            };
            alarm.insert(0, (1, Cbor::Text(message)));
            alarm.insert(0, (0, Cbor::Uint(severity)));
            alarm.sort_by_key(|(k, _)| *k);
            meta.push((1, Cbor::Uint(Kind::Alarm.code())));
            meta.push((14, Cbor::Map(alarm)));
            (Kind::Alarm, None, quote!(5))
        }
    };
    if let Some(d) = doc_of(&field.attrs) {
        meta.push((10, Cbor::Text(d)));
    }
    meta.sort_by_key(|(k, _)| *k);
    Ok((
        Port {
            field: ident,
            vis: field.vis.clone(),
            kind,
            inner,
            cycle_writer,
            ty_code,
            meta: Cbor::Map(meta),
            checks,
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

pub fn parse_range(text: &str, span: Span) -> syn::Result<(Option<f64>, Option<f64>)> {
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

fn handle(p: &Port, view: &TokenStream) -> TokenStream {
    let inner = &p.inner;
    match p.kind {
        Kind::Input => quote!(::joker::InputHandle<'a, #inner, #view>),
        Kind::Output => quote!(::joker::OutputHandle<'a, #inner, #view>),
        Kind::Param => quote!(::joker::ParamHandle<'a, #inner>),
        Kind::Internal => {
            let writer = if p.cycle_writer {
                quote!(::joker::view::CycleWriter)
            } else {
                quote!(::joker::view::AsyncWriter)
            };
            quote!(::joker::InternalHandle<'a, #inner, #view, #writer>)
        }
        Kind::Published => quote!(::joker::PublishedHandle<'a, #inner>),
        Kind::Counter => quote!(::joker::CounterHandle<'a>),
        Kind::Alarm => quote!(::joker::AlarmHandle<'a>),
        Kind::EventOut => quote!(::joker::EventOutputHandle<'a, #inner>),
    }
}

fn ctor(p: &Port) -> TokenStream {
    match p.kind {
        Kind::Input => quote!(::joker::InputHandle),
        Kind::Output => quote!(::joker::OutputHandle),
        Kind::Param => quote!(::joker::ParamHandle),
        Kind::Internal => quote!(::joker::InternalHandle),
        Kind::Published => quote!(::joker::PublishedHandle),
        Kind::Counter => quote!(::joker::CounterHandle),
        Kind::Alarm => quote!(::joker::AlarmHandle),
        Kind::EventOut => quote!(::joker::EventOutputHandle),
    }
}

fn names(list: &[Meta], arg: &str) -> syn::Result<Option<Vec<String>>> {
    for m in list {
        if let Meta::NameValue(nv) = m
            && nv.path.is_ident(arg)
        {
            let syn::Expr::Array(a) = &nv.value else {
                return Err(err(nv.value.span(), format!("{arg} = [\"…\", …]")));
            };
            let mut out = Vec::new();
            for e in &a.elems {
                let s = lit_str(e)?;
                if !is_ident(&s) {
                    return Err(err(e.span(), format!("`{s}` is not a snake_case name")));
                }
                out.push(s);
            }
            return Ok(Some(out));
        }
    }
    Ok(None)
}

pub fn expand(attr: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let mut item: ItemStruct = syn::parse2(item)?;
    let mut name = std::env::var("CARGO_PKG_NAME")
        .unwrap_or_default()
        .replace('-', "_");
    let mut version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let mut capacity = None;
    let mut clock = None;
    let list = args(attr)?;
    let secrets = names(&list, "secrets")?.unwrap_or_default();
    let optional_secrets = names(&list, "optional_secrets")?.unwrap_or_default();
    for arg in &list {
        match arg {
            Meta::NameValue(nv) if nv.path.is_ident("name") => name = lit_str(&nv.value)?,
            Meta::NameValue(nv) if nv.path.is_ident("version") => version = lit_str(&nv.value)?,
            Meta::NameValue(nv) if nv.path.is_ident("secrets") => {}
            Meta::NameValue(nv) if nv.path.is_ident("optional_secrets") => {}
            Meta::NameValue(nv) if nv.path.is_ident("requires_clock") => {
                clock = Some(match lit_str(&nv.value)?.as_str() {
                    "synced" => 0,
                    "holdover" => 1,
                    _ => {
                        return Err(err(
                            nv.value.span(),
                            "requires_clock is \"holdover\" or \"synced\"",
                        ));
                    }
                });
            }
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
            other => return Err(err(other.span(), "unknown argument of #[joker::app]")),
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
        (2, Cbor::Text(version.clone())),
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
    if let Some(c) = clock {
        meta.push((12, Cbor::Uint(c)));
    }
    if !secrets.is_empty() || !optional_secrets.is_empty() {
        let mut all: Vec<Cbor> = secrets
            .iter()
            .map(|s| Cbor::Map(vec![(0, Cbor::Text(s.clone()))]))
            .collect();
        all.extend(
            optional_secrets
                .iter()
                .map(|s| Cbor::Map(vec![(0, Cbor::Text(s.clone())), (1, Cbor::Bool(true))])),
        );
        meta.push((13, Cbor::Array(all)));
    }
    if let Some(c) = capacity {
        meta.push((16, Cbor::Map(vec![(0, Cbor::Uint(c))])));
    }
    if let Some(d) = doc_of(&item.attrs) {
        meta.push((17, Cbor::Text(d)));
    }
    let cbor = Cbor::Map(vec![(0, Cbor::Uint(0)), (1, Cbor::Map(meta))]);
    let (section, meta_slice) = fragment(&format_ident!("__JOKER_META_APP"), &cbor);
    let app = &item.ident;
    let cycle_view = format_ident!("__{}Cycle", app);
    let task_view = format_ident!("__{}Task", app);
    let vis = &item.vis;
    let cycle = quote!(::joker::view::Cycle);
    let task = quote!(::joker::view::Task);
    let fields_of = |view: &TokenStream| -> Vec<TokenStream> {
        ports
            .iter()
            .map(|p| {
                let (field, fvis) = (&p.field, &p.vis);
                let h = handle(p, view);
                quote!(#fvis #field: #h)
            })
            .collect()
    };
    let cycle_fields = fields_of(&cycle);
    let task_fields = fields_of(&task);
    let inits: Vec<TokenStream> = ports
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let field = &p.field;
            let name = field.to_string();
            let index = i as u32;
            let c = ctor(p);
            quote!(#field: #c::__new(src, #index, #name))
        })
        .collect();
    let decls = ports.iter().map(|p| {
        let name = p.field.to_string();
        let kind = p.kind.variant();
        let ty = &p.ty_code;
        quote!(::joker::__private::PortDecl {
            name: #name,
            kind: ::joker::__private::PortKind::#kind,
            ty: #ty,
        })
    });
    let checks = ports.iter().flat_map(|p| p.checks.iter());
    let cycle_doc = format!("The cycle view of [`{app}`], generated by `#[joker::app]`.");
    let task_doc = format!("The async view of [`{app}`], generated by `#[joker::app]`.");
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

        #[doc = #cycle_doc]
        #[allow(non_camel_case_types, dead_code)]
        #vis struct #cycle_view<'a> {
            #(#cycle_fields,)*
            #[doc(hidden)]
            pub __joker: ::core::marker::PhantomData<&'a ()>,
        }

        #[doc = #task_doc]
        #[allow(non_camel_case_types, dead_code)]
        #vis struct #task_view<'a> {
            #(#task_fields,)*
            #[doc(hidden)]
            pub __joker: ::core::marker::PhantomData<&'a ()>,
        }

        impl ::joker::App for #app {
            type Cycle<'a> = #cycle_view<'a>;
            type Task<'a> = #task_view<'a>;
            const NAME: &'static str = #name;
            const PORTS: &'static [::joker::__private::PortDecl] = &[#(#decls),*];

            fn cycle_view<'a>(cx: &'a ::joker::__private::CycleCx<'a>) -> Self::Cycle<'a> {
                let src = ::joker::__private::Src::Cycle(cx);
                #cycle_view { #(#inits,)* __joker: ::core::marker::PhantomData }
            }

            fn task_view(cx: ::joker::__private::TaskCx) -> Self::Task<'static> {
                let src = ::joker::__private::Src::Task(cx);
                #task_view { #(#inits,)* __joker: ::core::marker::PhantomData }
            }
        }

        #(#checks)*

        const _: () = ::core::assert!(
            ::joker::__private::is_crate_root(::core::module_path!()),
            "J-E0711: a crate holds exactly one #[joker::app], at the crate root"
        );

        #[doc(hidden)]
        #[allow(non_camel_case_types, dead_code)]
        struct J_E0711_a_crate_holds_exactly_one_joker_app;

        #section

        ::joker::__private::inventory::submit! {
            ::joker::__private::AppEntry {
                name: #name,
                version: #version,
                ports: <#app as ::joker::App>::PORTS,
                meta: #meta_slice,
            }
        }

        ::joker::__private::sys::export_guest!(::joker::__private::Runtime);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        let s = Span::call_site();
        assert_eq!(
            parse_range("20..5000", s).unwrap(),
            (Some(20.0), Some(5000.0))
        );
        assert_eq!(parse_range("0..=1", s).unwrap(), (Some(0.0), Some(1.0)));
        assert_eq!(parse_range("..3", s).unwrap(), (None, Some(3.0)));
        assert!(parse_range("5..1", s).is_err());
    }

    #[test]
    fn the_app_fragment_and_its_errors() {
        let item: TokenStream = quote! {
            /// Blinks an LED.
            pub struct Light {
                #[output]
                led: Output<bool>,
                #[param(unit = "ms", range = "20..5000", default = 500.0)]
                half_period: Param<f64>,
            }
        };
        let out = expand(quote!(name = "light", version = "0.1.0"), item)
            .unwrap()
            .to_string();
        assert!(out.contains("__JOKER_META_APP"));
        assert!(out.contains("export_guest"));
        let bad = quote! { pub struct Light { #[input] t: Input<f64> } };
        let e = expand(quote!(name = "light"), bad).unwrap_err().to_string();
        assert!(e.starts_with("J-E0703"), "{e}");
        let e = expand(
            quote!(name = "Joker"),
            quote!(
                pub struct A {}
            ),
        )
        .unwrap_err()
        .to_string();
        assert!(e.starts_with("J-E0710"), "{e}");
    }
}
