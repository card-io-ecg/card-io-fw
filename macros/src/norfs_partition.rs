use esp_idf_part::PartitionTable;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    parse::{Parse, ParseBuffer},
    ItemStruct, LitStr,
};

pub struct Args {
    name: String,
}

impl Parse for Args {
    fn parse(input: &ParseBuffer) -> syn::Result<Self> {
        input
            .parse::<LitStr>()
            .map(|name| Args { name: name.value() })
    }
}

pub fn implement(args: Args, item_struct: ItemStruct, input: TokenStream) -> TokenStream {
    let Args { name } = args;

    let struct_name = item_struct.ident;

    let csv = std::fs::read_to_string("partitions.csv").unwrap();
    let table = PartitionTable::try_from_str(csv).unwrap();
    let part = table.find(&name).expect("No partition found");

    let offset = part.offset() as usize;
    let size = part.size() as usize;

    quote! {
        #input

        impl InternalPartition for #struct_name {
            const OFFSET: usize = #offset;
            const SIZE: usize = #size;
        }
    }
}
