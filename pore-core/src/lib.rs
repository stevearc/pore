use chrono::DateTime;
use chrono::Local;
use chrono::NaiveDateTime;
use chrono::Utc;
use ignore::overrides::OverrideBuilder;
use ignore::WalkBuilder;
use ignore::WalkState;
use location::DocResult;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::collections::HashMap;
use std::env;
use std::fmt::Display;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use tantivy::collector::TopDocs;
use tantivy::doc;
use tantivy::query::Query;
use tantivy::ReloadPolicy;

use tantivy::directory::MmapDirectory;
use tantivy::schema::*;
use tantivy::tokenizer::*;
use tantivy::Index;

mod location;

type BytePositions = BinaryHeap<Reverse<u32>>;

#[derive(Debug)]
pub struct FileIndex {
    meta: FileMetadata,
    cache_dir: Option<PathBuf>,
    index: Index,
    filepath: Field,
    contents: Field,
}

#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq)]
pub struct IndexOptions {
    pub follow: bool,
    pub glob: Vec<String>,
    pub glob_case_insensitive: bool,
    pub hidden: bool,
    pub ignore_files: bool,
    pub language: Language,
    pub oglob: Vec<String>,
    pub threads: usize,
}

#[derive(Debug)]
pub struct SearchOptions {
    pub limit: usize,
    pub threshold: f32,
    pub filename_only: bool,
    pub root_dir: Option<String>,
}

pub trait IndexMetadata {
    fn config(&self) -> &IndexOptions;
    fn version(&self) -> &str;
    fn last_update(&self) -> &DateTime<Utc>;
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Metadata {
    version: String,
    last_update: DateTime<Utc>,
    config: IndexOptions,
}

impl Metadata {
    pub fn new(config: IndexOptions) -> Self {
        Metadata {
            config,
            version: env!("CARGO_PKG_VERSION").to_string(),
            last_update: DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(0, 0), Utc),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileMetadata {
    version: String,
    last_update: DateTime<Utc>,
    config: IndexOptions,
    for_dir: PathBuf,
}

impl FileMetadata {
    pub fn new(config: IndexOptions, for_dir: &Path) -> Result<Self, anyhow::Error> {
        Ok(FileMetadata {
            config,
            version: env!("CARGO_PKG_VERSION").to_string(),
            last_update: DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(0, 0), Utc),
            for_dir: fs::canonicalize(if for_dir.is_absolute() {
                for_dir.to_path_buf()
            } else {
                env::current_dir()?.join(for_dir)
            })?,
        })
    }
}

#[derive(Debug, Serialize)]
pub struct SearchResult {
    file: PathBuf,
    score: f32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    lines: Vec<Line>,
}

impl SearchResult {
    pub fn file(&self) -> &Path {
        &self.file
    }
    pub fn score(&self) -> f32 {
        self.score
    }
    pub fn lines(&self) -> &Vec<Line> {
        &self.lines
    }
}

#[derive(Debug, Serialize)]
pub struct Line {
    number: u32,
    text: String,
}

impl Line {
    pub fn number(&self) -> u32 {
        self.number
    }
    pub fn text(&self) -> &str {
        &self.text
    }
}

const METADATA_FILE: &str = "pore_meta.json";

impl IndexMetadata for Metadata {
    fn config(&self) -> &IndexOptions {
        &self.config
    }
    fn version(&self) -> &str {
        &self.version
    }
    fn last_update(&self) -> &DateTime<Utc> {
        &self.last_update
    }
}

impl IndexMetadata for FileMetadata {
    fn config(&self) -> &IndexOptions {
        &self.config
    }
    fn version(&self) -> &str {
        &self.version
    }
    fn last_update(&self) -> &DateTime<Utc> {
        &self.last_update
    }
}

impl FileMetadata {
    pub fn for_dir(&self) -> &Path {
        &self.for_dir
    }
}

impl FileIndex {
    pub fn index(&self) -> &Index {
        &self.index
    }
    pub fn filepath(&self) -> &Field {
        &self.filepath
    }
    pub fn contents(&self) -> &Field {
        &self.contents
    }
    pub fn delete(&self) -> Result<bool, anyhow::Error> {
        match &self.cache_dir {
            None => return Ok(false),
            Some(index_dir) => {
                if !index_dir.exists() {
                    return Ok(false);
                }
                let mut index_writer = self.index.writer(50_000_000)?;
                index_writer.delete_all_documents()?;
                index_writer.commit()?;
                let metafile = index_dir.join(METADATA_FILE);
                fs::remove_file(metafile).ok();
                fs::remove_dir(&index_dir).ok();
                Ok(true)
            }
        }
    }
    pub fn get_or_create(
        for_dir: &Path,
        cache_dir: Option<&Path>,
        config: &IndexOptions,
    ) -> Result<Self, anyhow::Error> {
        let (meta_opt, index): (Option<FileMetadata>, Index) = create_index(
            cache_dir,
            config,
            &vec![FieldConfig::Id("filepath"), FieldConfig::Text("contents")],
        )?;
        let meta = meta_opt.unwrap_or_else(|| FileMetadata::new(config.clone(), for_dir).unwrap());
        let filepath = index
            .schema()
            .get_field("filepath")
            .expect("No field named 'filepath'");
        let contents = index
            .schema()
            .get_field("contents")
            .expect("No field named 'contents'");
        Ok(Self {
            index,
            cache_dir: cache_dir.map(|p| fs::canonicalize(p).unwrap()),
            meta,
            filepath,
            contents,
        })
    }

    pub fn get_file_walker(&self) -> Result<WalkBuilder, anyhow::Error> {
        let mut builder = WalkBuilder::new(&self.meta.for_dir);
        builder
            .hidden(!self.meta.config.hidden)
            .threads(self.meta.config.threads)
            .ignore(self.meta.config.ignore_files)
            .git_global(self.meta.config.ignore_files)
            .git_ignore(self.meta.config.ignore_files)
            .git_exclude(self.meta.config.ignore_files)
            .follow_links(self.meta.config.follow);
        if !self.meta.config.glob.is_empty() {
            let mut globs = OverrideBuilder::new(&self.meta.for_dir);
            globs.case_insensitive(self.meta.config.glob_case_insensitive)?;
            for glob in &self.meta.config.glob {
                globs.add(&glob)?;
            }
            builder.overrides(globs.build()?);
        }
        if !self.meta.config.oglob.is_empty() {
            let mut globs = OverrideBuilder::new(&self.meta.for_dir);
            globs.case_insensitive(self.meta.config.glob_case_insensitive)?;
            for glob in &self.meta.config.oglob {
                globs.add(&glob)?;
            }
            let matcher = globs.build()?;
            builder.filter_entry(move |e| {
                if e.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                    return true;
                } else {
                    return matcher.matched(e.path(), false).is_whitelist();
                };
            });
        }
        Ok(builder)
    }

    pub fn update(&mut self, rebuild: bool) -> Result<&mut Self, anyhow::Error> {
        let mut index_writer = self.index.writer(50_000_000)?;
        let walker = self.get_file_walker()?;
        let now = Utc::now();
        walker.build_parallel().run(|| {
            Box::new(|result| {
                if let Ok(entry) = result {
                    if let Ok(contents) = fs::read_to_string(entry.path()) {
                        let modified: DateTime<Utc> =
                            entry.metadata().unwrap().modified().unwrap().into();
                        if rebuild || modified > self.meta.last_update {
                            let filepath = entry.path().strip_prefix(&self.meta.for_dir).unwrap();
                            let doc = doc!(
                                self.filepath => String::from(filepath.to_string_lossy()),
                                self.contents => contents,
                            );
                            index_writer.add_document(doc);
                        }
                    }
                }
                WalkState::Continue
            })
        });

        index_writer.commit()?;
        self.meta.last_update = now;
        if let Some(index_dir) = &self.cache_dir {
            fs::write(
                index_dir.join(METADATA_FILE),
                serde_json::to_string(&self.meta)?,
            )?;
        }

        return Ok(self);
    }

    pub fn search(
        &self,
        query: &Box<dyn Query>,
        opts: &SearchOptions,
    ) -> Result<Vec<SearchResult>, anyhow::Error> {
        let reader = self
            .index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommit)
            .try_into()?;
        let searcher = reader.searcher();
        let top_docs = searcher.search(query, &TopDocs::with_limit(opts.limit))?;
        let mut doc_results = Vec::new();
        for (score, doc_address) in top_docs {
            if score > opts.threshold {
                doc_results.push(DocResult {
                    score,
                    address: doc_address,
                });
            }
        }
        let mut position_map = location::get_search_results(self, query, &searcher, &doc_results)?;
        let mut results = Vec::new();
        for doc_result in doc_results {
            let doc = searcher.doc(doc_result.address)?;
            let filepath = doc.get_first(*self.filepath()).unwrap().text().unwrap();
            let fullpath = if let Some(root_dir) = opts.root_dir.as_deref() {
                PathBuf::from(root_dir).join(filepath)
            } else {
                PathBuf::from(self.meta.for_dir()).join(filepath)
            };

            let mut lines = Vec::new();
            if !opts.filename_only {
                if let Some(mut position_data) = position_map.get_mut(&doc_result.address) {
                    location::positions_to_lines(&self, &fullpath, &mut position_data, &mut lines)?
                };
            }
            results.push(SearchResult {
                file: fullpath,
                score: doc_result.score,
                lines,
            });
        }
        Ok(results)
    }
}

impl Display for FileIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Index({:?})", self.meta.for_dir)?;
        writeln!(f, "  version: {}", self.meta.version)?;
        if let Some(index_dir) = &self.cache_dir {
            writeln!(f, "  location: {:?}", index_dir)?;
            writeln!(
                f,
                "  last updated: {}",
                DateTime::<Local>::from(self.meta.last_update)
            )?;
        } else {
            writeln!(f, "  location: in-memory")?;
        }
        for field in serde_json::to_string(&self.meta.config)
            .unwrap_or("".to_string())
            .split("\n")
        {
            writeln!(f, "  {}", field.replace(" =", ":"))?;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct GenericIndex {
    meta: Metadata,
    cache_dir: Option<PathBuf>,
    index: Index,
}

pub enum FieldConfig<'a> {
    Id(&'a str),
    Text(&'a str),
}

fn create_index<T: IndexMetadata + DeserializeOwned>(
    cache_dir: Option<&Path>,
    config: &IndexOptions,
    fields: &Vec<FieldConfig>,
) -> Result<(Option<T>, Index), anyhow::Error> {
    let mut ret_meta: Option<T> = None;
    let metafile = cache_dir.map(|p| p.join(METADATA_FILE));
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
    for field_config in fields {
        match field_config {
            FieldConfig::Id(name) => {
                schema_builder.add_text_field(name, STRING | STORED);
            }
            FieldConfig::Text(name) => {
                let text_options = TextOptions::default().set_indexing_options(
                    TextFieldIndexing::default()
                        .set_tokenizer(&get_tokenizer(config.language))
                        .set_index_option(IndexRecordOption::WithFreqsAndPositions),
                );
                schema_builder.add_text_field(name, text_options);
            }
        }
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

impl GenericIndex {
    pub fn get_or_create<T: Into<TextOptions>>(
        fields: &Vec<FieldConfig>,
        config: &IndexOptions,
        cache_dir: Option<&Path>,
    ) -> Result<Self, anyhow::Error> {
        let (meta_opt, index) = create_index(cache_dir, config, fields)?;
        let meta = meta_opt.unwrap_or_else(|| Metadata::new(config.clone()));
        Ok(Self {
            index,
            cache_dir: cache_dir.map(|p| fs::canonicalize(p).unwrap()),
            meta,
        })
    }
}
