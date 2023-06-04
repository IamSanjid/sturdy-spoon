extern crate proc_macro;
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_macro_input, DeriveInput, Data, Error, Attribute};

use proc_macro2::TokenStream as TokenStream2;

#[proc_macro_derive(InternalServerError, attributes(code))]
pub fn derive_error(input: TokenStream) -> TokenStream {
    let node = parse_macro_input!(input as DeriveInput);

    let ty = &node.ident;
    let (impl_generics, ty_generics, _where_clause) = node.generics.split_for_impl();

    let enum_data = match node.data {
        Data::Enum(data) => data,
        _ => {
            return Error::new_spanned(
                node,
                "only enum type is supported",
            ).into_compile_error().into();
        }
    };

    let mut arms = Vec::new();

    for variant in enum_data.variants {
        let code_attr = match get_code_attr(&variant.attrs) {
            Ok(Some(code_attr)) => code_attr,
            Err(err) => {
                return err.into_compile_error().into()
            }
            _ => {
                return Error::new_spanned(
                    variant,
                    "missing #[code(...)] attribute",
                ).to_compile_error().into();
            }
        };
        let pat = if variant.fields.len() > 0 {
            let vars = variant.fields.iter().map(|_|{
                format_ident!("_")
            });
            quote!((#(#vars),*))
        } else {
            quote!({})
        };
        let ident = &variant.ident;
        arms.push(quote!{
            #ty::#ident #pat => (#code_attr, format!("{}", self)).into_response()
        });
    }

    let s = quote! {
        #[allow(unused_qualifications)]
        impl #impl_generics axum::response::IntoResponse for #ty #ty_generics {
            fn into_response(self) -> axum::response::Response {
                #[allow(unused_variables, deprecated, clippy::used_underscore_binding)]
                match self {
                    #(#arms,)*
                }
            }
        }
    };

    return s.into();
}

fn get_code_attr(attrs: &[Attribute]) -> Result<Option<&TokenStream2>, Error> {
    let mut attr_token_stream = None;
    for attr in attrs {
        if attr.path().is_ident("code") {
            if attr_token_stream.is_some() {
                return Err(Error::new_spanned(attr, "only one #[code(...)] attribute is allowed"));
            }
            if let syn::Meta::List(list) = &attr.meta {
                attr_token_stream = Some(&list.tokens);
            }
        }
    }
    Ok(attr_token_stream)
}