use crate::common::create_index;
use crate::common::delete_index;
use crate::common::IndexMetadata;
use crate::common::MetadataConfig;
use crate::common::METADATA_FILE;
use crate::language::LanguageRef;
use crate::location;
use crate::location::DocResult;
use chrono::DateTime;
use chrono::Local;
use chrono::NaiveDateTime;
use chrono::Utc;
use ignore::overrides::OverrideBuilder;
use ignore::WalkBuilder;
use ignore::WalkState;
use macros::create_option_copy;
use mlua::ToLua;
use serde::{Deserialize, Serialize};
use std::env;
use std::fmt::Display;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use tantivy::collector::TopDocs;
use tantivy::doc;
use tantivy::query::Query;
use tantivy::ReloadPolicy;

use tantivy::schema::*;
use tantivy::Index;

#[derive(Debug, Clone)]
pub struct FileIndex {
    meta: FileMetadata,
    cache_dir: Option<PathBuf>,
    index: Index,
    filepath: Field,
    contents: Field,
}

#[create_option_copy(FileIndexOptionsShape)]
#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq)]
pub struct FileIndexOptions {
    pub follow: bool,
    pub glob: Vec<String>,
    pub glob_case_insensitive: bool,
    pub hidden: bool,
    pub ignore_files: bool,
    pub language: LanguageRef,
    pub oglob: Vec<String>,
    // TODO move this elsewhere
    pub threads: usize,
}

impl Default for FileIndexOptions {
    fn default() -> FileIndexOptions {
        FileIndexOptions {
            follow: false,
            hidden: false,
            language: LanguageRef::English,
            ignore_files: true,
            glob_case_insensitive: false,
            glob: vec![],
            oglob: vec![],
            threads: 0,
        }
    }
}

#[create_option_copy(FileSearchOptionsShape)]
#[derive(Debug)]
pub struct FileSearchOptions {
    pub limit: usize,
    pub threshold: f32,
    pub filename_only: bool,
    pub root_dir: Option<String>,
}

impl Default for FileSearchOptions {
    fn default() -> Self {
        FileSearchOptions {
            limit: 1000,
            threshold: 0.0,
            filename_only: false,
            root_dir: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    version: String,
    last_update: DateTime<Utc>,
    config: FileIndexOptions,
    for_dir: PathBuf,
}

impl MetadataConfig for FileIndexOptions {
    fn language(&self) -> LanguageRef {
        self.language
    }
}

impl FileMetadata {
    pub fn new<P: AsRef<Path>>(
        config: FileIndexOptions,
        for_dir: P,
    ) -> Result<Self, anyhow::Error> {
        let path = for_dir.as_ref();
        Ok(FileMetadata {
            config,
            version: env!("CARGO_PKG_VERSION").to_string(),
            last_update: DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(0, 0), Utc),
            for_dir: fs::canonicalize(if path.is_absolute() {
                path.to_path_buf()
            } else {
                env::current_dir()?.join(path)
            })?,
        })
    }
}

#[derive(Debug, Serialize)]
pub struct FileSearchResult {
    file: PathBuf,
    score: f32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    lines: Vec<Line>,
}

impl FileSearchResult {
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

impl<'lua> ToLua<'lua> for FileSearchResult {
    fn to_lua(self, lua: &'lua mlua::Lua) -> mlua::Result<mlua::Value<'lua>> {
        let tbl = lua.create_table()?;
        tbl.set("file", self.file.to_string_lossy())?;
        tbl.set("score", self.score)?;
        if !self.lines.is_empty() {
            tbl.set("lines", self.lines)?;
        }
        Ok(mlua::Value::Table(tbl))
    }
}

#[derive(Debug, Serialize)]
pub struct Line {
    pub number: u32,
    pub text: String,
}

impl<'lua> ToLua<'lua> for Line {
    fn to_lua(self, lua: &'lua mlua::Lua) -> mlua::Result<mlua::Value<'lua>> {
        let tbl = lua.create_table()?;
        tbl.set("number", self.number)?;
        tbl.set("text", self.text)?;
        Ok(mlua::Value::Table(tbl))
    }
}

impl FileMetadata {
    pub fn for_dir(&self) -> &Path {
        &self.for_dir
    }
}

impl IndexMetadata<FileIndexOptions> for FileMetadata {
    fn config(&self) -> &FileIndexOptions {
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
    pub fn delete(&self) -> anyhow::Result<bool> {
        delete_index(&self.index, self.cache_dir.as_deref())
    }
    pub fn get_or_create<P: AsRef<Path>>(
        for_dir: P,
        cache_dir: Option<P>,
        config: &FileIndexOptions,
    ) -> Result<Self, anyhow::Error> {
        let (meta_opt, index): (Option<FileMetadata>, Index) =
            create_index(cache_dir.as_ref(), config, "filepath", vec!["contents"])?;
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
        opts: &FileSearchOptions,
    ) -> Result<Vec<FileSearchResult>, anyhow::Error> {
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
            results.push(FileSearchResult {
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
        for field in serde_json::to_string_pretty(&self.meta.config)
            .unwrap_or("".to_string())
            .split("\n")
        {
            writeln!(f, "  {}", field.replace(" =", ":"))?;
        }
        Ok(())
    }
}
