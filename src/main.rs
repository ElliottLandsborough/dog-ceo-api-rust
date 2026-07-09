use axum::{
    Json, Router,
    extract::{Path, Request, State},
    http::{StatusCode, header::CACHE_CONTROL},
    middleware::{self, Next},
    response::Response,
    routing::get,
};
use rand::prelude::IteratorRandom;
use rand::seq::SliceRandom;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::path::PathBuf;
use std::sync::Arc;

// Embedded at compile time. File is no longer needed at runtime.
const MANIFEST_BYTES: &[u8] = include_bytes!("../manifest.nul");
const MAX_BREED_SEGMENT_LEN: usize = 64;
const MAX_COUNT_INPUT_LEN: usize = 32;
const WORKER_THREADS_ENV: &str = "DOG_API_WORKER_THREADS";

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
    main_breeds: Arc<Vec<String>>,
    breed_images: Arc<BTreeMap<String, Vec<String>>>,
    sub_breed_images: Arc<BTreeMap<String, Vec<String>>>,
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
struct NotFoundWithCodeResponse {
    status: &'static str,
    message: &'static str,
    code: u16,
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
    Path(count): Path<String>,
    State(state): State<AppState>,
) -> Json<RandomImagesResponse> {
    let count = parse_count_or_default_one(&count);
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

async fn list_main_breeds(State(state): State<AppState>) -> Json<RandomImagesResponse> {
    Json(RandomImagesResponse {
        message: (*state.main_breeds).clone(),
        status: "success",
    })
}

async fn random_main_breed(
    State(state): State<AppState>,
) -> Result<Json<RandomImageResponse>, (StatusCode, Json<RandomImageResponse>)> {
    let mut rng = rand::thread_rng();

    let Some(breed) = state.main_breeds.choose(&mut rng).cloned() else {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(RandomImageResponse {
                message: "No breeds available".to_string(),
                status: "error",
            }),
        ));
    };

    Ok(Json(RandomImageResponse {
        message: breed,
        status: "success",
    }))
}

async fn random_main_breeds(
    Path(count): Path<String>,
    State(state): State<AppState>,
) -> Json<RandomImagesResponse> {
    let count = parse_count_or_default_one(&count);
    let capped = count.min(state.main_breeds.len());

    let mut rng = rand::thread_rng();
    let selected = state
        .main_breeds
        .choose_multiple(&mut rng, capped)
        .cloned()
        .collect();

    Json(RandomImagesResponse {
        message: selected,
        status: "success",
    })
}

async fn random_all_breeds(State(state): State<AppState>) -> Json<BreedListResponse> {
    let mut rng = rand::thread_rng();
    let mut out = BTreeMap::new();

    if let Some((breed, sub_breeds)) = state.breeds.iter().choose(&mut rng) {
        out.insert(breed.clone(), sub_breeds.clone());
    }

    Json(BreedListResponse {
        message: out,
        status: "success",
    })
}

async fn random_all_breeds_count(
    Path(count): Path<String>,
    State(state): State<AppState>,
) -> Json<BreedListResponse> {
    let count = parse_count_or_default_one(&count);
    let capped = count.min(state.breeds.len());

    let mut rng = rand::thread_rng();
    let mut message = BTreeMap::new();
    for (breed, sub_breeds) in state.breeds.iter().choose_multiple(&mut rng, capped) {
        message.insert(breed.clone(), sub_breeds.clone());
    }

    Json(BreedListResponse {
        message,
        status: "success",
    })
}

fn parse_worker_threads(raw: Option<&str>, fallback: usize) -> usize {
    match raw {
        Some(value) => value.parse::<usize>().ok().filter(|v| *v > 0).unwrap_or(fallback),
        None => fallback,
    }
}

fn configured_worker_threads() -> usize {
    let fallback = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);

    parse_worker_threads(env::var(WORKER_THREADS_ENV).ok().as_deref(), fallback)
}

async fn cache_control_middleware(req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();
    let mut response = next.run(req).await;

    let cache_value = if path.contains("/random") {
        "no-store"
    } else if path.starts_with("/breed/") || path.starts_with("/breeds/") {
        "public, max-age=300, s-maxage=600, stale-while-revalidate=30"
    } else {
        "no-store"
    };

    response.headers_mut().insert(
        CACHE_CONTROL,
        cache_value.parse().expect("valid cache-control header"),
    );

    response
}

async fn breed_images_endpoint(
    Path(breed): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<RandomImagesResponse>, (StatusCode, Json<NotFoundWithCodeResponse>)> {
    let Some(breed) = normalize_breed_segment(&breed) else {
        return Err(main_breed_not_found());
    };

    match state.breed_images.get(&breed) {
        Some(images) if !images.is_empty() => Ok(Json(RandomImagesResponse {
            message: images.clone(),
            status: "success",
        })),
        _ => Err((
            StatusCode::NOT_FOUND,
            Json(NotFoundWithCodeResponse {
                status: "error",
                message: "Breed not found (main breed does not exist)",
                code: 404,
            }),
        )),
    }
}

async fn random_breed_image_endpoint(
    Path(breed): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<RandomImageResponse>, (StatusCode, Json<NotFoundWithCodeResponse>)> {
    let Some(breed) = normalize_breed_segment(&breed) else {
        return Err(main_breed_not_found());
    };

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
            Json(NotFoundWithCodeResponse {
                status: "error",
                message: "Breed not found (main breed does not exist)",
                code: 404,
            }),
        )),
    }
}

async fn random_breed_images_endpoint(
    Path((breed, count)): Path<(String, String)>,
    State(state): State<AppState>,
) -> Result<Json<RandomImagesResponse>, (StatusCode, Json<NotFoundWithCodeResponse>)> {
    let Some(breed) = normalize_breed_segment(&breed) else {
        return Err(main_breed_not_found());
    };
    let count = parse_count_or_default_one(&count);

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
            Json(NotFoundWithCodeResponse {
                status: "error",
                message: "Breed not found (main breed does not exist)",
                code: 404,
            }),
        )),
    }
}

async fn list_sub_breeds_endpoint(
    Path(breed): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<RandomImagesResponse>, (StatusCode, Json<NotFoundWithCodeResponse>)> {
    let Some(breed) = normalize_breed_segment(&breed) else {
        return Err(main_breed_not_found());
    };

    match state.breeds.get(&breed) {
        Some(sub_breeds) => Ok(Json(RandomImagesResponse {
            message: sub_breeds.clone(),
            status: "success",
        })),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(NotFoundWithCodeResponse {
                status: "error",
                message: "Breed not found (main breed does not exist)",
                code: 404,
            }),
        )),
    }
}

async fn random_sub_breed_endpoint(
    Path(breed): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<RandomImageResponse>, (StatusCode, Json<NotFoundWithCodeResponse>)> {
    let Some(breed) = normalize_breed_segment(&breed) else {
        return Err(main_breed_not_found());
    };

    match state.breeds.get(&breed) {
        Some(sub_breeds) if !sub_breeds.is_empty() => {
            let mut rng = rand::thread_rng();
            let sub_breed = sub_breeds.choose(&mut rng).cloned().unwrap_or_default();

            Ok(Json(RandomImageResponse {
                message: sub_breed,
                status: "success",
            }))
        }
        Some(_) => Err((
            StatusCode::NOT_FOUND,
            Json(NotFoundWithCodeResponse {
                status: "error",
                message: "Breed not found (no sub breeds exist for this main breed)",
                code: 404,
            }),
        )),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(NotFoundWithCodeResponse {
                status: "error",
                message: "Breed not found (main breed does not exist)",
                code: 404,
            }),
        )),
    }
}

async fn random_sub_breeds_endpoint(
    Path((breed, count)): Path<(String, String)>,
    State(state): State<AppState>,
) -> Result<Json<RandomImagesResponse>, (StatusCode, Json<NotFoundWithCodeResponse>)> {
    let Some(breed) = normalize_breed_segment(&breed) else {
        return Err(main_breed_not_found());
    };
    let count = parse_sub_breed_random_count(&count);

    match state.breeds.get(&breed) {
        Some(sub_breeds) if !sub_breeds.is_empty() => {
            let capped = count.min(sub_breeds.len());
            let mut rng = rand::thread_rng();
            let selected = sub_breeds
                .choose_multiple(&mut rng, capped)
                .cloned()
                .collect();

            Ok(Json(RandomImagesResponse {
                message: selected,
                status: "success",
            }))
        }
        Some(_) => Err((
            StatusCode::NOT_FOUND,
            Json(NotFoundWithCodeResponse {
                status: "error",
                message: "Breed not found (no sub breeds exist for this main breed)",
                code: 404,
            }),
        )),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(NotFoundWithCodeResponse {
                status: "error",
                message: "Breed not found (main breed does not exist)",
                code: 404,
            }),
        )),
    }
}

async fn sub_breed_images_endpoint(
    Path((breed, sub_breed)): Path<(String, String)>,
    State(state): State<AppState>,
) -> Result<Json<RandomImagesResponse>, (StatusCode, Json<NotFoundWithCodeResponse>)> {
    let Some(breed) = normalize_breed_segment(&breed) else {
        return Err(main_breed_not_found());
    };
    let Some(sub_breed) = normalize_breed_segment(&sub_breed) else {
        return Err(sub_breed_not_found());
    };
    let key = format!("{breed}/{sub_breed}");

    match state.sub_breed_images.get(&key) {
        Some(images) if !images.is_empty() => Ok(Json(RandomImagesResponse {
            message: images.clone(),
            status: "success",
        })),
        _ => Err((
            StatusCode::NOT_FOUND,
            Json(NotFoundWithCodeResponse {
                status: "error",
                message: "Breed not found (sub breed does not exist)",
                code: 404,
            }),
        )),
    }
}

async fn random_sub_breed_image_endpoint(
    Path((breed, sub_breed)): Path<(String, String)>,
    State(state): State<AppState>,
) -> Result<Json<RandomImageResponse>, (StatusCode, Json<NotFoundWithCodeResponse>)> {
    let Some(breed) = normalize_breed_segment(&breed) else {
        return Err(main_breed_not_found());
    };
    let Some(sub_breed) = normalize_breed_segment(&sub_breed) else {
        return Err(sub_breed_not_found());
    };
    let key = format!("{breed}/{sub_breed}");

    match state.sub_breed_images.get(&key) {
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
            Json(NotFoundWithCodeResponse {
                status: "error",
                message: "Breed not found (sub breed does not exist)",
                code: 404,
            }),
        )),
    }
}

async fn random_sub_breed_images_endpoint(
    Path((breed, sub_breed, count)): Path<(String, String, String)>,
    State(state): State<AppState>,
) -> Result<Json<RandomImagesResponse>, (StatusCode, Json<NotFoundWithCodeResponse>)> {
    let Some(breed) = normalize_breed_segment(&breed) else {
        return Err(main_breed_not_found());
    };
    let Some(sub_breed) = normalize_breed_segment(&sub_breed) else {
        return Err(sub_breed_not_found());
    };
    let count = parse_count_or_default_one(&count);
    let key = format!("{breed}/{sub_breed}");

    match state.sub_breed_images.get(&key) {
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
            Json(NotFoundWithCodeResponse {
                status: "error",
                message: "Breed not found (sub breed does not exist)",
                code: 404,
            }),
        )),
    }
}

async fn breed_info_endpoint(
    Path(breed): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<RandomImageResponse>, (StatusCode, Json<NotFoundWithCodeResponse>)> {
    let Some(breed) = normalize_breed_segment(&breed) else {
        return Err(main_breed_not_found());
    };

    if state.breeds.contains_key(&breed) {
        return Err((
            StatusCode::NOT_FOUND,
            Json(NotFoundWithCodeResponse {
                status: "error",
                message: "Breed not found (No info file for this breed exists)",
                code: 404,
            }),
        ));
    }

    Err(main_breed_not_found())
}

async fn sub_breed_info_endpoint(
    Path((breed, sub_breed)): Path<(String, String)>,
    State(state): State<AppState>,
) -> Result<Json<RandomImageResponse>, (StatusCode, Json<NotFoundWithCodeResponse>)> {
    let Some(breed) = normalize_breed_segment(&breed) else {
        return Err(main_breed_not_found());
    };
    let Some(sub_breed) = normalize_breed_segment(&sub_breed) else {
        return Err(sub_breed_not_found());
    };

    let Some(sub_breeds) = state.breeds.get(&breed) else {
        return Err(main_breed_not_found());
    };

    if !sub_breeds.iter().any(|s| s == &sub_breed) {
        return Err(sub_breed_not_found());
    }

    Err((
        StatusCode::NOT_FOUND,
        Json(NotFoundWithCodeResponse {
            status: "error",
            message: "Breed not found (No info file for this breed exists)",
            code: 404,
        }),
    ))
}

fn to_public_url(path: &PathBuf) -> String {
    let path_str = path.to_string_lossy();
    let trimmed = path_str.strip_prefix("dog-api-images/").unwrap_or(&path_str);
    format!("https://images.dog.ceo/breeds/{trimmed}")
}

fn parse_count_or_default_one(value: &str) -> usize {
    if value.len() > MAX_COUNT_INPUT_LEN {
        return 1;
    }

    match value.parse::<isize>() {
        Ok(parsed) if parsed > 0 => parsed as usize,
        _ => 1,
    }
}

fn parse_sub_breed_random_count(value: &str) -> usize {
    if value.len() > MAX_COUNT_INPUT_LEN {
        return 1;
    }

    match value.parse::<isize>() {
        Ok(parsed) if parsed > 0 => parsed as usize,
        Ok(parsed) if parsed < 0 => 10,
        _ => 1,
    }
}

fn normalize_breed_segment(input: &str) -> Option<String> {
    if input.is_empty() || input.len() > MAX_BREED_SEGMENT_LEN || !input.is_ascii() {
        return None;
    }

    let lowered = input.to_ascii_lowercase();

    if lowered.bytes().all(|b| b.is_ascii_lowercase()) {
        Some(lowered)
    } else {
        None
    }
}

fn main_breed_not_found() -> (StatusCode, Json<NotFoundWithCodeResponse>) {
    (
        StatusCode::NOT_FOUND,
        Json(NotFoundWithCodeResponse {
            status: "error",
            message: "Breed not found (main breed does not exist)",
            code: 404,
        }),
    )
}

fn sub_breed_not_found() -> (StatusCode, Json<NotFoundWithCodeResponse>) {
    (
        StatusCode::NOT_FOUND,
        Json(NotFoundWithCodeResponse {
            status: "error",
            message: "Breed not found (sub breed does not exist)",
            code: 404,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_count_or_default_one_behaves_as_expected() {
        assert_eq!(parse_count_or_default_one("1"), 1);
        assert_eq!(parse_count_or_default_one("7"), 7);
        assert_eq!(parse_count_or_default_one("0"), 1);
        assert_eq!(parse_count_or_default_one("-5"), 1);
        assert_eq!(parse_count_or_default_one("abc"), 1);
        assert_eq!(parse_count_or_default_one("1e10"), 1);

        let huge = "9".repeat(500);
        assert_eq!(parse_count_or_default_one(&huge), 1);
    }

    #[test]
    fn parse_sub_breed_random_count_behaves_as_expected() {
        assert_eq!(parse_sub_breed_random_count("1"), 1);
        assert_eq!(parse_sub_breed_random_count("12"), 12);
        assert_eq!(parse_sub_breed_random_count("0"), 1);
        assert_eq!(parse_sub_breed_random_count("-1"), 10);
        assert_eq!(parse_sub_breed_random_count("-999"), 10);
        assert_eq!(parse_sub_breed_random_count("abc"), 1);

        let huge = "9".repeat(500);
        assert_eq!(parse_sub_breed_random_count(&huge), 1);
    }

    #[test]
    fn normalize_breed_segment_accepts_and_normalizes_ascii_letters() {
        assert_eq!(normalize_breed_segment("hound"), Some("hound".to_string()));
        assert_eq!(normalize_breed_segment("HOUND"), Some("hound".to_string()));
        assert_eq!(normalize_breed_segment("German"), Some("german".to_string()));
    }

    #[test]
    fn normalize_breed_segment_rejects_invalid_inputs() {
        assert_eq!(normalize_breed_segment(""), None);
        assert_eq!(normalize_breed_segment("hound123"), None);
        assert_eq!(normalize_breed_segment("hound-afghan"), None);
        assert_eq!(normalize_breed_segment("hound/afghan"), None);
        assert_eq!(normalize_breed_segment("💥"), None);

        let too_long = "a".repeat(MAX_BREED_SEGMENT_LEN + 1);
        assert_eq!(normalize_breed_segment(&too_long), None);
    }

    #[test]
    fn not_found_helpers_return_expected_payloads() {
        let (main_status, Json(main_body)) = main_breed_not_found();
        assert_eq!(main_status, StatusCode::NOT_FOUND);
        assert_eq!(main_body.status, "error");
        assert_eq!(main_body.message, "Breed not found (main breed does not exist)");
        assert_eq!(main_body.code, 404);

        let (sub_status, Json(sub_body)) = sub_breed_not_found();
        assert_eq!(sub_status, StatusCode::NOT_FOUND);
        assert_eq!(sub_body.status, "error");
        assert_eq!(sub_body.message, "Breed not found (sub breed does not exist)");
        assert_eq!(sub_body.code, 404);
    }

    #[test]
    fn parse_worker_threads_uses_fallback_for_invalid_values() {
        assert_eq!(parse_worker_threads(None, 4), 4);
        assert_eq!(parse_worker_threads(Some(""), 4), 4);
        assert_eq!(parse_worker_threads(Some("0"), 4), 4);
        assert_eq!(parse_worker_threads(Some("-2"), 4), 4);
        assert_eq!(parse_worker_threads(Some("abc"), 4), 4);
    }

    #[test]
    fn parse_worker_threads_accepts_positive_values() {
        assert_eq!(parse_worker_threads(Some("1"), 4), 1);
        assert_eq!(parse_worker_threads(Some("8"), 4), 8);
    }
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

fn build_sub_breed_image_map(paths: &[PathBuf]) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for path in paths {
        let path_str = path.to_string_lossy();
        let trimmed = path_str.strip_prefix("dog-api-images/").unwrap_or(&path_str);
        let Some(folder) = trimmed.split('/').next() else {
            continue;
        };

        if let Some((breed, sub_breed)) = folder.split_once('-') {
            let key = format!("{}/{}", breed.to_lowercase(), sub_breed.to_lowercase());
            out.entry(key).or_default().push(to_public_url(path));
        }
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

async fn run_server() {
    let manifest_paths = parse_manifest(MANIFEST_BYTES);
    let urls: Vec<String> = manifest_paths.iter().map(to_public_url).collect();
    let breeds = build_breed_map(&manifest_paths);
    let main_breeds: Vec<String> = breeds.keys().cloned().collect();
    let breed_images = build_breed_image_map(&manifest_paths);
    let sub_breed_images = build_sub_breed_image_map(&manifest_paths);
    println!("Loaded {} manifest entries", urls.len());
    println!("Loaded {} breeds", breeds.len());

    let state = AppState {
        urls: Arc::new(urls),
        breeds: Arc::new(breeds),
        main_breeds: Arc::new(main_breeds),
        breed_images: Arc::new(breed_images),
        sub_breed_images: Arc::new(sub_breed_images),
    };

    let app = Router::new()
        .route("/breeds/list", get(list_main_breeds))
        .route("/breeds/list/random", get(random_main_breed))
        .route("/breeds/list/random/{count}", get(random_main_breeds))
        .route("/breeds/list/all/random", get(random_all_breeds))
        .route(
            "/breeds/list/all/random/{count}",
            get(random_all_breeds_count),
        )
        .route("/breeds/image/random/{count}", get(random_images))
        .route("/breeds/image/random", get(random_image))
        .route("/breeds/list/all", get(list_all_breeds))
        .route("/breed/{breed}/list", get(list_sub_breeds_endpoint))
        .route("/breed/{breed}/list/random", get(random_sub_breed_endpoint))
        .route(
            "/breed/{breed}/list/random/{count}",
            get(random_sub_breeds_endpoint),
        )
        .route(
            "/breed/{breed}/{sub_breed}/images",
            get(sub_breed_images_endpoint),
        )
        .route(
            "/breed/{breed}/{sub_breed}/images/random",
            get(random_sub_breed_image_endpoint),
        )
        .route(
            "/breed/{breed}/{sub_breed}/images/random/{count}",
            get(random_sub_breed_images_endpoint),
        )
        .route("/breed/{breed}/images", get(breed_images_endpoint))
        .route("/breed/{breed}/images/random", get(random_breed_image_endpoint))
        .route(
            "/breed/{breed}/images/random/{count}",
            get(random_breed_images_endpoint),
        )
        .route("/breed/{breed}/{sub_breed}", get(sub_breed_info_endpoint))
        .route("/breed/{breed}", get(breed_info_endpoint))
        .layer(middleware::from_fn(cache_control_middleware))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("failed to bind server");

    println!("Listening on http://0.0.0.0:3000");
    axum::serve(listener, app)
        .await
        .expect("server encountered an error");
}

fn main() {
    let worker_threads = configured_worker_threads();
    println!(
        "Starting runtime with {worker_threads} worker thread(s) (set {WORKER_THREADS_ENV} to override)"
    );

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(worker_threads)
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");

    runtime.block_on(run_server());
}