#![cfg(feature = "experimental-ion-1-1")]
#![allow(dead_code)]

mod clause;
mod context;
mod continuation;
mod document;
mod fragment;
mod model;

use std::io::Read;
use std::path::{Path, PathBuf};

use ion_rs::{Element, IonError, IonStream, IonType, MapCatalog, SharedSymbolTable};

use clause::*;
use context::*;
use document::*;
use fragment::*;

#[allow(unused)]
pub(crate) mod prelude {
    pub(crate) use super::document::Document;
    pub(crate) use super::IonVersion;
    pub(crate) use super::TestCollection;
}

/// Specific errors used during parsing and test evaluation.
#[derive(Clone, Default, Debug, PartialEq)]
pub(crate) enum ConformanceErrorKind {
    #[default]
    UnknownError,
    IoError(std::io::ErrorKind),
    IonError(IonError),
    UnexpectedEndOfDocument,
    UnknownClause(String),
    ExpectedDocumentClause,
    ExpectedClause,
    ExpectedFragment,
    ExpectedExpectation,
    ExpectedModelValue,
    ExpectedFloatString,
    ExpectedAsciiCodepoint,
    ExpectedSymbolType,
    ExpectedInteger,
    ExpectedSignal(String),
    ExpectedString,
    ExpectedTimestampPrecision,
    ExpectedTimestampOffset,
    InvalidByte,
    InvalidHexString,
    MismatchedProduce,
    MismatchedDenotes,
    UnexpectedValue,
    UnknownVersion,
    UnexpectedContinuation,
}

impl From<std::io::Error> for ConformanceErrorKind {
    fn from(other: std::io::Error) -> Self {
        ConformanceErrorKind::IoError(other.kind())
    }
}

impl From<IonError> for ConformanceErrorKind {
    fn from(other: IonError) -> Self {
        ConformanceErrorKind::IonError(other)
    }
}

/// Error details for a user-facing error.
#[derive(Clone, Default, Debug)]
struct ConformanceErrorImpl {
    /// Path to the file containing the test.
    file: PathBuf,
    /// The document-level test name.
    test_name: String,
    /// The specific error kind.
    kind: ConformanceErrorKind,
}

#[derive(Clone, Default, Debug)]
pub struct ConformanceError(Box<ConformanceErrorImpl>);

impl From<ConformanceErrorImpl> for ConformanceError {
    fn from(inner: ConformanceErrorImpl) -> Self {
        ConformanceError(Box::new(inner))
    }
}

impl From<std::io::Error> for ConformanceError {
    fn from(other: std::io::Error) -> Self {
        ConformanceErrorImpl {
            kind: ConformanceErrorKind::IoError(other.kind()),
            ..Default::default()
        }
        .into()
    }
}

impl From<IonError> for ConformanceError {
    fn from(other: IonError) -> Self {
        ConformanceErrorImpl {
            kind: ConformanceErrorKind::IonError(other),
            ..Default::default()
        }
        .into()
    }
}

impl From<ConformanceErrorKind> for ConformanceError {
    fn from(other: ConformanceErrorKind) -> Self {
        ConformanceErrorImpl {
            kind: other,
            ..Default::default()
        }
        .into()
    }
}

/// Used for internal error handling.
type InnerResult<T> = std::result::Result<T, ConformanceErrorKind>;

/// Used for public conformance API error handling.
pub(crate) type Result<T> = std::result::Result<T, ConformanceError>;

/// Encoding captures whether an encoding is forced by including a text, or binary clause.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum IonEncoding {
    Text,        // Text clause used.
    Binary,      // Binary clause used.
    Unspecified, // No encoding specific clauses.
}

#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub(crate) enum IonVersion {
    #[default]
    Unspecified,
    V1_0,
    V1_1,
    V1_X,
}

/// A document-like is anything that matches the grammar of a document. Currently this includes
/// both Document, and Then clauses.
pub(crate) trait DocumentLike: Default {
    fn set_name(&mut self, name: &str);
    fn add_fragment(&mut self, frag: Fragment);
    fn set_continuation(&mut self, continuation: continuation::Continuation);
}

/// Parses a Clause that has the format of a Document clause. This includes, an optional name,
/// multiple fragments, followed by an expectation or multiple extensions.
pub(crate) fn parse_document_like<T: DocumentLike>(clause: &Clause) -> InnerResult<T> {
    // let clause: Clause = Clause::try_from(seq)?;
    let mut doc_like = T::default();
    let mut sequence_idx = 0;
    let mut continuation = continuation::Continuation::None;

    // We have an optional name as the second argument..
    if let Some(elem) = clause
        .body
        .first()
        .filter(|e| e.ion_type() == IonType::String)
    {
        if let Some(s) = elem.as_string() {
            doc_like.set_name(s);
        }
        sequence_idx += 1;
    }

    let mut handle_fragments = true;
    loop {
        if sequence_idx >= clause.body.len() {
            return Err(ConformanceErrorKind::ExpectedClause);
        }
        let element = clause.body.get(sequence_idx).expect("unwrapping element");
        if handle_fragments {
            let Some(seq) = element.as_sequence() else {
                return Err(ConformanceErrorKind::ExpectedClause);
            };
            let fragment = match Fragment::try_from(seq.clone()) {
                Ok(frag) => frag,
                Err(ConformanceErrorKind::ExpectedFragment) => {
                    handle_fragments = false;
                    continue;
                }
                Err(x) => return Err(x),
            };
            doc_like.add_fragment(fragment);
            sequence_idx += 1
        } else {
            let Some(seq) = element.as_sequence() else {
                return Err(ConformanceErrorKind::ExpectedClause);
            };

            let inner_clause: Clause = seq.clone().try_into().expect("unable to convert to clause");
            match continuation::parse_continuation(inner_clause) {
                Ok(c) => {
                    use continuation::Continuation::*;
                    continuation = match continuation {
                        None => c,
                        Each(..) | Then(..) => Extensions(vec![continuation, c]),
                        Extensions(ref mut exts) => {
                            exts.push(c.clone());
                            continuation
                        }
                        _ => {
                            // We cannot mix continuations and extensions
                            return Err(ConformanceErrorKind::UnexpectedContinuation);
                        }
                    };

                    // If we have a continuation already, we need to extend it to Extensions.
                    let expect_more =
                        continuation.is_extension() && (sequence_idx + 1) < clause.body.len();
                    if !expect_more {
                        doc_like.set_continuation(continuation);
                        break;
                    }

                    sequence_idx += 1;
                }
                Err(e) => return Err(e),
            }
        }
    }
    Ok(doc_like)
}

/// A collection of Tests, usually stored together in a file.
pub(crate) struct TestCollection {
    documents: Vec<Document>,
}

impl TestCollection {
    /// Loads a TestCollection from a file at the provided path.
    pub fn load<P: AsRef<Path>>(path: P) -> Result<TestCollection> {
        let test_file = std::fs::File::open(&path)?;
        match Self::load_from(test_file) {
            Err(e) => Err(ConformanceErrorImpl {
                file: path.as_ref().to_owned(),
                ..*e.0
            }
            .into()),
            Ok(t) => Ok(t),
        }
    }

    pub fn load_from<R: Read>(reader: R) -> Result<TestCollection> {
        let iter = Element::iter(IonStream::new(reader))?;
        let mut docs: Vec<Document> = vec![];

        for element in iter {
            let element = element?;
            match element.ion_type() {
                IonType::SExp => {
                    let seq = element.as_sexp().unwrap();
                    let doc = match Document::try_from(seq.clone()) {
                        Err(kind) => {
                            return Err(ConformanceErrorImpl {
                                kind,
                                ..Default::default()
                            }
                            .into())
                        }
                        Ok(doc) => doc,
                    };
                    docs.push(doc);
                }
                _ => todo!(),
            }
        }

        let collection = TestCollection { documents: docs };

        Ok(collection)
    }

    /// Evaluates the tests in all of the test documents contained in the collection.
    pub fn run(&self) -> Result<()> {
        for test in self.documents.iter() {
            test.run()?;
        }
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.documents.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Document> {
        self.documents.iter()
    }
}

/// Walks over all of the ion files contained in the ion-tests/catalog directory and extracts the
/// symbol tables from each. A Vec of the resulting SharedSymbolTables is returned.
pub(crate) fn build_ion_tests_symtables() -> Result<Vec<SharedSymbolTable>> {
    use std::{env, ffi::OsStr, fs};

    let mut catalog_dir = env::current_dir()?;
    catalog_dir.push("ion-tests/catalog");

    let mut symbol_tables = vec![];

    for entry in fs::read_dir(catalog_dir)? {
        let entry = entry?;
        let path = entry.path();

        if Some(OsStr::new("ion")) == path.extension() {
            let contents = fs::File::open(path)?;
            for element in Element::iter(contents)? {
                let element = element?;
                symbol_tables.push(element.try_into()?);
            }
        }
    }

    Ok(symbol_tables)
}

/// Parses a 'bytes*' expression. A bytes expression can be either an integer (0..255) or a string
/// containing hexadecimal digits (whitespace allowed). The `elems` provided should be all of the
/// arguments to be included in the bytes* expression.
pub(crate) fn parse_bytes_exp<'a, I: IntoIterator<Item = &'a Element>>(
    elems: I,
) -> InnerResult<Vec<u8>> {
    // Bytes can be of the form int (0..255), and a string containing hexadecimal digits.
    use std::result::Result;
    let mut bytes: Vec<u8> = vec![];
    for elem in elems.into_iter() {
        match elem.ion_type() {
            IonType::Int => match elem.as_i64() {
                Some(i) if (0..=255).contains(&i) => bytes.push(i as u8),
                _ => return Err(ConformanceErrorKind::InvalidByte),
            },
            IonType::String => {
                let hex = elem
                    .as_string()
                    .ok_or(ConformanceErrorKind::ExpectedString)?
                    .replace(" ", "");
                let hex_bytes = (0..hex.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
                    .collect::<Result<Vec<u8>, _>>();
                match hex_bytes {
                    Err(_) => return Err(ConformanceErrorKind::InvalidHexString),
                    Ok(v) => bytes.extend_from_slice(v.as_slice()),
                }
            }
            _ => return Err(ConformanceErrorKind::InvalidByte),
        }
    }
    Ok(bytes)
}

/// Parses a sequence of Elements that represent text data.
pub(crate) fn parse_text_exp<'a, I: IntoIterator<Item = &'a Element>>(
    elems: I,
) -> InnerResult<String> {
    let bytes: Vec<Vec<u8>> = elems
        .into_iter()
        .map(|v| match v.ion_type() {
            IonType::String => v.as_string().map(|s| Ok(s.as_bytes().to_vec())).unwrap(),
            IonType::Int => match v.as_i64() {
                Some(i) if i < 256 => Ok(vec![i as u8]),
                _ => Err(ConformanceErrorKind::ExpectedAsciiCodepoint),
            },
            _ => Err(ConformanceErrorKind::ExpectedModelValue),
        })
        .collect::<InnerResult<Vec<Vec<u8>>>>()?;

    let val_string = bytes
        .iter()
        .map(|v| unsafe { String::from_utf8_unchecked(v.to_vec()) })
        .collect();
    Ok(val_string)
}

/// Parses absent symbol notation from a symbol within a Toplevel fragment, or produces
/// Continuation. The notation is '#$<id>' for an absent symbol id, or '#$<name>#<id>' for a symbol
/// ID from a specific symbol table named 'name'.
pub(crate) fn parse_absent_symbol<T: AsRef<str>>(txt: T) -> (Option<String>, Option<usize>) {
    let txt = txt.as_ref();
    if let Some(id_txt) = txt.strip_prefix("#$") {
        let split_txt: Vec<&str> = id_txt.split('#').collect(); // format: '#$<name>#<id>' or '#$<id>'
        match split_txt.len() {
            1 => (
                None,
                split_txt[0].parse::<usize>().map(Some).unwrap_or(None),
            ),
            2 => (
                Some(split_txt[0].to_string()),
                split_txt[1].parse::<usize>().map(Some).unwrap_or(None),
            ),
            _ => panic!("invalid absent symbol notation"),
        }
    } else {
        (None, None)
    }
}

/// Given a LazyValue and an Element, this function will compare the two as symbols handling
/// parsing of the DSL's symbol notation and resolution.
pub(crate) fn compare_symbols_eq<D: ion_rs::Decoder>(
    ctx: &Context,
    actual: &ion_rs::LazyValue<'_, D>,
    expected: &Element,
) -> InnerResult<bool> {
    use ion_rs::ValueRef;

    if expected.ion_type() != IonType::Symbol || actual.ion_type() != IonType::Symbol {
        return Ok(false);
    }

    let expected_symbol = expected.as_symbol().unwrap(); // SAFETY: Tested above.
    let ValueRef::Symbol(actual_symbol_ref) = actual.read()? else {
        // SAFETY: Tested above.
        return Ok(false);
    };

    let (expected_symtab, expected_offset) =
        parse_absent_symbol(expected_symbol.text().unwrap_or(""));
    let (actual_symtab, actual_offset) =
        parse_absent_symbol(actual_symbol_ref.text().unwrap_or(""));

    let symbol_table = actual.symbol_table();

    // Extract the symbol text for the expected value.
    let expected_symbol_text = match (expected_symtab, expected_offset) {
        (None, None) => expected_symbol.text().map(|t| t.to_owned()),
        (None, Some(id)) => symbol_table.text_for(id).map(|t| t.to_owned()),
        (Some(symtab), Some(id)) => match ctx.get_symbol_from_table(symtab, id) {
            None => None,
            Some(shared_symbol) => shared_symbol.text().map(|t| t.to_owned()),
        },
        _ => unreachable!(), // Cannot have a symtab without an id.
    };

    // Extract the symbol text for the actual value.
    let actual_symbol_text = match (actual_symtab, actual_offset) {
        (None, None) => actual_symbol_ref.text().map(|t| t.to_owned()),
        (None, Some(id)) => symbol_table.text_for(id).map(|t| t.to_owned()),
        (Some(symtab), Some(id)) => match ctx.get_symbol_from_table(symtab, id) {
            None => None,
            Some(shared_symbol) => shared_symbol.text().map(|t| t.to_owned()),
        },
        _ => unreachable!(), // CAnnot have a symtab without an id.
    };

    Ok(expected_symbol_text == actual_symbol_text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_timestamps() {
        let tests: &[&str] = &[
            r#"(ion_1_1 "Timestamp Year" (text "2023T") (denotes (Timestamp year 2023)))"#,
            r#"(ion_1_1 "Timestamp Month" (text "2023-03T") (denotes (Timestamp month 2023 3)))"#,
            r#"(ion_1_1 "Timestamp Day" (text "2023-03-23T") (denotes (Timestamp day 2023 3 23)))"#,
            r#"(ion_1_1 "Timestamp Minute" (text "2023-03-23T10:12Z") (denotes (Timestamp minute 2023 3 23 (offset 0) 10 12)))"#,
            r#"(ion_1_1 "Timestamp Second" (text "2023-03-23T10:12:21Z") (denotes (Timestamp second 2023 3 23 (offset 0) 10 12 21))) "#,
            r#"(ion_1_1 "Timestamp Fractional" (text "2023-03-23T10:12:21.23Z") (denotes (Timestamp fraction 2023 3 23 (offset 0) 10 12 21 23 -2))) "#,
        ];

        for test in tests {
            Document::from_str(test)
                .unwrap_or_else(|e| panic!("Failed to load document: <<{test}>>\n{e:?}"))
                .run()
                .unwrap_or_else(|e| panic!("Test failed for simple doc: <<{test}>>\n{e:?}"));
        }
    }

    #[test]
    fn test_encoding() {
        let test: &str = r#"
             (ion_1_1
                 (encoding module _ (macro_table (macro m () 1)))
                 (text "(:m)")
                 (produces 1)
             )"#;
        Document::from_str(test)
            .unwrap_or_else(|e| panic!("Failed to load document:\n{e:?}"))
            .run()
            .unwrap_or_else(|e| panic!("Test failed: {e:?}"));
    }

    #[test]
    fn test_simple_docs() {
        let tests: &[&str] = &[
            "(document (produces ))",
            "(document (toplevel a) (produces a))",
            "(document (text \"a\") (produces a))",
            "(ion_1_0 (produces ))",
            "(ion_1_1 (produces ))",
            "(document (and (produces ) (produces )))",
            "(document (text \"a\") (not (and (produces b) (produces c))))",
            "(ion_1_1 (binary 0x60 0x61 0x01 0xEB 0x01) (produces 0 1 null.int))",
            r#"(ion_1_0 (then (text "a") (produces a)))"#,
            r#"(ion_1_1 (text "a") (text "b") (text "c") (produces a b c))"#,
            r#"(ion_1_1 (text "\"Hello\" null.int false") (denotes (String "Hello") (Null int) (Bool false)))"#,
            r#"(ion_1_1 (each
                             (text "0")
                             (binary 0x60)
                             (denotes (Int 0)))
                        )"#,
            r#"(document (ivm 1 2) (signals "Invalid Version"))"#,
            r#"(ion_1_1 (text "2.3") (denotes (Decimal 23 -1)))"#,
        ];
        for test in tests {
            println!("Testing: {test}");
            Document::from_str(test)
                .unwrap_or_else(|e| panic!("Failed to load document: <<{test}>>\n{e:?}"))
                .run()
                .unwrap_or_else(|e| panic!("Test failed for simple doc: <<{test}>>\n{e:?}"));
        }
    }

    #[test]
    fn test_eexp_transcription() {
        let tests: &[&str] = &[
            "(ion_1_1 (toplevel ('#$:make_list' (1 2))) (produces [1, 2]) )",
            "(ion_1_1 (mactab (macro twice (x*) (.values (%x) (%x)))) (toplevel ('#$:twice' foo)) (produces foo foo))",
            "(ion_1_1 (toplevel ('#$:make_list' (1 2) ('#$:make_list' (3 4)))) (produces [1, 2, 3, 4]) )",
            "(ion_1_1 (toplevel ('#$:values' ('#$::' 1 2 3))) (produces 1 2 3))",
            "(ion_1_1 (toplevel ('#$:values' ('#$::'))) (produces ))"
        ];

        for test in tests {
            println!("Testing: {test}");
            Document::from_str(test)
                .unwrap_or_else(|e| panic!("Failed to load document: <<{test}>>\n{e:?}"))
                .run()
                .unwrap_or_else(|e| panic!("Test failed for simple doc: <<{test}>>\n{e:?}"));
        }
    }

    #[test]
    fn test_then_clauses() {
        let test = r#"(ion_1_1 (mactab (macro foo () bar))
                 (then (text "(:foo)")
                       (produces bar))
                 (then (text "(:foo)")
                       (produces halb))
               )
            "#;
        println!("Testing: {test}");
        let doc = Document::from_str(test)
            .unwrap_or_else(|e| panic!("Failed to load document: <<{test}>>\n{e:?}"));
        println!("Document: {doc:?}");
        match doc.run() {
            Err(_) => (),
            Ok(_) => panic!("Unexpected successful test evaluation"),
        }
    }

    #[test]
    fn test_data_model_annot() {
        let tests: &[&str] = &[
            "(ion_1_1 (toplevel ('#$:make_list' (1 2))) (produces [1, 2]) )",
            "(ion_1_1 (mactab (macro twice (x*) (.values (%x) (%x)))) (toplevel ('#$:twice' foo)) (produces foo foo))",
            "(ion_1_1 (toplevel ('#$:make_list' (1 2) ('#$:make_list' (3 4)))) (produces [1, 2, 3, 4]) )",
            r#"(ion_1_1 (text "(:annotate (:: $ion) true)") (denotes (annot true $ion)))"#,
        ];

        for test in tests {
            println!("Testing: {test}");
            Document::from_str(test)
                .unwrap_or_else(|e| panic!("Failed to load document: <<{test}>>\n{e:?}"))
                .run()
                .unwrap_or_else(|e| panic!("Test failed for simple doc: <<{test}>>\n{e:?}"));
        }
    }

    #[test]
    fn test_symtab() {
        let source = r#"(ion_1_1 (symtab "a" "b") (text "$2") (produces b))"#;
        let doc = Document::from_str(source)
            .unwrap_or_else(|e| panic!("Failed to load document: <<{source}>>\n{e:?}"));
        println!("Document: {doc:?}");
        doc.run().expect("test document failed");
    }
}
