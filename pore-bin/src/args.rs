use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::{env, error, fs};

use clap::ArgGroup;
use clap::{App, Arg};
use tantivy::tokenizer::Language;

use crate::color_mode::ColorMode;
use crate::config::{IndexConfigOpt, SearchConfigOpt};

#[derive(Debug)]
pub enum CmdArg {
    Search,
    ListFiles,
    ListIndex,
    Delete,
}

#[derive(Debug)]
pub struct GlobalConfig {
    pub index: IndexConfigOpt,
    pub search: SearchConfigOpt,
    pub command: CmdArg,
    pub query: Option<String>,
    pub query_path: PathBuf,
    pub search_dir: String,
    pub index_name: Option<String>,
}

pub fn parse_args() -> Result<GlobalConfig, Box<dyn error::Error>> {
    let matches = App::new(env!("CARGO_PKG_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .about(env!("CARGO_PKG_DESCRIPTION"))
        .author(env!("CARGO_PKG_AUTHORS"))
        // Index args
        .arg(
            Arg::new("index")
                .short('i')
                .long("index")
                .takes_value(true)
                .conflicts_with_all(&["in_memory", "no_memory", "hidden", "no_hidden", "follow_links", "no_follow_links", "language", "glob", "oglob", "glob_case_insensitive"])
                .help("Use the specified index for querying (must be specified in the config file)")
        )
        .arg(
            Arg::new("update")
                .short('u')
                .long("update")
                .help("Update the index before searching (the default)"),
        )
        .arg(
            Arg::new("no_update")
                .long("no-update")
                .conflicts_with("update")
                .help("Do not update the index before performing the query"),
        )
        .arg(
            Arg::new("in_memory")
                .long("in-memory")
                .help("Do not store the text index on disk (will have to rebuild every time)"),
        )
        .arg(
            Arg::new("no_memory")
                .long("no-memory")
                .conflicts_with("in_memory")
                .help("Force the index to be saved to disk (overrides --in-memory)"),
        )
        .arg(
            Arg::new("hidden")
                .long("hidden")
                .help("Search hidden files and directories"),
        )
        .arg(
            Arg::new("no_hidden")
                .long("no-hidden")
                .conflicts_with("hidden")
                .help("Ignore hidden files and directories (overrides --hidden)"),
        )
        .arg(
            Arg::new("follow_links")
                .short('L')
                .long("follow")
                .help("Follow symbolic links"),
        )
        .arg(
            Arg::new("no_follow_links")
                .long("no-follow")
                .conflicts_with("follow_links")
                .help("Don't follow symbolic links (overrides --follow)"),
        )
        .arg(
            Arg::new("language")
                .long("language")
                .validator(|a| language_from_str(&a).map(|_|()).ok_or("Invalid language".to_string()))
                .takes_value(true)
                .help("The language to use for parsing files"),
        )
        .arg(
            Arg::new("glob")
                .short('g')
                .long("glob")
                .help("Include or exclude files and directories for searching that match the given glob. This always overrides any other ignore logic. Multiple glob flags may be used. Precede a glob with a ! to exclude it.")
                .value_delimiter(',')
                .use_delimiter(true)
                .require_delimiter(true)
                .multiple_values(true)
        )
        .arg(
            Arg::new("oglob")
                .long("oglob")
                .help("Only search files that match this glob. Files that do not match any of these globs will be ignored.")
                .value_delimiter(',')
                .use_delimiter(true)
                .require_delimiter(true)
                .multiple_values(true)
        )
        .arg(
            Arg::new("glob_case_insensitive")
                .long("glob-case-insensitive")
                .help("Patterns passed to --glob and --oglob will be matched in a case-insentive way.")
        )
        // Index args that don't conflict with --index
        .arg(
            Arg::new("threads")
                .short('j')
                .long("threads")
                .takes_value(true)
                .validator(|a| a.parse::<usize>().map(|_|()).map_err(|_|"threads must be an unsigned integer".to_string()))
                .help("The approximate number of threads to use. A value of 0 (which is the default) will choose the thread count using heuristics.")
        )
        .arg(
            Arg::new("rebuild_index")
            .long("rebuild")
            .help("Force rebuild the index before searching")
        )

        // Search args
        .arg(
            Arg::new("limit")
                .long("limit")
                .takes_value(true)
                .validator(|a| a.parse::<usize>().map(|_|()).map_err(|_|"limit must be an unsigned integer".to_string()))
                .help("Maximum number of files to return"),
        )
        .arg(
            Arg::new("threshold")
                .long("threshold")
                .takes_value(true)
                .validator(|a| a.parse::<f32>().map(|_|()).map_err(|_|"threshold must be a floating point number".to_string()))
                .help("Minimum score threshold for results"),
        )
        .arg(
            Arg::new("json")
                .long("json")
                .conflicts_with("commands")
                .help("Print the results as json"),
        )
        .arg(
            Arg::new("files_with_matches")
                .short('l')
                .long("files-with-matches")
                .conflicts_with("commands")
                .help("Print out the files that match the search (not the matching lines)."),
        )
        .arg(
            Arg::new("no_ignore")
                .long("no-ignore")
                .help("Don't respect .gitignore files"),
        )
        .arg(
            Arg::new("color")
                .long("color")
                .takes_value(true)
                .possible_values(&["never", "auto", "always", "ansi"])
                .hide_possible_values(true)
                .help("This flag controls when to use colors. The default setting is auto, which will try to guess when to use colors.")
                .long_help("This flag controls when to use colors. The default setting is auto, which will try to guess when to use colors.
   The possible values for this flag are:

       never    Colors will never be used.
       auto     Auto-detect if the terminal supports colors (default).
       always   Colors will always be used regardless of where output is sent.
       ansi     Like 'always', but emits ANSI escapes (even in a Windows console).")
        )
        .group(
            ArgGroup::new("commands")
             .args(&["files", "indexes", "delete"])
            )
        .arg(
            Arg::new("files")
                .long("files")
                .help("Print out the files that would be searched (do not perform the search)"),
        )
        .arg(
            Arg::new("indexes")
                .long("indexes")
                .help("print out the indexes that would be used (do not perform the search)")
        )
        .arg(
            Arg::new("delete")
                .long("delete")
                .help("Delete the cached index files for the directory (if any)")
        )
        .arg(Arg::new("query"))
        .arg(Arg::new("dir"))
        .get_matches();

    let mut index = IndexConfigOpt::default();
    // Parse index options
    if matches.is_present("hidden") {
        index.hidden = Some(true);
    } else if matches.is_present("no_hidden") {
        index.hidden = Some(false);
    }
    if matches.is_present("language") {
        index.language = Some(language_from_str(matches.value_of("language").unwrap()).unwrap());
    }
    if matches.is_present("follow_links") {
        index.follow = Some(true);
    } else if matches.is_present("no_follow_links") {
        index.follow = Some(false);
    }
    if matches.is_present("no_ignore") {
        index.ignore_files = Some(false);
    }
    if matches.is_present("glob_case_insensitive") {
        index.glob_case_insensitive = Some(true);
    }
    if matches.is_present("glob") {
        index.glob = Some(
            matches
                .values_of("glob")
                .unwrap()
                .into_iter()
                .map(|s| s.to_string())
                .collect(),
        );
    }
    if matches.is_present("oglob") {
        index.oglob = Some(
            matches
                .values_of("oglob")
                .unwrap()
                .into_iter()
                .map(|s| s.to_string())
                .collect(),
        );
    }
    if matches.is_present("threads") {
        index.threads = Some(matches.value_of("threads").unwrap().parse::<usize>()?);
    }
    if matches.is_present("in_memory") {
        index.in_memory = Some(true);
    } else if matches.is_present("no_memory") {
        index.in_memory = Some(false);
    }

    // Parse search options
    let mut search = SearchConfigOpt::default();
    if matches.is_present("json") {
        search.json = Some(true);
    }
    if matches.is_present("limit") {
        search.limit = Some(matches.value_of("limit").unwrap().parse::<usize>()?);
    }
    if matches.is_present("threshold") {
        search.threshold = Some(matches.value_of("threshold").unwrap().parse::<f32>()?);
    }
    if matches.is_present("files_with_matches") {
        search.filename_only = Some(true);
    }
    if matches.is_present("color") {
        let preference = matches.value_of("color").unwrap_or("auto");
        search.color = Some(ColorMode::from_str(preference).unwrap());
    }
    if matches.is_present("rebuild_index") {
        search.rebuild_index = Some(true);
    }
    if matches.is_present("no_update") {
        search.update = Some(false);
    } else if matches.is_present("update") {
        search.update = Some(true);
    };

    let mut command = CmdArg::Search;
    if matches.is_present("delete") {
        command = CmdArg::Delete;
    } else if matches.is_present("files") {
        command = CmdArg::ListFiles;
    } else if matches.is_present("indexes") {
        command = CmdArg::ListIndex;
    }
    let search_dir = matches.value_of("dir").unwrap_or("").to_string();
    let query_path = if search_dir.is_empty() {
        env::current_dir()?
    } else {
        fs::canonicalize(Path::new(&search_dir))?
    };

    return Ok(GlobalConfig {
        index,
        search,
        command,
        query: matches.value_of("query").map(|s| s.to_string()),
        query_path,
        search_dir,
        index_name: matches.value_of("index").map(|s| s.to_string()),
    });
}

fn language_from_str(string: &str) -> Option<Language> {
    return match string.to_lowercase().as_str() {
        "arabic" => Some(Language::Arabic),
        "danish" => Some(Language::Danish),
        "dutch" => Some(Language::Dutch),
        "english" => Some(Language::English),
        "finnish" => Some(Language::Finnish),
        "french" => Some(Language::French),
        "german" => Some(Language::German),
        "greek" => Some(Language::Greek),
        "hungarian" => Some(Language::Hungarian),
        "italian" => Some(Language::Italian),
        "norwegian" => Some(Language::Norwegian),
        "portuguese" => Some(Language::Portuguese),
        "romanian" => Some(Language::Romanian),
        "russian" => Some(Language::Russian),
        "spanish" => Some(Language::Spanish),
        "swedish" => Some(Language::Swedish),
        "tamil" => Some(Language::Tamil),
        "turkish" => Some(Language::Turkish),
        _ => None,
    };
}
