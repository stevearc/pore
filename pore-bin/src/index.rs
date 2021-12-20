use crate::config::IndexConfig;
use crate::config::SearchConfig;
use chrono::DateTime;
use chrono::Local;
use chrono::NaiveDateTime;
use chrono::Utc;
use ignore::overrides::OverrideBuilder;
use ignore::WalkBuilder;
use ignore::WalkState;
use serde::{Deserialize, Serialize};
use std::env;
use std::error;
use std::fmt::Display;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use tantivy::collector::TopDocs;
use tantivy::doc;
use tantivy::query::Query;
use tantivy::DocAddress;
use tantivy::LeasedItem;
use tantivy::ReloadPolicy;
use tantivy::Searcher;

use tantivy::directory::MmapDirectory;
use tantivy::schema::*;
use tantivy::tokenizer::*;
use tantivy::Index;

pub struct FileIndex {
    meta: Metadata,
    index_dir: PathBuf,
    index: Index,
    filepath: Field,
    contents: Field,
    need_rebuild: bool,
}

#[derive(Serialize, Deserialize)]
pub struct Metadata {
    version: String,
    for_dir: PathBuf,
    last_update: DateTime<Utc>,
    config: IndexConfig,
}

#[derive(Debug)]
pub struct DocResult {
    pub score: f32,
    pub address: DocAddress,
}

const METADATA_FILE: &str = "pore_meta.json";

impl Metadata {
    pub fn config(&self) -> &IndexConfig {
        &self.config
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
    pub fn delete(&self) -> Result<(), Box<dyn error::Error>> {
        if !self.index_dir.exists() {
            return Ok(());
        }
        eprintln!("Removing index files");
        let mut index_writer = self.index.writer(50_000_000)?;
        index_writer.delete_all_documents()?;
        index_writer.commit()?;
        let metafile = self.index_dir.join(METADATA_FILE);
        fs::remove_file(metafile).ok();
        fs::remove_dir(&self.index_dir).ok();
        Ok(())
    }
    fn find_index_dir(
        for_dir: &Path,
        index_name: Option<&str>,
    ) -> Result<PathBuf, Box<dyn error::Error>> {
        let mut cache_home = env::var("XDG_CACHE_HOME").unwrap_or("".to_string());
        if cache_home == "" {
            cache_home = env::var("HOME")? + "/.cache";
        }
        let mut index_root = PathBuf::from(cache_home);
        index_root.push(env!("CARGO_PKG_NAME"));
        if for_dir.is_absolute() {
            index_root.push(for_dir.strip_prefix("/")?);
        } else {
            index_root.push(env::current_dir()?.strip_prefix("/")?);
            index_root.push(for_dir)
        }
        if let Some(name) = index_name {
            index_root.push(format!("__index_{}", name));
        }
        return Ok(index_root);
    }
    pub fn get_or_create(
        for_dir: &Path,
        config: &IndexConfig,
        index_name: Option<&str>,
    ) -> Result<Self, Box<dyn error::Error>> {
        let mut index_dir = FileIndex::find_index_dir(&for_dir, index_name)?;
        let metafile = index_dir.join(METADATA_FILE);
        let mut meta: Metadata;
        let mut need_rebuild = false;
        if !config.in_memory && metafile.exists() {
            meta = serde_json::from_str(&fs::read_to_string(metafile)?)?;
            if meta.config() != config {
                need_rebuild = true;
                meta.config = config.clone();
            }
        } else {
            meta = Metadata {
                config: config.clone(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                last_update: DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(0, 0), Utc),
                for_dir: fs::canonicalize(if for_dir.is_absolute() {
                    for_dir.to_path_buf()
                } else {
                    env::current_dir()?.join(for_dir)
                })?,
            }
        }

        let mut schema_builder = Schema::builder();
        schema_builder.add_text_field("filepath", STRING | STORED);

        let tokenizer = TextAnalyzer::from(SimpleTokenizer)
            .filter(RemoveLongFilter::limit(50))
            .filter(LowerCaser)
            .filter(Stemmer::new(config.language));
        let tokenizer_name = "language_tokenizer";

        let text_options = TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer(&tokenizer_name)
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        );
        schema_builder.add_text_field("contents", text_options);
        let schema = schema_builder.build();

        let index;
        if config.in_memory {
            index = Index::create_in_ram(schema.clone());
        } else {
            fs::create_dir_all(&index_dir)?;
            index = Index::open_or_create(MmapDirectory::open(&index_dir)?, schema.clone())?;
            index_dir = fs::canonicalize(index_dir)?;
        }
        index.tokenizers().register(&tokenizer_name, tokenizer);

        Ok(Self {
            index,
            index_dir,
            meta,
            filepath: schema.get_field("filepath").unwrap(),
            contents: schema.get_field("contents").unwrap(),
            need_rebuild,
        })
    }

    pub fn get_file_walker(&self) -> Result<WalkBuilder, Box<dyn error::Error>> {
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

    pub fn update(&mut self, rebuild: bool) -> Result<&mut Self, Box<dyn error::Error>> {
        let mut index_writer = self.index.writer(50_000_000)?;
        let walker = self.get_file_walker()?;
        let now = Utc::now();
        walker.build_parallel().run(|| {
            Box::new(|result| {
                if let Ok(entry) = result {
                    if let Ok(contents) = fs::read_to_string(entry.path()) {
                        let modified: DateTime<Utc> =
                            entry.metadata().unwrap().modified().unwrap().into();
                        if self.need_rebuild || rebuild || modified > self.meta.last_update {
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
        if self.index_dir.exists() {
            fs::write(
                self.index_dir.join(METADATA_FILE),
                serde_json::to_string(&self.meta)?,
            )?;
        }

        return Ok(self);
    }

    pub fn search(
        &self,
        query: &Box<dyn Query>,
        config: &SearchConfig,
    ) -> tantivy::Result<(LeasedItem<Searcher>, Vec<DocResult>)> {
        let reader = self
            .index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommit)
            .try_into()?;
        let searcher = reader.searcher();
        let top_docs = searcher.search(query, &TopDocs::with_limit(config.limit))?;
        let mut results = Vec::new();
        for (score, doc_address) in top_docs {
            if score > config.threshold {
                results.push(DocResult {
                    score,
                    address: doc_address,
                });
            }
        }
        Ok((searcher, results))
    }
}

impl Display for FileIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Index({:?})", self.meta.for_dir)?;
        writeln!(f, "  version: {}", self.meta.version)?;
        if self.meta.config.in_memory {
            writeln!(f, "  location: in-memory")?;
        } else {
            writeln!(f, "  location: {:?}", &self.index_dir)?;
            writeln!(
                f,
                "  last updated: {}",
                DateTime::<Local>::from(self.meta.last_update)
            )?;
        }
        for field in toml::to_string(&self.meta.config)
            .unwrap_or("".to_string())
            .split("\n")
        {
            writeln!(f, "  {}", field.replace(" =", ":"))?;
        }
        Ok(())
    }
}
