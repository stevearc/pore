use std::path::PathBuf;
use std::str::FromStr;

use mlua::prelude::*;
use mlua::{MetaMethod, UserData, UserDataMethods};
use pore_core::{
    FileIndex, FileIndexOptionsShape, FileSearchOptionsShape, GenericIndex, IndexOptionsShape,
    SearchOptionsShape,
};
use tantivy::query::QueryParser;

#[mlua::lua_module]
fn pore_lua(lua: &Lua) -> LuaResult<LuaTable> {
    let exports = lua.create_table()?;

    exports.set("version", make_version_tbl(lua)?)?;
    let get_file_index = lua.create_function(
        |_, (for_dir, cache_dir, config): (String, Option<String>, FileIndexOptionsShape)| {
            let index = FileIndex::get_or_create(
                PathBuf::from_str(&for_dir)
                    .map_err(|_| LuaError::RuntimeError(format!("Invalid path {}", for_dir)))?,
                cache_dir
                    .as_ref()
                    .map(|s| {
                        PathBuf::from_str(&s).map_err(|_| {
                            LuaError::RuntimeError(format!("Invalid path {:?}", cache_dir))
                        })
                    })
                    .transpose()?,
                &config.into(),
            )
            .map_err(|e| LuaError::RuntimeError(format!("Error creating index {:?}", e)))?;
            Ok(FileIndexLua { index })
        },
    )?;
    exports.set("get_file_index", get_file_index)?;

    let get_index = lua.create_function(
        |_,
         (id_field, text_fields, config, cache_dir): (
            String,
            Vec<String>,
            IndexOptionsShape,
            Option<String>,
        )| {
            let index = GenericIndex::get_or_create(
                &id_field,
                text_fields,
                &config.into(),
                cache_dir
                    .as_ref()
                    .map(|s| {
                        PathBuf::from_str(&s).map_err(|_| {
                            LuaError::RuntimeError(format!("Invalid path {:?}", cache_dir))
                        })
                    })
                    .transpose()?
                    .as_deref(),
            )
            .map_err(|e| LuaError::RuntimeError(format!("Error creating index {:?}", e)))?;
            Ok(GenericIndexLua { index })
        },
    )?;
    exports.set("get_index", get_index)?;

    Ok(exports)
}

macro_rules! set_nonempty_env {
    ($tbl:ident, $key:literal, $env_key:literal) => {{
        let value = env!($env_key);
        if !value.is_empty() {
            $tbl.set($key, value)?;
        }
    }};
}

#[derive(Debug, Clone)]
struct FileIndexLua {
    index: FileIndex,
}

impl UserData for FileIndexLua {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method_mut("update", |_, this, (rebuild,): (Option<bool>,)| {
            this.index
                .update(rebuild.unwrap_or(false))
                .map_err(|e| LuaError::RuntimeError(e.to_string()))?;
            Ok(())
        });
        methods.add_method_mut("delete", |_, this, _: ()| {
            this.index
                .delete()
                .map_err(|e| LuaError::RuntimeError(e.to_string()))?;
            Ok(())
        });
        methods.add_method(
            "search",
            |_, this, (query_str, opts): (String, FileSearchOptionsShape)| {
                let query_parser =
                    QueryParser::for_index(this.index.index(), vec![*this.index.contents()]);
                let query = query_parser
                    .parse_query(&query_str)
                    .map_err(|_| LuaError::RuntimeError("Error parsing query".to_string()))?;
                let results = this
                    .index
                    .search(&query, &opts.into())
                    .map_err(|e| LuaError::RuntimeError(e.to_string()))?;
                Ok(results)
            },
        );
        methods.add_meta_function(MetaMethod::ToString, |_, this: FileIndexLua| {
            Ok(format!("{}", this.index))
        });
    }
}

#[derive(Debug, Clone)]
struct GenericIndexLua {
    index: GenericIndex,
}

impl UserData for GenericIndexLua {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method_mut("delete", |_, this, _: ()| {
            this.index
                .delete()
                .map_err(|e| LuaError::RuntimeError(e.to_string()))?;
            Ok(())
        });
        methods.add_method_mut("delete_documents", |_, this, (doc_ids,): (Vec<String>,)| {
            this.index
                .delete_documents(doc_ids)
                .map_err(|e| LuaError::RuntimeError(e.to_string()))?;
            Ok(())
        });
        methods.add_method_mut(
            "update_documents",
            |_, this, (documents,): (Vec<mlua::Table>,)| {
                this.index
                    .update_documents(documents)
                    .map_err(|e| LuaError::RuntimeError(e.to_string()))?;
                Ok(())
            },
        );
        methods.add_method_mut(
            "add_documents",
            |_, this, (documents,): (Vec<mlua::Table>,)| {
                this.index
                    .add_documents(documents)
                    .map_err(|e| LuaError::RuntimeError(e.to_string()))?;
                Ok(())
            },
        );
        methods.add_method(
            "search",
            |_, this, (query_str, opts): (String, SearchOptionsShape)| {
                let query_parser =
                    QueryParser::for_index(this.index.index(), this.index.get_text_fields());
                let query = query_parser
                    .parse_query(&query_str)
                    .map_err(|_| LuaError::RuntimeError("Error parsing query".to_string()))?;
                let results = this
                    .index
                    .search(&query, &opts.into())
                    .map_err(|e| LuaError::RuntimeError(e.to_string()))?;
                Ok(results)
            },
        );
        methods.add_meta_function(MetaMethod::ToString, |_, this: FileIndexLua| {
            Ok(format!("{}", this.index))
        });
    }
}

fn make_version_tbl(lua: &Lua) -> LuaResult<LuaTable> {
    let tbl = lua.create_table()?;

    set_nonempty_env!(tbl, "full", "CARGO_PKG_VERSION");
    set_nonempty_env!(tbl, "major", "CARGO_PKG_VERSION_MAJOR");
    set_nonempty_env!(tbl, "minor", "CARGO_PKG_VERSION_MINOR");
    set_nonempty_env!(tbl, "patch", "CARGO_PKG_VERSION_PATCH");
    set_nonempty_env!(tbl, "pre", "CARGO_PKG_VERSION_PRE");

    Ok(tbl)
}
