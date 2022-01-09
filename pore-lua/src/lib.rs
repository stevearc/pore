use mlua::prelude::*;

#[mlua::lua_module]
fn pore_lua(lua: &Lua) -> LuaResult<LuaTable> {
    let exports = lua.create_table()?;

    exports.set("version", make_version_tbl(lua)?)?;

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

fn make_version_tbl(lua: &Lua) -> LuaResult<LuaTable> {
    let tbl = lua.create_table()?;

    set_nonempty_env!(tbl, "full", "CARGO_PKG_VERSION");
    set_nonempty_env!(tbl, "major", "CARGO_PKG_VERSION_MAJOR");
    set_nonempty_env!(tbl, "minor", "CARGO_PKG_VERSION_MINOR");
    set_nonempty_env!(tbl, "patch", "CARGO_PKG_VERSION_PATCH");
    set_nonempty_env!(tbl, "pre", "CARGO_PKG_VERSION_PRE");

    Ok(tbl)
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
