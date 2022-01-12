use chrono::DateTime;
use chrono::NaiveDateTime;
use chrono::Utc;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;
use tantivy::doc;

use tantivy::directory::MmapDirectory;
use tantivy::schema::*;
use tantivy::tokenizer::*;
use tantivy::Index;

use crate::language::LanguageRef;

pub trait IndexMetadata<T: MetadataConfig + Eq> {
    fn config(&self) -> &T;
    fn version(&self) -> &str;
    fn last_update(&self) -> &DateTime<Utc>;
    fn set_last_update(&mut self, time: DateTime<Utc>);
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata<T: MetadataConfig + Eq> {
    version: String,
    last_update: DateTime<Utc>,
    config: T,
}

impl<T: MetadataConfig + Eq> Metadata<T> {
    pub fn new(config: T) -> Self {
        Metadata {
            config,
            version: env!("CARGO_PKG_VERSION").to_string(),
            last_update: DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(0, 0), Utc),
        }
    }
}

impl<T: MetadataConfig + Eq> IndexMetadata<T> for Metadata<T> {
    fn config(&self) -> &T {
        &self.config
    }
    fn version(&self) -> &str {
        &self.version
    }
    fn last_update(&self) -> &DateTime<Utc> {
        &self.last_update
    }
    fn set_last_update(&mut self, time: DateTime<Utc>) {
        self.last_update = time;
    }
}

pub trait MetadataConfig {
    fn language(&self) -> LanguageRef;
}

pub const METADATA_FILE: &str = "pore_meta.json";

pub fn create_index<
    T: IndexMetadata<U> + DeserializeOwned,
    U: MetadataConfig + Eq,
    P: AsRef<Path>,
    I: IntoIterator<Item = V>,
    V: Into<String>,
>(
    cache_dir: Option<P>,
    config: &U,
    id_field: &str,
    text_fields: I,
) -> Result<(Option<T>, Index), anyhow::Error> {
    let mut ret_meta: Option<T> = None;
    let metafile = cache_dir.as_ref().map(|p| p.as_ref().join(METADATA_FILE));
    if metafile.as_deref().map(|p| p.exists()).unwrap_or(false) {
        let meta_res = serde_json::from_str::<T>(&fs::read_to_string(metafile.unwrap())?);
        if let Ok(meta) = meta_res {
            if meta.config() == config {
                ret_meta = Some(meta);
            }
        }
    }

    let mut tokenizers = HashMap::new();
    let mut get_tokenizer = |lang: Language| {
        let key = format!("stemmer_{:?}", lang);
        if !tokenizers.contains_key(&key) {
            let tokenizer = TextAnalyzer::from(SimpleTokenizer)
                .filter(RemoveLongFilter::limit(40))
                .filter(LowerCaser)
                .filter(Stemmer::new(lang));
            tokenizers.insert(key.clone(), tokenizer);
        }
        return key;
    };
    let mut schema_builder = Schema::builder();
    schema_builder.add_text_field(id_field, STRING | STORED);
    for name in text_fields {
        let text_options = TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer(&get_tokenizer(config.language().into()))
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        );
        schema_builder.add_text_field(&name.into(), text_options);
    }
    let schema = schema_builder.build();
    let index = match cache_dir {
        None => Index::create_in_ram(schema.clone()),
        Some(index_dir) => {
            fs::create_dir_all(&index_dir)?;
            let mut index_res =
                Index::open_or_create(MmapDirectory::open(&index_dir)?, schema.clone());
            // If it fails to load, it's probably because the schema is different or the index is
            // corrupted. Delete all files in the dir and try again.
            if index_res.is_err() {
                eprintln!("Index is corrupted. Deleting index files");
                for dir_entry in fs::read_dir(&index_dir)? {
                    if let Ok(entry) = dir_entry {
                        if entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                            fs::remove_file(entry.path())?;
                        }
                    }
                }
                index_res = Index::open_or_create(MmapDirectory::open(&index_dir)?, schema.clone());
            }
            index_res?
        }
    };
    for (name, tokenizer) in tokenizers {
        index.tokenizers().register(&name, tokenizer);
    }
    Ok((ret_meta, index))
}

pub fn delete_index(index: &Index, cache_dir: Option<&Path>) -> anyhow::Result<bool> {
    match cache_dir {
        None => return Ok(false),
        Some(index_dir) => {
            if !index_dir.exists() {
                return Ok(false);
            }
            let mut index_writer = index.writer(50_000_000)?;
            index_writer.delete_all_documents()?;
            index_writer.commit()?;
            let metafile = index_dir.join(METADATA_FILE);
            fs::remove_file(metafile).ok();
            fs::remove_dir(&index_dir).ok();
            Ok(true)
        }
    }
}
