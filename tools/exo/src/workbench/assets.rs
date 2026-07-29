use axum::body::Body;
use axum::http::header::{
    CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, HeaderName, HeaderValue, REFERRER_POLICY,
    X_CONTENT_TYPE_OPTIONS,
};
use axum::http::{Response, StatusCode};
use std::sync::OnceLock;

#[cfg(feature = "ui")]
use include_dir::{Dir, include_dir};

#[cfg(feature = "ui")]
static ASSETS: Dir<'_> = include_dir!("$OUT_DIR/workbench-assets");

pub(super) const fn available() -> bool {
    cfg!(feature = "ui")
}

pub(super) fn hash() -> String {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(compute_hash).clone()
}

pub(super) fn response(path: &str) -> Response<Body> {
    if !available() {
        return secure_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "text/plain; charset=utf-8",
            Body::from("This Exo binary was built without embedded workbench assets."),
            false,
        );
    }

    let requested = path.trim_start_matches('/');
    let effective = if requested.is_empty() {
        "index.html"
    } else {
        requested
    };
    #[cfg(feature = "ui")]
    if let Some(file) = ASSETS.get_file(effective) {
        let mime = mime_guess::from_path(effective).first_or_octet_stream();
        return secure_response(
            StatusCode::OK,
            mime.essence_str(),
            Body::from(file.contents().to_vec()),
            effective.starts_with("_app/immutable/"),
        );
    }
    #[cfg(not(feature = "ui"))]
    let _ = effective;
    #[cfg(feature = "ui")]
    if let Some(index) = ASSETS.get_file("index.html") {
        return secure_response(
            StatusCode::OK,
            "text/html; charset=utf-8",
            Body::from(index.contents().to_vec()),
            false,
        );
    }
    secure_response(
        StatusCode::NOT_FOUND,
        "text/plain; charset=utf-8",
        Body::from("Not Found"),
        false,
    )
}

fn secure_response(
    status: StatusCode,
    content_type: &str,
    body: Body,
    immutable: bool,
) -> Response<Body> {
    let mut response = Response::new(body);
    *response.status_mut() = status;
    let headers = response.headers_mut();
    if let Ok(content_type) = HeaderValue::from_str(content_type) {
        headers.insert(CONTENT_TYPE, content_type);
    }
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    headers.insert(
        CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src 'self'; img-src 'self' data:; font-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'",
        ),
    );
    headers.insert(
        CACHE_CONTROL,
        HeaderValue::from_static(if immutable {
            "public, max-age=31536000, immutable"
        } else {
            "no-store"
        }),
    );
    headers.insert(
        HeaderName::from_static("x-frame-options"),
        HeaderValue::from_static("DENY"),
    );
    response
}

#[cfg(feature = "ui")]
fn compute_hash() -> String {
    let mut files = ASSETS.files().collect::<Vec<_>>();
    files.sort_by_key(|file| file.path());
    let mut hasher = blake3::Hasher::new();
    for file in files {
        hasher.update(file.path().to_string_lossy().as_bytes());
        hasher.update(&[0]);
        hasher.update(file.contents());
        hasher.update(&[0xff]);
    }
    format!("blake3:{}", hasher.finalize())
}

#[cfg(not(feature = "ui"))]
fn compute_hash() -> String {
    "unavailable".to_string()
}
