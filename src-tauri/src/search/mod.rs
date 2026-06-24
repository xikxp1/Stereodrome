use std::path::Path;

use log::{debug, info, warn};
use tantivy::{
    Index, IndexReader, IndexSettings, IndexWriter, TantivyDocument, Term,
    collector::TopDocs,
    directory::MmapDirectory,
    query::{BooleanQuery, Occur, QueryParser, TermQuery},
    schema::{
        Field, IndexRecordOption, STORED, STRING, Schema, TextFieldIndexing, TextOptions, Value,
    },
    tokenizer::{LowerCaser, NgramTokenizer, TextAnalyzer},
};

use crate::error::{AppError, AppResult};

const NGRAM_TOKENIZER: &str = "ngram3";
const INDEX_WRITER_HEAP_SIZE_BYTES: usize = 50_000_000;

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

    /// Build SearchSchema from an existing tantivy Schema (for reopening indexes)
    pub fn from_existing(schema: &Schema) -> AppResult<Self> {
        let get_field = |name: &str| -> AppResult<Field> {
            schema
                .get_field(name)
                .map_err(|_| AppError::Search(format!("Schema mismatch: missing field '{}'", name)))
        };

        Ok(Self {
            id: get_field("id")?,
            entity_type: get_field("entity_type")?,
            name: get_field("name")?,
            artist_name: get_field("artist_name")?,
            album_name: get_field("album_name")?,
            genre: get_field("genre")?,
            year: get_field("year")?,
            duration: get_field("duration")?,
            song_count: get_field("song_count")?,
            album_count: get_field("album_count")?,
            schema: schema.clone(),
        })
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
        // Create directory if it doesn't exist
        std::fs::create_dir_all(index_path)?;

        // Try to open existing index first, fall back to creating fresh
        Self::try_open_existing(index_path).or_else(|e| {
            warn!("Could not open existing index: {}, creating fresh", e);
            Self::create_fresh(index_path)
        })
    }

    /// Try to open an existing index
    fn try_open_existing(index_path: &Path) -> AppResult<Self> {
        let dir = MmapDirectory::open(index_path)
            .map_err(|e| AppError::Search(format!("Failed to open index dir: {}", e)))?;

        let index = Index::open(dir)
            .map_err(|e| AppError::Search(format!("Failed to open existing index: {}", e)))?;

        // Build schema from the opened index's schema
        let existing_schema = index.schema();
        let schema = SearchSchema::from_existing(&existing_schema)?;

        // Register tokenizer
        let ngram_tokenizer = TextAnalyzer::builder(NgramTokenizer::new(2, 8, false).unwrap())
            .filter(LowerCaser)
            .build();
        index
            .tokenizers()
            .register(NGRAM_TOKENIZER, ngram_tokenizer);

        let reader = index
            .reader()
            .map_err(|e| AppError::Search(format!("Failed to create reader: {}", e)))?;

        let mut query_parser = QueryParser::for_index(
            &index,
            vec![
                schema.name,
                schema.artist_name,
                schema.album_name,
                schema.genre,
            ],
        );
        query_parser.set_conjunction_by_default();

        info!(
            "Opened existing search index at {:?} with {} docs",
            index_path,
            reader.searcher().num_docs()
        );

        Ok(Self {
            index,
            reader,
            schema,
            query_parser,
        })
    }

    /// Create a fresh index
    fn create_fresh(index_path: &Path) -> AppResult<Self> {
        info!("Creating fresh search index at {:?}", index_path);

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

        let schema = SearchSchema::new();

        let dir = MmapDirectory::open(index_path)
            .map_err(|e| AppError::Search(format!("Failed to open index dir: {}", e)))?;

        let index = Index::create(dir, schema.schema().clone(), IndexSettings::default())
            .map_err(|e| AppError::Search(format!("Failed to create index: {}", e)))?;

        // Register tokenizer
        let ngram_tokenizer = TextAnalyzer::builder(NgramTokenizer::new(2, 8, false).unwrap())
            .filter(LowerCaser)
            .build();
        index
            .tokenizers()
            .register(NGRAM_TOKENIZER, ngram_tokenizer);

        let reader = index
            .reader()
            .map_err(|e| AppError::Search(format!("Failed to create reader: {}", e)))?;

        let mut query_parser = QueryParser::for_index(
            &index,
            vec![
                schema.name,
                schema.artist_name,
                schema.album_name,
                schema.genre,
            ],
        );
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
        let mut writer: IndexWriter = self
            .index
            .writer(INDEX_WRITER_HEAP_SIZE_BYTES)
            .map_err(|e| AppError::Search(format!("Failed to create writer: {}", e)))?;

        // Delete all existing documents
        writer
            .delete_all_documents()
            .map_err(|e| AppError::Search(format!("Failed to delete existing documents: {}", e)))?;

        debug!(
            "Indexing {} artists, {} albums, {} songs...",
            artists.len(),
            albums.len(),
            songs.len()
        );

        for artist in artists {
            self.add_artist_document(&writer, artist)?;
        }

        for album in albums {
            self.add_album_document(&writer, album)?;
        }

        for song in songs {
            self.add_song_document(&writer, song)?;
        }

        self.commit_writer(&mut writer)?;

        let doc_count = self.reader.searcher().num_docs();
        info!(
            "Search index rebuilt: {} artists, {} albums, {} songs (total {} docs in index)",
            artists.len(),
            albums.len(),
            songs.len(),
            doc_count
        );

        Ok(())
    }

    /// Delete and re-add only the documents affected by an incremental sync.
    pub fn upsert_documents(
        &self,
        artists: &[ArtistIndexData],
        albums: &[AlbumIndexData],
        songs: &[SongIndexData],
    ) -> AppResult<()> {
        let mut writer: IndexWriter = self
            .index
            .writer(INDEX_WRITER_HEAP_SIZE_BYTES)
            .map_err(|e| AppError::Search(format!("Failed to create writer: {}", e)))?;

        for artist in artists {
            self.delete_document(&writer, "artist", &artist.id)?;
            self.add_artist_document(&writer, artist)?;
        }

        for album in albums {
            self.delete_document(&writer, "album", &album.id)?;
            self.add_album_document(&writer, album)?;
        }

        for song in songs {
            self.delete_document(&writer, "song", &song.id)?;
            self.add_song_document(&writer, song)?;
        }

        self.commit_writer(&mut writer)?;

        debug!(
            "Search index incrementally updated: {} artists, {} albums, {} songs",
            artists.len(),
            albums.len(),
            songs.len()
        );

        Ok(())
    }

    fn commit_writer(&self, writer: &mut IndexWriter) -> AppResult<()> {
        writer
            .commit()
            .map_err(|e| AppError::Search(format!("Failed to commit index: {}", e)))?;

        // Reload reader to see committed changes
        self.reader
            .reload()
            .map_err(|e| AppError::Search(format!("Failed to reload after commit: {}", e)))?;

        Ok(())
    }

    fn delete_document(&self, writer: &IndexWriter, entity_type: &str, id: &str) -> AppResult<()> {
        let delete_query = BooleanQuery::new(vec![
            (
                Occur::Must,
                Box::new(TermQuery::new(
                    Term::from_field_text(self.schema.entity_type, entity_type),
                    IndexRecordOption::Basic,
                )),
            ),
            (
                Occur::Must,
                Box::new(TermQuery::new(
                    Term::from_field_text(self.schema.id, id),
                    IndexRecordOption::Basic,
                )),
            ),
        ]);

        writer
            .delete_query(Box::new(delete_query))
            .map_err(|e| AppError::Search(format!("Failed to delete document: {}", e)))?;

        Ok(())
    }

    fn add_artist_document(&self, writer: &IndexWriter, artist: &ArtistIndexData) -> AppResult<()> {
        let mut doc = TantivyDocument::new();
        doc.add_text(self.schema.id, &artist.id);
        doc.add_text(self.schema.entity_type, "artist");
        doc.add_text(self.schema.name, &artist.name);
        doc.add_i64(self.schema.album_count, artist.album_count as i64);
        writer
            .add_document(doc)
            .map_err(|e| AppError::Search(format!("Failed to add artist: {}", e)))?;

        Ok(())
    }

    fn add_album_document(&self, writer: &IndexWriter, album: &AlbumIndexData) -> AppResult<()> {
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

        Ok(())
    }

    fn add_song_document(&self, writer: &IndexWriter, song: &SongIndexData) -> AppResult<()> {
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

        debug!(
            "Search: query='{}', tantivy_query='{}', limit={}",
            query_str, query_text, limit
        );

        // Parse and execute query
        let query = self
            .query_parser
            .parse_query(query_text)
            .map_err(|e| AppError::Search(format!("Query parse error: {}", e)))?;

        debug!("Search: parsed query = {:?}", query);

        let top_docs = searcher
            .search(&query, &TopDocs::with_limit(limit).order_by_score())
            .map_err(|e| AppError::Search(format!("Search error: {}", e)))?;

        debug!(
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
pub fn get_index_path(app_handle: &crate::runtime::AppHandle) -> AppResult<std::path::PathBuf> {
    use tauri::Manager;

    let app_dir = app_handle.path().app_data_dir().map_err(|e| {
        AppError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            e.to_string(),
        ))
    })?;

    Ok(app_dir.join("search_index"))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{AlbumIndexData, ArtistIndexData, IndexManager};

    fn temp_index_path(test_name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "stereodrome-search-{test_name}-{}",
            uuid::Uuid::new_v4()
        ))
    }

    #[test]
    fn incremental_upsert_replaces_only_matching_entity_type() {
        let index_path = temp_index_path("upsert");
        let manager = IndexManager::new(&index_path).expect("create index");

        manager
            .rebuild_index(
                &[ArtistIndexData {
                    id: "shared-id".to_string(),
                    name: "Artist Stable".to_string(),
                    album_count: 1,
                }],
                &[AlbumIndexData {
                    id: "shared-id".to_string(),
                    name: "Album Old".to_string(),
                    artist_name: "Artist Stable".to_string(),
                    year: Some(2001),
                    song_count: 10,
                }],
                &[],
            )
            .expect("seed index");

        manager
            .upsert_documents(
                &[],
                &[AlbumIndexData {
                    id: "shared-id".to_string(),
                    name: "Album New".to_string(),
                    artist_name: "Artist Stable".to_string(),
                    year: Some(2001),
                    song_count: 10,
                }],
                &[],
            )
            .expect("upsert album");

        let artist_hits = manager.search("artist stable", 10).expect("search artist");
        assert!(
            artist_hits
                .iter()
                .any(|hit| hit.entity_type == "artist" && hit.name == "Artist Stable")
        );

        let old_album_hits = manager.search("album old", 10).expect("search old album");
        assert!(
            !old_album_hits
                .iter()
                .any(|hit| hit.entity_type == "album" && hit.name == "Album Old")
        );

        let new_album_hits = manager.search("album new", 10).expect("search new album");
        assert!(
            new_album_hits
                .iter()
                .any(|hit| hit.entity_type == "album" && hit.name == "Album New")
        );

        let _ = fs::remove_dir_all(index_path);
    }
}
