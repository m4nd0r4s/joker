//! The derives (ref/04 §4.2): `Record`, `Event` and `Enum`, with their
//! `schema-fragment` of `joker:meta` (§5.2).

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Expr, ExprLit, Fields, Lit, Type};

use crate::cbor::{Cbor, header_len};
use crate::util::{const_arg, err, fragment, generic, snake, type_text};

/// The `joker:meta` description of a record field, and the J-E0721 check of
/// the types that are never fields.
fn field_meta(name: &str, ty: &Type) -> syn::Result<Cbor> {
    let bad = |suggestion: &str| {
        err(
            ty.span(),
            format!(
                "J-E0721: field `{name}`: `{}` is not a record field type{suggestion}",
                type_text(ty)
            ),
        )
    };
    let mut m = vec![(0, Cbor::Text(name.to_owned()))];
    let mut inner = ty.clone();
    let mut optional = false;
    if let Some((n, args)) = generic(ty)
        && n == "Option"
        && let Some(syn::GenericArgument::Type(t)) = args.first()
    {
        inner = t.clone();
        optional = true;
    }
    let Some((n, args)) = generic(&inner) else {
        return Err(bad(""));
    };
    let text = quote!(#inner).to_string();
    m.push((1, Cbor::Small(format!("<{text} as ::joker::Field>::TYPE"))));
    match n.as_str() {
        "bool" | "i64" | "u64" | "f64" | "f32" | "Instant" | "MonoNanos" | "UtcNanos"
        | "UtcSeconds" | "Duration" | "Quality" | "Bits" | "BoundedBytes" => {}
        "BoundedString" => {
            let cap = args.first().and_then(const_arg).unwrap_or(0);
            if cap > 1024 {
                return Err(bad("; BoundedString<N> needs N <= 1024"));
            }
            m.push((3, Cbor::Uint(cap)));
        }
        "BoundedVec" => {
            let cap = args.get(1).and_then(const_arg).unwrap_or(0);
            if cap > 64 {
                return Err(bad("; BoundedVec<T, N> needs N <= 64"));
            }
            m.push((3, Cbor::Uint(cap)));
            if let Some(syn::GenericArgument::Type(t)) = args.first()
                && let Some((e, _)) = generic(t)
                && !is_builtin(&e)
            {
                m.push((4, Cbor::Text(e)));
            }
        }
        "i8" | "i16" | "i32" | "i128" | "isize" => return Err(bad("; use i64")),
        "u8" | "u16" | "u32" | "u128" | "usize" => return Err(bad("; use u64")),
        "String" | "str" => return Err(bad("; use BoundedString<N>")),
        "Vec" | "VecDeque" => return Err(bad("; use BoundedVec<T, N>")),
        "Box" | "Rc" | "Arc" | "HashMap" | "BTreeMap" | "HashSet" | "BTreeSet" | "Cell"
        | "RefCell" | "char" => return Err(bad("")),
        other => m.push((4, Cbor::Text(other.to_owned()))),
    }
    if optional {
        m.push((5, Cbor::Bool(true)));
    }
    m.sort_by_key(|(k, _)| *k);
    Ok(Cbor::Map(m))
}

fn is_builtin(n: &str) -> bool {
    matches!(
        n,
        "bool"
            | "i64"
            | "u64"
            | "f64"
            | "f32"
            | "Instant"
            | "MonoNanos"
            | "UtcNanos"
            | "UtcSeconds"
            | "Duration"
            | "Quality"
            | "Bits"
            | "BoundedBytes"
            | "BoundedString"
    )
}

pub fn expand_record(input: TokenStream, event: bool) -> syn::Result<TokenStream> {
    let input: DeriveInput = syn::parse2(input)?;
    let ty = &input.ident;
    let what = if event { "Event" } else { "Record" };
    if !input.generics.params.is_empty() {
        return Err(err(
            input.generics.span(),
            format!("a joker::{what} has no generic parameters"),
        ));
    }
    let Data::Struct(data) = &input.data else {
        return Err(err(
            input.span(),
            format!("joker::{what} is derived on a struct with named fields"),
        ));
    };
    let fields: Vec<(syn::Ident, Type)> = match &data.fields {
        Fields::Named(f) => f
            .named
            .iter()
            .filter_map(|f| f.ident.clone().map(|i| (i, f.ty.clone())))
            .collect(),
        Fields::Unit => Vec::new(),
        Fields::Unnamed(_) => {
            return Err(err(
                input.span(),
                format!("joker::{what} is derived on a struct with named fields"),
            ));
        }
    };
    let mut metas = Vec::new();
    for (name, fty) in &fields {
        metas.push(field_meta(&name.to_string(), fty)?);
    }
    let name = ty.to_string();
    let n = fields.len();
    let head = header_len(n as u64);
    let sizes = fields.iter().enumerate().map(|(i, (_, t))| {
        let key = header_len(i as u64);
        quote!(#key + <#t as ::joker::Field>::MAX_LEN)
    });
    let idents: Vec<&syn::Ident> = fields.iter().map(|(i, _)| i).collect();
    let types: Vec<&Type> = fields.iter().map(|(_, t)| t).collect();
    let keys: Vec<u64> = (0..n as u64).collect();
    let slots: Vec<syn::Ident> = (0..n).map(|i| format_ident!("__f{}", i)).collect();
    let mut schema = vec![
        (0, Cbor::Uint(0)),
        (1, Cbor::Text(name.clone())),
        (2, Cbor::Array(metas)),
    ];
    if event {
        schema.push((4, Cbor::Bool(true)));
    }
    let cbor = Cbor::Map(vec![(0, Cbor::Uint(2)), (1, Cbor::Map(schema))]);
    let section = format_ident!("__JOKER_META_SCHEMA_{}", name.to_uppercase());
    let (section, _) = fragment(&section, &cbor);
    let event_impl = event.then(|| quote!(impl ::joker::Event for #ty {}));
    Ok(quote! {
        impl ::joker::Field for #ty {
            const TYPE: u8 = 9;
            const MAX_LEN: usize = #head #(+ #sizes)*;

            fn encode(&self, w: &mut ::joker::cbor::Writer<'_>) {
                let n = 0usize #(+ usize::from(!::joker::Field::omitted(&self.#idents)))*;
                w.map(n);
                #(
                    if !::joker::Field::omitted(&self.#idents) {
                        w.uint(#keys);
                        ::joker::Field::encode(&self.#idents, w);
                    }
                )*
            }

            fn decode(r: &mut ::joker::cbor::Reader<'_>) -> ::core::option::Option<Self> {
                let n = r.map()?;
                #(let mut #slots: ::core::option::Option<#types> = ::core::option::Option::None;)*
                for _ in 0..n {
                    match r.uint()? {
                        #(#keys => #slots = ::core::option::Option::Some(<#types as ::joker::Field>::decode(r)?),)*
                        _ => r.skip()?,
                    }
                }
                ::core::option::Option::Some(Self {
                    #(#idents: match #slots {
                        ::core::option::Option::Some(v) => v,
                        ::core::option::Option::None => <#types as ::joker::Field>::absent()?,
                    },)*
                })
            }
        }

        impl ::joker::Record for #ty {
            const NAME: &'static str = #name;
        }

        #event_impl

        #section
    })
}

pub fn expand_enum(input: TokenStream) -> syn::Result<TokenStream> {
    let input: DeriveInput = syn::parse2(input)?;
    let ty = &input.ident;
    if !input.generics.params.is_empty() {
        return Err(err(
            input.generics.span(),
            "a joker::Enum has no generic parameters",
        ));
    }
    let Data::Enum(data) = &input.data else {
        return Err(err(input.span(), "joker::Enum is derived on an enum"));
    };
    let mut next = 0u64;
    let mut variants = Vec::new();
    for v in &data.variants {
        if !matches!(v.fields, Fields::Unit) {
            return Err(err(v.span(), "a joker::Enum has variants without data"));
        }
        let value = match &v.discriminant {
            Some((
                _,
                Expr::Lit(ExprLit {
                    lit: Lit::Int(i), ..
                }),
            )) => i.base10_parse::<u64>()?,
            Some((_, e)) => {
                return Err(err(e.span(), "a joker::Enum value is an integer literal"));
            }
            None => next,
        };
        if variants
            .iter()
            .any(|(_, v): &(syn::Ident, u64)| *v == value)
        {
            return Err(err(
                v.span(),
                format!("two variants with the value {value}"),
            ));
        }
        next = value + 1;
        variants.push((v.ident.clone(), value));
    }
    let Some((first, _)) = variants.first().cloned() else {
        return Err(err(input.span(), "a joker::Enum has at least one variant"));
    };
    let name = ty.to_string();
    let schema = Cbor::Map(vec![
        (0, Cbor::Uint(1)),
        (1, Cbor::Text(name.clone())),
        (
            2,
            Cbor::Array(
                variants
                    .iter()
                    .map(|(i, v)| {
                        Cbor::Array(vec![Cbor::Text(snake(&i.to_string())), Cbor::Uint(*v)])
                    })
                    .collect(),
            ),
        ),
    ]);
    let cbor = Cbor::Map(vec![(0, Cbor::Uint(2)), (1, schema)]);
    let section = format_ident!("__JOKER_META_SCHEMA_{}", name.to_uppercase());
    let (section, _) = fragment(&section, &cbor);
    let idents: Vec<&syn::Ident> = variants.iter().map(|(i, _)| i).collect();
    let values: Vec<u64> = variants.iter().map(|(_, v)| *v).collect();
    let texts: Vec<String> = idents.iter().map(|i| i.to_string()).collect();
    Ok(quote! {
        impl ::core::clone::Clone for #ty {
            fn clone(&self) -> Self {
                *self
            }
        }

        impl ::core::marker::Copy for #ty {}

        impl ::core::cmp::PartialEq for #ty {
            fn eq(&self, other: &Self) -> bool {
                ::joker::Enum::value(*self) == ::joker::Enum::value(*other)
            }
        }

        impl ::core::cmp::Eq for #ty {}

        impl ::core::fmt::Debug for #ty {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.write_str(match self { #(Self::#idents => #texts,)* })
            }
        }

        impl ::joker::Enum for #ty {
            fn value(self) -> u64 {
                match self { #(Self::#idents => #values,)* }
            }

            fn from_value(v: u64) -> ::core::option::Option<Self> {
                match v {
                    #(#values => ::core::option::Option::Some(Self::#idents),)*
                    _ => ::core::option::Option::None,
                }
            }
        }

        impl ::joker::__private::Sealed for #ty {}

        impl ::joker::PortValue for #ty {
            const TYPE: u8 = 5;

            fn from_bits(bits: u64) -> Self {
                <Self as ::joker::Enum>::from_value(bits).unwrap_or(Self::#first)
            }

            fn to_bits(self) -> u64 {
                ::joker::Enum::value(self)
            }
        }

        impl ::joker::Field for #ty {
            const TYPE: u8 = 5;
            const MAX_LEN: usize = 9;

            fn encode(&self, w: &mut ::joker::cbor::Writer<'_>) {
                w.uint(::joker::Enum::value(*self));
            }

            fn decode(r: &mut ::joker::cbor::Reader<'_>) -> ::core::option::Option<Self> {
                <Self as ::joker::Enum>::from_value(r.uint()?)
            }
        }

        #section
    })
}
