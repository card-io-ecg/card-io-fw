extern crate proc_macro;

use proc_macro::TokenStream;

mod filter;
mod norfs_partition;
mod task;

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
    let args = syn::parse_macro_input!(args as task::Args);
    let f = syn::parse_macro_input!(item as syn::ItemFn);

    task::run(args, f).into()
}

#[proc_macro]
pub fn designfilt(item: TokenStream) -> TokenStream {
    let spec = syn::parse_macro_input!(item as filter::FilterSpec);
    filter::run(spec).into()
}

#[proc_macro_attribute]
pub fn partition(args: TokenStream, item: TokenStream) -> TokenStream {
    let tokens = item.clone();

    let args = syn::parse_macro_input!(args as norfs_partition::Args);
    let s = syn::parse_macro_input!(tokens as syn::ItemStruct);

    norfs_partition::implement(args, s, item.into()).into()
}
