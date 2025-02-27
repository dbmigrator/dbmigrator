use std::path::PathBuf;

use dbmigrator_core::recipe::{
    find_sql_files, load_sql_recipes_iter, simple_kind_detector, RecipeMeta, RecipeScript,
    SIMPLE_FILENAME_PATTERN,
};
use proc_macro::TokenStream;
use quote::{quote, ToTokens, TokenStreamExt};
use syn::{parse_macro_input, LitStr};

pub(crate) fn crate_root() -> PathBuf {
    let crate_root = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR environment variable not present");
    PathBuf::from(crate_root)
}

struct MacroRecipeScript(PathBuf, RecipeScript);

impl ToTokens for MacroRecipeScript {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let recipe = &self.1;
        let path = self
            .0
            .canonicalize()
            .map_err(|e| format!("error canonicalizing recipe path {}: {e}", self.0.display()))
            .and_then(|path| {
                let path_str = path.to_str().ok_or_else(|| {
                    format!(
                        "recipe path cannot be represented as a string: {}",
                        path.display()
                    )
                })?;

                Ok(quote! { include_str!(#path_str) })
            })
            .unwrap_or_else(|err| quote!(compile_error!(#err)));
        let meta = MacroRecipeMeta(&recipe.meta);
        let checksum = &recipe.checksum;
        let version = &recipe.version;
        let name = &recipe.name;
        let ts = quote! {
            dbmigrator::__core::recipe::RecipeScript {
                version: ::std::borrow::Cow::Borrowed(#version),
                name: ::std::borrow::Cow::Borrowed(#name),
                checksum: ::std::borrow::Cow::Borrowed(#checksum),
                sql: ::std::borrow::Cow::Borrowed(#path),
                meta: #meta,
            }
        };
        tokens.append_all(ts);
    }
}

struct MacroRecipeMeta<'a>(&'a RecipeMeta);

impl<'a> ToTokens for MacroRecipeMeta<'a> {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let ts = match &self.0 {
            RecipeMeta::Baseline => quote!(dbmigrator::__core::recipe::RecipeMeta::Baseline),
            RecipeMeta::Upgrade => quote!(dbmigrator::__core::recipe::RecipeMeta::Upgrade),
            RecipeMeta::Revert {
                old_checksum,
                maximum_version,
            } => {
                quote!(dbmigrator::__core::recipe::RecipeMeta::Revert {
                    old_checksum: ::std::borrow::Cow::Borrowed(#old_checksum),
                    maximum_version: ::std::borrow::Cow::Borrowed(#maximum_version),
                })
            }
            RecipeMeta::Fixup {
                old_checksum,
                maximum_version,
                new_version,
                new_name,
                new_checksum,
            } => {
                quote!(dbmigrator::__core::recipe::RecipeMeta::Revert {
                    old_checksum: ::std::borrow::Cow::Borrowed(#old_checksum),
                    maximum_version: ::std::borrow::Cow::Borrowed(#maximum_version),
                    new_version: ::std::borrow::Cow::Borroed(#new_version),
                    new_name: ::std::borrow::Cow::Borowed(#new_name),
                    new_checksum: ::std::borrow::Cow::Borrowed(#new_checksum)
                })
            }
        };
        tokens.append_all(ts);
    }
}

#[proc_macro]
pub fn embed_migrations(input: TokenStream) -> TokenStream {
    let location = if input.is_empty() {
        crate_root().join("migrations")
    } else {
        let location: LitStr = parse_macro_input!(input);
        crate_root().join(location.value())
    };
    let files = find_sql_files(location).expect("error finding sql files");
    let mut recipes = Vec::new();
    for res in
        load_sql_recipes_iter(files, SIMPLE_FILENAME_PATTERN, Some(simple_kind_detector)).unwrap()
    {
        let (path, recipe) = res.unwrap();
        recipes.push(MacroRecipeScript(path, recipe).into_token_stream());
    }

    quote! {
        pub const fn recipes() -> &'static [dbmigrator::__core::recipe::RecipeScript] {
            const RECIPES: &[dbmigrator::__core::recipe::RecipeScript] = &[#(#recipes),*];
            &RECIPES
        }
        pub fn migrator(config: dbmigrator::Config, version_comparator: fn(&str, &str) -> std::cmp::Ordering) -> dbmigrator::Migrator {
            let mut migrator = dbmigrator::Migrator::new(config, version_comparator);
            migrator.set_recipes(recipes().to_owned()).unwrap();
            migrator
        }
    }
    .into()
}
