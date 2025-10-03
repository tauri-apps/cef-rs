use convert_case::{Case, Casing};
use quote::{format_ident, quote, ToTokens};
use regex::Regex;
use std::{
    cell::OnceCell,
    collections::BTreeMap,
    fmt::{self, Debug, Display, Formatter},
    fs,
    io::Write,
    iter::{self, Iterator},
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
};

pub fn generate_bindings(source_path: &Path) -> crate::Result<PathBuf> {
    let bindings = crate::read_bindings(source_path)?;
    let parsed = syn::parse_file(&bindings)?;
    let parse_tree = ParseTree::from(&parsed);

    let mut out_file = crate::dirs::get_out_dir();
    out_file.push("bindings.rs");
    let mut bindings = fs::File::create(&out_file)?;
    write!(bindings, "{parse_tree}")?;
    format_bindings(&out_file)?;

    Ok(out_file)
}

#[derive(Debug, Error)]
pub enum Unrecognized {
    #[error("Unrecognized Field Type")]
    FieldType,
    #[error("Unrecognized Function Argument")]
    FnArg,
    #[error("Unrecognized Generic Type")]
    Generic,
    #[error("Unrecognized Interface Declaration")]
    Interface,
    #[error("Failed to Parse Bindings")]
    Parse(#[from] syn::Error),
}

struct TypeAliasRef<'a> {
    name: String,
    ty: &'a syn::Type,
}

struct EnumRef<'a> {
    name: String,
    ty: Option<&'a syn::ItemEnum>,
}

struct FieldRef<'a> {
    name: String,
    ty: &'a syn::Type,
}

impl<'a> TryFrom<&'a syn::Field> for FieldRef<'a> {
    type Error = Unrecognized;

    fn try_from(value: &'a syn::Field) -> Result<Self, Self::Error> {
        let name = value
            .ident
            .as_ref()
            .ok_or(Unrecognized::FieldType)?
            .to_string();

        Ok(Self {
            name,
            ty: &value.ty,
        })
    }
}

#[derive(Clone)]
struct FnArgRef<'a> {
    name: String,
    ty: &'a syn::Type,
}

struct SignatureRef<'a> {
    name: String,
    inputs: Vec<FnArgRef<'a>>,
    output: Option<&'a syn::Type>,
    merged_params: OnceCell<Vec<MergedParam>>,
}

enum MergedParam {
    Receiver,
    Single {
        name: String,
        ty: Option<ModifiedType>,
    },
    Bounded {
        count_name: String,
        count_ty: ModifiedType,
        slice_name: String,
        slice_ty: ModifiedType,
    },
    Buffer {
        slice_name: String,
        slice_ty: ModifiedType,
        size_name: String,
        size_ty: ModifiedType,
    },
}

impl SignatureRef<'_> {
    fn capture_params(&self) -> Option<proc_macro2::TokenStream> {
        let arg_names = self
            .inputs
            .iter()
            .map(|arg| {
                let name = make_snake_case_value_name(arg.name.as_str());
                let local = format!("arg_{name}");
                (local, name)
            })
            .collect::<Vec<_>>();

        let local = arg_names
            .iter()
            .map(|(name, _)| format_ident!("{name}").to_token_stream());
        let name = arg_names
            .iter()
            .map(|(_, name)| format_ident!("{name}").to_token_stream());

        match arg_names.len() {
            0 => None,
            1 => Some(quote! { let #(#local)* = #(#name)*; }),
            _ => Some(quote! { let (#(#local),*) = (#(#name),*); }),
        }
    }

    fn merge_params(&self, tree: &ParseTree) -> impl Iterator<Item = &MergedParam> {
        self.merged_params
            .get_or_init(|| {
                let mut args = self
                    .inputs
                    .iter()
                    .map(|arg| {
                        Some(MergedParam::Single {
                            name: make_snake_case_value_name(&arg.name),
                            ty: tree.resolve_modified_type(arg.ty),
                        })
                    })
                    .collect::<Vec<_>>();

                for i in 1..(args.len()) {
                    let replacement = match (&args[i - 1], &args[i]) {
                        (
                            Some(MergedParam::Single {
                                name: count_name,
                                ty: Some(count_ty),
                            }),
                            Some(MergedParam::Single {
                                name: elem_name,
                                ty: Some(elem_ty),
                            }),
                        ) if count_name.as_str() == format!("{elem_name}_count").as_str() => {
                            if count_ty.ty.to_token_stream().to_string().as_str()
                                != format_ident!("usize")
                                    .to_token_stream()
                                    .to_string()
                                    .as_str()
                            {
                                continue;
                            }

                            let elem_tokens = elem_ty.ty.to_token_stream();
                            let elem_ty_string = elem_tokens.to_string();
                            match tree.cef_name_map.get(&elem_ty_string) {
                                Some(NameMapEntry {
                                    ty: NameMapType::StructDeclaration(_),
                                    ..
                                }) => {
                                    let modifiers = match (
                                        count_ty.modifiers.as_slice(),
                                        elem_ty.modifiers.as_slice(),
                                    ) {
                                        ([], [TypeModifier::ConstPtr, TypeModifier::MutPtr]) => {
                                            vec![TypeModifier::Slice]
                                        }
                                        (
                                            [TypeModifier::MutPtr],
                                            [TypeModifier::MutPtr, TypeModifier::MutPtr],
                                        ) => vec![TypeModifier::MutSlice],
                                        _ => continue,
                                    };
                                    let Ok(slice_ty) = syn::parse2::<syn::Type>(elem_tokens) else {
                                        continue;
                                    };

                                    Some(MergedParam::Bounded {
                                        count_name: count_name.clone(),
                                        count_ty: count_ty.clone(),
                                        slice_name: elem_name.clone(),
                                        slice_ty: ModifiedType {
                                            modifiers,
                                            ty: slice_ty,
                                        },
                                    })
                                }
                                _ => {
                                    let modifiers = match (
                                        count_ty.modifiers.as_slice(),
                                        elem_ty.modifiers.as_slice(),
                                    ) {
                                        ([], [TypeModifier::ConstPtr]) => {
                                            vec![TypeModifier::Slice]
                                        }
                                        ([TypeModifier::MutPtr], [TypeModifier::MutPtr]) => {
                                            vec![TypeModifier::MutSlice]
                                        }
                                        _ => continue,
                                    };
                                    let Ok(slice_ty) = syn::parse2::<syn::Type>(elem_tokens) else {
                                        continue;
                                    };

                                    Some(MergedParam::Bounded {
                                        count_name: count_name.clone(),
                                        count_ty: count_ty.clone(),
                                        slice_name: elem_name.clone(),
                                        slice_ty: ModifiedType {
                                            modifiers,
                                            ty: slice_ty,
                                        },
                                    })
                                }
                            }
                        }
                        (
                            Some(MergedParam::Single {
                                name: elem_name,
                                ty: Some(elem_ty),
                            }),
                            Some(MergedParam::Single {
                                name: size_name,
                                ty: Some(size_ty),
                            }),
                        ) if elem_ty.ty.to_token_stream().to_string()
                            == quote! { ::std::os::raw::c_void }.to_string() =>
                        {
                            if size_name.as_str() != format!("{elem_name}_size").as_str()
                                || size_ty.ty.to_token_stream().to_string().as_str()
                                    != format_ident!("usize")
                                        .to_token_stream()
                                        .to_string()
                                        .as_str()
                            {
                                continue;
                            }

                            let modifiers = match (
                                size_ty.modifiers.as_slice(),
                                elem_ty.modifiers.as_slice(),
                            ) {
                                ([], [TypeModifier::ConstPtr]) => {
                                    vec![TypeModifier::Slice]
                                }
                                ([], [TypeModifier::MutPtr])
                                | (
                                    [TypeModifier::MutPtr],
                                    [TypeModifier::MutPtr, TypeModifier::MutPtr],
                                ) => vec![TypeModifier::MutSlice],
                                _ => continue,
                            };

                            // Remove the size argument and replace the buffer pointer argument with a
                            // &[u8] or &mut &mut [u8] slice.
                            Some(MergedParam::Buffer {
                                slice_name: elem_name.clone(),
                                slice_ty: ModifiedType {
                                    modifiers,
                                    ty: elem_ty.ty.clone(),
                                },
                                size_name: size_name.clone(),
                                size_ty: size_ty.clone(),
                            })
                        }
                        (
                            Some(MergedParam::Single {
                                name: elem_name,
                                ty: Some(elem_ty),
                            }),
                            Some(MergedParam::Single {
                                name: size_name,
                                ty: Some(size_ty),
                            }),
                        ) if (size_name.as_str() == format!("{elem_name}_size").as_str()
                            || size_name.as_str() == format!("{elem_name}_len").as_str())
                            && size_ty.ty.to_token_stream().to_string().as_str()
                                == format_ident!("usize")
                                    .to_token_stream()
                                    .to_string()
                                    .as_str() =>
                        {
                            let modifiers = match (
                                size_ty.modifiers.as_slice(),
                                elem_ty.modifiers.as_slice(),
                            ) {
                                ([], [TypeModifier::ConstPtr]) => {
                                    vec![TypeModifier::Slice]
                                }
                                ([], [TypeModifier::MutPtr])
                                | (
                                    [TypeModifier::MutPtr],
                                    [TypeModifier::MutPtr, TypeModifier::MutPtr],
                                ) => vec![TypeModifier::MutSlice],
                                _ => continue,
                            };

                            // Remove the size argument and replace the buffer pointer argument with a
                            // &[elem_ty] or &mut &mut [elem_ty] slice.
                            Some(MergedParam::Buffer {
                                slice_name: elem_name.clone(),
                                slice_ty: ModifiedType {
                                    modifiers,
                                    ty: elem_ty.ty.clone(),
                                },
                                size_name: size_name.clone(),
                                size_ty: size_ty.clone(),
                            })
                        }
                        _ => None,
                    };

                    if let Some(replacement) = replacement {
                        args[i - 1] = Some(replacement);
                        args[i] = None;
                    }
                }

                args.into_iter()
                    .flatten()
                    .map(|arg| match arg {
                        MergedParam::Single { name, ty } => {
                            if name.as_str() == "self_" {
                                MergedParam::Receiver
                            } else {
                                MergedParam::Single { name, ty }
                            }
                        }
                        _ => arg,
                    })
                    .collect()
            })
            .iter()
    }

    fn capture_merged_params(&self, tree: &ParseTree) -> Option<proc_macro2::TokenStream> {
        let arg_names = self
            .merge_params(tree)
            .flat_map(|arg| match arg {
                MergedParam::Receiver => None,
                MergedParam::Single { name, .. } => Some(name.clone()),
                MergedParam::Bounded { slice_name, .. } => Some(slice_name.clone()),
                MergedParam::Buffer { slice_name, .. } => Some(slice_name.clone()),
            })
            .map(|name| {
                let local = format!("arg_{name}");
                (local, name)
            })
            .collect::<Vec<_>>();

        let local = arg_names
            .iter()
            .map(|(name, _)| format_ident!("{name}").to_token_stream());
        let name = arg_names
            .iter()
            .map(|(_, name)| format_ident!("{name}").to_token_stream());

        match arg_names.len() {
            0 => None,
            1 => Some(quote! { let #(#local)* = #(#name)*; }),
            _ => Some(quote! { let (#(#local),*) = (#(#name),*); }),
        }
    }

    fn get_rust_args(&self, tree: &ParseTree) -> proc_macro2::TokenStream {
        let args = self.merge_params(tree).filter_map(|arg| match arg {
            MergedParam::Receiver => Some(quote! { &self }),
            MergedParam::Single { name, ty: Some(ty) } => {
                let name = format_ident!("{name}");
                let ty = ty
                    .get_argument_type(tree)
                    .unwrap_or_else(|| ty.ty.to_token_stream());
                Some(quote! { #name: #ty })
            }
            MergedParam::Bounded {
                slice_name,
                slice_ty,
                ..
            } => {
                let slice_name = format_ident!("{slice_name}");
                let slice_ty = slice_ty
                    .get_argument_type(tree)
                    .unwrap_or_else(|| slice_ty.ty.to_token_stream());
                Some(quote! { #slice_name: #slice_ty })
            }
            MergedParam::Buffer {
                slice_name,
                slice_ty,
                ..
            } => {
                let slice_name = format_ident!("{slice_name}");
                let slice_ty = slice_ty
                    .get_argument_type(tree)
                    .unwrap_or_else(|| slice_ty.ty.to_token_stream());
                Some(quote! { #slice_name: #slice_ty })
            }
            _ => None,
        });

        quote! { #(#args),* }
    }

    fn unwrap_rust_args(&self, tree: &ParseTree) -> proc_macro2::TokenStream {
        let capture = self.capture_merged_params(tree);
        let args = self.merge_params(tree).filter_map(|arg| match arg {
            MergedParam::Receiver => Some(quote! {
                let arg_self_ = self.into_raw();
            }),
            MergedParam::Single {
                name,
                ty: Some(arg_ty),
            } => {
                let arg_name = format_ident!("arg_{name}");
                let out_name = format_ident!("out_{name}");
                let (modifiers, arg_ty) = (arg_ty.modifiers.as_slice(), &arg_ty.ty);
                let ty_tokens = arg_ty.to_token_stream();
                let ty_string = ty_tokens.to_string();
                let entry = tree.cef_name_map.get(&ty_string);
                (tree.root(&ty_string) == BASE_REF_COUNTED)
                    .then(|| {
                        match modifiers {
                            [TypeModifier::MutPtr, TypeModifier::MutPtr] => {
                                Some(quote! {
                                    let #out_name = #arg_name;
                                    let mut ptr = std::ptr::null_mut();
                                    let (#out_name, #arg_name) = #out_name
                                        .map(|arg| {
                                            if let Some(arg) = arg.as_mut() {
                                                arg.add_ref();
                                                ptr = arg.get_raw();
                                            }
                                            (Some(arg), std::ptr::from_mut(&mut ptr))
                                        })
                                        .unwrap_or((None, std::ptr::null_mut()));
                                })
                            }
                            _ => {
                                if ty_string.as_str() == BASE_REF_COUNTED {
                                    Some(quote!{
                                        let #out_name = #arg_name;
                                        let #arg_name = #out_name.map(|arg| {
                                            arg.add_ref();
                                            arg.into_raw()
                                        }).unwrap_or(std::ptr::null_mut());
                                    })
                                } else {
                                    let cast = entry.and_then(|entry| {
                                            syn::parse_str::<syn::Type>(&entry.name).ok()
                                        })
                                        .map(|ty| {
                                            let ty = ty.to_token_stream().to_string();
                                            let ty = format_ident!("Impl{ty}");
                                            quote!{ #ty::get_raw(arg) }
                                        })
                                        .unwrap_or_else(|| quote!{ arg.get_raw() });

                                    Some(quote! {
                                        let #arg_name = #arg_name.map(|arg| {
                                            arg.add_ref();
                                            #cast
                                        }).unwrap_or(std::ptr::null_mut());
                                    })
                                }
                            },
                        }
                    })
                    .flatten()
                    .or_else(|| {
                        match entry {
                            Some(NameMapEntry {
                                ty: NameMapType::StructDeclaration(_),
                                ..
                            }) if tree.lookup_struct_declaration
                                    .get(&ty_string)
                                    .and_then(|i| tree.struct_declarations.get(*i))
                                    .map(|s| s.methods.is_empty()
                                        && !s.fields.is_empty()
                                        && !s.fields.iter().map(|f| f.name.as_str()).eq(["_unused"]))
                                    .unwrap_or_default() =>
                            {
                                match modifiers {
                                    [TypeModifier::ConstPtr] => {
                                        Some(quote! {
                                            let #arg_name = #arg_name.cloned().map(|arg| arg.into());
                                            let #arg_name = #arg_name.as_ref().map(std::ptr::from_ref).unwrap_or(std::ptr::null());;
                                        })
                                    }
                                    [TypeModifier::MutPtr] => {
                                        Some(quote! {
                                            let mut #arg_name = #arg_name.cloned().map(|arg| arg.into());
                                            let #arg_name = #arg_name.as_mut().map(std::ptr::from_mut).unwrap_or(std::ptr::null_mut());;
                                        })
                                    }
                                    _ => None,
                                }
                            }
                            Some(NameMapEntry {
                                ty: NameMapType::StructDeclaration(_),
                                ..
                            }) => {
                                let impl_default = match modifiers {
                                    [TypeModifier::ConstPtr] => {
                                        quote! { unwrap_or(std::ptr::null()) }
                                    }
                                    [TypeModifier::MutPtr] => {
                                        quote! { unwrap_or(std::ptr::null_mut()) }
                                    }
                                    _ => quote! { unwrap_or_default() },
                                };

                                Some(quote! {
                                    let #arg_name = #arg_name.map(|arg| arg.into_raw()).#impl_default;
                                })
                            }
                            Some(NameMapEntry {
                                ty: NameMapType::TypeAlias,
                                ..
                            }) if ty_string.as_str() == "cef_string_t" => {
                                let impl_default = match modifiers {
                                    [TypeModifier::ConstPtr] => {
                                        quote! { unwrap_or(std::ptr::null()) }
                                    }
                                    [TypeModifier::MutPtr] => {
                                        quote! { unwrap_or(std::ptr::null_mut()) }
                                    }
                                    _ => quote! { unwrap_or_default() },
                                };

                                Some(quote! {
                                    let #arg_name = #arg_name.map(|arg| arg.into_raw()).#impl_default;
                                })
                            }
                            Some(_) => {
                                Some(quote! {
                                    let #arg_name = #arg_name.into_raw();
                                })
                            }
                            None => {
                                let is_void = ty_string == quote!{ ::std::os::raw::c_void }.to_string();
                                let cast = match modifiers {
                                    [TypeModifier::MutPtr] if !is_void => Some(quote! {
                                        #arg_name
                                            .map(std::ptr::from_mut)
                                            .unwrap_or(std::ptr::null_mut())
                                    }),
                                    [TypeModifier::ConstPtr] if !is_void  => Some(quote! {
                                        #arg_name
                                            .map(std::ptr::from_ref)
                                            .unwrap_or(std::ptr::null())
                                    }),
                                    [TypeModifier::MutPtr | TypeModifier::ConstPtr, ..] => Some(quote! { #arg_name.cast() }),
                                    _ => None,
                                };
                                cast.map(|cast| quote! {
                                    let #arg_name = #cast;
                                })
                            }
                        }
                    })
            }
            MergedParam::Bounded {
                count_name,
                count_ty:
                    ModifiedType {
                        modifiers: count_modifiers,
                        ..
                    },
                slice_name,
                slice_ty:
                    ModifiedType {
                        ty: slice_ty,
                        ..
                    }
            } => {
                let out_count = format_ident!("out_{count_name}");
                let arg_count = format_ident!("arg_{count_name}");
                let arg_name = format_ident!("arg_{slice_name}");
                let out_name = format_ident!("out_{slice_name}");
                let vec_name = format_ident!("vec_{slice_name}");
                let add_refs = if tree.root(&slice_ty.to_token_stream().to_string()) == BASE_REF_COUNTED {
                    Some(quote! { elem.add_ref(); })
                } else {
                    None
                };
                match count_modifiers.as_slice() {
                    [] => Some(quote! {
                        let #arg_count = #arg_name
                            .as_ref()
                            .map(|arg| arg.len())
                            .unwrap_or_default();
                        let #vec_name = #arg_name
                            .as_ref()
                            .map(|arg| arg
                                .iter()
                                .map(|elem| elem
                                    .as_ref()
                                    .map(|elem| {
                                        #add_refs
                                        elem.get_raw()
                                    })
                                    .unwrap_or(std::ptr::null_mut()))
                                .collect::<Vec<_>>())
                            .unwrap_or_default();
                        let #arg_name = if #vec_name.is_empty() {
                            std::ptr::null()
                        } else {
                            #vec_name.as_ptr()
                        };
                    }),
                    [TypeModifier::MutPtr] => Some(quote! {
                        let mut #out_count = #arg_name
                            .as_ref()
                            .map(|arg| arg.len())
                            .unwrap_or_default();
                        let #arg_count = &mut #out_count;
                        let #out_name = #arg_name;
                        let mut #vec_name = #out_name
                            .as_ref()
                            .map(|arg| arg
                                .iter()
                                .map(|elem| elem
                                    .as_ref()
                                    .map(|elem| {
                                        #add_refs
                                        elem.get_raw()
                                    })
                                    .unwrap_or(std::ptr::null_mut()))
                                .collect::<Vec<_>>())
                            .unwrap_or_default();
                        let #arg_name = if #vec_name.is_empty() {
                            std::ptr::null_mut()
                        } else {
                            #vec_name.as_mut_ptr()
                        };
                    }),
                    _ => None,
                }
            }
            MergedParam::Buffer {
                slice_name,
                slice_ty,
                size_name,
                size_ty:
                    ModifiedType {
                        modifiers: size_modifiers,
                        ..
                    },
            } => {
                let out_name = format_ident!("out_{slice_name}");
                let arg_name = format_ident!("arg_{slice_name}");
                let out_size = format_ident!("out_{size_name}");
                let arg_size = format_ident!("arg_{size_name}");
                match slice_ty.modifiers.as_slice() {
                    [TypeModifier::Slice] => Some(quote! {
                        let #arg_size = #arg_name
                            .as_ref()
                            .map(|arg| arg.len())
                            .unwrap_or_default();
                        let #arg_name = #arg_name.and_then(|arg| {
                            if arg.is_empty() {
                                None
                            } else {
                                Some(arg.as_ptr().cast())
                            }
                        })
                        .unwrap_or(std::ptr::null());
                    }),
                    [TypeModifier::MutSlice] => {
                        let arg_size = match size_modifiers.as_slice() {
                            [] => Some(quote! {
                                let #arg_size = #arg_name
                                    .as_ref()
                                    .map(|arg| arg.len())
                                    .unwrap_or_default();
                            }),
                            [TypeModifier::MutPtr] => Some(quote! {
                                let mut #out_size = #arg_name
                                    .as_ref()
                                    .map(|arg| arg.len())
                                    .unwrap_or_default();
                                let #arg_size = &mut #out_size;
                            }),
                            _ => None,
                        };

                        Some(quote! {
                            #arg_size
                            let mut #out_name = #arg_name;
                            let #arg_name = #out_name.as_mut().and_then(|arg| {
                                if arg.is_empty() {
                                    None
                                } else {
                                    Some(arg.as_mut_ptr().cast())
                                }
                            })
                            .unwrap_or(std::ptr::null_mut());
                        })
                    }
                    _ => None,
                }
            }
            _ => None,
        });

        quote! {
            #capture
            #(#args)*
        }
    }

    fn rewrap_rust_args(&self, tree: &ParseTree) -> proc_macro2::TokenStream {
        let args = self.merge_params(tree).filter_map(|arg| match arg {
            MergedParam::Single {
                name,
                ty: Some(arg_ty),
            } => {
                let (modifiers, arg_ty) = (arg_ty.modifiers.as_slice(), &arg_ty.ty);
                let ty_tokens = arg_ty.to_token_stream();
                let ty_string = ty_tokens.to_string();
                match modifiers {
                    [TypeModifier::MutPtr, TypeModifier::MutPtr] if tree.root(&ty_string) == BASE_REF_COUNTED => {
                        let arg_name = format_ident!("arg_{name}");
                        let out_name = format_ident!("out_{name}");
                        Some(quote! {
                            if let (Some(#out_name), Some(#arg_name)) = (#out_name, #arg_name.as_ref()) {
                                *#out_name = #arg_name.as_mut().map(|arg| std::ptr::from_mut(arg).wrap_result());
                            }
                        })
                    }
                    _ => None,
                }
            }
            MergedParam::Bounded {
                count_name,
                count_ty:
                    ModifiedType {
                        modifiers: count_modifiers,
                        ..
                    },
                slice_name,
                ..
            } if matches!(count_modifiers.as_slice(), [TypeModifier::MutPtr]) => {
                let out_count = format_ident!("out_{count_name}");
                let out_name = format_ident!("out_{slice_name}");
                let vec_name = format_ident!("vec_{slice_name}");
                Some(quote! {
                    if let Some(#out_name) = #out_name {
                        *#out_name = #vec_name
                            .into_iter()
                            .take(#out_count)
                            .map(|elem| if elem.is_null() { None } else { Some(elem.wrap_result()) })
                            .collect();
                    }
                })
            }
            MergedParam::Buffer {
                slice_name,
                slice_ty,
                size_name,
                size_ty:
                    ModifiedType {
                        modifiers: size_modifiers,
                        ..
                    },
            } if matches!(
                (slice_ty.modifiers.as_slice(), size_modifiers.as_slice()),
                ([TypeModifier::MutSlice], [TypeModifier::MutPtr])
            ) =>
            {
                let out_name = format_ident!("out_{slice_name}");
                let out_size = format_ident!("out_{size_name}");
                Some(quote! {
                    if let Some(#out_name) = #out_name {
                        #out_name.resize(#out_size, Default::default());
                    }
                })
            }
            _ => None,
        });

        quote! { #(#args)* }
    }

    fn get_rust_output(&self, tree: &ParseTree) -> Option<proc_macro2::TokenStream> {
        self.output.map(|output| {
            let ty = tree
                .resolve_modified_type(output)
                .and_then(|ty| ty.get_output_type(tree))
                .unwrap_or_else(|| {
                    let output = output.to_token_stream();
                    quote! { #output }
                });
            quote! { -> #ty }
        })
    }

    fn wrap_cef_args(&self, tree: &ParseTree) -> proc_macro2::TokenStream {
        let capture = self.capture_params();
        let args = self.merge_params(tree).filter_map(|arg| match arg {
            MergedParam::Receiver => Some(quote! {
                let arg_self_: &RcImpl<_, I> = RcImpl::get(arg_self_);
            }),
            MergedParam::Single {
                name,
                ty: Some(arg_ty),
            } => {
                let arg_name = format_ident!("arg_{name}");
                let out_name = format_ident!("out_{name}");
                let wrap_name = format_ident!("wrap_{name}");
                let (modifiers, arg_ty) = (arg_ty.modifiers.as_slice(), &arg_ty.ty);
                let ty_tokens = arg_ty.to_token_stream();
                let ty_string = ty_tokens.to_string();
                let entry = tree.cef_name_map.get(ty_string.as_str());
                let root = tree.root(&ty_string);

                (root == BASE_REF_COUNTED)
                    .then(|| {
                        match entry? {
                            NameMapEntry {
                                name,
                                ty: NameMapType::StructDeclaration(_),
                            } => {
                                let name = format_ident!("{name}");

                                match modifiers {
                                    [TypeModifier::ConstPtr] => Some(quote! {
                                        let #arg_name = unsafe { #arg_name.as_ref() }.map(|arg| {
                                            #name(unsafe { RefGuard::from_raw(arg) })
                                        });
                                        let #arg_name = #arg_name.as_ref();
                                    }),
                                    [TypeModifier::MutPtr] => Some(quote! {
                                        let mut #arg_name = unsafe { #arg_name.as_mut() }.map(|arg| {
                                            #name(unsafe { RefGuard::from_raw(arg) })
                                        });
                                        let #arg_name = #arg_name.as_mut();
                                    }),
                                    [TypeModifier::MutPtr, TypeModifier::MutPtr] => Some(quote! {
                                        let #out_name = #arg_name;
                                        let mut #wrap_name = unsafe { #arg_name.as_mut() }.and_then(|ptr| {
                                            if ptr.is_null() {
                                                None
                                            } else {
                                                Some(#name(unsafe { RefGuard::from_raw(*ptr) }))
                                            }
                                        });
                                        let #arg_name = Some(&mut #wrap_name);
                                    }),
                                    _ => None,
                                }
                            }
                            _ => None,
                        }
                    })
                    .flatten()
                    .or_else(|| {
                        if root == BASE_SCOPED {
                            let Some(NameMapEntry { name, ty: NameMapType::StructDeclaration(_) }) = entry else {
                                return None;
                            };
                            let name = format_ident!("{name}");
                            match modifiers {
                                [TypeModifier::MutPtr] => Some(quote! {
                                    let mut #arg_name = if #arg_name.is_null() { None } else { Some(#name(#arg_name)) };
                                    let #arg_name = #arg_name.as_mut();
                                }),
                                [TypeModifier::ConstPtr] => Some(quote! {
                                    let #arg_name = if #arg_name.is_null() { None } else { Some(#name(#arg_name)) };
                                    let #arg_name = #arg_name.as_ref();
                                }),
                                _ => None,
                            }
                        } else if ty_string.as_str() == "cef_string_t" ||
                            CUSTOM_STRING_TYPES.contains(&ty_string.as_str())
                        {
                            match modifiers {
                                [TypeModifier::MutPtr] => Some(quote! {
                                    let mut #arg_name = if #arg_name.is_null() { None } else { Some(#arg_name.into()) };
                                    let #arg_name = #arg_name.as_mut();
                                }),
                                [TypeModifier::ConstPtr] => Some(quote! {
                                    let #arg_name = if #arg_name.is_null() { None } else { Some(#arg_name.into()) };
                                    let #arg_name = #arg_name.as_ref();
                                }),
                                _ => None,
                            }
                        } else {
                            let ty = entry.and_then(|entry| syn::parse_str::<syn::Type>(&entry.name).ok());
                            let ty = ty.as_ref().unwrap_or(arg_ty).to_token_stream();

                            if ty.to_string() == quote!{ ::std::os::raw::c_void }.to_string() {
                                match modifiers {
                                    [TypeModifier::MutPtr] => Some(quote! {
                                        let #arg_name = #arg_name.cast();
                                    }),
                                    [TypeModifier::ConstPtr] => Some(quote! {
                                        let #arg_name = #arg_name.cast();
                                    }),
                                    _ => {
                                        Some(quote! {})
                                    }
                                }
                            } else {
                                match modifiers {
                                    [TypeModifier::MutPtr] => Some(quote! {
                                        let mut #arg_name = if #arg_name.is_null() {
                                            None
                                        } else {
                                            Some(WrapParamRef::<#ty, _>::from(#arg_name))
                                        };
                                        let #arg_name = #arg_name.as_mut().map(|arg| arg.as_mut());
                                    }),
                                    [TypeModifier::ConstPtr] => Some(quote! {
                                        let #arg_name = if #arg_name.is_null() {
                                            None
                                        } else {
                                            Some(WrapParamRef::<#ty, _>::from(#arg_name))
                                        };
                                        let #arg_name = #arg_name.as_ref().map(|arg| arg.as_ref());
                                    }),
                                    _ => None,
                                }
                            }
                        }
                    })
                    .or(Some(quote! { let #arg_name = #arg_name.into_raw(); }))
            }
            MergedParam::Bounded {
                count_name,
                slice_name,
                slice_ty,
                ..
            } => {
                let out_count = format_ident!("out_{count_name}");
                let arg_count = format_ident!("arg_{count_name}");
                let out_name = format_ident!("out_{slice_name}");
                let arg_name = format_ident!("arg_{slice_name}");
                let vec_name = format_ident!("vec_{slice_name}");

                let (modifiers, slice_ty) = (slice_ty.modifiers.as_slice(), &slice_ty.ty);
                let ty_tokens = slice_ty.to_token_stream();
                let ty_string = ty_tokens.to_string();
                let entry = tree.cef_name_map.get(ty_string.as_str());

                (tree.root(&ty_string) == BASE_REF_COUNTED)
                    .then(|| {
                        match entry? {
                            NameMapEntry {
                                name,
                                ty: NameMapType::StructDeclaration(_),
                            } => {
                                let name = format_ident!("{name}");

                                match modifiers {
                                    [TypeModifier::Slice] => {
                                        Some(quote! {
                                            let #vec_name = unsafe { #arg_name.as_ref() }.map(|arg| {
                                                let arg = unsafe { std::slice::from_raw_parts(std::ptr::from_ref(arg), #arg_count) };
                                                arg.iter()
                                                    .map(|arg| {
                                                        if arg.is_null() {
                                                            None
                                                        } else {
                                                            Some(#name(unsafe { RefGuard::from_raw(*arg) }))
                                                        }
                                                    })
                                                    .collect::<Vec<_>>()
                                            });
                                            let #arg_name = #vec_name.as_deref();
                                        })
                                    },
                                    [TypeModifier::MutSlice] => {
                                        Some(quote! {
                                            let #out_count = unsafe { #arg_count.as_mut() };
                                            let #out_name = unsafe { #arg_name.as_mut() };
                                            let #arg_count = #out_count
                                                .as_ref()
                                                .map(|count| **count)
                                                .unwrap_or_default();
                                            let mut #vec_name = unsafe { #arg_name.as_mut() }.map(|arg| {
                                                let arg = unsafe { std::slice::from_raw_parts_mut(std::ptr::from_mut(arg), #arg_count) };
                                                arg.iter_mut()
                                                    .map(|arg| {
                                                        if arg.is_null() {
                                                            None
                                                        } else {
                                                            Some(#name(unsafe { RefGuard::from_raw(*arg) }))
                                                        }
                                                    })
                                                    .collect::<Vec<_>>()
                                            });
                                            let #arg_name = #vec_name.as_mut();
                                        })
                                    },
                                    _ => None,
                                }
                            }
                            _ => None,
                        }
                    })
                    .flatten()
                    .or_else(|| {
                        if ty_string.as_str() == "cef_string_t" ||
                            CUSTOM_STRING_TYPES.contains(&ty_string.as_str())
                        {
                            None
                        } else {
                            let ty =
                                entry.and_then(|entry| syn::parse_str::<syn::Type>(&entry.name).ok());
                            let ty = ty.as_ref().unwrap_or(slice_ty).to_token_stream();

                            match modifiers {
                                [TypeModifier::MutPtr, ..] => Some(quote! {
                                    let mut #arg_name = WrapParamRef::<#ty, _>::from(#arg_name);
                                    let #arg_name = #arg_name.as_mut();
                                }),
                                [TypeModifier::ConstPtr, ..] => Some(quote! {
                                    let #arg_name = WrapParamRef::<#ty, _>::from(#arg_name);
                                    let #arg_name = #arg_name.as_ref();
                                }),
                                _ => None,
                            }
                        }
                    })
                    .or(Some(quote! { let #arg_name = #arg_name.into_raw(); }))
            }
            MergedParam::Buffer {
                slice_name,
                slice_ty: ModifiedType { modifiers: slice_modifiers, .. },
                size_name,
                size_ty:
                    ModifiedType {
                        modifiers: size_modifiers,
                        ..
                    },
            } => {
                let out_name = format_ident!("out_{slice_name}");
                let arg_name = format_ident!("arg_{slice_name}");
                let vec_name = format_ident!("vec_{slice_name}");
                let out_size = format_ident!("out_{size_name}");
                let arg_size = format_ident!("arg_{size_name}");
                match slice_modifiers.as_slice() {
                    [TypeModifier::Slice] => Some(quote! {
                        let #arg_name = (!#arg_name.is_null() && #arg_size > 0).then(|| unsafe {
                            std::slice::from_raw_parts(#arg_name.cast(), #arg_size)
                        });
                    }),
                    [TypeModifier::MutSlice] => {
                        let out_size = match size_modifiers.as_slice() {
                            [TypeModifier::MutPtr] => Some(quote! {
                                let #out_size = unsafe { #arg_size.as_mut() };
                                let #arg_size = #out_size.as_ref().map(|size| **size).unwrap_or_default();
                            }),
                            _ => None,
                        };

                        Some(quote! {
                            #out_size
                            let #out_name = (!#arg_name.is_null() && #arg_size > 0).then(|| unsafe {
                                std::slice::from_raw_parts_mut(#arg_name.cast(), #arg_size)
                            });
                            let mut #vec_name = #out_name.as_ref().map(|arg| arg.to_vec());
                            let #arg_name = #vec_name.as_mut();
                        })
                    }
                    _ => None,
                }
                .or(Some(quote! { let #arg_name = #arg_name.into_raw(); }))
            }
            _ => None,
        });

        quote! {
            #capture
            #(#args)*
        }
    }

    fn unwrap_cef_args(&self, tree: &ParseTree) -> proc_macro2::TokenStream {
        let args = self.merge_params(tree).filter_map(|arg| match arg {
            MergedParam::Single {
                name,
                ty: Some(arg_ty),
            } => {
                let (modifiers, arg_ty) = (arg_ty.modifiers.as_slice(), &arg_ty.ty);
                let ty_tokens = arg_ty.to_token_stream();
                let ty_string = ty_tokens.to_string();
                match modifiers {
                    [TypeModifier::MutPtr, TypeModifier::MutPtr] if tree.root(&ty_string) == BASE_REF_COUNTED => {
                        let out_name = format_ident!("out_{name}");
                        let wrap_name = format_ident!("wrap_{name}");
                        Some(quote! {
                            if let (Some(#out_name), Some(#wrap_name)) = (unsafe { #out_name.as_mut() }, #wrap_name) {
                                *#out_name = #wrap_name.wrap_result();
                            }
                        })
                    }
                    _ => None,
                }
            }
            MergedParam::Bounded {
                count_name,
                count_ty:
                    ModifiedType {
                        modifiers: count_modifiers,
                        ..
                    },
                slice_name,
                slice_ty: ModifiedType {
                        ty: slice_ty,
                        modifiers: slice_modifiers,
                    },
            } => {
                let out_count = format_ident!("out_{count_name}");
                let vec_name = format_ident!("vec_{slice_name}");
                let update_count = match count_modifiers.as_slice() {
                    [TypeModifier::MutPtr] => {
                        Some(quote! {
                            *#out_count = size;
                        })
                    }
                    _ => None,
                };
                let add_refs = match (slice_modifiers.as_slice(), tree.root(&slice_ty.to_token_stream().to_string())) {
                    ([TypeModifier::MutSlice], BASE_REF_COUNTED) => {
                        Some(quote! {
                            for elem in &mut #vec_name[..size] {
                                if let Some(elem) = elem.as_ref() {
                                    unsafe { elem.add_ref() };
                                }
                            }
                        })
                    }
                    _ => None,
                };
                match (add_refs, update_count) {
                    (None, None) => None,
                    (add_refs, update_count) => {
                        Some(quote! {
                            if let (Some(#out_count), Some(#vec_name)) = (#out_count, #vec_name.as_mut()) {
                                let size = #vec_name.len().min(*#out_count);
                                #add_refs;
                                #update_count;
                            }
                        })
                    },
                }
            }
            MergedParam::Buffer {
                slice_name,
                slice_ty,
                size_name,
                size_ty:
                    ModifiedType {
                        modifiers: size_modifiers,
                        ..
                    },
            } => {
                let out_name = format_ident!("out_{slice_name}");
                let vec_name = format_ident!("vec_{slice_name}");
                let out_size = format_ident!("out_{size_name}");
                match (slice_ty.modifiers.as_slice(), size_modifiers.as_slice()) {
                    ([TypeModifier::MutSlice], [TypeModifier::MutPtr]) => {
                        Some(quote! {
                            if let (Some(#out_size), Some(#out_name), Some(#vec_name)) = (#out_size, #out_name, #vec_name.as_mut()) {
                                *#out_size = #vec_name.len().min(*#out_size);
                                #out_name[..(*#out_size)].copy_from_slice(&#vec_name[..(*#out_size)]);
                            }
                        })
                    }
                    ([TypeModifier::MutSlice], []) => {
                        Some(quote! {
                            if let (Some(#out_name), Some(#vec_name)) = (#out_name, #vec_name.as_mut()) {
                                let size = #vec_name.len().min(#out_name.len());
                                #out_name[..size].copy_from_slice(&#vec_name[..size]);
                            }
                        })
                    }
                    _ => None,
                }
            }
            _ => None,
        });

        quote! { #(#args)* }
    }

    fn get_signature(&self, tree: &ParseTree) -> proc_macro2::TokenStream {
        let name = make_rust_method_name(&self.name);
        let name = format_ident!("{name}");
        let args = self.get_rust_args(tree);
        let output = self.get_rust_output(tree);
        quote! { fn #name(#args) #output }
    }
}

impl<'a> TryFrom<&'a syn::Field> for SignatureRef<'a> {
    type Error = Unrecognized;

    fn try_from(value: &'a syn::Field) -> Result<Self, Self::Error> {
        let name = value
            .ident
            .as_ref()
            .ok_or(Unrecognized::FieldType)?
            .to_string();

        // Look for a type matching std::option::Option<T>
        let syn::Type::Path(syn::TypePath {
            qself: None,
            path: syn::Path { segments, .. },
        }) = &value.ty
        else {
            return Err(Unrecognized::FieldType);
        };
        let mut segments_iter = segments.iter();
        let (
            Some(syn::PathSegment {
                ident: ident_std,
                arguments: syn::PathArguments::None,
            }),
            Some(syn::PathSegment {
                ident: ident_option,
                arguments: syn::PathArguments::None,
            }),
            Some(syn::PathSegment {
                ident: ident_type,
                arguments:
                    syn::PathArguments::AngleBracketed(syn::AngleBracketedGenericArguments {
                        args, ..
                    }),
            }),
            None,
        ) = (
            segments_iter.next(),
            segments_iter.next(),
            segments_iter.next(),
            segments_iter.next(),
        )
        else {
            return Err(Unrecognized::FieldType);
        };
        if *ident_std != "std"
            || *ident_option != "option"
            || *ident_type != "Option"
            || args.len() != 1
        {
            return Err(Unrecognized::FieldType);
        }

        // See if the Option<T> type is a function pointer
        let mut args = args.iter();
        let (
            Some(syn::GenericArgument::Type(syn::Type::BareFn(syn::TypeBareFn {
                unsafety: Some(_),
                abi: Some(syn::Abi {
                    name: Some(abi), ..
                }),
                inputs,
                variadic: None,
                output,
                ..
            }))),
            None,
        ) = (args.next(), args.next())
        else {
            return Err(Unrecognized::FieldType);
        };
        if abi.value() != "C" {
            return Err(Unrecognized::FieldType);
        }

        let inputs = inputs
            .iter()
            .filter_map(|arg| {
                arg.name.as_ref().map(|(ident, _)| FnArgRef {
                    name: ident.to_string(),
                    ty: &arg.ty,
                })
            })
            .collect();
        let output = match output {
            syn::ReturnType::Default => None,
            syn::ReturnType::Type(_, ty) => Some(ty.as_ref()),
        };

        Ok(Self {
            name,
            inputs,
            output,
            merged_params: Default::default(),
        })
    }
}

const BASE_REF_COUNTED: &str = "_cef_base_ref_counted_t";

const BASE_SCOPED: &str = "_cef_base_scoped_t";

const CUSTOM_STRING_TYPES: &[&str] = &[
    "_cef_string_utf8_t",
    "_cef_string_utf16_t",
    "_cef_string_wide_t",
    "_cef_string_list_t",
    "_cef_string_map_t",
    "_cef_string_multimap_t",
];

const CUSTOM_STRING_USERFREE_ALIASES: &[&str] = &[
    "cef_string_userfree_utf8_t",
    "cef_string_userfree_utf16_t",
    "cef_string_userfree_wide_t",
];

fn is_custom_string_userfree_alias(name: &str) -> bool {
    name == "cef_string_t"
        || name == "cef_string_userfree_t"
        || CUSTOM_STRING_USERFREE_ALIASES.contains(&name)
}

struct StructDeclarationRef<'a> {
    name: String,
    fields: Vec<FieldRef<'a>>,
    methods: Vec<SignatureRef<'a>>,
    is_sealed: bool,
}

#[derive(Clone, Copy, Debug)]
enum NameMapType {
    TypeAlias,
    EnumName,
    StructDeclaration(bool /* is_sealed */),
}

struct NameMapEntry {
    name: String,
    ty: NameMapType,
}

#[derive(Clone, Debug)]
enum TypeModifier {
    MutPtr,
    ConstPtr,
    MutSlice,
    MutRef,
    Slice,
    Ref,
    Array { size: proc_macro2::TokenStream },
}

#[derive(Clone)]
struct ModifiedType {
    modifiers: Vec<TypeModifier>,
    ty: syn::Type,
}

impl ModifiedType {
    fn get_argument_type(&self, tree: &ParseTree) -> Option<proc_macro2::TokenStream> {
        let elem = self.ty.to_token_stream();
        let elem_string = elem.to_string();
        match tree.cef_name_map.get(&elem_string) {
            Some(NameMapEntry {
                name,
                ty: NameMapType::StructDeclaration(is_sealed),
            }) => {
                let is_sealed = *is_sealed;
                let root = tree.root(&elem_string);
                if BASE_REF_COUNTED == root && root != elem_string.as_str() {
                    let impl_trait = format_ident!("Impl{name}");
                    let name = format_ident!("{name}");

                    match self.modifiers.as_slice() {
                        [TypeModifier::ConstPtr] => Some(if is_sealed {
                            quote! { Option<&#name> }
                        } else {
                            quote! { Option<&impl #impl_trait> }
                        }),
                        [TypeModifier::MutPtr] => Some(quote! { Option<&mut #name> }),
                        [TypeModifier::MutPtr, TypeModifier::MutPtr] => {
                            Some(quote! { Option<&mut Option<#name>> })
                        }
                        [TypeModifier::Slice] => Some(if is_sealed {
                            quote! { Option<&[Option<#name>]> }
                        } else {
                            quote! { Option<&[Option<impl #impl_trait>]> }
                        }),
                        [TypeModifier::MutSlice] => {
                            Some(quote! { Option<&mut Vec<Option<#name>>> })
                        }
                        _ => None,
                    }
                } else {
                    let name = format_ident!("{name}");

                    match self.modifiers.as_slice() {
                        [TypeModifier::ConstPtr] => Some(quote! { Option<&#name> }),
                        [TypeModifier::MutPtr] => Some(quote! { Option<&mut #name> }),
                        [TypeModifier::MutPtr, TypeModifier::MutPtr] => {
                            Some(quote! { Option<&mut Option<#name>> })
                        }
                        [TypeModifier::Slice] => Some(quote! { Option<&[Option<#name>]> }),
                        [TypeModifier::MutSlice] => {
                            Some(quote! { Option<&mut Vec<Option<#name>>> })
                        }
                        _ => None,
                    }
                }
            }
            Some(NameMapEntry {
                name,
                ty: NameMapType::TypeAlias,
            }) if is_custom_string_userfree_alias(elem_string.as_str()) => {
                let name = format_ident!("{name}");

                match self.modifiers.as_slice() {
                    [] => Some(quote! { #name }),
                    [TypeModifier::ConstPtr] => Some(quote! { Option<&#name> }),
                    [TypeModifier::MutPtr] => Some(quote! { Option<&mut #name> }),
                    [TypeModifier::MutPtr, TypeModifier::MutPtr] => {
                        Some(quote! { Option<&mut Option<#name>> })
                    }
                    [TypeModifier::Slice] => Some(quote! { Option<&[Option<#name>]> }),
                    [TypeModifier::MutSlice] => Some(quote! { Option<&mut Vec<Option<#name>>> }),
                    _ => None,
                }
            }
            Some(NameMapEntry {
                name,
                ty: NameMapType::EnumName,
            }) => {
                let name = format_ident!("{name}");

                match self.modifiers.as_slice() {
                    [] => Some(quote! { #name }),
                    [TypeModifier::MutPtr] => Some(quote! { Option<&mut #name> }),
                    [TypeModifier::Slice] => Some(quote! { Option<&[#name]> }),
                    [TypeModifier::MutSlice] => Some(quote! { Option<&mut Vec<#name>> }),
                    _ => None,
                }
            }
            None => {
                let is_void = elem_string == quote! { ::std::os::raw::c_void }.to_string();
                let elem = if is_void {
                    quote! { u8 }
                } else {
                    elem
                };
                match self.modifiers.as_slice() {
                    [TypeModifier::ConstPtr] if !is_void => Some(quote! { Option<&#elem> }),
                    [TypeModifier::MutPtr] if !is_void => Some(quote! { Option<&mut #elem> }),
                    [TypeModifier::Slice] => Some(quote! { Option<&[#elem]> }),
                    [TypeModifier::MutSlice] => Some(quote! { Option<&mut Vec<#elem>> }),
                    modifiers => {
                        let modifiers = modifiers
                            .iter()
                            .map(|modifier| match modifier {
                                TypeModifier::MutPtr => Some(quote! { *mut }),
                                TypeModifier::ConstPtr => Some(quote! { *const }),
                                _ => None,
                            })
                            .collect::<Vec<_>>();
                        if modifiers.iter().any(Option::is_none) {
                            None
                        } else {
                            Some(quote! { #(#modifiers)* #elem})
                        }
                    }
                }
            }
            _ => {
                let modifiers = self
                    .modifiers
                    .iter()
                    .map(|modifier| match modifier {
                        TypeModifier::MutPtr => Some(quote! { *mut }),
                        TypeModifier::ConstPtr => Some(quote! { *const }),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                if modifiers.iter().any(Option::is_none) {
                    None
                } else {
                    Some(quote! { #(#modifiers)* #elem})
                }
            }
        }
    }

    fn get_output_type(&self, tree: &ParseTree) -> Option<proc_macro2::TokenStream> {
        let elem = self.ty.to_token_stream();
        let elem_name = elem.to_string();
        tree.cef_name_map
            .get(&elem_name)
            .and_then(|entry| match entry {
                NameMapEntry {
                    name,
                    ty: NameMapType::StructDeclaration(_),
                } => {
                    let name = format_ident!("{name}");

                    match self.modifiers.as_slice() {
                        [] => Some(quote! { #name }),
                        [TypeModifier::MutPtr] => Some(quote! { Option<#name> }),
                        [TypeModifier::ConstPtr] => Some(quote! { Option<&#name> }),
                        [TypeModifier::ConstPtr, TypeModifier::MutPtr] => {
                            Some(quote! { Option<&mut [#name>]> })
                        }
                        [TypeModifier::ConstPtr, TypeModifier::ConstPtr] => {
                            Some(quote! { Option<&[#name]> })
                        }
                        _ => None,
                    }
                }
                NameMapEntry {
                    name,
                    ty: NameMapType::TypeAlias,
                } if is_custom_string_userfree_alias(elem_name.as_str()) => {
                    let name = format_ident!("{name}");

                    match self.modifiers.as_slice() {
                        [] => Some(quote! { #name }),
                        [TypeModifier::MutPtr] => Some(quote! { Option<#name> }),
                        [TypeModifier::ConstPtr] => Some(quote! { Option<&#name> }),
                        [TypeModifier::ConstPtr, TypeModifier::MutPtr] => {
                            Some(quote! { Option<&mut [#name>]> })
                        }
                        [TypeModifier::ConstPtr, TypeModifier::ConstPtr] => {
                            Some(quote! { Option<&[#name]> })
                        }
                        _ => None,
                    }
                }
                NameMapEntry {
                    name,
                    ty: NameMapType::EnumName,
                } => {
                    let name = format_ident!("{name}");

                    match self.modifiers.as_slice() {
                        [] => Some(quote! { #name }),
                        [TypeModifier::ConstPtr] => Some(quote! { Option<&[#name]> }),
                        [TypeModifier::MutPtr] => Some(quote! { Option<&mut [#name]> }),
                        _ => None,
                    }
                }
                _ => match self.modifiers.as_slice() {
                    [TypeModifier::MutPtr] => Some(quote! { Option<&mut #elem> }),
                    [TypeModifier::ConstPtr] => Some(quote! { Option<&[#elem]> }),
                    [TypeModifier::MutPtr, TypeModifier::MutPtr] => {
                        Some(quote! { Option<&mut Vec<#elem>> })
                    }
                    _ => None,
                },
            })
    }
}

impl syn::parse::Parse for ModifiedType {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut modifiers = vec![];
        loop {
            let lookahead = input.lookahead1();
            if lookahead.peek(syn::Token![*]) {
                let _ = input.parse::<syn::Token![*]>()?;
                let lookahead = input.lookahead1();
                if lookahead.peek(syn::Token![const]) {
                    let _ = input.parse::<syn::Token![const]>()?;
                    modifiers.push(TypeModifier::ConstPtr)
                } else {
                    let _ = input.parse::<syn::Token![mut]>()?;
                    modifiers.push(TypeModifier::MutPtr)
                }
            } else if lookahead.peek(syn::Token![&]) {
                let _ = input.parse::<syn::Token![&]>()?;
                let lookahead = input.lookahead1();
                if lookahead.peek(syn::Token![mut]) {
                    let _ = input.parse::<syn::Token![mut]>()?;
                    let lookahead = input.lookahead1();
                    if lookahead.peek(syn::token::Bracket) {
                        let ty;
                        let _ = syn::bracketed!(ty in input);
                        let ty = ty.parse()?;
                        modifiers.push(TypeModifier::MutSlice);
                        return Ok(Self { modifiers, ty });
                    } else {
                        modifiers.push(TypeModifier::MutRef)
                    }
                } else if lookahead.peek(syn::token::Bracket) {
                    let ty;
                    let _ = syn::bracketed!(ty in input);
                    let ty = ty.parse()?;
                    modifiers.push(TypeModifier::Slice);
                    return Ok(Self { modifiers, ty });
                } else {
                    modifiers.push(TypeModifier::Ref)
                }
            } else if lookahead.peek(syn::token::Bracket) {
                let content;
                let _ = syn::bracketed!(content in input);
                let ty = content.parse()?;
                let _ = content.parse::<syn::Token![;]>()?;
                modifiers.push(TypeModifier::Array {
                    size: content.parse()?,
                });
                return Ok(Self { modifiers, ty });
            } else {
                break;
            }
        }

        let ty = input.parse()?;
        Ok(Self { modifiers, ty })
    }
}

#[derive(Default)]
struct ParseTree<'a> {
    type_aliases: Vec<TypeAliasRef<'a>>,
    enum_names: Vec<EnumRef<'a>>,
    struct_declarations: Vec<StructDeclarationRef<'a>>,
    global_function_declarations: Vec<SignatureRef<'a>>,

    cef_name_map: BTreeMap<String, NameMapEntry>,
    rust_name_map: BTreeMap<String, NameMapEntry>,

    lookup_type_alias: BTreeMap<String, usize>,
    lookup_enum_name: BTreeMap<String, usize>,
    lookup_struct_declaration: BTreeMap<String, usize>,
    lookup_global_function_declaration: BTreeMap<String, usize>,

    base_types: BTreeMap<String, String>,
}

impl ParseTree<'_> {
    pub fn write_prelude(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let header = quote! {
            #![allow(
                dead_code,
                improper_ctypes_definitions,
                non_camel_case_types,
                unused_variables,
                clippy::not_unsafe_ptr_arg_deref,
                clippy::too_many_arguments
            )]
            use crate::rc::{
                ConvertParam, ConvertReturnValue, Rc, RcImpl, RefGuard, WrapParamRef,
            };
            use cef_dll_sys::*;
        }
        .to_string();
        writeln!(f, "{header}")?;

        writeln!(
            f,
            "\n/// Perform the conversion between CEF and Rust types in field initializers."
        )?;
        let init_array_field = quote! {
            fn init_array_field<T, U, const N: usize>(mut value: [U; N]) -> [T; N]
            where
                T: Sized,
                U: Sized + Into<T>,
            {
                std::array::from_fn(move |i| {
                    let mut elem = unsafe { std::mem::zeroed() };
                    std::mem::swap(&mut value[i], &mut elem);
                    elem.into()
                })
            }
        }
        .to_string();
        writeln!(f, "{init_array_field}")
    }

    fn resolve_type_aliases(&self, ty: &syn::Type) -> proc_macro2::TokenStream {
        match ty {
            syn::Type::Path(syn::TypePath { qself: None, path }) => {
                let ty = path.to_token_stream().to_string();
                if is_custom_string_userfree_alias(ty.as_str()) {
                    path.to_token_stream()
                } else {
                    match self.cef_name_map.get(&ty) {
                        Some(NameMapEntry {
                            ty: NameMapType::TypeAlias,
                            ..
                        }) => self
                            .lookup_type_alias
                            .get(&ty)
                            .and_then(|&i| self.type_aliases.get(i))
                            .map(|alias| self.resolve_type_aliases(alias.ty))
                            .unwrap_or_else(|| path.to_token_stream()),
                        _ => path.to_token_stream(),
                    }
                }
            }
            syn::Type::Tuple(syn::TypeTuple { elems, .. }) => {
                let elems = elems.iter().map(|elem| self.resolve_type_aliases(elem));
                quote! { #(#elems),* }
            }
            syn::Type::Array(syn::TypeArray { elem, len, .. }) => {
                let elem = self.resolve_type_aliases(elem);
                let len = len.to_token_stream();
                quote! { [#elem; #len] }
            }
            syn::Type::Slice(syn::TypeSlice { elem, .. }) => {
                let elem = self.resolve_type_aliases(elem);
                quote! { [#elem] }
            }
            syn::Type::Ptr(syn::TypePtr {
                const_token, elem, ..
            }) => {
                let elem = self.resolve_type_aliases(elem.as_ref());
                if const_token.is_some() {
                    quote! { *const #elem }
                } else {
                    quote! { *mut #elem }
                }
            }
            _ => ty.to_token_stream(),
        }
    }

    fn resolve_modified_type(&self, ty: &syn::Type) -> Option<ModifiedType> {
        let ty = self.resolve_type_aliases(ty);
        syn::parse2::<ModifiedType>(ty.clone()).ok()
    }

    pub fn write_aliases(&self, f: &mut Formatter<'_>) -> fmt::Result {
        for TypeAliasRef { name, ty } in self.type_aliases.iter() {
            let comment_ty: String = syn::parse2::<ModifiedType>(ty.to_token_stream())
                .map(|ty| ty.ty.to_token_stream())
                .unwrap_or_else(|_| ty.to_token_stream())
                .to_string()
                .split_whitespace()
                .flat_map(|word| word.chars())
                .collect();
            let comment = format!("See [`{comment_ty}`] for more documentation.");
            let (Some(rust_name), Some(arg_ty)) = (
                make_rust_type_name(name.as_str()),
                self.resolve_modified_type(ty),
            ) else {
                continue;
            };
            let ty = arg_ty.ty.to_token_stream().to_string();
            let rust_name_ident = format_ident!("{rust_name}");

            if CUSTOM_STRING_USERFREE_ALIASES.contains(&name.as_str()) {
                writeln!(f, "\n/// {comment}")?;
                self.write_custom_string_type(f, &rust_name_ident)?;
                continue;
            }

            if ty == quote! { ::std::os::raw::c_void }.to_string() {
                continue;
            }
            let ty = make_rust_type_name(&ty).unwrap_or(ty);
            if rust_name == ty.as_str() {
                continue;
            }
            let ty = syn::parse_str::<syn::Type>(&ty).unwrap_or(arg_ty.ty);
            let modifiers = arg_ty
                .modifiers
                .iter()
                .filter_map(|modifier| match modifier {
                    TypeModifier::MutPtr => Some(quote! { *mut }),
                    TypeModifier::ConstPtr => Some(quote! { *const }),
                    TypeModifier::MutRef => Some(quote! { &mut }),
                    TypeModifier::Ref => Some(quote! { & }),
                    _ => None,
                });
            let ty = match arg_ty.modifiers.last() {
                Some(TypeModifier::MutSlice) => quote! { &mut [#ty] },
                Some(TypeModifier::Slice) => quote! { &[#ty] },
                Some(TypeModifier::Array { size }) => quote! { [#ty; #size] },
                _ => ty.to_token_stream(),
            };

            let alias = quote! {
                pub type #rust_name_ident = #(#modifiers)* #ty;
            }
            .to_string();

            writeln!(f, "\n/// {comment}")?;
            writeln!(f, "{alias}")?;
        }
        Ok(())
    }

    fn base(&self, name: &str) -> Option<&str> {
        self.base_types.get(name).map(String::as_str)
    }

    fn root<'b: 'c, 'c>(&'b self, name: &'c str) -> &'c str {
        self.base(name).map(|base| self.root(base)).unwrap_or(name)
    }

    fn write_custom_string_type(
        &self,
        f: &mut Formatter<'_>,
        rust_name: &syn::Ident,
    ) -> fmt::Result {
        let wrapper = quote! {
            pub use crate::string::#rust_name;
        }
        .to_string();
        writeln!(f, "{wrapper}")
    }

    fn write_sealed_struct(
        &self,
        f: &mut Formatter<'_>,
        s: &StructDeclarationRef<'_>,
        root: &str,
        name_ident: &syn::Ident,
        rust_name: &syn::Ident,
    ) -> fmt::Result {
        let name = s.name.as_str();
        let methods = s.methods.iter().map(|m| {
            let sig = m.get_signature(self);
            let name = &m.name;
            let name = format_ident!("{name}");
            let pre_forward_args = m.unwrap_rust_args(self);
            let args: Vec<_> = m
                .inputs
                .iter()
                .map(|arg| {
                    let name = make_snake_case_value_name(&arg.name);
                    let name = format_ident!("arg_{name}");
                    quote! { #name }
                })
                .collect();
            let post_forward_args = m.rewrap_rust_args(self);
            let output_type = m.output.and_then(|ty| {
                let ty = self.resolve_type_aliases(ty);
                syn::parse2::<ModifiedType>(ty.to_token_stream()).ok()
            });

            output_type
                .as_ref()
                .map(|ModifiedType { modifiers, ty, .. }| {
                    let ty = ty.to_token_stream().to_string();
                    let wrap_result = match modifiers.as_slice() {
                        [TypeModifier::ConstPtr | TypeModifier::MutPtr]
                            if ty != quote! { ::std::os::raw::c_void }.to_string() =>
                        {
                            match self.cef_name_map.get(&ty) {
                                Some(NameMapEntry {
                                    ty: NameMapType::StructDeclaration(_),
                                    ..
                                }) => Some(quote! {
                                    if result.is_null() {
                                        None
                                    } else {
                                        Some(result.wrap_result())
                                    }
                                }),
                                _ => None,
                            }
                        }
                        _ => None,
                    }
                    .unwrap_or(quote! { result.wrap_result() });

                    let impl_default = output_type
                        .as_ref()
                        .and_then(|ty| {
                            (ty.ty.to_token_stream().to_string()
                                != quote! { ::std::os::raw::c_void }.to_string())
                            .then(|| quote! { .unwrap_or_default() })
                        })
                        .unwrap_or(quote! { .unwrap_or_else(|| std::mem::zeroed()) });

                    quote! {
                        #sig {
                            unsafe {
                                self.0.#name.map(|f| {
                                    #pre_forward_args
                                    let result = f(#(#args),*);
                                    #post_forward_args
                                    #wrap_result
                                })
                                #impl_default
                            }
                        }
                    }
                })
                .unwrap_or(quote! {
                    #sig {
                        unsafe {
                            if let Some(f) = self.0.#name {
                                #pre_forward_args
                                f(#(#args),*);
                                #post_forward_args
                            }
                        }
                    }
                })
        });

        let base_name = self.base(name);
        let impl_trait = format_ident!("Impl{rust_name}");
        let impl_base_name = base_name
            .filter(|base| *base != BASE_REF_COUNTED)
            .and_then(|base| self.cef_name_map.get(base))
            .map(|entry| {
                let base = &entry.name;
                let base = format_ident!("Impl{base}");
                quote! { #base }
            });
        let impl_get_raw = impl_base_name
            .as_ref()
            .map(|impl_base_name| {
                quote! {
                    fn get_raw(&self) -> *mut #name_ident {
                        <Self as #impl_base_name>::get_raw(self).cast()
                    }
                }
            })
            .unwrap_or(quote! { fn get_raw(&self) -> *mut #name_ident; });
        let impl_base_name = impl_base_name.unwrap_or(quote! { Clone + Sized + Rc });
        let impl_methods = s.methods.iter().map(|m| {
            let sig = m.get_signature(self);
            let method_name = &m.name;
            let comment = format!("See [`{name}::{method_name}`] for more documentation.");
            quote! {
                #[doc = #comment]
                #sig;
            }
        });

        let mut base_name = base_name;
        let mut base_structs = vec![];
        while let Some(next_base) = base_name
            .filter(|base| *base != root)
            .and_then(|base| self.lookup_struct_declaration.get(base))
            .and_then(|&i| self.struct_declarations.get(i))
        {
            base_name = self.base(&next_base.name);
            base_structs.push(next_base);
        }

        let impl_bases = base_structs
            .into_iter()
            .filter_map(|base_struct| {
                self.cef_name_map.get(&base_struct.name).map(|entry| {
                    let base = &base_struct.name;
                    let base_ident = format_ident!("{base}");
                    let base = &entry.name;
                    let base_trait = format_ident!("Impl{base}");
                    let base = format_ident!("{base}");
                    let base_methods = base_struct.methods.iter().map(|m| {
                        let sig = m.get_signature(self);
                        let name = make_rust_method_name(&m.name);
                        let name = format_ident!("{name}");
                        let args = m.merge_params(self).filter_map(|arg| match arg {
                            MergedParam::Single { name, .. } => {
                                let name = format_ident!("{name}");
                                Some(quote! { #name })
                            }
                            MergedParam::Bounded { slice_name, .. }
                            | MergedParam::Buffer { slice_name, .. } => {
                                let name = format_ident!("{slice_name}");
                                Some(quote! { #name })
                            }
                            _ => None,
                        });
                        quote! {
                            #sig {
                                #base::from(self).#name(#(#args),*)
                            }
                        }
                    });

                    quote! {
                        impl #base_trait for #rust_name {
                            #(#base_methods)*

                            fn get_raw(&self) -> *mut #base_ident {
                                unsafe { RefGuard::into_raw(&self.0).cast() }
                            }
                        }

                        impl std::convert::From<&#rust_name> for #base {
                            fn from(from: &#rust_name) -> Self {
                                #base(unsafe {
                                    RefGuard::from_raw_add_ref(RefGuard::into_raw(&from.0).cast())
                                })
                            }
                        }
                    }
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .rev();

        let base_ident = format_ident!("{BASE_REF_COUNTED}");

        let wrapper = quote! {
            #[derive(Clone)]
            pub struct #rust_name(RefGuard<#name_ident>);

            pub trait #impl_trait : #impl_base_name {
                #(#impl_methods)*

                #impl_get_raw
            }

            #(#impl_bases)*

            impl #impl_trait for #rust_name {
                #(#methods)*

                fn get_raw(&self) -> *mut #name_ident {
                    unsafe { RefGuard::into_raw(&self.0) }
                }
            }

            impl Rc for #name_ident {
                fn as_base(&self) -> &#base_ident {
                    self.base.as_base()
                }
            }

            impl Rc for #rust_name {
                fn as_base(&self) -> &#base_ident {
                    self.0.as_base()
                }
            }

            impl ConvertParam<*mut #name_ident> for &#rust_name {
                fn into_raw(self) -> *mut #name_ident {
                    #impl_trait::get_raw(self)
                }
            }

            impl ConvertParam<*mut #name_ident> for &mut #rust_name {
                fn into_raw(self) -> *mut #name_ident {
                    #impl_trait::get_raw(self)
                }
            }

            impl ConvertReturnValue<#rust_name> for *mut #name_ident {
                fn wrap_result(self) -> #rust_name {
                    #rust_name(unsafe { RefGuard::from_raw(self) })
                }
            }

            impl From<#rust_name> for *mut #name_ident {
                fn from(value: #rust_name) -> Self {
                    let object = #impl_trait::get_raw(&value);
                    std::mem::forget(value);
                    object
                }
            }

            impl Default for #rust_name {
                fn default() -> Self {
                    unsafe { std::mem::zeroed() }
                }
            }
        }
        .to_string();

        writeln!(f, "{wrapper}")
    }

    fn write_ref_counted_struct(
        &self,
        f: &mut Formatter<'_>,
        s: &StructDeclarationRef<'_>,
        root: &str,
        name_ident: &syn::Ident,
        rust_name: &syn::Ident,
    ) -> fmt::Result {
        if BASE_REF_COUNTED == s.name {
            let wrapper = quote! {
                #[derive(Clone)]
                pub struct #rust_name(RefGuard<#name_ident>);

                impl #rust_name {
                    fn get_raw(&self) -> *mut #name_ident {
                        unsafe { RefGuard::into_raw(&self.0) }
                    }
                }

                impl Rc for #rust_name {
                    fn as_base(&self) -> &#name_ident {
                        self.0.as_base()
                    }
                }

                impl ConvertParam<*mut #name_ident> for &#rust_name {
                    fn into_raw(self) -> *mut #name_ident {
                        self.get_raw()
                    }
                }

                impl ConvertParam<*mut #name_ident> for &mut #rust_name {
                    fn into_raw(self) -> *mut #name_ident {
                        self.get_raw()
                    }
                }

                impl ConvertReturnValue<#rust_name> for *mut #name_ident {
                    fn wrap_result(self) -> #rust_name {
                        #rust_name(unsafe { RefGuard::from_raw(self) })
                    }
                }

                impl From<#rust_name> for *mut #name_ident {
                    fn from(value: #rust_name) -> Self {
                        let object = value.get_raw();
                        std::mem::forget(value);
                        object
                    }
                }

                impl Default for #rust_name {
                    fn default() -> Self {
                        Self(unsafe { RefGuard::from_raw(std::ptr::null_mut()) })
                    }
                }
            }
            .to_string();

            return writeln!(f, "{wrapper}");
        }

        let name = s.name.as_str();
        let methods = s.methods.iter().map(|m| {
            let sig = m.get_signature(self);
            let name = &m.name;
            let name = format_ident!("{name}");
            let pre_forward_args = m.unwrap_rust_args(self);
            let args: Vec<_> = m
                .inputs
                .iter()
                .map(|arg| {
                    let name = make_snake_case_value_name(&arg.name);
                    let name = format_ident!("arg_{name}");
                    quote! { #name }
                })
                .collect();
            let post_forward_args = m.rewrap_rust_args(self);
            let output_type = m.output.and_then(|ty| {
                let ty = self.resolve_type_aliases(ty);
                syn::parse2::<ModifiedType>(ty.to_token_stream()).ok()
            });

            output_type
                .as_ref()
                .map(|ModifiedType { modifiers, ty, .. }| {
                    let ty = ty.to_token_stream().to_string();
                    let wrap_result = match modifiers.as_slice() {
                        [TypeModifier::ConstPtr | TypeModifier::MutPtr]
                            if ty != quote! { ::std::os::raw::c_void }.to_string() =>
                        {
                            match self.cef_name_map.get(&ty) {
                                Some(NameMapEntry {
                                    ty: NameMapType::StructDeclaration(_),
                                    ..
                                }) => Some(quote! {
                                    if result.is_null() {
                                        None
                                    } else {
                                        Some(result.wrap_result())
                                    }
                                }),
                                _ => None,
                            }
                        }
                        _ => None,
                    }
                    .unwrap_or(quote! { result.wrap_result() });

                    let impl_default = output_type
                        .as_ref()
                        .and_then(|ty| {
                            (ty.ty.to_token_stream().to_string()
                                != quote! { ::std::os::raw::c_void }.to_string())
                            .then(|| quote! { .unwrap_or_default() })
                        })
                        .unwrap_or(quote! { .unwrap_or_else(|| std::mem::zeroed()) });

                    quote! {
                        #sig {
                            unsafe {
                                self.0.#name.map(|f| {
                                    #pre_forward_args
                                    let result = f(#(#args),*);
                                    #post_forward_args
                                    #wrap_result
                                })
                                #impl_default
                            }
                        }
                    }
                })
                .unwrap_or(quote! {
                    #sig {
                        unsafe {
                            if let Some(f) = self.0.#name {
                                #pre_forward_args
                                f(#(#args),*);
                                #post_forward_args
                            }
                        }
                    }
                })
        });

        let base_name = self.base(name);
        let impl_trait = format_ident!("Impl{rust_name}");
        let wrap_trait = format_ident!("Wrap{rust_name}");
        let impl_base_name = base_name
            .filter(|base| *base != BASE_REF_COUNTED)
            .and_then(|base| self.cef_name_map.get(base))
            .map(|entry| {
                let base = &entry.name;
                let base = format_ident!("Impl{base}");
                quote! { #base }
            });
        let impl_get_raw = impl_base_name
            .as_ref()
            .map(|impl_base_name| {
                quote! {
                    fn get_raw(&self) -> *mut #name_ident {
                        <Self as #impl_base_name>::get_raw(self).cast()
                    }
                }
            })
            .unwrap_or(quote! { fn get_raw(&self) -> *mut #name_ident; });
        let impl_base_name = impl_base_name.unwrap_or(quote! { Clone + Sized + Rc });
        let impl_methods = s.methods.iter().map(|m| {
            let sig = m.get_signature(self);
            let method_name = &m.name;
            let comment = format!("See [`{name}::{method_name}`] for more documentation.");
            let impl_default =
                m.output.map(
                    |ty| match syn::parse2::<ModifiedType>(ty.to_token_stream()) {
                        Ok(ty)
                            if ty.ty.to_token_stream().to_string()
                                != quote! { ::std::os::raw::c_void }.to_string() =>
                        {
                            quote! {  Default::default() }
                        }
                        _ => quote! { unsafe { std::mem::zeroed() } },
                    },
                );
            quote! {
                #[doc = #comment]
                #sig {
                    #impl_default
                }
            }
        });

        let mut base_name = base_name;
        let mut base_structs = vec![];
        while let Some(next_base) = base_name
            .filter(|base| *base != root)
            .and_then(|base| self.lookup_struct_declaration.get(base))
            .and_then(|&i| self.struct_declarations.get(i))
        {
            base_name = self.base(&next_base.name);
            base_structs.push(next_base);
        }

        let init_bases = base_structs
            .iter()
            .enumerate()
            .map(|(i, base_struct)| {
                let name = &base_struct.name;
                let name = format_ident!("{name}");
                let impl_mod = format_ident!("impl{name}");
                let bases = iter::repeat_n(format_ident!("base"), i + 1);
                quote! {
                    #impl_mod::init_methods::<Self>(&mut object.#(#bases).*);
                }
            })
            .collect::<Vec<_>>()
            .into_iter()
            .rev();

        let impl_bases = base_structs
            .into_iter()
            .filter_map(|base_struct| {
                self.cef_name_map.get(&base_struct.name).map(|entry| {
                    let base = &base_struct.name;
                    let base_ident = format_ident!("{base}");
                    let base = &entry.name;
                    let base_trait = format_ident!("Impl{base}");
                    let base = format_ident!("{base}");
                    let base_methods = base_struct.methods.iter().map(|m| {
                        let sig = m.get_signature(self);
                        let name = make_rust_method_name(&m.name);
                        let name = format_ident!("{name}");
                        let args = m.merge_params(self).filter_map(|arg| match arg {
                            MergedParam::Single { name, .. } => {
                                let name = format_ident!("{name}");
                                Some(quote! { #name })
                            }
                            MergedParam::Bounded { slice_name, .. }
                            | MergedParam::Buffer { slice_name, .. } => {
                                let name = format_ident!("{slice_name}");
                                Some(quote! { #name })
                            }
                            _ => None,
                        });
                        quote! {
                            #sig {
                                #base(unsafe {
                                    RefGuard::from_raw_add_ref(RefGuard::into_raw(&self.0).cast())
                                })
                                .#name(#(#args),*)
                            }
                        }
                    });

                    quote! {
                        impl #base_trait for #rust_name {
                            #(#base_methods)*

                            fn get_raw(&self) -> *mut #base_ident {
                                unsafe { RefGuard::into_raw(&self.0).cast() }
                            }
                        }
                    }
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .rev();

        let name = &s.name;
        let impl_mod = format_ident!("impl{name}");
        let init_methods = s.methods.iter().map(|m| {
            let name = &m.name;
            let name = format_ident!("{name}");
            quote! {
                object.#name = Some(#name::<I>);
            }
        });

        let wrapped_methods = s.methods.iter().map(|m| {
            let name = &m.name;
            let rust_method_name = make_rust_method_name(name);
            let name = format_ident!("{name}");
            let rust_method_name = format_ident!("{rust_method_name}");
            let args = m.inputs.iter().map(|arg| {
                let name = make_snake_case_value_name(&arg.name);
                let name = format_ident!("{name}");
                let ty = self.resolve_type_aliases(arg.ty);
                quote! { #name: #ty }
            });
            let wrapped_args = m.wrap_cef_args(self);
            let unwrapped_args = m.unwrap_cef_args(self);
            let forward_args = m.merge_params(self).filter_map(|arg| match arg {
                MergedParam::Single { name, .. } => {
                    let name = format_ident!("arg_{name}");
                    Some(quote! { #name })
                }
                MergedParam::Bounded { slice_name, .. }
                | MergedParam::Buffer { slice_name, .. } => {
                    let name = format_ident!("arg_{slice_name}");
                    Some(quote! { #name })
                }
                _ => None,
            });
            let original_output = m.output.map(|ty| self.resolve_type_aliases(ty));
            let output = original_output.as_ref().map(|output| {
                quote! { -> #output }
            });
            let forward_output = original_output.and_then(|output| {
                match syn::parse2::<ModifiedType>(output) {
                    Ok(ModifiedType { ty, modifiers }) => {
                        self.cef_name_map
                            .get(&ty.to_token_stream().to_string())
                            .map(|entry| match entry {
                                NameMapEntry {
                                    ty: NameMapType::StructDeclaration(_),
                                    ..
                                } => match modifiers.as_slice() {
                                    [TypeModifier::ConstPtr] => {
                                        quote! { result.map(|result| result.into()).unwrap_or(std::ptr::null()) }
                                    }
                                    [TypeModifier::MutPtr] => {
                                        quote! { result.map(|result| result.into()).unwrap_or(std::ptr::null_mut()) }
                                    }
                                    _ => quote! { result.into() },
                                }
                                _ => quote! { result.into() },
                            })
                            .or_else(|| {
                                if unwrapped_args.is_empty() {
                                    None
                                } else {
                                    Some(quote! { result })
                                }
                            })
                    }
                    _ => None,
                }
            });
            let mut call_impl =
                quote! { #impl_trait::#rust_method_name(&arg_self_.interface, #(#forward_args),*) };
            if forward_output.is_some() || !unwrapped_args.is_empty() {
                call_impl = quote! { let result = #call_impl; };
            }

            quote! {
                extern "C" fn #name<I: #impl_trait>(#(#args),*) #output {
                    #wrapped_args
                    #call_impl
                    #unwrapped_args
                    #forward_output
                }
            }
        });

        let base_ident = format_ident!("{BASE_REF_COUNTED}");

        let wrapper = quote! {
            #[derive(Clone)]
            pub struct #rust_name(RefGuard<#name_ident>);

            impl #rust_name {
                pub fn new<T>(interface: T) -> Self
                where
                    T: #wrap_trait
                {
                    unsafe {
                        let mut cef_object = std::mem::zeroed();
                        <T as #impl_trait>::init_methods(&mut cef_object);
                        let object = RcImpl::new(cef_object, interface);
                        <T as #wrap_trait>::wrap_rc(&mut (*object).interface, object);
                        let object: *mut #name_ident = object.cast();
                        object.wrap_result()
                    }
                }
            }

            pub trait #wrap_trait : #impl_trait {
                fn wrap_rc(&mut self, object: *mut RcImpl<#name_ident, Self>);
            }

            pub trait #impl_trait : #impl_base_name {
                #(#impl_methods)*

                fn init_methods(object: &mut #name_ident) {
                    #(#init_bases)*
                    #impl_mod::init_methods::<Self>(object);
                }

                #impl_get_raw
            }

            mod #impl_mod {
                use super::*;

                pub fn init_methods<I: #impl_trait>(object: &mut #name_ident) {
                    #(#init_methods)*
                }

                #(#wrapped_methods)*
            }

            #(#impl_bases)*

            impl #impl_trait for #rust_name {
                #(#methods)*

                fn get_raw(&self) -> *mut #name_ident {
                    unsafe { RefGuard::into_raw(&self.0) }
                }
            }

            impl Rc for #name_ident {
                fn as_base(&self) -> &#base_ident {
                    self.base.as_base()
                }
            }

            impl Rc for #rust_name {
                fn as_base(&self) -> &#base_ident {
                    self.0.as_base()
                }
            }

            impl ConvertParam<*mut #name_ident> for &#rust_name {
                fn into_raw(self) -> *mut #name_ident {
                    #impl_trait::get_raw(self)
                }
            }

            impl ConvertParam<*mut #name_ident> for &mut #rust_name {
                fn into_raw(self) -> *mut #name_ident {
                    #impl_trait::get_raw(self)
                }
            }

            impl ConvertReturnValue<#rust_name> for *mut #name_ident {
                fn wrap_result(self) -> #rust_name {
                    #rust_name(unsafe { RefGuard::from_raw(self) })
                }
            }

            impl From <#rust_name> for *mut #name_ident {
                fn from(value: #rust_name) -> Self {
                    let object = #impl_trait::get_raw(&value);
                    std::mem::forget(value);
                    object
                }
            }

            impl Default for #rust_name {
                fn default() -> Self {
                    unsafe { std::mem::zeroed() }
                }
            }
        }
        .to_string();

        writeln!(f, "{wrapper}")
    }

    fn write_scoped_struct(
        &self,
        f: &mut Formatter<'_>,
        s: &StructDeclarationRef<'_>,
        root: &str,
        name_ident: &syn::Ident,
        rust_name: &syn::Ident,
    ) -> fmt::Result {
        if BASE_SCOPED == s.name {
            let wrapper = quote! {
                #[derive(Clone, Copy)]
                pub struct #rust_name(*mut #name_ident);

                impl #rust_name {
                    fn get_raw(&self) -> *mut #name_ident {
                        self.0
                    }
                }

                impl ConvertParam<*mut #name_ident> for &#rust_name {
                    fn into_raw(self) -> *mut #name_ident {
                        self.get_raw()
                    }
                }

                impl ConvertParam<*mut #name_ident> for &mut #rust_name {
                    fn into_raw(self) -> *mut #name_ident {
                        self.get_raw()
                    }
                }

                impl ConvertReturnValue<#rust_name> for *mut #name_ident {
                    fn wrap_result(self) -> #rust_name {
                        #rust_name(self)
                    }
                }

                impl From<#rust_name> for *mut #name_ident {
                    fn from(value: #rust_name) -> Self {
                        value.get_raw()
                    }
                }

                impl Default for #rust_name {
                    fn default() -> Self {
                        Self(std::ptr::null_mut())
                    }
                }
            }
            .to_string();

            return writeln!(f, "{wrapper}");
        }

        let name = s.name.as_str();
        let methods = s.methods.iter().map(|m| {
            let sig = m.get_signature(self);
            let name = &m.name;
            let name = format_ident!("{name}");
            let pre_forward_args = m.unwrap_rust_args(self);
            let args: Vec<_> = m
                .inputs
                .iter()
                .map(|arg| {
                    let name = make_snake_case_value_name(&arg.name);
                    let name = format_ident!("arg_{name}");
                    quote! { #name }
                })
                .collect();
            let post_forward_args = m.rewrap_rust_args(self);
            let output_type = m.output.and_then(|ty| {
                let ty = self.resolve_type_aliases(ty);
                syn::parse2::<ModifiedType>(ty.to_token_stream()).ok()
            });

            output_type
                .as_ref()
                .map(|ModifiedType { modifiers, ty, .. }| {
                    let ty = ty.to_token_stream().to_string();
                    let wrap_result = match modifiers.as_slice() {
                        [TypeModifier::ConstPtr | TypeModifier::MutPtr]
                            if ty != quote! { ::std::os::raw::c_void }.to_string() =>
                        {
                            match self.cef_name_map.get(&ty) {
                                Some(NameMapEntry {
                                    ty: NameMapType::StructDeclaration(_),
                                    ..
                                }) => Some(quote! {
                                    if result.is_null() {
                                        None
                                    } else {
                                        Some(result.wrap_result())
                                    }
                                }),
                                _ => None,
                            }
                        }
                        _ => None,
                    }
                    .unwrap_or(quote! { result.wrap_result() });

                    let impl_default = output_type
                        .as_ref()
                        .and_then(|ty| {
                            (ty.ty.to_token_stream().to_string()
                                != quote! { ::std::os::raw::c_void }.to_string())
                            .then(|| quote! { .unwrap_or_default() })
                        })
                        .unwrap_or(quote! { .unwrap_or_else(|| std::mem::zeroed()) });

                    quote! {
                        #sig {
                            unsafe {
                                self.0.as_ref().and_then(|this| this.#name).map(|f| {
                                    #pre_forward_args
                                    let result = f(#(#args),*);
                                    #post_forward_args
                                    #wrap_result
                                })
                                #impl_default
                            }
                        }
                    }
                })
                .unwrap_or(quote! {
                    #sig {
                        unsafe {
                            if let Some(f) = self.0.as_ref().and_then(|this| this.#name) {
                                #pre_forward_args
                                f(#(#args),*);
                                #post_forward_args
                            }
                        }
                    }
                })
        });

        let base_name = self.base(name);
        let impl_trait = format_ident!("Impl{rust_name}");
        let impl_base_name = base_name
            .filter(|base| *base != BASE_SCOPED)
            .and_then(|base| self.cef_name_map.get(base))
            .map(|entry| {
                let base = &entry.name;
                let base = format_ident!("Impl{base}");
                quote! { #base }
            });
        let impl_get_raw = impl_base_name
            .as_ref()
            .map(|impl_base_name| {
                quote! {
                    fn get_raw(&self) -> *mut #name_ident {
                        <Self as #impl_base_name>::get_raw(self).cast()
                    }
                }
            })
            .unwrap_or(quote! { fn get_raw(&self) -> *mut #name_ident; });
        let impl_base_name = impl_base_name.unwrap_or(quote! { Sized });
        let impl_methods = s.methods.iter().map(|m| {
            let sig = m.get_signature(self);
            let method_name = &m.name;
            let comment = format!("See [`{name}::{method_name}`] for more documentation.");
            quote! {
                #[doc = #comment]
                #sig;
            }
        });

        let mut base_name = base_name;
        let mut base_structs = vec![];
        while let Some(next_base) = base_name
            .filter(|base| *base != root)
            .and_then(|base| self.lookup_struct_declaration.get(base))
            .and_then(|&i| self.struct_declarations.get(i))
        {
            base_name = self.base(&next_base.name);
            base_structs.push(next_base);
        }

        let init_bases = base_structs
            .iter()
            .enumerate()
            .map(|(i, base_struct)| {
                let name = &base_struct.name;
                let name = format_ident!("{name}");
                let impl_mod = format_ident!("impl{name}");
                let bases = iter::repeat_n(format_ident!("base"), i + 1);
                quote! {
                    #impl_mod::init_methods::<Self>(&mut object.#(#bases).*);
                }
            })
            .collect::<Vec<_>>()
            .into_iter()
            .rev();

        let impl_bases = base_structs
            .into_iter()
            .filter_map(|base_struct| {
                self.cef_name_map.get(&base_struct.name).map(|entry| {
                    let name = &base_struct.name;
                    let name_ident = format_ident!("{name}");
                    let base = &entry.name;
                    let base_trait = format_ident!("Impl{base}");
                    let base = format_ident!("{base}");
                    let base_methods = base_struct.methods.iter().map(|m| {
                        let sig = m.get_signature(self);
                        let name = &m.name;
                        let name = format_ident!("{name}");
                        let args = m.merge_params(self).filter_map(|arg| match arg {
                            MergedParam::Single { name, .. } => {
                                let name = format_ident!("{name}");
                                Some(quote! { #name })
                            }
                            MergedParam::Bounded { slice_name, .. }
                            | MergedParam::Buffer { slice_name, .. } => {
                                let name = format_ident!("{slice_name}");
                                Some(quote! { #name })
                            }
                            _ => None,
                        });
                        quote! {
                            #sig {
                                #base(self.0.cast()).#name(#(#args),*)
                            }
                        }
                    });

                    quote! {
                        impl #base_trait for #rust_name {
                            #(#base_methods)*

                            fn get_raw(&self) -> *mut #name_ident {
                                self.0.cast()
                            }
                        }
                    }
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .rev();

        let name = &s.name;
        let impl_mod = format_ident!("impl{name}");
        let init_methods = s.methods.iter().map(|m| {
            let name = &m.name;
            let name = format_ident!("{name}");
            quote! {
                object.#name = Some(#name::<I>);
            }
        });

        let wrapped_methods = s.methods.iter().map(|m| {
                let name = &m.name;
                let rust_method_name = make_rust_method_name(name);
                let name = format_ident!("{name}");
                let rust_method_name = format_ident!("{rust_method_name}");
                let args = m.inputs.iter().map(|arg| {
                    let name = make_snake_case_value_name(&arg.name);
                    let name = format_ident!("{name}");
                    let ty = self.resolve_type_aliases(arg.ty);
                    quote! { #name: #ty }
                });
                let wrapped_args = m.wrap_cef_args(self);
                let unwrapped_args = m.unwrap_cef_args(self);
                let forward_args = m.merge_params(self).filter_map(|arg| match arg {
                    MergedParam::Single { name, .. } => {
                        let name = format_ident!("arg_{name}");
                        Some(quote! { #name })
                    }
                    MergedParam::Bounded { slice_name, .. }
                    | MergedParam::Buffer { slice_name, .. } => {
                        let name = format_ident!("arg_{slice_name}");
                        Some(quote! { #name })
                    }
                    _ => None,
                });
                let original_output = m.output.map(|ty| self.resolve_type_aliases(ty));
                let output = original_output.as_ref().map(|output| {
                    quote! { -> #output }
                });
                let forward_output = original_output.and_then(|output| {
                    match syn::parse2::<ModifiedType>(output) {
                        Ok(ModifiedType { ty, modifiers }) => {
                            self.cef_name_map
                                .get(&ty.to_token_stream().to_string())
                                .map(|entry| match entry {
                                    NameMapEntry {
                                        ty: NameMapType::StructDeclaration(_),
                                        ..
                                    } => match modifiers.as_slice() {
                                        [TypeModifier::ConstPtr] => {
                                            quote! { result.map(|result| result.into()).unwrap_or(std::ptr::null()) }
                                        }
                                        [TypeModifier::MutPtr] => {
                                            quote! { result.map(|result| result.into()).unwrap_or(std::ptr::null_mut()) }
                                        }
                                        _ => quote! { result.into() },
                                    }
                                    _ => quote! { result.into() },
                                })
                                .or_else(|| {
                                    if unwrapped_args.is_empty() {
                                        None
                                    } else {
                                        Some(quote! { result })
                                    }
                                })
                        }
                        _ => None,
                    }
                });
                let mut call_impl =
                    quote! { #impl_trait::#rust_method_name(&arg_self_.interface, #(#forward_args),*) };
                if forward_output.is_some() || !unwrapped_args.is_empty() {
                    call_impl = quote! { let result = #call_impl; };
                }

                quote! {
                    extern "C" fn #name<I: #impl_trait>(#(#args),*) #output {
                        #wrapped_args
                        #call_impl
                        #unwrapped_args
                        #forward_output
                    }
                }
            });

        let wrapper = quote! {
            #[derive(Clone, Copy)]
            pub struct #rust_name(*mut #name_ident);

            pub trait #impl_trait : #impl_base_name {
                #(#impl_methods)*

                fn init_methods(object: &mut #name_ident) {
                    #(#init_bases)*
                    #impl_mod::init_methods::<Self>(object);
                }

                #impl_get_raw
            }

            mod #impl_mod {
                use super::*;

                pub fn init_methods<I: #impl_trait>(object: &mut #name_ident) {
                    #(#init_methods)*
                }

                #(#wrapped_methods)*
            }

            #(#impl_bases)*

            impl #impl_trait for #rust_name {
                #(#methods)*

                fn get_raw(&self) -> *mut #name_ident {
                    self.0
                }
            }

            impl ConvertParam<*mut #name_ident> for &#rust_name {
                fn into_raw(self) -> *mut #name_ident {
                    #impl_trait::get_raw(self)
                }
            }

            impl ConvertParam<*mut #name_ident> for &mut #rust_name {
                fn into_raw(self) -> *mut #name_ident {
                    #impl_trait::get_raw(self)
                }
            }

            impl ConvertReturnValue<#rust_name> for *mut #name_ident {
                fn wrap_result(self) -> #rust_name {
                    #rust_name(self)
                }
            }

            impl From<#rust_name> for *mut #name_ident {
                fn from(value: #rust_name) -> Self {
                    #impl_trait::get_raw(&value)
                }
            }

            impl Default for #rust_name {
                fn default() -> Self {
                    Self(std::ptr::null_mut())
                }
            }
        }
        .to_string();

        writeln!(f, "{wrapper}")
    }

    pub fn write_structs(&self, f: &mut Formatter<'_>) -> fmt::Result {
        for s in self.struct_declarations.iter() {
            let Some(NameMapEntry {
                name: rust_name,
                ty: NameMapType::StructDeclaration(_),
            }) = self.cef_name_map.get(&s.name)
            else {
                continue;
            };
            let rust_name = format_ident!("{rust_name}");

            let name = s.name.as_str();
            writeln!(f, "\n/// See [`{name}`] for more documentation.")?;

            if CUSTOM_STRING_TYPES.contains(&name) {
                self.write_custom_string_type(f, &rust_name)?;
                continue;
            }

            let name_ident = format_ident!("{name}");
            let root = self.root(name);
            if root == BASE_REF_COUNTED {
                if s.is_sealed {
                    self.write_sealed_struct(f, s, root, &name_ident, &rust_name)
                } else {
                    self.write_ref_counted_struct(f, s, root, &name_ident, &rust_name)
                }?;
                continue;
            }

            if root == BASE_SCOPED {
                self.write_scoped_struct(f, s, root, &name_ident, &rust_name)?;
                continue;
            }

            if !s.methods.is_empty()
                || s.fields.is_empty()
                || s.fields.iter().map(|f| f.name.as_str()).eq(["_unused"])
            {
                let wrapper = quote! {
                    pub struct #rust_name(#name_ident);

                    impl From<#name_ident> for #rust_name {
                        fn from(value: #name_ident) -> Self {
                            Self(value)
                        }
                    }

                    impl From<&#rust_name> for *const #name_ident {
                        fn from(value: &#rust_name) -> Self {
                            std::ptr::from_ref(value).cast()
                        }
                    }

                    impl From<&mut #rust_name> for *mut #name_ident  {
                        fn from(value: &mut #rust_name) -> Self {
                            std::ptr::from_mut(value).cast()
                        }
                    }

                    impl From<#rust_name> for #name_ident  {
                        fn from(value: #rust_name) -> Self {
                            self.0
                        }
                    }

                    impl AsRef<#name_ident> for #rust_name {
                        fn as_ref(&self) -> &#name_ident {
                            &self.0
                        }
                    }

                    impl AsMut<#name_ident> for #rust_name {
                        fn as_mut(&mut self) -> &mut #name_ident {
                            &mut self.0
                        }
                    }

                    impl Default for #rust_name {
                        fn default() -> Self {
                            unsafe { std::mem::zeroed() }
                        }
                    }
                }
                .to_string();
                writeln!(f, "{wrapper}")?;
                continue;
            }

            let fields = s
                .fields
                .iter()
                .map(|f| {
                    let name = &f.name;
                    let rust_name = make_snake_case_value_name(name);
                    let rust_name = format_ident!("{rust_name}");
                    let name = format_ident!("{name}");
                    let ty = self
                        .resolve_modified_type(f.ty)
                        .unwrap_or_else(|| ModifiedType {
                            modifiers: Default::default(),
                            ty: f.ty.clone(),
                        });
                    let rust_ty = ty.ty.to_token_stream();
                    let ty_string = rust_ty.to_string();
                    let entry = self.cef_name_map.get(&ty_string);
                    let rust_ty = match entry.as_ref() {
                        Some(NameMapEntry { name, .. }) => {
                            let name = format_ident!("{name}");
                            quote! { #name }
                        }
                        _ => rust_ty,
                    };
                    let modifiers = ty.modifiers.iter().filter_map(|modifier| match modifier {
                        TypeModifier::MutPtr => Some(quote! { *mut }),
                        TypeModifier::ConstPtr => Some(quote! { *const }),
                        TypeModifier::MutRef => Some(quote! { &mut }),
                        TypeModifier::Ref => Some(quote! { & }),
                        _ => None,
                    });
                    let rust_ty = match ty.modifiers.last() {
                        Some(TypeModifier::MutSlice) => quote! { &mut [#rust_ty] },
                        Some(TypeModifier::Slice) => quote! { &[#rust_ty] },
                        Some(TypeModifier::Array { size }) => quote! { [#rust_ty; #size] },
                        _ => rust_ty,
                    };
                    (
                        rust_name,
                        name.clone(),
                        entry,
                        quote! { #(#modifiers)* #rust_ty },
                    )
                })
                .collect::<Vec<_>>();
            let fields_decl = fields.iter().map(|(rust_name, _, _, ty)| {
                quote! { pub #rust_name: #ty, }
            });
            let from_cef_fields = fields.iter().filter_map(|(rust_name, name, entry, ty)| {
                let ty = syn::parse2::<ModifiedType>(ty.clone()).ok()?;
                Some(match (ty.modifiers.last(), entry) {
                    (Some(TypeModifier::Array { .. }), _) => {
                        quote! { #rust_name: init_array_field(value.#name), }
                    }
                    (_, Some(_)) => quote! { #rust_name: value.#name.into(), },
                    _ => quote! { #rust_name: value.#name, },
                })
            });
            let from_rust_fields = fields.iter().filter_map(|(rust_name, name, entry, ty)| {
                let ty = syn::parse2::<ModifiedType>(ty.clone()).ok()?;
                Some(match (ty.modifiers.last(), entry) {
                    (Some(TypeModifier::Array { .. }), _) => {
                        quote! { #name: init_array_field(value.#rust_name), }
                    }
                    (_, Some(_)) => quote! { #name: value.#rust_name.into(), },
                    _ => quote! { #name: value.#rust_name, },
                })
            });
            let impl_default = match s.fields.first() {
                Some(f) if f.name.as_str() == "size" => {
                    quote! {
                        Self {
                            size: std::mem::size_of::<#name_ident>(),
                            ..unsafe { std::mem::zeroed() }
                        }
                    }
                }
                _ => quote! { unsafe { std::mem::zeroed() } },
            };

            let wrapper = quote! {
                #[derive(Clone)]
                pub struct #rust_name {
                    #(#fields_decl)*
                }

                impl From<#name_ident> for #rust_name {
                    fn from(value: #name_ident) -> Self {
                        Self {
                            #(#from_cef_fields)*
                        }
                    }
                }

                impl From<#rust_name> for #name_ident {
                    fn from(value: #rust_name) -> Self {
                        Self {
                            #(#from_rust_fields)*
                        }
                    }
                }

                impl Default for #rust_name {
                    fn default() -> Self {
                        #impl_default
                    }
                }
            }
            .to_string();
            writeln!(f, "{wrapper}")?;
        }
        Ok(())
    }

    pub fn write_enums(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let enum_names = self
            .enum_names
            .iter()
            .filter_map(|e| make_rust_type_name(&e.name).map(|rust_name| (rust_name, e)));
        for (rust_name, e) in enum_names {
            let name = &e.name;
            writeln!(f, "\n/// See [`{name}`] for more documentation.")?;
            let name = format_ident!("{name}");
            let rust_name = format_ident!("{rust_name}");
            let impl_default =
                e.ty.and_then(|ty| ty.variants.first())
                    .map(|v| {
                        let v = &v.ident;
                        quote! { Self(#name::#v) }
                    })
                    .unwrap_or(quote! { unsafe { std::mem::zeroed() } });
            let wrapper = quote! {
                #[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
                pub struct #rust_name(#name);

                impl AsRef<#name> for #rust_name {
                    fn as_ref(&self) -> &#name {
                        &self.0
                    }
                }

                impl AsMut<#name> for #rust_name {
                    fn as_mut(&mut self) -> &mut #name {
                        &mut self.0
                    }
                }

                impl From<#name> for #rust_name {
                    fn from(value: #name) -> Self {
                        Self(value)
                    }
                }

                impl From<#rust_name> for #name  {
                    fn from(value: #rust_name) -> Self {
                        value.0
                    }
                }

                impl Default for #rust_name {
                    fn default() -> Self {
                        #impl_default
                    }
                }
            }
            .to_string();
            writeln!(f, "{wrapper}")?;
        }
        Ok(())
    }

    pub fn write_globals(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let pattern = Regex::new(r"^cef_(\w+)$").unwrap();

        for global_fn in self.global_function_declarations.iter() {
            let original_name = global_fn.name.as_str();
            writeln!(f, "\n/// See [`{original_name}`] for more documentation.")?;
            let name = pattern
                .captures(original_name)
                .and_then(|captures| captures.get(1))
                .map(|name| name.as_str())
                .unwrap_or(original_name);
            let name = format_ident!("{name}");
            let original_name = format_ident!("{original_name}");
            let args = global_fn.get_rust_args(self);
            let output = global_fn.get_rust_output(self);
            let inputs = global_fn
                .inputs
                .iter()
                .map(|arg| {
                    let rust_name = make_snake_case_value_name(&arg.name);
                    let rust_name = format_ident!("arg_{rust_name}");
                    let ty = self.resolve_type_aliases(arg.ty);
                    (rust_name, ty)
                })
                .collect::<Vec<_>>();
            let unwrap_args = global_fn.unwrap_rust_args(self);
            let forward_args: Vec<_> = inputs
                .iter()
                .map(|(rust_name, _)| {
                    quote! { #rust_name }
                })
                .collect();
            let rewrap_args = global_fn.rewrap_rust_args(self);
            let wrapper = global_fn
                .output
                .as_ref()
                .and_then(|ty| syn::parse2::<ModifiedType>(self.resolve_type_aliases(ty)).ok())
                .map(|ModifiedType { modifiers, ty, .. }| {
                    let ty = ty.to_token_stream().to_string();
                    let wrap_result = match modifiers.as_slice() {
                        [TypeModifier::ConstPtr | TypeModifier::MutPtr]
                            if ty != quote! { ::std::os::raw::c_void }.to_string() =>
                        {
                            match self.cef_name_map.get(&ty) {
                                Some(NameMapEntry {
                                    ty: NameMapType::StructDeclaration(_),
                                    ..
                                }) => Some(quote! {
                                    if result.is_null() {
                                        None
                                    } else {
                                        Some(result.wrap_result())
                                    }
                                }),
                                _ => None,
                            }
                        }
                        _ => None,
                    }
                    .unwrap_or(quote! { result.wrap_result() });

                    quote! {
                        pub fn #name(#args) #output {
                            unsafe {
                                #unwrap_args
                                let result = #original_name(#(#forward_args),*);
                                #rewrap_args
                                #wrap_result
                            }
                        }
                    }
                })
                .unwrap_or(quote! {
                    pub fn #name(#args) {
                        unsafe {
                            #unwrap_args
                            #original_name(#(#forward_args),*);
                            #rewrap_args
                        }
                    }
                })
                .to_string();
            writeln!(f, "{wrapper}")?;
        }
        Ok(())
    }
}

impl Display for ParseTree<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.write_prelude(f)?;
        self.write_aliases(f)?;
        self.write_structs(f)?;
        self.write_enums(f)?;
        self.write_globals(f)
    }
}

impl<'a> From<&'a syn::File> for ParseTree<'a> {
    fn from(value: &'a syn::File) -> Self {
        let mut tree = Self::default();

        tree.type_aliases = value
            .items
            .iter()
            .filter_map(|item| match item {
                syn::Item::Type(item_type) => Some(TypeAliasRef {
                    name: item_type.ident.to_string(),
                    ty: item_type.ty.as_ref(),
                }),
                _ => None,
            })
            .collect();

        tree.enum_names = value
            .items
            .iter()
            .filter_map(|item| match item {
                syn::Item::Enum(e) => Some(EnumRef {
                    name: e.ident.to_string(),
                    ty: Some(e),
                }),
                syn::Item::Struct(item_struct) => match &item_struct.fields {
                    syn::Fields::Unnamed(fields) if fields.unnamed.len() == 1 => Some(EnumRef {
                        name: item_struct.ident.to_string(),
                        ty: None,
                    }),
                    _ => None,
                },
                _ => None,
            })
            .collect();

        tree.struct_declarations = value
            .items
            .iter()
            .filter_map(|item| match item {
                syn::Item::Struct(item_struct) => match &item_struct.fields {
                    syn::Fields::Named(syn::FieldsNamed { named, .. }) => {
                        let mut fields = vec![];
                        let mut methods = vec![];

                        for member in named.iter() {
                            if let Ok(method) = SignatureRef::try_from(member) {
                                methods.push(method);
                            } else if let Ok(field) = FieldRef::try_from(member) {
                                fields.push(field);
                            }
                        }

                        let is_sealed = item_struct
                            .attrs
                            .iter()
                            .find_map(|attr| match attr {
                                syn::Attribute {
                                    style: syn::AttrStyle::Outer,
                                    meta:
                                        syn::Meta::NameValue(syn::MetaNameValue {
                                            path,
                                            value:
                                                syn::Expr::Lit(syn::ExprLit {
                                                    lit: syn::Lit::Str(value),
                                                    ..
                                                }),
                                            ..
                                        }),
                                    ..
                                } if path.to_token_stream().to_string()
                                    == quote! { doc }.to_string() =>
                                {
                                    Some(value.to_token_stream().to_string())
                                }
                                _ => None,
                            })
                            .map(|comment| {
                                comment.ends_with(r#"NOTE: This struct is allocated DLL-side.\n""#)
                            })
                            .unwrap_or_default();

                        Some(StructDeclarationRef {
                            name: item_struct.ident.to_string(),
                            fields,
                            methods,
                            is_sealed,
                        })
                    }
                    _ => None,
                },
                _ => None,
            })
            .collect();

        tree.global_function_declarations = value
            .items
            .iter()
            .filter_map(|item| match item {
                syn::Item::ForeignMod(syn::ItemForeignMod {
                    unsafety: Some(_),
                    abi:
                        syn::Abi {
                            name: Some(abi), ..
                        },
                    items,
                    ..
                }) if abi.value() == "C" => Some(items),
                _ => None,
            })
            .flat_map(|items| {
                items.iter().filter_map(|item| match item {
                    syn::ForeignItem::Fn(syn::ForeignItemFn {
                        sig:
                            syn::Signature {
                                ident,
                                inputs,
                                output,
                                ..
                            },
                        ..
                    }) => Some(SignatureRef {
                        name: ident.to_string(),
                        inputs: inputs
                            .iter()
                            .map(|arg| match arg {
                                syn::FnArg::Receiver(_) => {
                                    unreachable!("unexpected function receiver")
                                }
                                syn::FnArg::Typed(syn::PatType { pat, ty, .. }) => {
                                    match pat.as_ref() {
                                        syn::Pat::Ident(syn::PatIdent { ident, .. }) => FnArgRef {
                                            name: ident.to_string(),
                                            ty: ty.as_ref(),
                                        },
                                        _ => unreachable!("unexpected argument name type"),
                                    }
                                }
                            })
                            .collect(),
                        output: match output {
                            syn::ReturnType::Default => None,
                            syn::ReturnType::Type(_, ty) => Some(ty.as_ref()),
                        },
                        merged_params: Default::default(),
                    }),
                    _ => None,
                })
            })
            .collect();

        tree.cef_name_map = tree
            .type_aliases
            .iter()
            .map(|alias| alias.name.as_str())
            .map(|cef_name| (cef_name, NameMapType::TypeAlias))
            .chain(
                tree.enum_names
                    .iter()
                    .map(|e| e.name.as_str())
                    .map(|cef_name| (cef_name, NameMapType::EnumName)),
            )
            .chain(
                tree.struct_declarations
                    .iter()
                    .map(|s| (s.name.as_str(), s.is_sealed))
                    .map(|(cef_name, is_sealed)| {
                        (cef_name, NameMapType::StructDeclaration(is_sealed))
                    }),
            )
            .filter_map(|(cef_name, ty)| {
                make_rust_type_name(cef_name).map(|rust_name| (cef_name, (rust_name, ty)))
            })
            .filter_map(|(cef_name, (rust_name, ty))| {
                if cef_name == rust_name.as_str() {
                    None
                } else {
                    Some((
                        cef_name.to_owned(),
                        NameMapEntry {
                            name: rust_name,
                            ty,
                        },
                    ))
                }
            })
            .collect();
        tree.rust_name_map = tree
            .cef_name_map
            .iter()
            .map(|(a, NameMapEntry { name: b, ty })| {
                (
                    b.clone(),
                    NameMapEntry {
                        name: a.clone(),
                        ty: *ty,
                    },
                )
            })
            .collect();

        tree.lookup_type_alias = tree
            .type_aliases
            .iter()
            .enumerate()
            .map(|(index, alias)| (alias.name.clone(), index))
            .collect();
        tree.lookup_enum_name = tree
            .enum_names
            .iter()
            .enumerate()
            .map(|(index, e)| (e.name.clone(), index))
            .collect();
        tree.lookup_struct_declaration = tree
            .struct_declarations
            .iter()
            .enumerate()
            .map(|(index, s)| (s.name.clone(), index))
            .collect();
        tree.lookup_global_function_declaration = tree
            .global_function_declarations
            .iter()
            .enumerate()
            .map(|(index, f)| (f.name.clone(), index))
            .collect();

        tree.base_types = tree
            .struct_declarations
            .iter()
            .filter_map(|s| match s.fields.as_slice() {
                [FieldRef { name, ty }] if name.as_str() == "base" => {
                    Some((s.name.clone(), tree.resolve_type_aliases(ty).to_string()))
                }
                _ => None,
            })
            .collect();

        tree
    }
}

fn format_bindings(source_path: &Path) -> crate::Result<()> {
    let mut cmd = Command::new("rustfmt");
    cmd.arg(source_path);
    cmd.output()?;
    Ok(())
}

fn make_rust_type_name(name: &str) -> Option<String> {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    let pattern = PATTERN.get_or_init(|| Regex::new(r"^_?cef_(\w+)_t$").unwrap());
    pattern
        .captures(name)
        .and_then(|captures| captures.get(1))
        .map(|name| {
            let name = name
                .as_str()
                .from_case(Case::Snake)
                .to_case(Case::UpperCamel);
            if name.starts_with("String") {
                format!("Cef{name}")
            } else {
                name
            }
        })
}

fn make_snake_case_value_name(name: &str) -> String {
    name.from_case(Case::Camel).to_case(Case::Snake)
}

const RUST_METHOD_EXCEPTIONS: &[&str] = &["get_type", "get_layout"];

fn make_rust_method_name(name: &str) -> &str {
    if RUST_METHOD_EXCEPTIONS.contains(&name) {
        name
    } else {
        name.trim_start_matches("get_")
    }
}
