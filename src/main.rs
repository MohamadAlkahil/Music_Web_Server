use axum::{
    Router,
    extract::{Json, Path, Query, State},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::Mutex;

// the struct to be used to keep track of the get request site visit count as well as the connection pool for the database
struct AppState {
    db: SqlitePool,
    site_visit_count: Mutex<u128>,
}

// the struct to be used to represent songs for requests
#[derive(Serialize, Deserialize, Debug, sqlx::FromRow)]
struct Song {
    #[serde(skip_deserializing)]
    id: Option<i64>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    artist: Option<String>,
    #[serde(default)]
    genre: Option<String>,
    #[serde(skip_deserializing)]
    play_count: Option<i64>,
}

#[tokio::main]
async fn main() {
    // configure the sqllite connection
    let opts = SqliteConnectOptions::from_str("sqlite://data.db")
        .unwrap()
        .create_if_missing(true);
    // the connection pool
    let pool = SqlitePool::connect_with(opts).await.unwrap();

    // create the table if it does not exist
    let _ = sqlx::query(
        "CREATE TABLE IF NOT EXISTS songs(
        id INTEGER PRIMARY KEY ASC,
        title TEXT NOT NULL,
        artist TEXT NOT NULL,
        genre TEXT NOT NULL,
        play_count INTEGER DEFAULT 0

    )",
    )
    .execute(&pool)
    .await;

    // the state to be used by all requests
    let state = Arc::new(AppState {
        db: pool,
        site_visit_count: Mutex::new(0u128),
    });
    // the different routes the server handles
    let app = Router::new()
        .route("/", get(welcome))
        .route("/count", get(increment_count))
        .route("/songs/new", post(add_song))
        .route("/songs/search", get(search_song))
        .route("/songs/play/{id}", get(play_song))
        .with_state(state);

    // listen for any requests
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8080")
        .await
        .unwrap();

    println!("The server is currently listening on localhost:8080.");
    axum::serve(listener, app).await.unwrap();
}

/*
Breif Explanation: prints the response for the basic / get request

Parameters:
    NA

Returns:
    String - the basic welcome to the server response
*/
async fn welcome() -> String {
    String::from("Welcome to the Rust-powered web server!")
}

/*
Breif Explanation: prints the number of calls to the get count request

Parameters:
    state: Arc<AppState> - the shared app state that contains the mutex used to keep track of the number of calls made to the /count get request

Returns:
    String - the number of of calls made to the get count request
*/
async fn increment_count(State(state): State<Arc<AppState>>) -> String {
    // get the lock
    let mut inc_count = state.site_visit_count.lock().await;
    // increment the site visit count
    *inc_count += 1;
    format!("Visit count: {}", inc_count)
}

/*
Breif Explanation: adds a new song to the database

Parameters:
    state: Arc<AppState> - the shared app state that contains the pool used to connect to the database
    payload: Json<Song> - deseralize the json request body into Song Struct
Returns:
    Response - seralize the song instance into json to be sent to client as response or return "failed to add song" as json
*/
async fn add_song(State(state): State<Arc<AppState>>, Json(payload): Json<Song>) -> Response {
    // get the connection pool
    let pool = &state.db;
    // send a query to database using the request body as values
    match sqlx::query_as::<_, Song>(
        "INSERT INTO songs(title, artist, genre) 
        VALUES (?, ?, ?)
        RETURNING id, title, artist, genre, play_count
    ",
    )
    .bind(&payload.title)
    .bind(&payload.artist)
    .bind(&payload.genre)
    // return zero or one row to be seralized into a song instance
    .fetch_optional(pool)
    .await
    {
        Ok(option) => match option {
            // convert song instance to json to be sent as a response
            Some(s) => Json(s).into_response(),
            // if zero rows were returned that means query was unsuccessful
            None => Json(("Failed to add song").to_string()).into_response(),
        },
        // some sqlx error occured so let the client know
        Err(e) => Json(format!("Failed to add song: {}", e)).into_response(),
    }
}

/*
Breif Explanation: searchs for a song in the database based on optional title, artist, and genre paramaters

Parameters:
    state: Arc<AppState> - the shared app state that contains the pool used to connect to the database
    params: Query<Song> - deseralize the request params into Song Struct
Returns:
    Response - seralize the vector of song instances into json to be sent to client as response or return "failed to add song" as json
*/
async fn search_song(State(state): State<Arc<AppState>>, Query(params): Query<Song>) -> Response {
    // get the connection pool
    let pool = &state.db;
    // set up if title or artist or genre will be used to query database
    let mut where_exprs: Vec<String> = Vec::new();
    if params.title.is_some() {
        // LOWER used to ensure case insensitive match
        where_exprs.push("LOWER(title) LIKE LOWER(?)".to_string());
    }
    if params.artist.is_some() {
        where_exprs.push("LOWER(artist) LIKE LOWER(?)".to_string());
    }
    if params.genre.is_some() {
        where_exprs.push("LOWER(genre) LIKE LOWER(?)".to_string());
    }
    // if vector is empty that means no valid parameters where passed
    let sql_stmt = if where_exprs.is_empty() {
        String::from("SELECT * FROM songs ")
    } else {
        format!(
            "
        SELECT * FROM songs
        WHERE {}",
            where_exprs.join(" AND ")
        )
    };
    // set up the query to be passed to database
    let mut query = sqlx::query_as::<_, Song>(&sql_stmt[..]);
    // bind the passed in params into the query
    if let Some(title) = params.title {
        // % used to complete wild card searches
        query = query.bind(format!("%{}%", title));
    }
    if let Some(artist) = params.artist {
        query = query.bind(format!("%{}%", artist));
    }
    if let Some(genre) = params.genre {
        query = query.bind(format!("%{}%", genre));
    }
    // return all rows that match to be seralized into a vec of song instances
    match query.fetch_all(pool).await {
        Ok(songs) => Json(songs).into_response(),
        // some sqlx error occured so let the client know
        Err(e) => Json(format!("Failed to add song: {}", e)).into_response(),
    }
}

/*
Breif Explanation: searchs for a song in the database based on song id and increments the play_count
Parameters:
    state: Arc<AppState> - the shared app state that contains the pool used to connect to the database
    song_id: Path<i64> - deseralize the song id from the path parameter
Returns:
    Response - seralize the song instance into json to be sent to client as response or return "error":"Song not found" as json
*/
async fn play_song(State(state): State<Arc<AppState>>, Path(song_id): Path<i64>) -> Response {
    // get the connection pool
    let pool = &state.db;
    // the query to update the play_count
    match sqlx::query_as::<_, Song>(
        "UPDATE songs
            SET play_count = play_count+1
            WHERE ID = ?
            RETURNING id, title, artist, genre, play_count",
    )
    .bind(song_id)
    .fetch_optional(pool)
    .await
    {
        Ok(option) => match option {
            // take the returned updated row from the query and convert song instance to json to be sent as a response
            Some(s) => Json(s).into_response(),
            // if zero rows were returned that means query was unsuccessful
            None => Json(json!({"error":"Song not found"})).into_response(),
        },
        // some sqlx error occured so let the client know
        Err(_) => Json(json!({"error":"Song not found"})).into_response(),
    }
}
