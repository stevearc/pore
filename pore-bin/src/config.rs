use macros::create_option_copy;
use pore_core::IndexOptions;
use serde::{Deserialize, Serialize};
use std::env;
use std::error;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use tantivy::tokenizer::Language;
use toml::Value;

use crate::color_mode::ColorMode;
const CONFIG_FILE: &str = "pore.toml";

#[create_option_copy(IndexConfigOpt)]
#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq)]
pub struct IndexConfig {
    pub follow: bool,
    pub glob: Vec<String>,
    pub glob_case_insensitive: bool,
    pub hidden: bool,
    pub ignore_files: bool,
    pub in_memory: bool,
    pub language: Language,
    pub oglob: Vec<String>,
    pub threads: usize,
}

impl Default for IndexConfig {
    fn default() -> IndexConfig {
        return IndexConfig {
            follow: false,
            hidden: false,
            language: Language::English,
            ignore_files: true,
            glob_case_insensitive: false,
            glob: vec![],
            oglob: vec![],
            threads: 0,
            in_memory: false,
        };
    }
}

impl Into<IndexOptions> for IndexConfig {
    fn into(self) -> IndexOptions {
        return IndexOptions {
            follow: self.follow,
            hidden: self.hidden,
            language: self.language,
            ignore_files: self.ignore_files,
            glob_case_insensitive: self.glob_case_insensitive,
            glob: self.glob.clone(),
            oglob: self.oglob.clone(),
            threads: self.threads,
        };
    }
}

#[create_option_copy(SearchConfigOpt)]
#[derive(Debug, Deserialize, Clone)]
pub struct SearchConfig {
    pub json: bool,
    pub limit: usize,
    pub threshold: f32,
    pub filename_only: bool,
    pub color: ColorMode,
    pub rebuild_index: bool,
    pub update: bool,
}

impl Default for SearchConfig {
    fn default() -> SearchConfig {
        return SearchConfig {
            json: false,
            limit: 1000,
            threshold: 0.0,
            filename_only: false,
            color: ColorMode::Auto,
            rebuild_index: false,
            update: true,
        };
    }
}

pub fn load_config(
    path: &Path,
    index_name: Option<&str>,
) -> Result<(IndexConfigOpt, SearchConfigOpt), Box<dyn error::Error>> {
    let path_str = path.to_string_lossy();
    let mut config_home = env::var("XDG_CONFIG_HOME").unwrap_or("".to_string());
    if config_home == "" {
        config_home = env::var("HOME")? + "/.config";
    }
    let config_file = PathBuf::from(config_home).join(CONFIG_FILE);
    if config_file.exists() {
        let contents = &fs::read_to_string(&config_file)?;
        let value = contents
            .parse::<Value>()
            .expect(&format!("Error parsing config file {:?}", config_file));
        let mut index: IndexConfigOpt = value.clone().try_into()?;
        let mut search: SearchConfigOpt = value.clone().try_into()?;

        let mut found_index = false;
        if let Value::Table(table) = &value {
            // Look for a local configuration with a matching path
            for (_, val) in table.iter() {
                if let Value::Table(local_config) = val {
                    if local_config.get("path") == Some(&Value::String(path_str.to_string())) {
                        index.merge_from(&val.clone().try_into()?);
                        search.merge_from(&val.clone().try_into()?);
                        // Look for an index the local config
                        if let Some(idx_name) = index_name {
                            if let Some(local_index_config) = local_config.get(idx_name) {
                                index.merge_from(&local_index_config.clone().try_into()?);
                                search.merge_from(&local_index_config.clone().try_into()?);
                                found_index = true;
                            }
                        }
                        break;
                    }
                }
            }
            // if index exists, find global index and load it
            if index_name.is_some() {
                if let Some(global_index) = table.get(&format!("index-{}", index_name.unwrap())) {
                    index.merge_from(&global_index.clone().try_into()?);
                    search.merge_from(&global_index.clone().try_into()?);
                    found_index = true;
                }
            }
        }
        if index_name.is_some() && !found_index {
            bail!(format!("Could not find index '{}'", index_name.unwrap()));
        }

        return Ok((index, search));
    }
    Ok((IndexConfigOpt::default(), SearchConfigOpt::default()))
}

#[cfg(test)]
mod tests {
    use std::{env, fs, path::PathBuf, str::FromStr};

    use toml::Value;

    use crate::config::{IndexConfig, IndexConfigOpt, SearchConfigOpt};

    use super::{load_config, CONFIG_FILE};

    #[test]
    fn parsing_opt_configs_works() {
        let contents = "follow = false
threads = 100
limit = 4
";
        let index: IndexConfigOpt = toml::from_str(contents).unwrap();
        assert_eq!(index.follow, Some(false));
        assert_eq!(index.threads, Some(100));
        assert_eq!(index.ignore_files, None);
        let search: SearchConfigOpt = toml::from_str(contents).unwrap();
        assert_eq!(search.limit, Some(4));
        assert_eq!(search.json, None);
    }

    #[test]
    fn merging_opt_configs_works() {
        let mut i1 = IndexConfigOpt {
            follow: Some(true),
            ..Default::default()
        };
        let i2 = IndexConfigOpt {
            threads: Some(20),
            ..Default::default()
        };
        i1.merge(&i2);
        assert_eq!(i1.follow, Some(true));
        assert_eq!(i1.threads, Some(20));
        assert_eq!(i1.language, None);
        let conf: IndexConfig = i1.into();
        assert_eq!(conf.follow, true);
        assert_eq!(conf.threads, 20);
        assert_eq!(conf.hidden, false);
    }

    #[test]
    fn can_load_and_merge_defaults() {
        let tmpdir = tempfile::tempdir().unwrap();
        env::set_var("XDG_CONFIG_HOME", tmpdir.path().as_os_str());
        let conf_file = PathBuf::from(tmpdir.path()).join(CONFIG_FILE);
        fs::write(
            conf_file,
            "threads = 10
        [index-global_index]
        threads = 20

        [local-1]
        path = '/foo'
        threads = 30

        [local-1.local_index]
            threads = 40
            ",
        )
        .unwrap();

        let (index, _) = load_config(&PathBuf::from_str("/").unwrap(), None).unwrap();
        assert_eq!(index.threads, Some(10));
        let (index, _) =
            load_config(&PathBuf::from_str("/").unwrap(), Some("global_index")).unwrap();
        assert_eq!(index.threads, Some(20));
        let (index, _) = load_config(&PathBuf::from_str("/foo").unwrap(), None).unwrap();
        assert_eq!(index.threads, Some(30));
        let (index, _) =
            load_config(&PathBuf::from_str("/foo").unwrap(), Some("local_index")).unwrap();
        assert_eq!(index.threads, Some(40));
    }

    #[test]
    fn example_file_is_complete() {
        let example = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("pore.example.toml");
        let contents = &fs::read_to_string(&example).unwrap();
        let value = contents
            .parse::<Value>()
            .expect(&format!("Error parsing config file {:?}", example));
        let index: IndexConfigOpt = value.clone().try_into().unwrap();
        let search: SearchConfigOpt = value.clone().try_into().unwrap();
        if let Err(missing_fields) = index.all() {
            panic!("pore.example.toml is missing fields: {:?}", missing_fields);
        }
        if let Err(missing_fields) = search.all() {
            panic!("pore.example.toml is missing fields: {:?}", missing_fields);
        }
    }
}
