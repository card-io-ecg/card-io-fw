use proc_macro2::{Span, TokenStream};
use quote::quote;
use sci_rs::signal::filter::design::{
    iirfilter_dyn, BaFormatFilter, DigitalFilter, FilterBandType, FilterOutputType, FilterType,
};
use std::collections::HashMap;
use syn::{
    parse::{Parse, ParseBuffer},
    Lit, LitStr, Token,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FilterKind {
    HighPassIir,
}

impl Parse for FilterKind {
    fn parse(input: &ParseBuffer) -> syn::Result<Self> {
        let kind = input.parse::<LitStr>()?;

        let filter_kind = kind.value().to_ascii_lowercase();
        let filter_kind = match filter_kind.as_str() {
            "highpassiir" => FilterKind::HighPassIir,

            _ => {
                return Err(syn::Error::new(
                    kind.span(),
                    format!("unknown filter type: {filter_kind}"),
                ));
            }
        };

        Ok(filter_kind)
    }
}

pub struct FilterSpec {
    filter_kind: FilterKind,
    span: Span,
    options: HashMap<String, f32>,
}

impl Parse for FilterSpec {
    fn parse(input: &ParseBuffer) -> syn::Result<Self> {
        let filter_kind = input.parse::<FilterKind>()?;
        let mut options = HashMap::new();

        while !input.is_empty() {
            input.parse::<Token![,]>()?;
            let key = input.parse::<LitStr>()?;

            let keystr = validate_filter_option(filter_kind, &key)?;

            input.parse::<Token![,]>()?;
            let value = input.parse::<Lit>()?;

            let value = match value {
                Lit::Int(lit) => lit.base10_parse().unwrap(),
                Lit::Float(lit) => lit.base10_parse().unwrap(),
                _ => return Err(syn::Error::new_spanned(value, "expected a number")),
            };

            if options.insert(keystr, value).is_some() {
                return Err(syn::Error::new(
                    key.span(),
                    format!("duplicate option: {}", key.value()),
                ));
            }
        }

        Ok(Self {
            filter_kind,
            span: input.span(),
            options,
        })
    }
}

fn validate_filter_option(filter_kind: FilterKind, key: &LitStr) -> syn::Result<String> {
    let expected = match filter_kind {
        FilterKind::HighPassIir => &[
            "filterorder",
            "passbandfrequency",
            "halfpowerfrequency",
            "passbandripple",
            "samplerate",
        ],
    };

    let value = key.value().to_ascii_lowercase();
    if !expected.contains(&value.as_str()) {
        return Err(syn::Error::new(
            key.span(),
            format!("'{value}' is not expected for '{filter_kind:?}'"),
        ));
    }

    Ok(value)
}

pub fn run(args: FilterSpec) -> TokenStream {
    let module = quote! {crate::filter};

    match args.filter_kind {
        FilterKind::HighPassIir => {
            let Some(&order) = args.options.get("filterorder") else {
                return syn::Error::new(args.span, "missing required option 'FilterOrder'")
                    .to_compile_error();
            };

            let filter = iirfilter_dyn(
                order as usize,
                vec![args.options.get("halfpowerfrequency").copied().unwrap()],
                None,
                None,
                Some(FilterBandType::Highpass),
                Some(FilterType::Butterworth),
                Some(false),
                Some(FilterOutputType::Ba),
                args.options.get("samplerate").copied(),
            );

            let DigitalFilter::Ba(BaFormatFilter { mut b, mut a }) = filter else {
                unreachable!()
            };

            // count trailing zeros
            let zeros_a = a.iter().rev().take_while(|&&x| x == 0.0).count();
            let zeros_b = b.iter().rev().take_while(|&&x| x == 0.0).count();

            let remove = zeros_a.min(zeros_b);

            a.truncate(a.len() - remove);
            b.truncate(b.len() - remove);

            // Strip off always-1 coefficient
            assert!(a.swap_remove(0) == 1.0);

            a.reverse();
            b.reverse();

            let n = a.len();
            let kind = quote! { HighPass };

            quote! {
                #module::iir::Iir::<#kind, #n>::new(&[#(#b,)*], &[#(#a,)*])
            }
        }
    }
}
