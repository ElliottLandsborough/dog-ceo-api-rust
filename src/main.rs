use axum::{
    Json, Router,
    extract::{Path, State},
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
    breed_images: Arc<BTreeMap<String, Vec<String>>>,
}

#[derive(Serialize)]
struct RandomImageResponse {
    message: String,
    status: &'static str,
}

#[derive(Serialize)]
struct RandomImagesResponse {
    message: Vec<String>,
    status: &'static str,
}

#[derive(Serialize)]
struct BreedListResponse {
    message: BTreeMap<String, Vec<String>>,
    status: &'static str,
}

#[derive(Serialize)]
struct ErrorResponse {
    message: String,
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

async fn random_images(
    Path(count): Path<usize>,
    State(state): State<AppState>,
) -> Json<RandomImagesResponse> {
    let capped = count.min(50);
    let mut rng = rand::thread_rng();
    let urls = state
        .urls
        .choose_multiple(&mut rng, capped)
        .cloned()
        .collect();

    Json(RandomImagesResponse {
        message: urls,
        status: "success",
    })
}

async fn list_all_breeds(State(state): State<AppState>) -> Json<BreedListResponse> {
    Json(BreedListResponse {
        message: (*state.breeds).clone(),
        status: "success",
    })
}

async fn breed_images_endpoint(
    Path(breed): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<RandomImagesResponse>, (StatusCode, Json<ErrorResponse>)> {
    let breed = breed.to_lowercase();

    match state.breed_images.get(&breed) {
        Some(images) if !images.is_empty() => Ok(Json(RandomImagesResponse {
            message: images.clone(),
            status: "success",
        })),
        _ => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                message: format!("Breed not found: {breed}"),
                status: "error",
            }),
        )),
    }
}

async fn random_breed_image_endpoint(
    Path(breed): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<RandomImageResponse>, (StatusCode, Json<ErrorResponse>)> {
    let breed = breed.to_lowercase();

    match state.breed_images.get(&breed) {
        Some(images) if !images.is_empty() => {
            let mut rng = rand::thread_rng();
            let image = images.choose(&mut rng).cloned().unwrap_or_default();

            Ok(Json(RandomImageResponse {
                message: image,
                status: "success",
            }))
        }
        _ => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                message: format!("Breed not found: {breed}"),
                status: "error",
            }),
        )),
    }
}

async fn random_breed_images_endpoint(
    Path((breed, count)): Path<(String, usize)>,
    State(state): State<AppState>,
) -> Result<Json<RandomImagesResponse>, (StatusCode, Json<ErrorResponse>)> {
    let breed = breed.to_lowercase();

    match state.breed_images.get(&breed) {
        Some(images) if !images.is_empty() => {
            let capped = count.min(50);
            let mut rng = rand::thread_rng();
            let selected = images
                .choose_multiple(&mut rng, capped)
                .cloned()
                .collect();

            Ok(Json(RandomImagesResponse {
                message: selected,
                status: "success",
            }))
        }
        _ => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                message: format!("Breed not found: {breed}"),
                status: "error",
            }),
        )),
    }
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

fn build_breed_image_map(paths: &[PathBuf]) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for path in paths {
        let path_str = path.to_string_lossy();
        let trimmed = path_str.strip_prefix("dog-api-images/").unwrap_or(&path_str);
        let Some(folder) = trimmed.split('/').next() else {
            continue;
        };

        if folder.is_empty() {
            continue;
        }

        let breed = folder
            .split_once('-')
            .map(|(breed, _)| breed)
            .unwrap_or(folder)
            .to_string();

        out.entry(breed).or_default().push(to_public_url(path));
    }

    out
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
    let breed_images = build_breed_image_map(&manifest_paths);
    println!("Loaded {} manifest entries", urls.len());
    println!("Loaded {} breeds", breeds.len());

    let state = AppState {
        urls: Arc::new(urls),
        breeds: Arc::new(breeds),
        breed_images: Arc::new(breed_images),
    };

    let app = Router::new()
        .route("/breeds/image/random/{count}", get(random_images))
        .route("/breeds/image/random", get(random_image))
        .route("/breeds/list/all", get(list_all_breeds))
        .route("/breed/{breed}/images", get(breed_images_endpoint))
        .route("/breed/{breed}/images/random", get(random_breed_image_endpoint))
        .route(
            "/breed/{breed}/images/random/{count}",
            get(random_breed_images_endpoint),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("failed to bind server");

    println!("Listening on http://0.0.0.0:3000");
    axum::serve(listener, app)
        .await
        .expect("server encountered an error");
}