use chrono::Utc;
use macros::create_option_copy;
use mlua::ToLua;
use serde::Deserialize;
use serde::Serialize;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use tantivy::collector::TopDocs;
use tantivy::doc;
use tantivy::query::Query;
use tantivy::ReloadPolicy;

use tantivy::schema::*;
use tantivy::Index;

use crate::common::create_index;
use crate::common::delete_index;
use crate::common::IndexMetadata;
use crate::common::Metadata;
use crate::common::MetadataConfig;
use crate::common::METADATA_FILE;
use crate::field_map::FieldMap;
use crate::language::LanguageRef;

#[derive(Debug, Clone)]
pub struct GenericIndex {
    meta: Metadata<IndexOptions>,
    cache_dir: Option<PathBuf>,
    index: Index,
}

#[create_option_copy(SearchOptionsShape)]
#[derive(Debug)]
pub struct SearchOptions {
    pub limit: usize,
    pub threshold: f32,
}

impl Default for SearchOptions {
    fn default() -> Self {
        SearchOptions {
            limit: 1000,
            threshold: 0.0,
        }
    }
}

#[create_option_copy(IndexOptionsShape)]
#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq)]
pub struct IndexOptions {
    pub language: LanguageRef,
}

impl Default for IndexOptions {
    fn default() -> Self {
        IndexOptions {
            language: LanguageRef::English,
        }
    }
}

impl MetadataConfig for IndexOptions {
    fn language(&self) -> LanguageRef {
        self.language
    }
}

#[derive(Debug, Serialize)]
pub struct SearchResult {
    id: String,
    score: f32,
}

impl<'lua> ToLua<'lua> for SearchResult {
    fn to_lua(self, lua: &'lua mlua::Lua) -> mlua::Result<mlua::Value<'lua>> {
        let tbl = lua.create_table()?;
        tbl.set("id", self.id)?;
        tbl.set("score", self.score)?;
        Ok(mlua::Value::Table(tbl))
    }
}

impl GenericIndex {
    pub fn index(&self) -> &Index {
        &self.index
    }
    pub fn delete(&self) -> anyhow::Result<bool> {
        delete_index(&self.index, self.cache_dir.as_deref())
    }

    pub fn get_or_create<I, T>(
        id_field: &str,
        text_fields: I,
        config: &IndexOptions,
        cache_dir: Option<&Path>,
    ) -> Result<Self, anyhow::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<String>,
    {
        let (meta_opt, index) = create_index(cache_dir, config, id_field, text_fields)?;
        let meta = meta_opt.unwrap_or_else(|| Metadata::new(config.clone()));
        Ok(Self {
            index,
            cache_dir: cache_dir.map(|p| fs::canonicalize(p).unwrap()),
            meta,
        })
    }

    fn get_id_field(&self) -> anyhow::Result<Field> {
        for (field, entry) in self.index.schema().fields() {
            if entry.is_stored() {
                return Ok(field);
            }
        }
        Err(anyhow!("Could not find stored ID field in index"))
    }

    pub fn get_text_fields(&self) -> Vec<Field> {
        let mut ret = Vec::new();
        for (field, entry) in self.index.schema().fields() {
            if !entry.is_stored() {
                ret.push(field);
            }
        }
        ret
    }

    pub fn delete_documents<I, T>(&mut self, document_ids: I) -> anyhow::Result<()>
    where
        I: IntoIterator<Item = T>,
        T: Into<String>,
    {
        let mut index_writer = self.index.writer(50_000_000)?;
        let id_field = self.get_id_field()?;
        for id in document_ids {
            index_writer.delete_term(Term::from_field_text(id_field, id.into().as_str()));
        }
        index_writer.commit()?;
        Ok(())
    }

    pub fn update_documents<T: FieldMap>(&mut self, documents: Vec<T>) -> anyhow::Result<()> {
        let id_field = self.get_id_field()?;
        let schema = self.index.schema();
        let id_field_entry = schema.get_field_entry(id_field);
        let id_name = id_field_entry.name();
        let document_ids = documents
            .iter()
            .map(|d| d.get_field(id_name).unwrap().to_owned());
        self.delete_documents(document_ids)?;
        self.add_documents(documents)
    }

    pub fn add_documents<T: FieldMap>(&mut self, documents: Vec<T>) -> anyhow::Result<()> {
        let mut index_writer = self.index.writer(50_000_000)?;
        let now = Utc::now();
        for document in documents {
            let mut doc = Document::default();
            for (field, entry) in self.index.schema().fields() {
                let text = document.get_field(entry.name())?;
                doc.add(FieldValue::new(field, text.as_ref().into()));
            }
            index_writer.add_document(doc);
        }
        index_writer.commit()?;
        self.meta.set_last_update(now);
        if let Some(index_dir) = &self.cache_dir {
            fs::write(
                index_dir.join(METADATA_FILE),
                serde_json::to_string(&self.meta)?,
            )?;
        }
        Ok(())
    }

    pub fn search(
        &self,
        query: &Box<dyn Query>,
        opts: &SearchOptions,
    ) -> anyhow::Result<Vec<SearchResult>> {
        let reader = self
            .index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommit)
            .try_into()?;
        let searcher = reader.searcher();
        let top_docs = searcher.search(query, &TopDocs::with_limit(opts.limit))?;
        let id_field = self.get_id_field()?;
        let mut results = Vec::new();
        for (score, doc_address) in top_docs {
            if score > opts.threshold {
                let doc = searcher.doc(doc_address)?;
                let id = doc.get_first(id_field).unwrap().text().unwrap().to_string();
                results.push(SearchResult { id, score });
            }
        }
        Ok(results)
    }
}
