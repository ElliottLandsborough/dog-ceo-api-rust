use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::get,
};
use rand::seq::SliceRandom;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

// Embedded at compile time. File is no longer needed at runtime.
const MANIFEST_BYTES: &[u8] = include_bytes!("../manifest.nul");

fn parse_manifest(bytes: &[u8]) -> Vec<PathBuf> {
    bytes
        .split(|b| *b == 0)
        .filter(|chunk| !chunk.is_empty())
        .map(bytes_to_path)
        .collect()
}

#[derive(Clone)]
struct AppState {
    urls: Arc<Vec<String>>,
    breeds: Arc<BTreeMap<String, Vec<String>>>,
}

#[derive(Serialize)]
struct RandomImageResponse {
    message: String,
    status: &'static str,
}

#[derive(Serialize)]
struct BreedListResponse {
    message: BTreeMap<String, Vec<String>>,
    status: &'static str,
}

async fn random_image(
    State(state): State<AppState>,
) -> Result<Json<RandomImageResponse>, (StatusCode, Json<RandomImageResponse>)> {
    let mut rng = rand::thread_rng();

    let Some(url) = state.urls.choose(&mut rng).cloned() else {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(RandomImageResponse {
                message: "No images available".to_string(),
                status: "error",
            }),
        ));
    };

    Ok(Json(RandomImageResponse {
        message: url,
        status: "success",
    }))
}

async fn list_all_breeds(State(state): State<AppState>) -> Json<BreedListResponse> {
    Json(BreedListResponse {
        message: (*state.breeds).clone(),
        status: "success",
    })
}

fn to_public_url(path: &PathBuf) -> String {
    let path_str = path.to_string_lossy();
    let trimmed = path_str.strip_prefix("dog-api-images/").unwrap_or(&path_str);
    format!("https://images.dog.ceo/breeds/{trimmed}")
}

fn build_breed_map(paths: &[PathBuf]) -> BTreeMap<String, Vec<String>> {
    let mut temp: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for path in paths {
        let path_str = path.to_string_lossy();
        let trimmed = path_str.strip_prefix("dog-api-images/").unwrap_or(&path_str);
        let Some(folder) = trimmed.split('/').next() else {
            continue;
        };

        if folder.is_empty() {
            continue;
        }

        if let Some((breed, subbreed)) = folder.split_once('-') {
            temp.entry(breed.to_string())
                .or_default()
                .insert(subbreed.to_string());
        } else {
            temp.entry(folder.to_string()).or_default();
        }
    }

    temp.into_iter()
        .map(|(breed, subbreeds)| (breed, subbreeds.into_iter().collect()))
        .collect()
}

#[cfg(unix)]
fn bytes_to_path(bytes: &[u8]) -> PathBuf {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(OsStr::from_bytes(bytes))
}

#[cfg(not(unix))]
fn bytes_to_path(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

#[tokio::main]
async fn main() {
    let manifest_paths = parse_manifest(MANIFEST_BYTES);
    let urls: Vec<String> = manifest_paths.iter().map(to_public_url).collect();
    let breeds = build_breed_map(&manifest_paths);
    println!("Loaded {} manifest entries", urls.len());
    println!("Loaded {} breeds", breeds.len());

    let state = AppState {
        urls: Arc::new(urls),
        breeds: Arc::new(breeds),
    };

    let app = Router::new()
        .route("/breeds/image/random", get(random_image))
        .route("/breeds/list/all", get(list_all_breeds))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("failed to bind server");

    println!("Listening on http://0.0.0.0:3000");
    axum::serve(listener, app)
        .await
        .expect("server encountered an error");
}