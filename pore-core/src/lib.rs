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

#[derive(Debug)]
pub struct FileIndex {
    meta: Metadata,
    index_dir: Option<PathBuf>,
    index: Index,
    filepath: Field,
    contents: Field,
    need_rebuild: bool,
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

#[derive(Debug, Serialize, Deserialize)]
pub struct Metadata {
    version: String,
    for_dir: PathBuf,
    last_update: DateTime<Utc>,
    config: IndexOptions,
}

#[derive(Debug)]
pub struct DocResult {
    pub score: f32,
    pub address: DocAddress,
}

const METADATA_FILE: &str = "pore_meta.json";

impl Metadata {
    pub fn version(&self) -> &str {
        &self.version
    }
    pub fn for_dir(&self) -> &Path {
        &self.for_dir
    }
    pub fn last_update(&self) -> &DateTime<Utc> {
        &self.last_update
    }
    pub fn config(&self) -> &IndexOptions {
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
        match &self.index_dir {
            None => return Ok(()),
            Some(index_dir) => {
                if !index_dir.exists() {
                    return Ok(());
                }
                eprintln!("Removing index files");
                let mut index_writer = self.index.writer(50_000_000)?;
                index_writer.delete_all_documents()?;
                index_writer.commit()?;
                let metafile = index_dir.join(METADATA_FILE);
                fs::remove_file(metafile).ok();
                fs::remove_dir(&index_dir).ok();
                Ok(())
            }
        }
    }
    pub fn get_or_create(
        for_dir: &Path,
        cache_dir: Option<&Path>,
        config: &IndexOptions,
    ) -> Result<Self, Box<dyn error::Error>> {
        let mut meta: Metadata;
        let mut need_rebuild = false;
        let metafile = cache_dir.map(|p| p.join(METADATA_FILE));
        if metafile.as_deref().map(|p| p.exists()).unwrap_or(false) {
            meta = serde_json::from_str(&fs::read_to_string(metafile.unwrap())?)?;
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
        match cache_dir {
            None => {
                index = Index::create_in_ram(schema.clone());
            }
            Some(index_dir) => {
                fs::create_dir_all(&index_dir)?;
                index = Index::open_or_create(MmapDirectory::open(&index_dir)?, schema.clone())?;
            }
        }
        index.tokenizers().register(&tokenizer_name, tokenizer);

        Ok(Self {
            index,
            index_dir: cache_dir.map(|p| fs::canonicalize(p).unwrap()),
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
        if let Some(index_dir) = &self.index_dir {
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
        limit: usize,
        threshold: f32,
    ) -> tantivy::Result<(LeasedItem<Searcher>, Vec<DocResult>)> {
        let reader = self
            .index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommit)
            .try_into()?;
        let searcher = reader.searcher();
        let top_docs = searcher.search(query, &TopDocs::with_limit(limit))?;
        let mut results = Vec::new();
        for (score, doc_address) in top_docs {
            if score > threshold {
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
        if let Some(index_dir) = &self.index_dir {
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
