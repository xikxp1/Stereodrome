use std::path::Path;

use tantivy::{
    collector::TopDocs,
    directory::MmapDirectory,
    query::QueryParser,
    schema::{
        Field, IndexRecordOption, Schema, TextFieldIndexing, TextOptions, Value, STORED, STRING,
    },
    tokenizer::{LowerCaser, NgramTokenizer, TextAnalyzer},
    Index, IndexReader, IndexSettings, IndexWriter, TantivyDocument,
};

use crate::error::{AppError, AppResult};

const NGRAM_TOKENIZER: &str = "ngram3";

/// Data for indexing an artist
pub struct ArtistIndexData {
    pub id: String,
    pub name: String,
    pub album_count: i32,
}

/// Data for indexing an album
pub struct AlbumIndexData {
    pub id: String,
    pub name: String,
    pub artist_name: String,
    pub year: Option<i32>,
    pub song_count: i32,
}

/// Data for indexing a song
pub struct SongIndexData {
    pub id: String,
    pub title: String,
    pub artist_name: String,
    pub album_name: String,
    pub genre: Option<String>,
    pub year: Option<i32>,
    pub duration: Option<i32>,
}

/// Search result from Tantivy
#[derive(Debug)]
pub struct SearchHit {
    pub id: String,
    pub entity_type: String,
    pub name: String,
    pub artist_name: Option<String>,
    pub album_name: Option<String>,
    pub year: Option<i64>,
    pub duration: Option<i64>,
    pub song_count: Option<i64>,
    pub album_count: Option<i64>,
}

/// Schema field handles for the search index
pub struct SearchSchema {
    pub id: Field,
    pub entity_type: Field,
    pub name: Field,
    pub artist_name: Field,
    pub album_name: Field,
    pub genre: Field,
    pub year: Field,
    pub duration: Field,
    pub song_count: Field,
    pub album_count: Field,
    schema: Schema,
}

impl SearchSchema {
    pub fn new() -> Self {
        let mut builder = Schema::builder();

        // Configure ngram text field for substring matching
        let text_indexing = TextFieldIndexing::default()
            .set_tokenizer(NGRAM_TOKENIZER)
            .set_index_option(IndexRecordOption::WithFreqsAndPositions);
        let text_options = TextOptions::default()
            .set_indexing_options(text_indexing)
            .set_stored();

        // Non-stored ngram field (for genre - we don't need to retrieve it)
        let text_indexing_no_store = TextFieldIndexing::default()
            .set_tokenizer(NGRAM_TOKENIZER)
            .set_index_option(IndexRecordOption::WithFreqsAndPositions);
        let text_options_no_store =
            TextOptions::default().set_indexing_options(text_indexing_no_store);

        // STRING = indexed but not tokenized, STORED = retrievable
        let id = builder.add_text_field("id", STRING | STORED);
        let entity_type = builder.add_text_field("entity_type", STRING | STORED);
        let name = builder.add_text_field("name", text_options.clone());
        let artist_name = builder.add_text_field("artist_name", text_options.clone());
        let album_name = builder.add_text_field("album_name", text_options);
        let genre = builder.add_text_field("genre", text_options_no_store);
        let year = builder.add_i64_field("year", STORED);
        let duration = builder.add_i64_field("duration", STORED);
        let song_count = builder.add_i64_field("song_count", STORED);
        let album_count = builder.add_i64_field("album_count", STORED);

        Self {
            id,
            entity_type,
            name,
            artist_name,
            album_name,
            genre,
            year,
            duration,
            song_count,
            album_count,
            schema: builder.build(),
        }
    }

    pub fn schema(&self) -> &Schema {
        &self.schema
    }
}

impl Default for SearchSchema {
    fn default() -> Self {
        Self::new()
    }
}

/// Manages the Tantivy search index
pub struct IndexManager {
    index: Index,
    reader: IndexReader,
    schema: SearchSchema,
    query_parser: QueryParser,
}

impl IndexManager {
    /// Create or open an index at the given path
    pub fn new(index_path: &Path) -> AppResult<Self> {
        let schema = SearchSchema::new();

        // Create directory if it doesn't exist
        std::fs::create_dir_all(index_path)?;

        // Always create fresh index to ensure schema matches
        // Delete existing index files if any
        if index_path.exists() {
            for entry in std::fs::read_dir(index_path)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_file() {
                    std::fs::remove_file(&path)?;
                }
            }
        }

        let dir = MmapDirectory::open(index_path)
            .map_err(|e| AppError::Search(format!("Failed to open index dir: {}", e)))?;
        let index = Index::create(dir, schema.schema().clone(), IndexSettings::default())
            .map_err(|e| AppError::Search(format!("Failed to create index: {}", e)))?;

        // Register ngram tokenizer for substring matching (min 2, max 8 chars)
        let ngram_tokenizer = TextAnalyzer::builder(NgramTokenizer::new(2, 8, false).unwrap())
            .filter(LowerCaser)
            .build();
        index
            .tokenizers()
            .register(NGRAM_TOKENIZER, ngram_tokenizer);

        let reader = index
            .reader()
            .map_err(|e| AppError::Search(format!("Failed to create reader: {}", e)))?;

        // Pre-create query parser for search fields
        let mut query_parser = QueryParser::for_index(
            &index,
            vec![
                schema.name,
                schema.artist_name,
                schema.album_name,
                schema.genre,
            ],
        );
        // Be lenient with query parsing errors
        query_parser.set_conjunction_by_default();

        Ok(Self {
            index,
            reader,
            schema,
            query_parser,
        })
    }

    /// Rebuild the entire index from scratch with provided data
    pub fn rebuild_index(
        &self,
        artists: &[ArtistIndexData],
        albums: &[AlbumIndexData],
        songs: &[SongIndexData],
    ) -> AppResult<()> {
        // Create writer with 50MB buffer
        let mut writer: IndexWriter = self
            .index
            .writer(50_000_000)
            .map_err(|e| AppError::Search(format!("Failed to create writer: {}", e)))?;

        // Delete all existing documents
        writer
            .delete_all_documents()
            .map_err(|e| AppError::Search(format!("Failed to delete existing documents: {}", e)))?;

        eprintln!(
            "Indexing {} artists, {} albums, {} songs...",
            artists.len(),
            albums.len(),
            songs.len()
        );

        // Index artists
        for artist in artists {
            let mut doc = TantivyDocument::new();
            doc.add_text(self.schema.id, &artist.id);
            doc.add_text(self.schema.entity_type, "artist");
            doc.add_text(self.schema.name, &artist.name);
            doc.add_i64(self.schema.album_count, artist.album_count as i64);
            writer
                .add_document(doc)
                .map_err(|e| AppError::Search(format!("Failed to add artist: {}", e)))?;
        }

        // Index albums
        for album in albums {
            let mut doc = TantivyDocument::new();
            doc.add_text(self.schema.id, &album.id);
            doc.add_text(self.schema.entity_type, "album");
            doc.add_text(self.schema.name, &album.name);
            doc.add_text(self.schema.artist_name, &album.artist_name);
            if let Some(year) = album.year {
                doc.add_i64(self.schema.year, year as i64);
            }
            doc.add_i64(self.schema.song_count, album.song_count as i64);
            writer
                .add_document(doc)
                .map_err(|e| AppError::Search(format!("Failed to add album: {}", e)))?;
        }

        // Index songs
        for song in songs {
            let mut doc = TantivyDocument::new();
            doc.add_text(self.schema.id, &song.id);
            doc.add_text(self.schema.entity_type, "song");
            doc.add_text(self.schema.name, &song.title);
            doc.add_text(self.schema.artist_name, &song.artist_name);
            doc.add_text(self.schema.album_name, &song.album_name);
            if let Some(ref genre) = song.genre {
                doc.add_text(self.schema.genre, genre);
            }
            if let Some(year) = song.year {
                doc.add_i64(self.schema.year, year as i64);
            }
            if let Some(duration) = song.duration {
                doc.add_i64(self.schema.duration, duration as i64);
            }
            writer
                .add_document(doc)
                .map_err(|e| AppError::Search(format!("Failed to add song: {}", e)))?;
        }

        // Commit changes atomically
        writer
            .commit()
            .map_err(|e| AppError::Search(format!("Failed to commit index: {}", e)))?;

        // Reload reader to see committed changes
        self.reader
            .reload()
            .map_err(|e| AppError::Search(format!("Failed to reload after commit: {}", e)))?;

        let doc_count = self.reader.searcher().num_docs();
        eprintln!(
            "Search index rebuilt: {} artists, {} albums, {} songs (total {} docs in index)",
            artists.len(),
            albums.len(),
            songs.len(),
            doc_count
        );

        Ok(())
    }

    /// Search the index and return matching hits
    pub fn search(&self, query_str: &str, limit: usize) -> AppResult<Vec<SearchHit>> {
        // Reload reader to pick up any committed changes
        self.reader
            .reload()
            .map_err(|e| AppError::Search(format!("Failed to reload reader: {}", e)))?;

        let searcher = self.reader.searcher();

        let query_str = query_str.trim().to_lowercase();
        if query_str.is_empty() {
            return Ok(Vec::new());
        }

        // With ngram tokenizer, we can search directly without wildcards
        // Filter out special query syntax characters
        let query_text: String = query_str
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c.is_whitespace() {
                    c
                } else {
                    ' '
                }
            })
            .collect();

        let query_text = query_text.trim();
        if query_text.is_empty() {
            return Ok(Vec::new());
        }

        eprintln!(
            "Search: query='{}', tantivy_query='{}', limit={}",
            query_str, query_text, limit
        );

        // Parse and execute query
        let query = self
            .query_parser
            .parse_query(query_text)
            .map_err(|e| AppError::Search(format!("Query parse error: {}", e)))?;

        eprintln!("Search: parsed query = {:?}", query);

        let top_docs = searcher
            .search(&query, &TopDocs::with_limit(limit))
            .map_err(|e| AppError::Search(format!("Search error: {}", e)))?;

        eprintln!(
            "Search: searcher has {} docs, found {} matches",
            searcher.num_docs(),
            top_docs.len()
        );

        let mut results = Vec::with_capacity(top_docs.len());

        for (_score, doc_address) in top_docs {
            let doc: TantivyDocument = searcher
                .doc(doc_address)
                .map_err(|e| AppError::Search(format!("Failed to retrieve doc: {}", e)))?;

            let get_text = |field: Field| -> Option<String> {
                doc.get_first(field)
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            };

            let get_i64 =
                |field: Field| -> Option<i64> { doc.get_first(field).and_then(|v| v.as_i64()) };

            if let (Some(id), Some(entity_type), Some(name)) = (
                get_text(self.schema.id),
                get_text(self.schema.entity_type),
                get_text(self.schema.name),
            ) {
                results.push(SearchHit {
                    id,
                    entity_type,
                    name,
                    artist_name: get_text(self.schema.artist_name),
                    album_name: get_text(self.schema.album_name),
                    year: get_i64(self.schema.year),
                    duration: get_i64(self.schema.duration),
                    song_count: get_i64(self.schema.song_count),
                    album_count: get_i64(self.schema.album_count),
                });
            }
        }

        Ok(results)
    }
}

/// Get the path for the search index directory
pub fn get_index_path(app_handle: &tauri::AppHandle) -> AppResult<std::path::PathBuf> {
    use tauri::Manager;

    let app_dir = app_handle.path().app_data_dir().map_err(|e| {
        AppError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            e.to_string(),
        ))
    })?;

    Ok(app_dir.join("search_index"))
}
