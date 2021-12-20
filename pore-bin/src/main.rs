#[macro_use]
extern crate simple_error;

use args::CmdArg;
use config::load_config;
use config::{IndexConfig, SearchConfig};
use ignore::WalkState;
use index::FileIndex;
use std::error;
use std::process;
use tantivy::query::QueryParser;

mod args;
mod color_mode;
mod config;
mod index;
mod output;

fn main() {
    match run_cmd() {
        Err(err) => {
            eprintln!("Error: {}", err);
            process::exit(2);
        }
        Ok(false) => {
            process::exit(1);
        }
        _ => {}
    }
}

fn run_cmd() -> Result<bool, Box<dyn error::Error>> {
    let conf = args::parse_args()?;
    let (mut index_opt, mut search_opt) =
        load_config(&conf.query_path, conf.index_name.as_deref())?;
    search_opt.merge_from(&conf.search);
    if conf.index_name.is_some() {
        if conf.index.any() {
            bail!("Cannot use those arguments with --index");
        }
    } else {
        index_opt.merge_from(&conf.index);
    }
    let index: IndexConfig = index_opt.into();
    let search: SearchConfig = search_opt.into();

    let mut index = FileIndex::get_or_create(&conf.query_path, &index, conf.index_name.as_deref())?;

    match conf.command {
        CmdArg::Delete => {
            index.delete()?;
            return Ok(true);
        }
        CmdArg::ListFiles => {
            let walker = index.get_file_walker()?;
            walker.build_parallel().run(|| {
                Box::new(|result| {
                    if let Ok(entry) = result {
                        println!("{}", entry.path().to_string_lossy());
                    }
                    WalkState::Continue
                })
            });
            return Ok(true);
        }
        CmdArg::ListIndex => {
            println!("{}", index);
            return Ok(true);
        }
        CmdArg::Search => {
            if search.update || search.rebuild_index {
                index.update(search.rebuild_index)?;
            }
            if let Some(query) = conf.query {
                let query_parser = QueryParser::for_index(&index.index(), vec![*index.contents()]);
                let query = query_parser.parse_query(&query)?;
                let (searcher, results) = index.search(&query, &search)?;
                return output::print_results(
                    &conf.search_dir,
                    index,
                    &query,
                    searcher,
                    results,
                    &search,
                );
            } else {
                return Ok(true);
            }
        }
    }
}
