use std::{
    cmp::Reverse,
    collections::{BTreeMap, BinaryHeap, HashMap},
    fs::File,
    io::{self, BufRead},
    path::Path,
};

use tantivy::{
    query::Query, schema::IndexRecordOption, DocAddress, DocSet, LeasedItem, Postings, Searcher,
    TERMINATED,
};

use crate::{BytePositions, FileIndex, Line};

#[derive(Debug)]
pub struct DocResult {
    pub score: f32,
    pub address: DocAddress,
}

/// Get the position data for a query's search terms
///
/// The only way I've found to do this is to iterate through each of the index segments and look up
/// the docs for each of the query terms in that segment. For each doc, get the term position data
/// and save it.
///
/// TODO: this may not work well for FuzzyTermQuery or PhraseQuery. Needs testing.
///
/// This effectively amounts to a second full-index scan, doubling the performance cost of the
/// query (at least). A better way to do this would be to implement a custom Collector (and
/// possibly Weight and other traits) that keep track of term positions while the search query is
/// being performed.
pub fn get_search_results(
    index: &FileIndex,
    query: &Box<dyn Query>,
    searcher: &LeasedItem<Searcher>,
    results: &Vec<DocResult>,
) -> Result<HashMap<DocAddress, BytePositions>, anyhow::Error> {
    let mut position_map: HashMap<DocAddress, BytePositions> = HashMap::new();
    for result in results {
        position_map.insert(result.address, BinaryHeap::new());
    }
    let mut terms = BTreeMap::new();
    query.query_terms(&mut terms);
    // this buffer will be used to request for positions
    let mut positions: Vec<u32> = Vec::with_capacity(100);
    for (segment_ord, segment_reader) in searcher.segment_readers().iter().enumerate() {
        let inverted_index = segment_reader.inverted_index(*index.contents())?;
        for term in terms.keys() {
            if let Some(mut segment_postings) =
                inverted_index.read_postings(&term, IndexRecordOption::WithFreqsAndPositions)?
            {
                let mut doc_id = segment_postings.doc();
                while doc_id != TERMINATED {
                    // This MAY contains deleted documents as well.
                    if segment_reader.is_deleted(doc_id) {
                        doc_id = segment_postings.advance();
                        continue;
                    }

                    if let Some(position_data) = position_map.get_mut(&DocAddress {
                        segment_ord: segment_ord.try_into()?,
                        doc_id,
                    }) {
                        segment_postings.positions(&mut positions);
                        for pos in &positions {
                            position_data.push(Reverse(*pos));
                        }
                    }
                    doc_id = segment_postings.advance();
                }
            }
        }
    }

    Ok(position_map)
}

/// Converts token positions to lines of text
///
/// Tantivy stores position data, but that just means token offsets relative to other tokens in the
/// file. In order to find the actual lines of text that match a term, we have some work to do. At
/// the moment this process involves reading the file from disk and then tokenizing it line-by-line
/// as a means to recover the line-number-to-token-offset mapping.
///
/// At some point in the future it might be nice to modify Tantivy to *also* store byte offsets or
/// line offsets for the terms. It would generate larger indexes, but then we wouldn't have to
/// retokenize to recover the matched text.
pub fn positions_to_lines(
    index: &FileIndex,
    filepath: &Path,
    positions: &mut BytePositions,
    lines: &mut Vec<Line>,
) -> Result<(), anyhow::Error> {
    let tokenizer = index.index().tokenizer_for_field(*index.contents())?;
    if let Some(Reverse(mut next_pos)) = positions.peek() {
        let file = File::open(filepath)?;
        let mut reader = io::BufReader::new(file);
        let mut line = String::new();
        let mut line_no = 1;
        let mut num_tokens = 0;
        'outer: while let Ok(bytes) = reader.read_line(&mut line) {
            if bytes == 0 {
                break;
            }
            let mut line_tokens = 0;
            {
                let mut token_stream = tokenizer.token_stream(&line);
                while let Some(_) = token_stream.next() {
                    line_tokens += 1;
                }
            }
            if num_tokens <= next_pos && next_pos < num_tokens + line_tokens {
                lines.push(Line {
                    number: line_no,
                    text: line.trim_end().to_string(),
                });
                while next_pos < num_tokens + line_tokens {
                    match positions.pop() {
                        None => break 'outer,
                        Some(Reverse(pos)) => next_pos = pos,
                    };
                }
            }
            num_tokens += line_tokens;
            line.clear();
            line_no += 1;
        }
    }

    Ok(())
}
