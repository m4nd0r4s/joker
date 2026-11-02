//! The task macros (ref/04 §4.3) and the state macros (§4.12):
//! `#[joker::cycle]`, `#[joker::task]`, `#[joker::on_params]`,
//! `#[joker::state]` and `#[joker::migrate]`.

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::spanned::Spanned;
use syn::{Expr, ExprLit, Fields, FnArg, ItemFn, ItemStruct, Lit, Meta, ReturnType, Type};

use crate::cbor::Cbor;
use crate::util::{
    args, doc_of, duration_arg, err, fragment, generic, has_lifetime, is_ident, lit_bool, lit_str,
    type_arg, type_text,
};

fn task_name(func: &ItemFn) -> syn::Result<String> {
    let name = func.sig.ident.to_string();
    if !is_ident(&name) {
        return Err(err(
            func.sig.ident.span(),
            format!("task name `{name}` must match [a-z][a-z0-9_]{{0,31}}"),
        ));
    }
    Ok(name)
}

/// The app of a view argument: `&mut CycleIo<App>` or `&TaskIo<App>`.
fn view_app(arg: &FnArg, view: &str, mutable: bool) -> Option<Type> {
    let FnArg::Typed(pt) = arg else { return None };
    let Type::Reference(r) = &*pt.ty else {
        return None;
    };
    if r.mutability.is_some() != mutable {
        return None;
    }
    let (name, app) = type_arg(&r.elem)?;
    (name == view).then_some(app)
}

fn arg_type(arg: &FnArg) -> Option<&Type> {
    match arg {
        FnArg::Typed(pt) => Some(&pt.ty),
        FnArg::Receiver(_) => None,
    }
}

pub fn expand_cycle(attr: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let func: ItemFn = syn::parse2(item)?;
    let ident = &func.sig.ident;
    let name = task_name(&func)?;
    let signature_error = || {
        err(
            func.sig.span(),
            "J-E0712: expected `fn(&mut CycleIo<App>, &mut State) -> Result<()>` or `fn(&mut CycleIo<App>) -> Result<()>`",
        )
    };
    if func.sig.asyncness.is_some()
        || !func.sig.generics.params.is_empty()
        || matches!(func.sig.output, ReturnType::Default)
    {
        return Err(signature_error());
    }
    let inputs: Vec<&FnArg> = func.sig.inputs.iter().collect();
    let app = inputs
        .first()
        .and_then(|a| view_app(a, "CycleIo", true))
        .ok_or_else(signature_error)?;
    let state = match inputs.as_slice() {
        [_] => None,
        [_, s] => {
            let Some(Type::Reference(r)) = arg_type(s) else {
                return Err(signature_error());
            };
            if r.mutability.is_none() {
                return Err(signature_error());
            }
            if has_lifetime(&r.elem) {
                return Err(err(
                    r.elem.span(),
                    format!(
                        "J-E0719: cycle state `{}` must be 'static",
                        type_text(&r.elem)
                    ),
                ));
            }
            Some((*r.elem).clone())
        }
        _ => return Err(signature_error()),
    };
    let mut period = None;
    let mut wcet = None;
    let mut optional = false;
    for arg in args(attr)? {
        match &arg {
            Meta::NameValue(nv) if nv.path.is_ident("period") => {
                period = Some(duration_arg(&nv.value)?);
            }
            Meta::NameValue(nv) if nv.path.is_ident("wcet") => {
                wcet = Some((duration_arg(&nv.value)?, nv.value.span()));
            }
            Meta::Path(p) if p.is_ident("optional") => optional = true,
            Meta::NameValue(nv) if nv.path.is_ident("optional") => {
                optional = lit_bool(&nv.value)?;
            }
            Meta::NameValue(nv) if nv.path.is_ident("init") => {
                return Err(err(
                    nv.span(),
                    "`init = …` is not served by this SDK version yet; the state starts at Default",
                ));
            }
            other => return Err(err(other.span(), "unknown argument of #[joker::cycle]")),
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
    let cbor = Cbor::Map(vec![(0, Cbor::Uint(1)), (1, Cbor::Map(task))]);
    let section = format_ident!("__JOKER_META_TASK_{}", name.to_uppercase());
    let (section, meta_slice) = fragment(&section, &cbor);
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

        #section

        ::joker::__private::inventory::submit! {
            ::joker::__private::CycleTask {
                name: #name,
                run: |cx: &mut ::joker::__private::CycleRun<'_>| #run,
                state: #init,
                meta: #meta_slice,
            }
        }
    })
}

enum Trigger {
    Every(u64),
    On(Vec<String>),
    OnEvent(String),
    At(String),
    Startup,
}

enum TriggerArg {
    Sample(Type),
    Change,
    Delivery(Type),
    Event(Type),
}

pub fn expand_task(attr: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let func: ItemFn = syn::parse2(item)?;
    let ident = &func.sig.ident;
    let name = task_name(&func)?;
    let signature_error = || {
        err(
            func.sig.span(),
            "J-E0712: expected `async fn(&TaskIo<App>) -> Result<()>`, with the argument of the trigger (`s: Sample<T>`, `c: Change`, `e: E` or `d: Delivery<E>`) and `st: &Shared<S>` after it",
        )
    };
    if func.sig.asyncness.is_none()
        || !func.sig.generics.params.is_empty()
        || matches!(func.sig.output, ReturnType::Default)
    {
        return Err(signature_error());
    }
    let inputs: Vec<&FnArg> = func.sig.inputs.iter().collect();
    let app = inputs
        .first()
        .and_then(|a| view_app(a, "TaskIo", false))
        .ok_or_else(signature_error)?;
    let mut trigger_arg = None;
    let mut shared = None;
    for arg in inputs.iter().skip(1) {
        let ty = arg_type(arg).ok_or_else(signature_error)?;
        if let Type::Reference(r) = ty
            && r.mutability.is_none()
            && let Some((n, s)) = type_arg(&r.elem)
            && n == "Shared"
            && shared.is_none()
        {
            shared = Some(s);
            continue;
        }
        if trigger_arg.is_some() || shared.is_some() {
            return Err(signature_error());
        }
        trigger_arg = Some(match type_arg(ty) {
            Some((n, t)) if n == "Sample" => TriggerArg::Sample(t),
            Some((n, t)) if n == "Delivery" => TriggerArg::Delivery(t),
            _ if generic(ty).is_some_and(|(n, _)| n == "Change") => TriggerArg::Change,
            _ if matches!(ty, Type::Path(_)) => TriggerArg::Event(ty.clone()),
            _ => return Err(signature_error()),
        });
    }
    let list = args(attr)?;
    let mut triggers: Vec<(Trigger, Span)> = Vec::new();
    let mut checks = Vec::new();
    let mut jitter = None;
    for arg in &list {
        match arg {
            Meta::NameValue(nv) if nv.path.is_ident("every") => {
                let p = duration_arg(&nv.value)?;
                if p == 0 {
                    return Err(err(
                        nv.value.span(),
                        "J-E0705: `every` needs a period above 0",
                    ));
                }
                triggers.push((Trigger::Every(p), nv.span()));
            }
            Meta::NameValue(nv) if nv.path.is_ident("jitter") => {
                jitter = Some((duration_arg(&nv.value)?, nv.value.span()));
            }
            Meta::NameValue(nv) if nv.path.is_ident("on") => {
                let ports: Vec<String> = match &nv.value {
                    Expr::Array(a) => a.elems.iter().map(lit_str).collect::<syn::Result<_>>()?,
                    e => vec![lit_str(e)?],
                };
                if ports.is_empty() {
                    return Err(err(
                        nv.value.span(),
                        "J-E0713: `on` names at least one port",
                    ));
                }
                for p in &ports {
                    let message =
                        format!("J-E0713: `on = \"{p}\"`: the app has no port `{p}` with a sample");
                    checks.push(quote! {
                        const _: () = ::core::assert!(
                            ::joker::__private::port_index(
                                <#app as ::joker::App>::PORTS,
                                #p,
                                &[
                                    ::joker::__private::PortKind::Input,
                                    ::joker::__private::PortKind::Output,
                                    ::joker::__private::PortKind::Param,
                                    ::joker::__private::PortKind::Internal,
                                    ::joker::__private::PortKind::Alarm,
                                ],
                            ).is_some(),
                            #message
                        );
                    });
                }
                triggers.push((Trigger::On(ports), nv.span()));
            }
            Meta::NameValue(nv) if nv.path.is_ident("on_event") => {
                let p = lit_str(&nv.value)?;
                let message =
                    format!("J-E0713: `on_event = \"{p}\"`: the app has no event input `{p}`");
                checks.push(quote! {
                    const _: () = ::core::assert!(
                        ::joker::__private::port_index(<#app as ::joker::App>::PORTS, #p, &[]).is_some(),
                        #message
                    );
                });
                triggers.push((Trigger::OnEvent(p), nv.span()));
            }
            Meta::NameValue(nv) if nv.path.is_ident("at") => {
                let text = lit_str(&nv.value)?;
                let cron = text
                    .strip_prefix("cron:")
                    .map(str::trim)
                    .filter(|c| c.split_whitespace().count() == 6)
                    .ok_or_else(|| {
                        err(
                            nv.value.span(),
                            "J-E0713: `at` is a 6-field UTC cron: \"cron: 0 */15 * * * *\"",
                        )
                    })?
                    .to_owned();
                triggers.push((Trigger::At(cron), nv.span()));
            }
            Meta::Path(p) if p.is_ident("startup") => triggers.push((Trigger::Startup, p.span())),
            other => return Err(err(other.span(), "unknown argument of #[joker::task]")),
        }
    }
    if triggers.len() != 1 {
        let span = triggers.get(1).map_or_else(Span::call_site, |t| t.1);
        return Err(err(
            span,
            "J-E0713: a task needs exactly one trigger: every, on, on_event, at or startup",
        ));
    }
    let Some((trigger, _)) = triggers.pop() else {
        return Err(err(Span::call_site(), "J-E0713: no trigger"));
    };
    if let Some((_, span)) = jitter
        && !matches!(trigger, Trigger::Every(_))
    {
        return Err(err(span, "J-E0713: `jitter` goes with `every`"));
    }
    let (trigger_meta, decl) = match &trigger {
        Trigger::Every(period) => {
            let jitter_ns = jitter.map_or(0, |j| j.0);
            let mut m = vec![(0, Cbor::Uint(0)), (1, Cbor::Uint(*period))];
            if jitter.is_some() {
                m.push((2, Cbor::Uint(jitter_ns)));
            }
            (
                Cbor::Map(m),
                quote!(::joker::__private::TriggerDecl::Every { period_ns: #period, jitter_ns: #jitter_ns }),
            )
        }
        Trigger::On(ports) => (
            Cbor::Map(vec![
                (0, Cbor::Uint(1)),
                (
                    1,
                    Cbor::Array(ports.iter().map(|p| Cbor::Text(p.clone())).collect()),
                ),
            ]),
            quote!(::joker::__private::TriggerDecl::On(&[#(#ports),*])),
        ),
        Trigger::OnEvent(p) => (
            Cbor::Map(vec![(0, Cbor::Uint(2)), (1, Cbor::Text(p.clone()))]),
            quote!(::joker::__private::TriggerDecl::OnEvent(#p)),
        ),
        Trigger::At(cron) => (
            Cbor::Map(vec![(0, Cbor::Uint(3)), (1, Cbor::Text(cron.clone()))]),
            quote!(::joker::__private::TriggerDecl::At(#cron)),
        ),
        Trigger::Startup => (
            Cbor::Map(vec![(0, Cbor::Uint(4))]),
            quote!(::joker::__private::TriggerDecl::Startup),
        ),
    };
    let ons = match &trigger {
        Trigger::On(p) => p.len(),
        _ => 0,
    };
    let is_event = matches!(trigger, Trigger::OnEvent(_));
    let call_arg = match &trigger_arg {
        None => None,
        Some(TriggerArg::Sample(t)) if ons == 1 => Some(quote!(args.sample::<#t>()?)),
        Some(TriggerArg::Change) if ons > 1 => Some(quote!(args.change::<#app>()?)),
        Some(TriggerArg::Delivery(e)) if is_event => Some(quote!(args.delivery::<#e>()?)),
        Some(TriggerArg::Event(e)) if is_event => Some(quote!(args.delivery::<#e>()?.event)),
        Some(_) => return Err(signature_error()),
    };
    let (state_arg, state_init) = match &shared {
        Some(s) => (
            Some(quote!(args.state::<::joker::Shared<#s>>()?)),
            quote!(::joker::__private::new_shared::<#s>),
        ),
        None => (None, quote!(|| ::core::option::Option::None)),
    };
    let call_args = call_arg.into_iter().chain(state_arg);
    let mut task = vec![
        (0, Cbor::Text(name.clone())),
        (1, Cbor::Uint(1)),
        (6, trigger_meta),
    ];
    if let Some(d) = doc_of(&func.attrs) {
        task.push((8, Cbor::Text(d)));
    }
    let cbor = Cbor::Map(vec![(0, Cbor::Uint(1)), (1, Cbor::Map(task))]);
    let section = format_ident!("__JOKER_META_TASK_{}", name.to_uppercase());
    let (section, meta_slice) = fragment(&section, &cbor);
    Ok(quote! {
        #func

        #section

        #(#checks)*

        ::joker::__private::inventory::submit! {
            ::joker::__private::AsyncTask {
                name: #name,
                trigger: #decl,
                run: |args: ::joker::__private::TaskArgs| {
                    ::joker::__private::boxed(async move {
                        let cx = args.cx();
                        let io = ::joker::__private::task_io::<#app>(cx);
                        #ident(&io, #(#call_args),*).await
                    })
                },
                state: #state_init,
                meta: #meta_slice,
            }
        }
    })
}

pub fn expand_on_params(attr: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    if !attr.is_empty() {
        return Err(err(attr.span(), "#[joker::on_params] takes no argument"));
    }
    let func: ItemFn = syn::parse2(item)?;
    let ident = &func.sig.ident;
    let ok = func.sig.asyncness.is_none()
        && func.sig.generics.params.is_empty()
        && func.sig.inputs.len() == 1
        && !matches!(func.sig.output, ReturnType::Default)
        && func.sig.inputs.first().and_then(arg_type).is_some_and(|t| {
            matches!(t, Type::Reference(r) if r.mutability.is_none()
                && generic(&r.elem).is_some_and(|(n, _)| n == "ParamChanges"))
        });
    if !ok {
        return Err(err(
            func.sig.span(),
            "J-E0712: expected `fn(&ParamChanges) -> Result<()>`",
        ));
    }
    Ok(quote! {
        #func

        ::joker::__private::inventory::submit! {
            ::joker::__private::OnParams { run: #ident }
        }
    })
}

/// A field type that can never be persisted, by its name.
fn not_persistable(ty: &Type) -> bool {
    match ty {
        Type::Path(_) => generic(ty).is_some_and(|(n, _)| {
            matches!(
                n.as_str(),
                "Vec"
                    | "String"
                    | "Box"
                    | "Rc"
                    | "Arc"
                    | "HashMap"
                    | "BTreeMap"
                    | "HashSet"
                    | "BTreeSet"
                    | "VecDeque"
                    | "RefCell"
                    | "Cell"
                    | "i8"
                    | "i16"
                    | "i32"
                    | "isize"
                    | "u8"
                    | "u16"
                    | "u32"
                    | "usize"
                    | "char"
            )
        }),
        _ => true,
    }
}

pub fn expand_state(attr: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let item: ItemStruct = syn::parse2(item)?;
    let mut persist = false;
    let mut version = 1u32;
    for arg in args(attr)? {
        match &arg {
            Meta::Path(p) if p.is_ident("persist") => persist = true,
            Meta::NameValue(nv) if nv.path.is_ident("version") => {
                version = match &nv.value {
                    Expr::Lit(ExprLit {
                        lit: Lit::Int(i), ..
                    }) => i.base10_parse()?,
                    other => return Err(err(other.span(), "version = N, an integer")),
                };
                if version == 0 {
                    return Err(err(nv.value.span(), "the state version starts at 1"));
                }
            }
            other => return Err(err(other.span(), "unknown argument of #[joker::state]")),
        }
    }
    if !item.generics.params.is_empty() {
        return Err(err(
            item.generics.span(),
            "a task state has no generic parameters",
        ));
    }
    let Fields::Named(fields) = &item.fields else {
        return Err(err(item.span(), "a task state has named fields"));
    };
    let ty = &item.ident;
    let ty_name = ty.to_string();
    let mut saves = Vec::new();
    let mut loads = Vec::new();
    for f in &fields.named {
        let Some(fname) = &f.ident else { continue };
        let key = fname.to_string();
        let fty = &f.ty;
        if persist && not_persistable(fty) {
            return Err(err(
                fty.span(),
                format!("J-E0720: field `{key}` of `{ty_name}` cannot be persisted"),
            ));
        }
        saves.push(quote! {
            w.text(#key);
            ::joker::Persist::save(&self.#fname, w);
        });
        loads.push(quote! {
            if let ::core::option::Option::Some(v) = ::joker::__private::load_field::<#fty>(blob, #key) {
                s.#fname = v;
            }
        });
    }
    let n = saves.len();
    let codec = persist.then(|| {
        quote! {
            ::joker::__private::inventory::submit! {
                ::joker::__private::StateCodec {
                    type_id: ::core::any::TypeId::of::<#ty>,
                    version: #version,
                    persist: true,
                    save: ::joker::__private::save_state::<#ty>,
                    load: ::joker::__private::load_state::<#ty>,
                }
            }
        }
    });
    let (save_body, load_body) = if persist {
        (
            quote! {
                w.map(#n);
                #(#saves)*
            },
            quote! {
                let mut s = <Self as ::core::default::Default>::default();
                #(#loads)*
                s
            },
        )
    } else {
        (
            quote! { w.map(0); },
            quote! {
                let _ = blob;
                <Self as ::core::default::Default>::default()
            },
        )
    };
    Ok(quote! {
        #item

        impl ::joker::State for #ty {
            const VERSION: u32 = #version;
            const PERSIST: bool = #persist;

            fn save(&self, w: &mut ::joker::cbor::Writer<'_>) {
                #save_body
            }

            fn load(blob: &::joker::StateBlob<'_>) -> Self {
                #load_body
            }
        }

        #codec
    })
}

pub fn expand_migrate(attr: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let ty: Type = syn::parse2(attr)?;
    let func: ItemFn = syn::parse2(item)?;
    let ident = &func.sig.ident;
    if func.sig.asyncness.is_some() || func.sig.inputs.len() != 2 {
        return Err(err(
            func.sig.span(),
            format!(
                "J-E0712: expected `fn(StateBlob<'_>, &str) -> Result<{}>`",
                type_text(&ty)
            ),
        ));
    }
    Ok(quote! {
        #func

        ::joker::__private::inventory::submit! {
            ::joker::__private::Migration {
                type_id: ::core::any::TypeId::of::<#ty>,
                run: |prev, from| ::joker::__private::migrated::<#ty>(#ident(prev, from)),
            }
        }
    })
}
