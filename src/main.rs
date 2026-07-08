use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::get,
};
use rand::prelude::IteratorRandom;
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
    let selected: Vec<(String, Vec<String>)> = state
        .breeds
        .iter()
        .choose_multiple(&mut rng, capped)
        .into_iter()
        .map(|(breed, sub_breeds)| (breed.clone(), sub_breeds.clone()))
        .collect();

    let message = selected.into_iter().collect();

    Json(BreedListResponse {
        message,
        status: "success",
    })
}

async fn breed_images_endpoint(
    Path(breed): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<RandomImagesResponse>, (StatusCode, Json<NotFoundWithCodeResponse>)> {
    let breed = breed.to_lowercase();

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
            Json(NotFoundWithCodeResponse {
                status: "error",
                message: "Breed not found (main breed does not exist)",
                code: 404,
            }),
        )),
    }
}

async fn random_breed_images_endpoint(
    Path((breed, count)): Path<(String, usize)>,
    State(state): State<AppState>,
) -> Result<Json<RandomImagesResponse>, (StatusCode, Json<NotFoundWithCodeResponse>)> {
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
    let breed = breed.to_lowercase();

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
    let breed = breed.to_lowercase();

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
    let breed = breed.to_lowercase();
    let count = parse_count_or_default_one(&count);

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
    let key = format!("{}/{}", breed.to_lowercase(), sub_breed.to_lowercase());

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
    let key = format!("{}/{}", breed.to_lowercase(), sub_breed.to_lowercase());

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
    Path((breed, sub_breed, count)): Path<(String, String, usize)>,
    State(state): State<AppState>,
) -> Result<Json<RandomImagesResponse>, (StatusCode, Json<NotFoundWithCodeResponse>)> {
    let key = format!("{}/{}", breed.to_lowercase(), sub_breed.to_lowercase());

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

fn to_public_url(path: &PathBuf) -> String {
    let path_str = path.to_string_lossy();
    let trimmed = path_str.strip_prefix("dog-api-images/").unwrap_or(&path_str);
    format!("https://images.dog.ceo/breeds/{trimmed}")
}

fn parse_count_or_default_one(value: &str) -> usize {
    match value.parse::<isize>() {
        Ok(parsed) if parsed > 0 => parsed as usize,
        _ => 1,
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

#[tokio::main]
async fn main() {
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
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("failed to bind server");

    println!("Listening on http://0.0.0.0:3000");
    axum::serve(listener, app)
        .await
        .expect("server encountered an error");
}