use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput, Data, Fields};

/// See [`pathmap::zipper::PolyZipper`] for documentation
#[proc_macro_derive(PolyZipper)]
pub fn derive_poly_zipper(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let enum_name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    // Extract enum variants
    let variants = match &input.data {
        Data::Enum(data_enum) => &data_enum.variants,
        _ => panic!("PolyZipper can only be derived for enums"),
    };

    // Generate From and TryFrom impls for each variant
    let from_impls = variants.iter().map(|variant| {
        let variant_name = &variant.ident;

        // Get the inner type (assuming single unnamed field)
        let inner_type = match &variant.fields {
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                &fields.unnamed[0].ty
            }
            _ => panic!("Each variant must have exactly one unnamed field"),
        };

        quote! {
            impl #impl_generics From<#inner_type> for #enum_name #ty_generics #where_clause {
                fn from(value: #inner_type) -> Self {
                    #enum_name::#variant_name(value)
                }
            }

            impl #impl_generics TryFrom<#enum_name #ty_generics> for #inner_type #where_clause {
                type Error = ();

                fn try_from(value: #enum_name #ty_generics) -> Result<Self, Self::Error> {
                    match value {
                        #enum_name::#variant_name(inner) => Ok(inner),
                        _ => Err(()),
                    }
                }
            }
        }
    });

    let variant_arms: Vec<_> = variants.iter().map(|variant| {
        let variant_name = &variant.ident;
        quote! { Self::#variant_name(inner) }
    }).collect();

    let inner_types: Vec<_> = variants.iter().map(|variant| {
        match &variant.fields {
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                &fields.unnamed[0].ty
            }
            _ => panic!("Each variant must have exactly one unnamed field"),
        }
    }).collect();

    // Generate Zipper trait implementation
    let zipper_impl = {
        let variant_arms = &variant_arms;
        quote! {
            impl #impl_generics pathmap::zipper::Zipper for #enum_name #ty_generics #where_clause {
                fn path_exists(&self) -> bool {
                    match self {
                        #(#variant_arms => inner.path_exists(),)*
                    }
                }

                fn is_val(&self) -> bool {
                    match self {
                        #(#variant_arms => inner.is_val(),)*
                    }
                }

                fn child_count(&self) -> usize {
                    match self {
                        #(#variant_arms => inner.child_count(),)*
                    }
                }

                fn child_mask(&self) -> pathmap::utils::ByteMask {
                    match self {
                        #(#variant_arms => inner.child_mask(),)*
                    }
                }
            }
        }
    };

    // Generate ZipperValues trait implementation
    let zipper_values_impl = {
        let variant_arms = &variant_arms;
        quote! {
            impl #impl_generics pathmap::zipper::ZipperValues<V> for #enum_name #ty_generics
            where
                #(#inner_types: pathmap::zipper::ZipperValues<V>,)*
                #where_clause
            {
                fn val(&self) -> Option<&V> {
                    match self {
                        #(#variant_arms => inner.val(),)*
                    }
                }
            }
        }
    };

    let expanded = quote! {
        #(#from_impls)*
        #zipper_impl
        #zipper_values_impl
    };

    TokenStream::from(expanded)
}
