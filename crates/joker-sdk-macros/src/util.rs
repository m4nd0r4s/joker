//! Parsing helpers shared by the macros.

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{Attribute, Expr, ExprLit, GenericArgument, Lit, Meta, PathArguments, Token, Type};

use crate::cbor::{Cbor, part_tokens};

pub fn err(span: Span, message: impl std::fmt::Display) -> syn::Error {
    syn::Error::new(span, message.to_string())
}

pub fn args(attr: TokenStream) -> syn::Result<Vec<Meta>> {
    Ok(Punctuated::<Meta, Token![,]>::parse_terminated
        .parse2(attr)?
        .into_iter()
        .collect())
}

pub fn lit_str(expr: &Expr) -> syn::Result<String> {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Str(s), ..
        }) => Ok(s.value()),
        other => Err(err(other.span(), "expected a string literal")),
    }
}

pub fn lit_bool(expr: &Expr) -> syn::Result<bool> {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Bool(b), ..
        }) => Ok(b.value),
        other => Err(err(other.span(), "expected true or false")),
    }
}

pub fn lit_number(expr: &Expr) -> syn::Result<f64> {
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
pub fn duration_ns(text: &str, span: Span) -> syn::Result<u64> {
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

pub fn duration_arg(expr: &Expr) -> syn::Result<u64> {
    duration_ns(&lit_str(expr)?, expr.span())
}

pub fn size_bytes(text: &str, span: Span) -> syn::Result<u64> {
    let bad = || {
        err(
            span,
            format!("invalid size `{text}`; expected e.g. \"16 KiB\""),
        )
    };
    let t = text.trim();
    let split = t.find(|c: char| !c.is_ascii_digit()).unwrap_or(t.len());
    let (number, unit) = t.split_at(split);
    let n: u64 = number.parse().map_err(|_| bad())?;
    let scale = match unit.trim() {
        "" | "B" => 1,
        "KiB" => 1024,
        "MiB" => 1024 * 1024,
        _ => return Err(bad()),
    };
    Ok(n * scale)
}

pub fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some('a'..='z'))
        && s.len() <= 32
        && chars.all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_'))
}

pub fn is_semver(v: &str) -> bool {
    let core = v.split(['-', '+']).next().unwrap_or("");
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.chars().all(|c| c.is_ascii_digit())
                && (p.len() == 1 || !p.starts_with('0'))
        })
}

pub fn doc_of(attrs: &[Attribute]) -> Option<String> {
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

/// The last segment of a path type and its generic arguments.
pub fn generic(ty: &Type) -> Option<(String, Vec<GenericArgument>)> {
    let Type::Path(p) = ty else { return None };
    if p.qself.is_some() {
        return None;
    }
    let last = p.path.segments.last()?;
    let args = match &last.arguments {
        PathArguments::AngleBracketed(a) => a.args.iter().cloned().collect(),
        PathArguments::None => Vec::new(),
        PathArguments::Parenthesized(_) => return None,
    };
    Some((last.ident.to_string(), args))
}

/// The single type argument of `Name<T>`.
pub fn type_arg(ty: &Type) -> Option<(String, Type)> {
    let (name, args) = generic(ty)?;
    match args.first() {
        Some(GenericArgument::Type(t)) => Some((name, t.clone())),
        _ => None,
    }
}

/// The literal `usize` of a const generic argument.
pub fn const_arg(arg: &GenericArgument) -> Option<u64> {
    let GenericArgument::Const(e) = arg else {
        return None;
    };
    let e = e.clone();
    match e {
        Expr::Lit(ExprLit {
            lit: Lit::Int(i), ..
        }) => i.base10_parse().ok(),
        Expr::Block(b) => match b.block.stmts.as_slice() {
            [
                syn::Stmt::Expr(
                    Expr::Lit(ExprLit {
                        lit: Lit::Int(i), ..
                    }),
                    None,
                ),
            ] => i.base10_parse().ok(),
            _ => None,
        },
        _ => None,
    }
}

pub fn type_text(ty: &Type) -> String {
    quote!(#ty).to_string().replace(' ', "")
}

/// A type with a lifetime other than `'static` (`J-E0719`).
pub fn has_lifetime(ty: &Type) -> bool {
    let text = quote!(#ty).to_string();
    let mut rest = text.as_str();
    while let Some(i) = rest.find('\'') {
        rest = &rest[i + 1..];
        if !rest.starts_with("static") {
            return true;
        }
    }
    false
}

/// The static that holds a `joker:meta` fragment, and the slice the native
/// harness reads.
pub fn fragment(name: &syn::Ident, cbor: &Cbor) -> (TokenStream, TokenStream) {
    let parts = cbor.to_parts();
    let len = parts.len();
    let bytes = part_tokens(&parts);
    let section = quote! {
        ::joker::__private::sys::meta_fragment!(#name, #len, [#(#bytes),*]);
    };
    let slice = quote!({
        const __META: [u8; #len] = [#(#bytes),*];
        &__META
    });
    (section, slice)
}

/// Upper camel case to snake case: `ActiveAcked` → `active_acked`.
pub fn snake(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_sizes_and_versions() {
        let s = Span::call_site();
        assert_eq!(duration_ns("20ms", s).unwrap(), 20_000_000);
        assert_eq!(duration_ns("5us", s).unwrap(), 5_000);
        assert_eq!(duration_ns("1.5s", s).unwrap(), 1_500_000_000);
        assert_eq!(duration_ns("90m", s).unwrap(), 5_400_000_000_000);
        assert!(duration_ns("20", s).is_err());
        assert!(duration_ns("ms", s).is_err());
        assert!(duration_ns("0.5ns", s).is_err());
        assert_eq!(size_bytes("16 KiB", s).unwrap(), 16384);
        assert!(is_semver("0.1.0"));
        assert!(is_semver("1.4.2-rc.1"));
        assert!(!is_semver("0.1"));
        assert!(!is_semver("01.1.0"));
        assert_eq!(snake("ActiveAcked"), "active_acked");
    }

    #[test]
    fn lifetimes_in_state_types() {
        let t: Type = syn::parse_quote!(Window<'_>);
        assert!(has_lifetime(&t));
        let t: Type = syn::parse_quote!(Window<'static>);
        assert!(!has_lifetime(&t));
        let t: Type = syn::parse_quote!(Ctl);
        assert!(!has_lifetime(&t));
    }
}
