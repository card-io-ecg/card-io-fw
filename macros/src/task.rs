use darling::{export::NestedMeta, FromMeta};
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::{parse_quote, Expr, ExprLit, ItemFn, Lit, LitInt, ReturnType, Type};

#[derive(Debug, FromMeta)]
struct Args {
    #[darling(default)]
    pool_size: Option<syn::Expr>,
}

pub fn run(args: &[NestedMeta], f: syn::ItemFn) -> TokenStream {
    let args = match Args::from_list(args) {
        Ok(args) => args,
        Err(e) => return e.write_errors().into(),
    };

    let pool_size = args.pool_size.unwrap_or(Expr::Lit(ExprLit {
        attrs: vec![],
        lit: Lit::Int(LitInt::new("1", Span::call_site())),
    }));

    if f.sig.asyncness.is_none() {
        return syn::Error::new_spanned(&f.sig, "task functions must be async").to_compile_error();
    }
    if !f.sig.generics.params.is_empty() {
        return syn::Error::new_spanned(&f.sig, "task functions must not be generic")
            .to_compile_error();
    }
    if !f.sig.generics.where_clause.is_none() {
        return syn::Error::new_spanned(&f.sig, "task functions must not have `where` clauses")
            .to_compile_error();
    }
    if !f.sig.abi.is_none() {
        return syn::Error::new_spanned(&f.sig, "task functions must not have an ABI qualifier")
            .to_compile_error();
    }
    if !f.sig.variadic.is_none() {
        return syn::Error::new_spanned(&f.sig, "task functions must not be variadic")
            .to_compile_error();
    }
    match &f.sig.output {
        ReturnType::Default => {}
        ReturnType::Type(_, ty) => match &**ty {
            Type::Tuple(tuple) if tuple.elems.is_empty() => {}
            Type::Never(_) => {}
            _ => {
                return syn::Error::new_spanned(
                    &f.sig,
                    "task functions must either not return a value, return `()` or return `!`",
                )
                .to_compile_error()
            }
        },
    }

    let mut arg_names = Vec::new();
    let mut fargs = f.sig.inputs.clone();

    for arg in fargs.iter_mut() {
        match arg {
            syn::FnArg::Receiver(_) => {
                return syn::Error::new_spanned(
                    arg,
                    "task functions must not have receiver arguments",
                )
                .to_compile_error();
            }
            syn::FnArg::Typed(t) => match t.pat.as_mut() {
                syn::Pat::Ident(id) => {
                    arg_names.push(id.ident.clone());
                    id.mutability = None;
                }
                _ => {
                    return syn::Error::new_spanned(
                        arg,
                        "pattern matching in task arguments is not yet supported",
                    )
                    .to_compile_error();
                }
            },
        }
    }

    let task_ident = f.sig.ident.clone();
    let task_inner_ident = format_ident!("__{}_task", task_ident);

    let mut task_inner = f;
    let visibility = task_inner.vis.clone();
    task_inner.vis = syn::Visibility::Inherited;
    task_inner.sig.ident = task_inner_ident.clone();

    let mut task_outer: ItemFn = parse_quote! {
        #visibility fn #task_ident(#fargs) -> ::embassy_executor::SpawnToken<impl Sized> {
            type Fut = impl ::core::future::Future + 'static;
            const POOL_SIZE: usize = #pool_size;
            static POOL: embassy_alloc_taskpool::AllocTaskPool<Fut, POOL_SIZE> = embassy_alloc_taskpool::AllocTaskPool::new();
            unsafe { POOL._spawn_async_fn(move || #task_inner_ident(#(#arg_names,)*)) }
        }
    };

    task_outer.attrs.append(&mut task_inner.attrs.clone());

    let result = quote! {
        // This is the user's task function, renamed.
        // We put it outside the #task_ident fn below, because otherwise
        // the items defined there (such as POOL) would be in scope
        // in the user's code.
        #[doc(hidden)]
        #task_inner

        #task_outer
    };

    result
}
