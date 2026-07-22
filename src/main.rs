use axum::{
    body::Bytes,
    Json, Router,
    extract::{Path, Request, State},
    http::{
        StatusCode,
        header::{CACHE_CONTROL, CONTENT_TYPE},
    },
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use ahash::AHashMap;
use rand::rngs::SmallRng;
use rand::seq::SliceRandom;
use rand::SeedableRng;
use serde::Serialize;
use std::cell::RefCell;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

type FastMap<K, V> = AHashMap<K, V>;

// Embedded at compile time. File is no longer needed at runtime.
const MANIFEST_BYTES: &[u8] = include_bytes!("../manifest.nul");
const MAX_BREED_SEGMENT_LEN: usize = 64;
const MAX_COUNT_INPUT_LEN: usize = 32;

thread_local! {
    static FAST_RNG: RefCell<SmallRng> = RefCell::new(SmallRng::from_entropy());
}

fn with_fast_rng<T>(f: impl FnOnce(&mut SmallRng) -> T) -> T {
    FAST_RNG.with(|rng| {
        let mut rng = rng.borrow_mut();
        f(&mut rng)
    })
}

fn pick_random_ref<T>(items: &[T]) -> Option<&T> {
    if items.is_empty() {
        None
    } else {
        Some(&items[fastrand::usize(..items.len())])
    }
}

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
    breeds_lookup: Arc<FastMap<String, Vec<String>>>,
    main_breeds: Arc<Vec<String>>,
    breed_images: Arc<FastMap<String, Vec<String>>>,
    sub_breed_images: Arc<FastMap<String, FastMap<String, Vec<String>>>>,
    list_all_breeds_json: Bytes,
    list_main_breeds_json: Bytes,
}

#[derive(Serialize)]
struct RandomImageResponse {
    message: String,
    status: &'static str,
}

#[derive(Serialize)]
struct RandomImageRefResponse<'a> {
    message: &'a str,
    status: &'static str,
}

#[derive(Serialize)]
struct RandomImagesPickedRefResponse<'a> {
    message: Vec<&'a str>,
    status: &'static str,
}

#[derive(Serialize)]
struct RandomImagesRefResponse<'a> {
    message: &'a [String],
    status: &'static str,
}

#[derive(Serialize)]
struct BreedMapRefResponse<'a> {
    message: BTreeMap<&'a str, &'a [String]>,
    status: &'static str,
}

#[derive(Serialize)]
struct BreedListRefResponse<'a> {
    message: &'a BTreeMap<String, Vec<String>>,
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
) -> Result<Response, (StatusCode, Json<RandomImageResponse>)> {
    let selected = pick_random_ref(&state.urls).map(String::as_str);

    let Some(url) = selected else {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(RandomImageResponse {
                message: "No images available".to_string(),
                status: "error",
            }),
        ));
    };

    Ok(Json(RandomImageRefResponse {
        message: url,
        status: "success",
    })
    .into_response())
}

async fn random_images(
    Path(count): Path<String>,
    State(state): State<AppState>,
) -> Response {
    let count = parse_count_or_default_one(&count);
    let capped = count.min(50).min(state.urls.len());
    let urls = if capped == 1 {
        pick_random_ref(&state.urls)
            .map(|s| vec![s.as_str()])
            .unwrap_or_default()
    } else {
        with_fast_rng(|rng| {
            state
                .urls
                .choose_multiple(rng, capped)
                .map(String::as_str)
                .collect::<Vec<&str>>()
        })
    };

    Json(RandomImagesPickedRefResponse {
        message: urls,
        status: "success",
    })
    .into_response()
}

async fn list_all_breeds(State(state): State<AppState>) -> Response {
    cached_json_response(&state.list_all_breeds_json)
}

async fn list_main_breeds(State(state): State<AppState>) -> Response {
    cached_json_response(&state.list_main_breeds_json)
}

async fn random_main_breed(
    State(state): State<AppState>,
) -> Result<Response, (StatusCode, Json<RandomImageResponse>)> {
    let selected = pick_random_ref(&state.main_breeds).map(String::as_str);

    let Some(breed) = selected else {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(RandomImageResponse {
                message: "No breeds available".to_string(),
                status: "error",
            }),
        ));
    };

    Ok(Json(RandomImageRefResponse {
        message: breed,
        status: "success",
    })
    .into_response())
}

async fn random_main_breeds(
    Path(count): Path<String>,
    State(state): State<AppState>,
) -> Response {
    let count = parse_count_or_default_one(&count);
    let capped = count.min(state.main_breeds.len());
    let selected = if capped == 1 {
        pick_random_ref(&state.main_breeds)
            .map(|s| vec![s.as_str()])
            .unwrap_or_default()
    } else {
        with_fast_rng(|rng| {
            state
                .main_breeds
                .choose_multiple(rng, capped)
                .map(String::as_str)
                .collect::<Vec<&str>>()
        })
    };

    Json(RandomImagesPickedRefResponse {
        message: selected,
        status: "success",
    })
    .into_response()
}

async fn random_all_breeds(State(state): State<AppState>) -> Response {
    let selected = pick_random_ref(&state.main_breeds).map(String::as_str);

    let mut message: BTreeMap<&str, &[String]> = BTreeMap::new();

    if let Some(breed) = selected {
        if let Some(sub_breeds) = state.breeds_lookup.get(breed) {
            message.insert(breed, sub_breeds.as_slice());
        }
    }

    Json(BreedMapRefResponse {
        message,
        status: "success",
    })
    .into_response()
}

async fn random_all_breeds_count(
    Path(count): Path<String>,
    State(state): State<AppState>,
) -> Response {
    let count = parse_count_or_default_one(&count);
    let capped = count.min(state.main_breeds.len());

    let selected = with_fast_rng(|rng| {
        state
            .main_breeds
            .choose_multiple(rng, capped)
            .map(String::as_str)
            .collect::<Vec<&str>>()
    });

    let mut message: BTreeMap<&str, &[String]> = BTreeMap::new();

    for breed in selected {
        if let Some(sub_breeds) = state.breeds_lookup.get(breed) {
            message.insert(breed, sub_breeds.as_slice());
        }
    }

    Json(BreedMapRefResponse {
        message,
        status: "success",
    })
    .into_response()
}

fn cached_json_response(payload: &Bytes) -> Response {
    (
        [(CONTENT_TYPE, "application/json")],
        payload.clone(),
    )
        .into_response()
}

async fn cache_control_middleware(req: Request, next: Next) -> Response {
    let is_random = req.uri().path().contains("/random");
    let is_breed_or_breeds = {
        let path = req.uri().path();
        path.starts_with("/breed/") || path.starts_with("/breeds/")
    };
    let mut response = next.run(req).await;

    let cache_value = if is_random {
        "no-store"
    } else if is_breed_or_breeds {
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
) -> Result<Response, (StatusCode, Json<NotFoundWithCodeResponse>)> {
    let Some(breed) = normalize_breed_segment(&breed) else {
        return Err(main_breed_not_found());
    };

    match state.breed_images.get(breed.as_ref()) {
        Some(images) if !images.is_empty() => Ok(Json(RandomImagesRefResponse {
            message: images.as_slice(),
            status: "success",
        })
        .into_response()),
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
) -> Result<Response, (StatusCode, Json<NotFoundWithCodeResponse>)> {
    let Some(breed) = normalize_breed_segment(&breed) else {
        return Err(main_breed_not_found());
    };

    match state.breed_images.get(breed.as_ref()) {
        Some(images) if !images.is_empty() => {
            let selected = pick_random_ref(images).map(String::as_str);
            let image = selected.unwrap_or_default();

            Ok(Json(RandomImageRefResponse {
                message: image,
                status: "success",
            })
            .into_response())
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
) -> Result<Response, (StatusCode, Json<NotFoundWithCodeResponse>)> {
    let Some(breed) = normalize_breed_segment(&breed) else {
        return Err(main_breed_not_found());
    };
    let count = parse_count_or_default_one(&count);

    match state.breed_images.get(breed.as_ref()) {
        Some(images) if !images.is_empty() => {
            let capped = count.min(50).min(images.len());
            let selected = if capped == 1 {
                pick_random_ref(images)
                    .map(|s| vec![s.as_str()])
                    .unwrap_or_default()
            } else {
                with_fast_rng(|rng| {
                    images
                        .choose_multiple(rng, capped)
                        .map(String::as_str)
                        .collect::<Vec<&str>>()
                })
            };

            Ok(Json(RandomImagesPickedRefResponse {
                message: selected,
                status: "success",
            })
            .into_response())
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
) -> Result<Response, (StatusCode, Json<NotFoundWithCodeResponse>)> {
    let Some(breed) = normalize_breed_segment(&breed) else {
        return Err(main_breed_not_found());
    };

    match state.breeds_lookup.get(breed.as_ref()) {
        Some(sub_breeds) => Ok(Json(RandomImagesRefResponse {
            message: sub_breeds.as_slice(),
            status: "success",
        })
        .into_response()),
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
) -> Result<Response, (StatusCode, Json<NotFoundWithCodeResponse>)> {
    let Some(breed) = normalize_breed_segment(&breed) else {
        return Err(main_breed_not_found());
    };

    match state.breeds_lookup.get(breed.as_ref()) {
        Some(sub_breeds) if !sub_breeds.is_empty() => {
            let selected = pick_random_ref(sub_breeds).map(String::as_str);
            let sub_breed = selected.unwrap_or_default();

            Ok(Json(RandomImageRefResponse {
                message: sub_breed,
                status: "success",
            })
            .into_response())
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
) -> Result<Response, (StatusCode, Json<NotFoundWithCodeResponse>)> {
    let Some(breed) = normalize_breed_segment(&breed) else {
        return Err(main_breed_not_found());
    };
    let count = parse_sub_breed_random_count(&count);

    match state.breeds_lookup.get(breed.as_ref()) {
        Some(sub_breeds) if !sub_breeds.is_empty() => {
            let capped = count.min(sub_breeds.len());
            let selected = if capped == 1 {
                pick_random_ref(sub_breeds)
                    .map(|s| vec![s.as_str()])
                    .unwrap_or_default()
            } else {
                with_fast_rng(|rng| {
                    sub_breeds
                        .choose_multiple(rng, capped)
                        .map(String::as_str)
                        .collect::<Vec<&str>>()
                })
            };

            Ok(Json(RandomImagesPickedRefResponse {
                message: selected,
                status: "success",
            })
            .into_response())
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
) -> Result<Response, (StatusCode, Json<NotFoundWithCodeResponse>)> {
    let Some(breed) = normalize_breed_segment(&breed) else {
        return Err(main_breed_not_found());
    };
    let Some(sub_breed) = normalize_breed_segment(&sub_breed) else {
        return Err(sub_breed_not_found());
    };
    let maybe_images = state
        .sub_breed_images
        .get(breed.as_ref())
        .and_then(|subs| subs.get(sub_breed.as_ref()));

    match maybe_images {
        Some(images) if !images.is_empty() => Ok(Json(RandomImagesRefResponse {
            message: images.as_slice(),
            status: "success",
        })
        .into_response()),
        _ => Err(sub_breed_not_found()),
    }
}

async fn random_sub_breed_image_endpoint(
    Path((breed, sub_breed)): Path<(String, String)>,
    State(state): State<AppState>,
) -> Result<Response, (StatusCode, Json<NotFoundWithCodeResponse>)> {
    let Some(breed) = normalize_breed_segment(&breed) else {
        return Err(main_breed_not_found());
    };
    let Some(sub_breed) = normalize_breed_segment(&sub_breed) else {
        return Err(sub_breed_not_found());
    };
    let maybe_images = state
        .sub_breed_images
        .get(breed.as_ref())
        .and_then(|subs| subs.get(sub_breed.as_ref()));

    match maybe_images {
        Some(images) if !images.is_empty() => {
            let selected = pick_random_ref(images).map(String::as_str);
            let image = selected.unwrap_or_default();

            Ok(Json(RandomImageRefResponse {
                message: image,
                status: "success",
            })
            .into_response())
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
) -> Result<Response, (StatusCode, Json<NotFoundWithCodeResponse>)> {
    let Some(breed) = normalize_breed_segment(&breed) else {
        return Err(main_breed_not_found());
    };
    let Some(sub_breed) = normalize_breed_segment(&sub_breed) else {
        return Err(sub_breed_not_found());
    };
    let count = parse_count_or_default_one(&count);
    let maybe_images = state
        .sub_breed_images
        .get(breed.as_ref())
        .and_then(|subs| subs.get(sub_breed.as_ref()));

    match maybe_images {
        Some(images) if !images.is_empty() => {
            let capped = count.min(50).min(images.len());
            let selected = if capped == 1 {
                pick_random_ref(images)
                    .map(|s| vec![s.as_str()])
                    .unwrap_or_default()
            } else {
                with_fast_rng(|rng| {
                    images
                        .choose_multiple(rng, capped)
                        .map(String::as_str)
                        .collect::<Vec<&str>>()
                })
            };

            Ok(Json(RandomImagesPickedRefResponse {
                message: selected,
                status: "success",
            })
            .into_response())
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

    if state.breeds_lookup.contains_key(breed.as_ref()) {
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

    let Some(sub_breeds) = state.breeds_lookup.get(breed.as_ref()) else {
        return Err(main_breed_not_found());
    };

    if !sub_breeds.iter().any(|s| s == sub_breed.as_ref()) {
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

fn normalize_breed_segment(input: &str) -> Option<Cow<'_, str>> {
    if input.is_empty() || input.len() > MAX_BREED_SEGMENT_LEN || !input.is_ascii() {
        return None;
    }

    if !input.bytes().all(|b| b.is_ascii_alphabetic()) {
        return None;
    }

    if input.bytes().all(|b| b.is_ascii_lowercase()) {
        Some(Cow::Borrowed(input))
    } else {
        Some(Cow::Owned(input.to_ascii_lowercase()))
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
        assert_eq!(normalize_breed_segment("hound").as_deref(), Some("hound"));
        assert_eq!(normalize_breed_segment("HOUND").as_deref(), Some("hound"));
        assert_eq!(normalize_breed_segment("German").as_deref(), Some("german"));
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

fn build_breed_image_map(paths: &[PathBuf]) -> FastMap<String, Vec<String>> {
    let mut out: FastMap<String, Vec<String>> = FastMap::new();

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

fn build_sub_breed_image_map(
    paths: &[PathBuf],
) -> FastMap<String, FastMap<String, Vec<String>>> {
    let mut out: FastMap<String, FastMap<String, Vec<String>>> = FastMap::new();

    for path in paths {
        let path_str = path.to_string_lossy();
        let trimmed = path_str.strip_prefix("dog-api-images/").unwrap_or(&path_str);
        let Some(folder) = trimmed.split('/').next() else {
            continue;
        };

        if let Some((breed, sub_breed)) = folder.split_once('-') {
            let breed = breed.to_lowercase();
            let sub_breed = sub_breed.to_lowercase();
            out.entry(breed)
                .or_default()
                .entry(sub_breed)
                .or_default()
                .push(to_public_url(path));
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
    let breeds_lookup: FastMap<String, Vec<String>> =
        breeds.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    let main_breeds: Vec<String> = breeds.keys().cloned().collect();
    let list_all_breeds_json = Bytes::from(
        serde_json::to_vec(&BreedListRefResponse {
            message: &breeds,
            status: "success",
        })
        .expect("serialize cached /breeds/list/all response"),
    );
    let list_main_breeds_json = Bytes::from(
        serde_json::to_vec(&RandomImagesRefResponse {
            message: main_breeds.as_slice(),
            status: "success",
        })
        .expect("serialize cached /breeds/list response"),
    );
    let breed_images = build_breed_image_map(&manifest_paths);
    let sub_breed_images = build_sub_breed_image_map(&manifest_paths);
    println!("Loaded {} manifest entries", urls.len());
    println!("Loaded {} breeds", breeds.len());

    let state = AppState {
        urls: Arc::new(urls),
        breeds_lookup: Arc::new(breeds_lookup),
        main_breeds: Arc::new(main_breeds),
        breed_images: Arc::new(breed_images),
        sub_breed_images: Arc::new(sub_breed_images),
        list_all_breeds_json,
        list_main_breeds_json,
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
    println!("Starting runtime in current-thread mode");

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");

    runtime.block_on(run_server());
}