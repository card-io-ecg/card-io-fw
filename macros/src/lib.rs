extern crate proc_macro;

use darling::ast::NestedMeta;
use proc_macro::TokenStream;

use syn::{
    parse::{Parse, ParseBuffer},
    punctuated::Punctuated,
    Token,
};

mod task;

struct Args {
    meta: Vec<NestedMeta>,
}

impl Parse for Args {
    fn parse(input: &ParseBuffer) -> syn::Result<Self> {
        let meta = Punctuated::<NestedMeta, Token![,]>::parse_terminated(input)?;
        Ok(Args {
            meta: meta.into_iter().collect(),
        })
    }
}

/// Declares an async task that can be run by `embassy-executor`. The optional `pool_size` parameter can be used to specify how
/// many concurrent tasks can be spawned (default is 1) for the function.
///
///
/// The following restrictions apply:
///
/// * The function must be declared `async`.
/// * The function must not use generics.
/// * The optional `pool_size` attribute must be 1 or greater.
///
///
/// ## Examples
///
/// Declaring a task taking no arguments:
///
/// ``` rust
/// #[embassy_executor::task]
/// async fn mytask() {
///     // Function body
/// }
/// ```
///
/// Declaring a task with a given pool size:
///
/// ``` rust
/// #[embassy_executor::task(pool_size = 4)]
/// async fn mytask() {
///     // Function body
/// }
/// ```
#[proc_macro_attribute]
pub fn task(args: TokenStream, item: TokenStream) -> TokenStream {
    let args = syn::parse_macro_input!(args as Args);
    let f = syn::parse_macro_input!(item as syn::ItemFn);

    task::run(&args.meta, f).into()
}
