use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput, Data, Fields};

// See the docs for `PolyZipper` in `pathmap::zipper::PolyZipper`
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

            impl #impl_generics core::convert::TryFrom<#enum_name #ty_generics> for #inner_type #where_clause {
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

    // Generate ZipperReadOnlyValues trait implementation
    let zipper_read_only_values_impl = {
        let variant_arms = &variant_arms;
        quote! {
            impl #impl_generics pathmap::zipper::ZipperReadOnlyValues<'trie, V> for #enum_name #ty_generics
            where
                #(#inner_types: pathmap::zipper::ZipperReadOnlyValues<'trie, V>,)*
                #where_clause
            {
                fn get_val(&self) -> Option<&'trie V> {
                    match self {
                        #(#variant_arms => inner.get_val(),)*
                    }
                }
            }
        }
    };

    // Generate witness enum name and variant names for conditional traits
    let witness_enum_name = syn::Ident::new(&format!("{}Witness", enum_name), enum_name.span());
    let variant_names: Vec<_> = variants.iter().map(|variant| &variant.ident).collect();

    // Generate ZipperReadOnlyConditionalValues trait implementation with witness enum
    let zipper_read_only_conditional_values_impl = {
        quote! {
            //GOAT TODO: I think we could get into trouble if the witness types have fewer generics than
            // the outer PolyZipper enum.  So we probably should add a phantom case too.
            pub enum #witness_enum_name #impl_generics
            where
                #(#inner_types: pathmap::zipper::ZipperReadOnlyConditionalValues<'trie, V>,)*
                #where_clause
            {
                #(#variant_names(<#inner_types as pathmap::zipper::ZipperReadOnlyConditionalValues<'trie, V>>::WitnessT),)*
            }

            impl #impl_generics pathmap::zipper::ZipperReadOnlyConditionalValues<'trie, V> for #enum_name #ty_generics
            where
                #(#inner_types: pathmap::zipper::ZipperReadOnlyConditionalValues<'trie, V>,)*
                #where_clause
            {
                type WitnessT = #witness_enum_name #ty_generics;

                fn witness<'w>(&self) -> Self::WitnessT {
                    match self {
                        #(Self::#variant_names(inner) => #witness_enum_name::#variant_names(inner.witness()),)*
                    }
                }

                fn get_val_with_witness<'w>(&self, witness: &'w Self::WitnessT) -> Option<&'w V> where 'trie: 'w {
                    match (self, witness) {
                        #((Self::#variant_names(inner), #witness_enum_name::#variant_names(w)) => inner.get_val_with_witness(w),)*
                        _ => {
                            debug_assert!(false, "Witness variant must match zipper + variant");
                            None
                        },
                    }
                }
            }
        }
    };

    //GOAT, this is probably dead code given the decision (described in the PolyZipper docs)
    // not to support a `ZipperForking` impl
    //
    //I can't seem to figure out how to align the `'a` and the `'read_z` lifetimes without changing the trait definition,
    // and ideally we'd want to return a new enum-based zipper.  The elegant way to do that would be to actually invoke
    // the PolyZipper macro recursively to get maximum support on the new zipper, but there are a bunch of obnoxious details
    // to make that work. - like what to do about the recursion so we don't have infinite recursion in the macro, and how
    // to map the lifetimes required by the trait onto the lifetimes of the new zippers.
    //
    // // Generate ZipperForking trait implementation
    // let zipper_forking_impl = {
    //     let variant_arms = &variant_arms;
    //     let first_inner_type = &inner_types[0];
    //     let other_inner_types = &inner_types[1..];

    //     // Create modified generics with additional lifetime
    //     let mut forking_generics = generics.clone();
    //     forking_generics.params.insert(0, syn::parse_quote!('read_z));
    //     let (forking_impl_generics, _, _) = forking_generics.split_for_impl();

    //     quote! {
    //         impl #forking_impl_generics pathmap::zipper::ZipperForking<V> for #enum_name #ty_generics
    //         where
    //             #(#inner_types: pathmap::zipper::ZipperForking<V>,)*
    //             #(#other_inner_types: pathmap::zipper::ZipperForking<V, ReadZipperT<'read_z> = <#first_inner_type as pathmap::zipper::ZipperForking<V>>::ReadZipperT<'read_z>>,)*
    //             Self: 'read_z,
    //             #where_clause
    //         {
    //             type ReadZipperT<'a> = <#first_inner_type as pathmap::zipper::ZipperForking<V>>::ReadZipperT<'a> where Self: 'a;

    //             fn fork_read_zipper<'a>(&'a self) -> Self::ReadZipperT<'a> {
    //                 match self {
    //                     #(#variant_arms => inner.fork_read_zipper(),)*
    //                 }
    //             }
    //         }
    //     }
    // };

    // Generate ZipperPath trait implementation
    let zipper_path_impl = {
        let variant_arms = &variant_arms;
        quote! {
            impl #impl_generics pathmap::zipper::ZipperPath for #enum_name #ty_generics
            where
                #(#inner_types: pathmap::zipper::ZipperPath,)*
                #where_clause
            {
                fn path(&self) -> &[u8] {
                    match self {
                        #(#variant_arms => inner.path(),)*
                    }
                }
            }
        }
    };

    // Generate ZipperMoving trait implementation
    let zipper_moving_impl = {
        let variant_arms = &variant_arms;
        quote! {
            impl #impl_generics pathmap::zipper::ZipperMoving for #enum_name #ty_generics
            where
                #(#inner_types: pathmap::zipper::ZipperMoving,)*
                #where_clause
            {
                fn at_root(&self) -> bool {
                    match self {
                        #(#variant_arms => inner.at_root(),)*
                    }
                }

                #[inline]
                fn focus_byte(&self) -> Option<u8> {
                    match self {
                        #(#variant_arms => inner.focus_byte(),)*
                    }
                }

                fn reset(&mut self) {
                    match self {
                        #(#variant_arms => inner.reset(),)*
                    }
                }

                fn val_count(&self) -> usize {
                    match self {
                        #(#variant_arms => inner.val_count(),)*
                    }
                }

                fn descend_to<K: AsRef<[u8]>>(&mut self, k: K) {
                    match self {
                        #(#variant_arms => inner.descend_to(k),)*
                    }
                }

                fn descend_to_existing<K: AsRef<[u8]>>(&mut self, k: K) -> usize {
                    match self {
                        #(#variant_arms => inner.descend_to_existing(k),)*
                    }
                }

                fn descend_to_val<K: AsRef<[u8]>>(&mut self, k: K) -> usize {
                    match self {
                        #(#variant_arms => inner.descend_to_val(k),)*
                    }
                }

                fn descend_to_check<K: AsRef<[u8]>>(&mut self, k: K) -> bool {
                    match self {
                        #(#variant_arms => inner.descend_to_check(k),)*
                    }
                }

                #[inline]
                fn descend_to_byte(&mut self, k: u8) {
                    match self {
                        #(#variant_arms => inner.descend_to_byte(k),)*
                    }
                }

                #[inline]
                fn descend_to_existing_byte(&mut self, k: u8) -> bool {
                    match self {
                        #(#variant_arms => inner.descend_to_existing_byte(k),)*
                    }
                }

                fn descend_indexed_byte(&mut self, idx: usize) -> Option<u8> {
                    match self {
                        #(#variant_arms => inner.descend_indexed_byte(idx),)*
                    }
                }

                fn descend_first_byte(&mut self) -> Option<u8> {
                    match self {
                        #(#variant_arms => inner.descend_first_byte(),)*
                    }
                }

                fn descend_last_byte(&mut self) -> Option<u8> {
                    match self {
                        #(#variant_arms => inner.descend_last_byte(),)*
                    }
                }

                fn descend_until<Obs: pathmap::zipper::PathObserver>(&mut self, obs: &mut Obs) -> bool {
                    match self {
                        #(#variant_arms => inner.descend_until(obs),)*
                    }
                }

                fn ascend(&mut self, steps: usize) -> usize {
                    match self {
                        #(#variant_arms => inner.ascend(steps),)*
                    }
                }

                fn ascend_byte(&mut self) -> bool {
                    match self {
                        #(#variant_arms => inner.ascend_byte(),)*
                    }
                }

                fn ascend_until(&mut self) -> usize {
                    match self {
                        #(#variant_arms => inner.ascend_until(),)*
                    }
                }

                fn ascend_until_branch(&mut self) -> usize {
                    match self {
                        #(#variant_arms => inner.ascend_until_branch(),)*
                    }
                }

                fn to_next_sibling_byte(&mut self) -> Option<u8> {
                    match self {
                        #(#variant_arms => inner.to_next_sibling_byte(),)*
                    }
                }

                fn to_prev_sibling_byte(&mut self) -> Option<u8> {
                    match self {
                        #(#variant_arms => inner.to_prev_sibling_byte(),)*
                    }
                }

                fn to_next_step(&mut self) -> bool {
                    match self {
                        #(#variant_arms => inner.to_next_step(),)*
                    }
                }
            }
        }
    };

    // Generate ZipperConcrete trait implementation
    let zipper_concrete_impl = {
        let variant_arms = &variant_arms;
        quote! {
            impl #impl_generics pathmap::zipper::ZipperConcrete for #enum_name #ty_generics
            where
                #(#inner_types: pathmap::zipper::ZipperConcrete,)*
                #where_clause
            {
                fn shared_node_id(&self) -> Option<u64> {
                    match self {
                        #(#variant_arms => inner.shared_node_id(),)*
                    }
                }
                fn is_shared(&self) -> bool {
                    match self {
                        #(#variant_arms => inner.is_shared(),)*
                    }
                }
            }
        }
    };

    // Generate ZipperAbsolutePath trait implementation
    let zipper_absolute_path_impl = {
        let variant_arms = &variant_arms;
        quote! {
            impl #impl_generics pathmap::zipper::ZipperAbsolutePath for #enum_name #ty_generics
            where
                #(#inner_types: pathmap::zipper::ZipperAbsolutePath,)*
                #where_clause
            {
                fn origin_path(&self) -> &[u8] {
                    match self {
                        #(#variant_arms => inner.origin_path(),)*
                    }
                }

                fn root_prefix_path(&self) -> &[u8] {
                    match self {
                        #(#variant_arms => inner.root_prefix_path(),)*
                    }
                }
            }
        }
    };

    // Generate ZipperPathBuffer trait implementation
    let zipper_path_buffer_impl = {
        let variant_arms = &variant_arms;
        quote! {
            impl #impl_generics pathmap::zipper::ZipperPathBuffer for #enum_name #ty_generics
            where
                #(#inner_types: pathmap::zipper::ZipperPathBuffer,)*
                #where_clause
            {
                unsafe fn origin_path_assert_len(&self, len: usize) -> &[u8] {
                    match self {
                        #(#variant_arms => unsafe { inner.origin_path_assert_len(len) },)*
                    }
                }

                fn prepare_buffers(&mut self) {
                    match self {
                        #(#variant_arms => inner.prepare_buffers(),)*
                    }
                }

                fn reserve_buffers(&mut self, path_len: usize, stack_depth: usize) {
                    match self {
                        #(#variant_arms => inner.reserve_buffers(path_len, stack_depth),)*
                    }
                }
            }
        }
    };

    // Generate ZipperIteration trait implementation
    let zipper_iteration_impl = {
        let variant_arms = &variant_arms;
        quote! {
            impl #impl_generics pathmap::zipper::ZipperIteration for #enum_name #ty_generics
            where
                #(#inner_types: pathmap::zipper::ZipperIteration,)*
                #where_clause
            {
                fn to_next_val(&mut self) -> bool {
                    match self {
                        #(#variant_arms => inner.to_next_val(),)*
                    }
                }

                fn descend_last_path(&mut self) -> bool {
                    match self {
                        #(#variant_arms => inner.descend_last_path(),)*
                    }
                }

                fn descend_first_k_path(&mut self, k: usize) -> bool {
                    match self {
                        #(#variant_arms => inner.descend_first_k_path(k),)*
                    }
                }

                fn to_next_k_path(&mut self, k: usize) -> bool {
                    match self {
                        #(#variant_arms => inner.to_next_k_path(k),)*
                    }
                }
            }
        }
    };

    // Generate ZipperReadOnlyIteration trait implementation
    let zipper_read_only_iteration_impl = {
        let variant_arms = &variant_arms;
        quote! {
            impl #impl_generics pathmap::zipper::ZipperReadOnlyIteration<'trie, V> for #enum_name #ty_generics
            where
                #(#inner_types: pathmap::zipper::ZipperReadOnlyIteration<'trie, V>,)*
                #where_clause
            {
                fn to_next_get_val(&mut self) -> Option<&'trie V> {
                    match self {
                        #(#variant_arms => inner.to_next_get_val(),)*
                    }
                }

                #[deprecated]
                fn to_next_get_value(&mut self) -> Option<&'trie V> {
                    match self {
                        #(#variant_arms => inner.to_next_get_value(),)*
                    }
                }
            }
        }
    };

    // Generate ZipperReadOnlyConditionalIteration trait implementation
    let zipper_read_only_conditional_iteration_impl = {
        quote! {
            impl #impl_generics pathmap::zipper::ZipperReadOnlyConditionalIteration<'trie, V> for #enum_name #ty_generics
            where
                #(#inner_types: pathmap::zipper::ZipperReadOnlyConditionalIteration<'trie, V>,)*
                #where_clause
            {
                fn to_next_get_val_with_witness<'w>(&mut self, witness: &'w Self::WitnessT) -> Option<&'w V> where 'trie: 'w {
                    match (self, witness) {
                        #((Self::#variant_names(inner), #witness_enum_name::#variant_names(w)) => inner.to_next_get_val_with_witness(w),)*
                        _ => {
                            debug_assert!(false, "Witness variant must match zipper variant");
                            None
                        },
                    }
                }
            }
        }
    };

    let expanded = quote! {
        #(#from_impls)*
        #zipper_impl
        #zipper_values_impl
        #zipper_read_only_values_impl
        #zipper_read_only_conditional_values_impl
        // #zipper_forking_impl
        #zipper_path_impl
        #zipper_moving_impl
        #zipper_concrete_impl
        #zipper_absolute_path_impl
        #zipper_path_buffer_impl
        #zipper_iteration_impl
        #zipper_read_only_iteration_impl
        #zipper_read_only_conditional_iteration_impl
    };

    TokenStream::from(expanded)
}
