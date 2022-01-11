use std::str::FromStr;

use serde::{Deserialize, Serialize};
use tantivy::tokenizer::Language;

#[derive(Debug, Deserialize, Serialize, Clone, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LanguageRef {
    Arabic,
    Danish,
    Dutch,
    English,
    Finnish,
    French,
    German,
    Greek,
    Hungarian,
    Italian,
    Norwegian,
    Portuguese,
    Romanian,
    Russian,
    Spanish,
    Swedish,
    Tamil,
    Turkish,
}

impl Into<Language> for LanguageRef {
    fn into(self) -> Language {
        match self {
            LanguageRef::Arabic => Language::Arabic,
            LanguageRef::Danish => Language::Danish,
            LanguageRef::Dutch => Language::Dutch,
            LanguageRef::English => Language::English,
            LanguageRef::Finnish => Language::Finnish,
            LanguageRef::French => Language::French,
            LanguageRef::German => Language::German,
            LanguageRef::Greek => Language::Greek,
            LanguageRef::Hungarian => Language::Hungarian,
            LanguageRef::Italian => Language::Italian,
            LanguageRef::Norwegian => Language::Norwegian,
            LanguageRef::Portuguese => Language::Portuguese,
            LanguageRef::Romanian => Language::Romanian,
            LanguageRef::Russian => Language::Russian,
            LanguageRef::Spanish => Language::Spanish,
            LanguageRef::Swedish => Language::Swedish,
            LanguageRef::Tamil => Language::Tamil,
            LanguageRef::Turkish => Language::Turkish,
        }
    }
}

impl FromStr for LanguageRef {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "arabic" => Ok(LanguageRef::Arabic),
            "danish" => Ok(LanguageRef::Danish),
            "dutch" => Ok(LanguageRef::Dutch),
            "english" => Ok(LanguageRef::English),
            "finnish" => Ok(LanguageRef::Finnish),
            "french" => Ok(LanguageRef::French),
            "german" => Ok(LanguageRef::German),
            "greek" => Ok(LanguageRef::Greek),
            "hungarian" => Ok(LanguageRef::Hungarian),
            "italian" => Ok(LanguageRef::Italian),
            "norwegian" => Ok(LanguageRef::Norwegian),
            "portuguese" => Ok(LanguageRef::Portuguese),
            "romanian" => Ok(LanguageRef::Romanian),
            "russian" => Ok(LanguageRef::Russian),
            "spanish" => Ok(LanguageRef::Spanish),
            "swedish" => Ok(LanguageRef::Swedish),
            "tamil" => Ok(LanguageRef::Tamil),
            "turkish" => Ok(LanguageRef::Turkish),
            _ => Err(anyhow!("Invalid language value '{}'", s)),
        }
    }
}

impl<'lua> mlua::FromLua<'lua> for LanguageRef {
    fn from_lua(lua_value: mlua::Value<'lua>, _lua: &'lua mlua::Lua) -> mlua::Result<Self> {
        return match &lua_value {
            mlua::Value::String(str) => LanguageRef::from_str(str.to_str()?).map_err(|e| {
                mlua::Error::FromLuaConversionError {
                    from: lua_value.type_name(),
                    to: "Language",
                    message: Some(e.to_string()),
                }
            }),
            _ => Err(mlua::Error::FromLuaConversionError {
                from: lua_value.type_name(),
                to: "Language",
                message: Some("Value is not a string".to_string()),
            }),
        };
    }
}
