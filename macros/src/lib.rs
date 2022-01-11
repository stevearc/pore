use proc_macro2::{Ident, Span};
use quote::quote;
use syn::punctuated::Punctuated;
use syn::token::Colon2;
use syn::{
    parse_macro_input, AngleBracketedGenericArguments, Data, DeriveInput, GenericArgument, Path,
    PathArguments, PathSegment, Type, TypePath, VisPublic, Visibility,
};
use syn::{Fields, Token};

#[proc_macro_attribute]
pub fn create_option_copy(
    attr: proc_macro::TokenStream,
    input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let input: DeriveInput = parse_macro_input!(input as DeriveInput);

    let source_name = &input.ident;
    let copy_name: Ident = syn::parse(attr).expect("expected a single name of the option struct");
    let mut field_names: Vec<Ident> = vec![];
    match &input.data {
        Data::Struct(_) => {
            let mut new_struct: DeriveInput = input.clone();
            new_struct.ident = copy_name.clone();
            match &mut new_struct.data {
                Data::Struct(source) => match &mut source.fields {
                    Fields::Named(fields) => {
                        for field in fields.named.iter_mut() {
                            field_names.push(field.ident.clone().unwrap());
                            field.vis = Visibility::Public(VisPublic {
                                pub_token: Token![pub](Span::call_site()),
                            });

                            let mut option_args = Punctuated::new();
                            option_args.push(GenericArgument::Type(field.ty.clone()));
                            let mut segments = Punctuated::<PathSegment, Colon2>::new();
                            segments.push(PathSegment {
                                ident: Ident::new("Option", Span::call_site()),
                                arguments: PathArguments::AngleBracketed(
                                    AngleBracketedGenericArguments {
                                        colon2_token: None,
                                        args: option_args,
                                        lt_token: Token![<](Span::call_site()),
                                        gt_token: Token![>](Span::call_site()),
                                    },
                                ),
                            });

                            if !type_is_option(&field.ty) {
                                field.ty = Type::Path(TypePath {
                                    qself: None,
                                    path: Path {
                                        leading_colon: None,
                                        segments,
                                    },
                                });
                            }
                        }
                    }
                    _ => panic!("Unit and Unnamed field structs are unsupported"),
                },
                _ => panic!("Must use creation_option_copy on a struct"),
            }
            let expanded = quote!(
                #input
                #new_struct
                // Utility methods for the opt copy
                impl #copy_name {
                    #[doc = "Set fields from other if not already set"]
                    pub fn merge(self: &mut Self, other: &#copy_name) {
                        #(if self.#field_names.is_none() {
                            self.#field_names = other.#field_names.clone();
                        })*
                    }

                    #[doc = "Override self fields with other when present"]
                    pub fn merge_from(self: &mut Self, other: &#copy_name) {
                        #(if other.#field_names.is_some() {
                            self.#field_names = other.#field_names.clone();
                        })*
                    }

                    #[doc = "Returns true if any fields are Some"]
                    pub fn any(self: &Self) -> bool {
                        #(if self.#field_names.is_some() {
                            return true;
                        })*
                        return false;
                    }

                    #[doc = "Returns Ok if all fields are set"]
                    pub fn all(self: &Self) -> Result<(), Vec<&str>> {
                        let mut has_all = true;
                        let mut missing_fields = vec![];
                        #(if self.#field_names.is_none() {
                            has_all = false;
                            missing_fields.push(stringify!(#field_names));
                        })*
                        if has_all {
                            Ok(())
                        } else {
                            Err(missing_fields)
                        }
                    }
                }

                // Conversion from Lua value
                impl<'lua> mlua::FromLua<'lua> for #copy_name {
                    fn from_lua(lua_value: mlua::Value<'lua>, lua: &'lua mlua::Lua) -> mlua::Result<Self> {
                        let mut ret = #copy_name::default();
                        match lua_value {
                            mlua::Value::Table(table) => {
                                #(if table.contains_key("#field_names")? {
                                    ret.#field_names = Some(table.get("#field_names")?);
                                })*
                            }
                            mlua::Value::Nil => {}
                            _ => {
                                return Err(mlua::Error::FromLuaConversionError {
                                    from: lua_value.type_name(),
                                    to: "#copy_name",
                                    message: Some("Value is not a table".to_string()),
                                });
                            }
                        }
                        Ok(ret)
                    }
                }

                impl Into<#source_name> for #copy_name {
                    fn into(self) -> #source_name {
                        let mut ret = #source_name::default();
                        #(if let Some(#field_names) = self.#field_names {
                            ret.#field_names = #field_names;
                        })*
                        return ret;
                    }
                }

                impl Default for #copy_name {
                    fn default() -> #copy_name {
                        return #copy_name {
                            #(#field_names: None,)*
                        };
                    }
                }
            );
            proc_macro::TokenStream::from(expanded)
        }
        _ => panic!("expected struct"),
    }
}

fn type_is_option(ty: &Type) -> bool {
    if let Type::Path(path) = ty {
        return path_is_option(&path.path);
    }
    return false;
}

fn path_is_option(path: &Path) -> bool {
    path.get_ident().map_or(false, |ident| ident == "Option")
}
